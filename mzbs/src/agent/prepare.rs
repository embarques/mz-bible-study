//! Cloud prepare: rasterize pages → no-repo agent → download artifacts.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::agent::client::{self, CursorClient};
use crate::agent::{AGENTS_MD, PREPARE_STUDY_MD};
use crate::job::Audience;
use crate::paths;
use crate::section_styles::{section_style_for_study, style_prompt_block};
use crate::{pdf_page_count, rasterize_pages};

pub struct PrepareRequest {
    pub study: u32,
    pub pdf: PathBuf,
    pub pages: (u32, u32),
    pub omit_proximo: bool,
    pub next_pages: Option<(u32, u32)>,
    pub audience: Audience,
    pub model: String,
    pub api_key: String,
    pub stream: bool,
    pub root: PathBuf,
}

pub struct Deliverables {
    pub json: PathBuf,
    pub img1: PathBuf,
    pub img2: PathBuf,
    pub img3: PathBuf,
}

pub fn expected_paths(study: u32, audience: Audience, root: &Path) -> Deliverables {
    let base = root.join("studies").join(audience.as_str());
    Deliverables {
        json: base.join(format!("{study}.json")),
        img1: base.join("media").join(format!("{study}-section1.png")),
        img2: base.join("media").join(format!("{study}-section2.png")),
        img3: base.join("media").join(format!("{study}-section3.png")),
    }
}

pub fn verify_deliverables(study: u32, audience: Audience, root: &Path) -> Result<Deliverables> {
    let paths = expected_paths(study, audience, root);
    for p in [&paths.json, &paths.img1, &paths.img2, &paths.img3] {
        if !p.exists() {
            bail!("deliverable missing: {}", p.display());
        }
    }
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&paths.json)?)?;
    if data.get("numero").and_then(|v| v.as_u64()) != Some(study as u64)
        && data.get("numero").and_then(|v| v.as_i64()) != Some(study as i64)
    {
        // also accept string numero
        let ok = data
            .get("numero")
            .map(|v| v.to_string().contains(&study.to_string()))
            .unwrap_or(false);
        if !ok {
            bail!(
                "{} numero is {:?}, expected {study}",
                paths.json.display(),
                data.get("numero")
            );
        }
    }
    let imgs = data
        .get("section_images")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if imgs != 3 {
        bail!(
            "{} must list 3 section_images, got {imgs}",
            paths.json.display()
        );
    }
    let expected = section_style_for_study(study);
    let id = data
        .pointer("/section_style/id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if id != expected.id {
        bail!(
            "{} must include section_style.id={:?} (assigned for estudio {study}); got {:?}",
            paths.json.display(),
            expected.id,
            id
        );
    }
    Ok(paths)
}

