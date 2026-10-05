//! OpenAI / ChatGPT prepare: vision → study JSON, then Images API → 3 section PNGs.
//! Independent of the Cursor Cloud Agents path in [`super::prepare`].

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::agent::openai::{self, OpenAiClient};
use crate::agent::prepare::{
    adult_image_basenames, adult_media_dir, expected_paths, stamp_adult_image_paths, stamp_audience,
    verify_deliverables, Deliverables, PrepareRequest,
};
use crate::agent::{AGENTS_MD, PREPARE_STUDY_MD};
use crate::job::Audience;
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
    crate::progress::phase(format!(
        "ChatGPT prepare (model={}, images={})",
        req.model, req.image_model
    ));

    let (system, user) = build_openai_prepare_prompts(req)?;
    let json_spin = crate::progress::Spinner::start("Extracting study JSON from scans…");
    let mut study_json = match client.chat_json(&req.model, &system, &user, &images).await {
        Ok(v) => {
            json_spin.succeed("study JSON extracted");
            v
        }
        Err(e) => {
            json_spin.fail("study JSON extraction failed");
            return Err(e).context("OpenAI vision → study JSON");
        }
    };

    match req.audience {
        Audience::Youth => normalize_study_json(&mut study_json, req, &dest)?,
        Audience::Adult => normalize_adult_study_json(&mut study_json, req)?,
    }
    let pretty = serde_json::to_string_pretty(&study_json)?;
    std::fs::write(&dest.json, &pretty)
        .with_context(|| format!("write {}", dest.json.display()))?;
    crate::progress::ok(format!("wrote {}", dest.json.display()));

    match req.audience {
        Audience::Youth => generate_youth_section_images(req, &client, &dest, &study_json).await?,
        Audience::Adult => generate_adult_images(req, &client, &study_json).await?,
    }

    stamp_audience(&dest.json, req.audience)?;
    if req.audience == Audience::Adult {
        stamp_adult_image_paths(&dest.json, req.study, &req.root)?;
    }
    verify_deliverables(req.study, req.audience, &req.root)
}

/// Host-side parallel images from an existing study JSON (Cursor fast path).
pub async fn generate_host_images(req: &PrepareRequest) -> Result<()> {
    let key = req
        .image_api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .context(
            "host image generation needs openai_api_key / OPENAI_API_KEY \
             (set in config.toml even when agent_provider=cursor)",
        )?;
    let client = OpenAiClient::new(key)?;
    let dest = expected_paths(req.study, req.audience, &req.root);
    let study_json: Value = serde_json::from_str(&std::fs::read_to_string(&dest.json)?)
        .with_context(|| format!("parse {}", dest.json.display()))?;

    crate::progress::now_step(
        3,
        4,
        "Host images (parallel)",
        match req.audience {
            Audience::Youth => "Generating 3 section images via OpenAI (×3 parallel)…",
            Audience::Adult => {
                "Generating ~13 scenic/definición images via OpenAI (×4 parallel)…"
            }
        },
    );

    match req.audience {
        Audience::Youth => {
            generate_youth_section_images(req, &client, &dest, &study_json).await?;
        }
        Audience::Adult => {
            generate_adult_images(req, &client, &study_json).await?;
        }
    }

    stamp_audience(&dest.json, req.audience)?;
    if req.audience == Audience::Adult {
        stamp_adult_image_paths(&dest.json, req.study, &req.root)?;
    } else {
        stamp_youth_section_images(&dest)?;
    }
    crate::progress::ok("Step 3/4 done — host images ready");
    Ok(())
}

