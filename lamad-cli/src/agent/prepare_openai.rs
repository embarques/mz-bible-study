//! OpenAI / ChatGPT prepare: vision → study JSON, then Images API → 3 section PNGs.
//! Independent of the Cursor Cloud Agents path in [`super::prepare`].

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::Path;

use crate::agent::openai::{self, OpenAiClient};
use crate::agent::prepare::{expected_paths, stamp_audience, verify_deliverables, Deliverables, PrepareRequest};
use crate::agent::{AGENTS_MD, PREPARE_STUDY_MD};
use crate::section_styles::{section_style_for_study, style_prompt_block};
use crate::{paths, rasterize_pages};

pub async fn run_prepare_openai(req: &PrepareRequest) -> Result<Deliverables> {
    paths::studies_dir(req.audience)?;
    let pages_dir = paths::generated_dir()?.join("pages").join(format!("study{}", req.study));
    std::fs::create_dir_all(&pages_dir)?;

    let mut page_nums: Vec<u32> = (req.pages.0..=req.pages.1).collect();
    if let Some((na, _)) = req.next_pages {
        if !page_nums.contains(&na) && page_nums.len() < 5 {
            page_nums.push(na);
        }
    }
    page_nums.truncate(5);

    let rasters = rasterize_pages(
        &req.pdf,
        &page_nums,
        &pages_dir,
        req.pdftoppm_path.as_deref(),
    )?;
    let mut images = Vec::new();
    for r in &rasters {
        images.push(openai::vision_image_from_path(r)?);
    }

    let dest = expected_paths(req.study, req.audience, &req.root);
    std::fs::create_dir_all(dest.json.parent().unwrap())?;
    std::fs::create_dir_all(dest.img1.parent().unwrap())?;

    let client = OpenAiClient::new(&req.api_key)?;
    println!(
        "  ChatGPT prepare (model={}, images={})",
        req.model, req.image_model
    );

    let (system, user) = build_openai_prepare_prompts(req)?;
    println!("  Extracting study JSON from scans…");
    let mut study_json = client
        .chat_json(&req.model, &system, &user, &images)
        .await
        .context("OpenAI vision → study JSON")?;

    normalize_study_json(&mut study_json, req, &dest)?;
    let pretty = serde_json::to_string_pretty(&study_json)?;
    std::fs::write(&dest.json, &pretty)
        .with_context(|| format!("write {}", dest.json.display()))?;
    println!("  wrote {}", dest.json.display());

    let prompts = section_image_prompts(req.study, &study_json)?;
    let slots = [&dest.img1, &dest.img2, &dest.img3];
    for (i, (prompt, path)) in prompts.iter().zip(slots.iter()).enumerate() {
        let n = i + 1;
        println!("  Generating section image {n}/3…");
        let png = match client
            .generate_section_png(&req.image_model, prompt)
            .await
        {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  section {n} blocked/failed ({e:#}); retrying safer prompt…");
                let safer = format!(
                    "{prompt}\n\nSafer framing: no graphic violence, no addiction paraphernalia \
                     close-ups, respectful biblical illustration suitable for youth ministry."
                );
                client
                    .generate_section_png(&req.image_model, &safer)
                    .await
                    .with_context(|| format!("OpenAI image section {n}"))?
            }
        };
        std::fs::write(path, &png)
            .with_context(|| format!("write {}", path.display()))?;
        println!("  wrote {}", path.display());
    }

    stamp_audience(&dest.json, req.audience)?;
    verify_deliverables(req.study, req.audience, &req.root)
}

fn build_openai_prepare_prompts(req: &PrepareRequest) -> Result<(String, String)> {
    let aud = req.audience.as_str();
    let study = req.study;
    let proximo_block = if req.omit_proximo {
        "Próximo: **omit** (last study — no `proximo` object, or null).".to_string()
    } else if let Some(np) = req.next_pages {
        format!(
            "Próximo: read from the **next** study’s title page \
             (attached image of the first page of pages {}–{}). \
             Put número, título, base bíblica in JSON `proximo`.",
            np.0, np.1
        )
    } else {
        bail!("próximo metadata missing and no next_pages");
    };

    let style_block = style_prompt_block(study);
    let base = format!("studies/{aud}");

    let system = format!(
        r#"You extract Mount Zion Church Spanish Bible-study content from scan images into JSON.
Follow PREPARE_STUDY.md and AGENTS.md. Reply with a single JSON object only (no markdown).
Do NOT generate images — the host will create section PNGs separately.
Do NOT build a .pptx."#
    );

    let user = format!(
        r#"## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Required JSON fields
Include at least: numero, titulo, base_biblica, lectura {{cita, versiculos}},
propositos (exactly 3), idea_principal, para_memorizar {{texto, cita?}},
comentario or comentario_slides, introduccion or introduccion_slides,
puntos (exactly 3, each with n, titulo, rango, texto_biblico, A {{titulo, cuerpo|slides}}, B {{titulo, cuerpo|slides}}),
conclusion or conclusion_slides, proximo (or null if last),
section_images (exactly 3 paths — use placeholders below),
section_style (must match assigned id), audience.

Use these exact section_images paths:
- `{base}/media/{study}-section1.png`
- `{base}/media/{study}-section2.png`
- `{base}/media/{study}-section3.png`

## Section style — HARD
{style_block}

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- Prefer packed `*_slides` when helpful; otherwise provide full body text fields.
- Inline scripture refs stay as printed.

---
# PREPARE_STUDY.md

{prepare}

---
# AGENTS.md (rules)

{agents}
"#,
        p0 = req.pages.0,
        p1 = req.pages.1,
        prepare = PREPARE_STUDY_MD,
        agents = AGENTS_MD,
    );

    Ok((system, user))
}

/// Force CLI-owned fields the model must not invent wrong.
fn normalize_study_json(v: &mut Value, req: &PrepareRequest, dest: &Deliverables) -> Result<()> {
    let style = section_style_for_study(req.study);
    let obj = v
        .as_object_mut()
        .context("study JSON root must be an object")?;

    obj.insert(
        "numero".into(),
        Value::Number(req.study.into()),
    );
    obj.insert(
        "audience".into(),
        Value::String(req.audience.as_str().into()),
    );
    obj.insert(
        "section_style".into(),
        serde_json::json!({ "id": style.id, "name": style.name }),
    );

    let rel = |p: &Path| -> String {
        p.strip_prefix(&req.root)
            .map(|x| x.display().to_string())
            .unwrap_or_else(|_| p.display().to_string())
    };
    obj.insert(
        "section_images".into(),
        Value::Array(vec![
            Value::String(rel(&dest.img1)),
            Value::String(rel(&dest.img2)),
            Value::String(rel(&dest.img3)),
        ]),
    );

    if req.omit_proximo {
        obj.insert("proximo".into(), Value::Null);
    }

    Ok(())
}

fn section_image_prompts(study: u32, json: &Value) -> Result<[String; 3]> {
    let style = section_style_for_study(study);
    let puntos = json
        .get("puntos")
        .and_then(|p| p.as_array())
        .context("study JSON missing puntos array")?;
    if puntos.len() < 3 {
        bail!("study JSON must have 3 puntos for section images, got {}", puntos.len());
    }

    let mut out = [String::new(), String::new(), String::new()];
    for i in 0..3 {
        let p = &puntos[i];
        let titulo = p
            .get("titulo")
            .and_then(|t| t.as_str())
            .unwrap_or("tema bíblico");
        let rango = p.get("rango").and_then(|t| t.as_str()).unwrap_or("");
        out[i] = format!(
            "Widescreen 16:9 Bible-study slide background illustration for youth ministry. \
             Design family: {name} ({id}). Recipe: {recipe}. \
             Scene theme for section {n}: \"{titulo}\" ({rango}). \
             Subject on the right/center-right; left ~40% is a calm text-safe wash matching the family. \
             No text, letters, logos, watermarks, captions, or yellow dashed/dotted arcs. \
             Finished photographic/painted art, respectful Christian biblical atmosphere.",
            name = style.name,
            id = style.id,
            recipe = style.recipe,
            n = i + 1,
            titulo = titulo,
            rango = rango,
        );
    }
    Ok(out)
}