pub async fn run_prepare_agent(req: PrepareRequest) -> Result<Deliverables> {
    let aud = req.audience.as_str();
    paths::studies_dir(req.audience)?;
    let pages_dir = paths::generated_dir()?.join("pages").join(format!("study{}", req.study));
    std::fs::create_dir_all(&pages_dir)?;

    // Rasterize content pages (+ próximo title page when needed), max 5 images.
    let mut page_nums: Vec<u32> = (req.pages.0..=req.pages.1).collect();
    if let Some((na, _)) = req.next_pages {
        if !page_nums.contains(&na) && page_nums.len() < 5 {
            page_nums.push(na);
        }
    }
    // Cap at 5
    page_nums.truncate(5);

    let rasters = rasterize_pages(&req.pdf, &page_nums, &pages_dir)?;
    let mut images = Vec::new();
    for r in &rasters {
        images.push(client::image_from_path(r)?);
    }

    let prompt = build_cloud_prepare_prompt(&req)?;
    let client = CursorClient::new(&req.api_key)?;
    let created = client
        .create_agent(
            &prompt,
            &images,
            &req.model,
            &format!("mzbs-prepare-{}", req.study),
        )
        .await?;

    println!(
        "  Cloud agent {} run {}",
        created.agent.id, created.run.id
    );
    client
        .wait_run(&created.agent.id, &created.run.id, req.stream)
        .await?;

    // Download artifacts into studies/{aud}/
    let artifacts = client.list_artifacts(&created.agent.id).await?;
    if artifacts.is_empty() {
        bail!(
            "cloud agent finished but listed no artifacts. \
             The agent must write JSON + 3 PNGs under artifacts/. \
             Do not fall back to the Python CLI."
        );
    }

    let dest = expected_paths(req.study, req.audience, &req.root);
    std::fs::create_dir_all(dest.json.parent().unwrap())?;
    std::fs::create_dir_all(dest.img1.parent().unwrap())?;

    let mut got_json = false;
    let mut got_imgs = 0u32;
    for art in &artifacts {
        let lower = art.path.to_lowercase();
        let dest_path = if lower.ends_with(".json") || lower.contains(&format!("{}.json", req.study))
        {
            got_json = true;
            dest.json.clone()
        } else if lower.contains("section1") || lower.contains("-section-1") {
            got_imgs += 1;
            dest.img1.clone()
        } else if lower.contains("section2") || lower.contains("-section-2") {
            got_imgs += 1;
            dest.img2.clone()
        } else if lower.contains("section3") || lower.contains("-section-3") {
            got_imgs += 1;
            dest.img3.clone()
        } else if lower.ends_with(".png") {
            // fallback order by count
            got_imgs += 1;
            match got_imgs {
                1 => dest.img1.clone(),
                2 => dest.img2.clone(),
                3 => dest.img3.clone(),
                _ => continue,
            }
        } else {
            continue;
        };
        println!("  artifact {} → {}", art.path, dest_path.display());
        client
            .download_artifact(&created.agent.id, &art.path, &dest_path)
            .await?;
    }

    if !got_json || got_imgs < 3 {
        bail!(
            "incomplete artifacts (json={got_json}, pngs≈{got_imgs}). \
             Expected studies/{aud}/{}.json + 3 section PNGs under artifacts/.",
            req.study
        );
    }

    // Stamp audience
    stamp_audience(&dest.json, req.audience)?;

    let _ = pdf_page_count; // silence if unused in some builds
    verify_deliverables(req.study, req.audience, &req.root)
}

fn stamp_audience(json_path: &Path, audience: Audience) -> Result<()> {
    let text = std::fs::read_to_string(json_path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "audience".into(),
            serde_json::Value::String(audience.as_str().into()),
        );
    }
    std::fs::write(json_path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

fn build_cloud_prepare_prompt(req: &PrepareRequest) -> Result<String> {
    let aud = req.audience.as_str();
    let study = req.study;
    let proximo_block = if req.omit_proximo {
        "Próximo: **omit** (last study — no próximo object, or null).".to_string()
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

    Ok(format!(
        r#"You are a no-repo Cursor Cloud Agent preparing a Monte de Sion Bible study.

Follow PREPARE_STUDY.md and AGENTS.md (full text below). Do **NOT** build a .pptx.

## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Deliverables — write under `artifacts/` (mandatory)
The host CLI will download these. Use these exact names when possible:
1. `artifacts/{study}.json`
2. `artifacts/{study}-section1.png`
3. `artifacts/{study}-section2.png`
4. `artifacts/{study}-section3.png`

JSON must include `"audience": "{aud}"`, `section_images` pointing at the three PNG paths
(as `{base}/media/{study}-section{{1,2,3}}.png`), and the assigned `section_style`.

## Section images — HARD
{style_block}

Also: 16:9 ≈1408×768; no text/logos/watermarks/yellow dashed arcs; calm left for title.

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- Pack body ~360–400 chars; Lectura/Texto = whole verses only.

When finished, confirm the four artifact paths and `section_style.id`.

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
    ))
}