fn stamp_youth_section_images(dest: &Deliverables) -> Result<()> {
    let text = std::fs::read_to_string(&dest.json)?;
    let mut v: Value = serde_json::from_str(&text)?;
    let obj = v
        .as_object_mut()
        .context("youth JSON root must be an object")?;
    // Prefer paths relative to project if possible; absolute is OK for builder.
    obj.insert(
        "section_images".into(),
        Value::Array(vec![
            Value::String(dest.img1.display().to_string()),
            Value::String(dest.img2.display().to_string()),
            Value::String(dest.img3.display().to_string()),
        ]),
    );
    std::fs::write(&dest.json, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

async fn generate_youth_section_images(
    req: &PrepareRequest,
    client: &OpenAiClient,
    dest: &Deliverables,
    study_json: &Value,
) -> Result<()> {
    use futures_util::stream::{self, StreamExt};

    let prompts = section_image_prompts(req.study, study_json)?;
    let slots = [dest.img1.clone(), dest.img2.clone(), dest.img3.clone()];
    let img_bar = crate::progress::bar(3, "Generating section images (parallel)");
    let model = req.image_model.clone();
    let jobs: Vec<(usize, String, PathBuf)> = prompts
        .into_iter()
        .zip(slots)
        .enumerate()
        .map(|(i, (prompt, path))| (i + 1, prompt, path))
        .collect();

    let results: Vec<Result<()>> = stream::iter(jobs)
        .map(|(n, prompt, path)| {
            let client = client;
            let model = model.clone();
            let bar = img_bar.clone();
            async move {
                let png = generate_png_with_retry(client, &model, &prompt, n).await?;
                std::fs::write(&path, &png)
                    .with_context(|| format!("write {}", path.display()))?;
                bar.inc(1);
                Ok(())
            }
        })
        .buffer_unordered(3)
        .collect()
        .await;
    for r in results {
        r?;
    }
    img_bar.finish_with_message("Section images ready");
    Ok(())
}

async fn generate_adult_images(
    req: &PrepareRequest,
    client: &OpenAiClient,
    study_json: &Value,
) -> Result<()> {
    use futures_util::stream::{self, StreamExt};

    let media = adult_media_dir(req.study, &req.root);
    std::fs::create_dir_all(&media)?;
    let prompts = adult_image_prompts(req.study, study_json)?;
    let names = adult_image_basenames();
    let img_bar =
        crate::progress::bar(names.len() as u64, "Generating adult images (×4 parallel)");
    let model = req.image_model.clone();
    let jobs: Vec<(usize, String, PathBuf)> = names
        .iter()
        .zip(prompts.into_iter())
        .enumerate()
        .map(|(i, (name, prompt))| (i + 1, prompt, media.join(name)))
        .collect();

    let results: Vec<Result<()>> = stream::iter(jobs)
        .map(|(n, prompt, path)| {
            let client = client;
            let model = model.clone();
            let bar = img_bar.clone();
            async move {
                let png = generate_png_with_retry(client, &model, &prompt, n).await?;
                std::fs::write(&path, &png)
                    .with_context(|| format!("write {}", path.display()))?;
                bar.inc(1);
                Ok(())
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;
    for r in results {
        r?;
    }
    img_bar.finish_with_message("Adult images ready");
    Ok(())
}

async fn generate_png_with_retry(
    client: &OpenAiClient,
    model: &str,
    prompt: &str,
    n: usize,
) -> Result<Vec<u8>> {
    match client.generate_section_png(model, prompt).await {
        Ok(b) => Ok(b),
        Err(e) => {
            crate::progress::warn(format!(
                "image {n} blocked/failed ({e:#}); retrying safer prompt…"
            ));
            let safer = format!(
                "{prompt}\n\nSafer framing: no graphic violence, respectful biblical \
                 illustration suitable for church adult Bible study."
            );
            client
                .generate_section_png(model, &safer)
                .await
                .with_context(|| format!("OpenAI image {n}"))
        }
    }
}

fn build_openai_prepare_prompts(req: &PrepareRequest) -> Result<(String, String)> {
    match req.audience {
        Audience::Youth => build_openai_youth_prompts(req),
        Audience::Adult => build_openai_adult_prompts(req),
    }
}

fn proximo_block(req: &PrepareRequest) -> Result<String> {
    if req.omit_proximo {
        Ok("Próximo: **omit** (last study — no `proximo` object, or null).".to_string())
    } else if let Some(np) = req.next_pages {
        Ok(format!(
            "Próximo: read from the **next** study’s title page \
             (attached image of the first page of pages {}–{}). \
             Put número, título, base bíblica in JSON `proximo`.",
            np.0, np.1
        ))
    } else {
        bail!("próximo metadata missing and no next_pages")
    }
}

fn build_openai_youth_prompts(req: &PrepareRequest) -> Result<(String, String)> {
    let aud = req.audience.as_str();
    let study = req.study;
    let proximo_block = proximo_block(req)?;
    let style_block = style_prompt_block(study);
    let base = format!("studies/{aud}");

    let system = r#"You extract Mount Zion Church Spanish Bible-study content from scan images into JSON.
Follow PREPARE_STUDY.md and AGENTS.md. Reply with a single JSON object only (no markdown).
Do NOT generate images — the host will create section PNGs separately.
Do NOT build a .pptx."#
        .to_string();

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

fn build_openai_adult_prompts(req: &PrepareRequest) -> Result<(String, String)> {
    let study = req.study;
    let proximo_block = proximo_block(req)?;
    let system = r#"You extract Mount Zion Church Spanish **adult** Bible-study content from scan images into JSON.
Adult schema (not youth): lectura_antifonal, objetivos, pensamiento_central, texto_aureo,
ensenanza, datos_generales, introduccion_slides, temas[3] with A/B blocks.
Reply with a single JSON object only (no markdown). Do NOT generate images. Do NOT build a .pptx."#
        .to_string();

    let user = format!(
        r#"## Inputs
- Estudio **{study}** (audience=adult)
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Required adult JSON keys
numero, titulo, base_biblica (array of citation lines; trailing `;` OK except last),
lectura_antifonal (array of {{cita, versiculos}}),
objetivos (exactly 3),
pensamiento_central (string),
texto_aureo {{texto, cita}} — cita without parentheses,
ensenanza, datos_generales {{autor, personajes, fecha, lugar}},
introduccion_slides (array of paragraphs),
temas (exactly 3): each {{titulo, rango, definiciones[], A {{titulo, texto_slides[], texto_biblico}}, B {{…}}}},
proximo (or null if last).
Host stamps scenic_images / definicion_images — omit or leave empty.

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; skip Ideas para el maestro / Preguntas.
- Whole verses only in lectura/texto_biblico.

---
# AGENTS.md (rules)

{agents}
"#,
        p0 = req.pages.0,
        p1 = req.pages.1,
        agents = AGENTS_MD,
    );

    Ok((system, user))
}

fn normalize_adult_study_json(v: &mut Value, req: &PrepareRequest) -> Result<()> {
    let obj = v
        .as_object_mut()
        .context("study JSON root must be an object")?;
    obj.insert("numero".into(), Value::Number(req.study.into()));
    obj.insert(
        "audience".into(),
        Value::String(Audience::Adult.as_str().into()),
    );
    if req.omit_proximo {
        obj.insert("proximo".into(), Value::Null);
    }
    Ok(())
}

fn adult_image_prompts(study: u32, json: &Value) -> Result<Vec<String>> {
    let titulo = json
        .get("titulo")
        .and_then(|t| t.as_str())
        .unwrap_or("estudio bíblico");
    let temas = json
        .get("temas")
        .and_then(|t| t.as_array())
        .context("adult JSON missing temas array")?;
    let tema_title = |i: usize| -> String {
        temas
            .get(i)
            .and_then(|t| t.get("titulo"))
            .and_then(|t| t.as_str())
            .unwrap_or("tema bíblico")
            .to_string()
    };
    let ab_title = |ti: usize, side: &str| -> String {
        let key = if side == "A" { "A" } else { "B" };
        temas
            .get(ti)
            .and_then(|t| t.get(key))
            .and_then(|b| b.get("titulo"))
            .and_then(|t| t.as_str())
            .unwrap_or("punto bíblico")
            .to_string()
    };

    let scenic = |theme: &str| {
        format!(
            "Widescreen 16:9 cinematic biblical illustration for adult church Bible study \
             \"{titulo}\" (estudio {study}). Theme: {theme}. Full-bleed scenic photo/paint; \
             calm left mist for overlay text; subject right/center-right. \
             No text, letters, logos, watermarks, captions, or yellow dashed arcs."
        )
    };
    let def_card = |i: usize| {
        let term = temas
            .get(i)
            .and_then(|t| t.get("definiciones"))
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .and_then(|d| d.get("termino"))
            .and_then(|t| t.as_str())
            .unwrap_or("definición");
        format!(
            "Widescreen 16:9 DEFINICIÓN Y ETIMOLOGÍA educational card graphic for adult \
             Bible study. Clean modern layout with header bar 'DEFINICIÓN Y ETIMOLOGÍA', \
             rows for theological terms (primary term: {term}). Spanish UI text allowed. \
             No watermarks. Theme context: {}.",
            tema_title(i)
        )
    };

    Ok(vec![
        scenic(&format!("introducción — {titulo}")),
        scenic(&tema_title(0)),
        scenic(&tema_title(1)),
        scenic(&tema_title(2)),
        scenic(&ab_title(0, "A")),
        scenic(&ab_title(0, "B")),
        scenic(&ab_title(1, "A")),
        scenic(&ab_title(1, "B")),
        scenic(&ab_title(2, "A")),
        scenic(&ab_title(2, "B")),
        def_card(0),
        def_card(1),
        def_card(2),
    ])
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
