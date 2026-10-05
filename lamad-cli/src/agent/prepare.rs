//! Cloud prepare: rasterize pages → agent backend → study JSON + section PNGs.
//!
//! Backends: Cursor Cloud Agents (default) or OpenAI ChatGPT
//! ([`super::prepare_openai`]).

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::client::{self, Artifact, CursorClient};
use crate::agent::provider::AgentProvider;
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
    pub provider: AgentProvider,
    pub model: String,
    /// OpenAI Images model for host-side parallel generation.
    pub image_model: String,
    pub api_key: String,
    /// When set, Cursor/ChatGPT prepare uses JSON-only agent + host images.
    pub image_api_key: Option<String>,
    pub stream: bool,
    pub root: PathBuf,
    /// From `config.toml` `pdftoppm_path` (optional).
    pub pdftoppm_path: Option<PathBuf>,
}

impl PrepareRequest {
    /// Fast path: cloud agent writes JSON; host generates images in parallel.
    pub fn host_images(&self) -> bool {
        self.image_api_key
            .as_ref()
            .map(|k| !k.is_empty())
            .unwrap_or(false)
    }
}

pub struct Deliverables {
    pub json: PathBuf,
    pub img1: PathBuf,
    pub img2: PathBuf,
    pub img3: PathBuf,
}

/// Adult scenic / definición PNGs live under `studies/adult/{N}/`.
pub fn adult_media_dir(study: u32, root: &Path) -> PathBuf {
    root.join("studies")
        .join(Audience::Adult.as_str())
        .join(study.to_string())
}

/// Scenic full-bleed art (required for a good adult deck).
pub fn adult_scenic_basenames() -> &'static [&'static str] {
    &[
        "intro-header.png",
        "tema-1.png",
        "tema-2.png",
        "tema-3.png",
        "ab-1A.png",
        "ab-1B.png",
        "ab-2A.png",
        "ab-2B.png",
        "ab-3A.png",
        "ab-3B.png",
    ]
}

/// DEFINICIÓN Y ETIMOLOGÍA cards (preferred; builder can fall back to text).
pub fn adult_definicion_basenames() -> &'static [&'static str] {
    &["definicion-1.png", "definicion-2.png", "definicion-3.png"]
}

/// All adult image basenames (scenic + definición).
pub fn adult_image_basenames() -> &'static [&'static str] {
    &[
        "intro-header.png",
        "tema-1.png",
        "tema-2.png",
        "tema-3.png",
        "ab-1A.png",
        "ab-1B.png",
        "ab-2A.png",
        "ab-2B.png",
        "ab-3A.png",
        "ab-3B.png",
        "definicion-1.png",
        "definicion-2.png",
        "definicion-3.png",
    ]
}

pub fn expected_paths(study: u32, audience: Audience, root: &Path) -> Deliverables {
    let base = root.join("studies").join(audience.as_str());
    match audience {
        Audience::Youth => Deliverables {
            json: base.join(format!("{study}.json")),
            img1: base.join("media").join(format!("{study}-section1.png")),
            img2: base.join("media").join(format!("{study}-section2.png")),
            img3: base.join("media").join(format!("{study}-section3.png")),
        },
        // Review / summary still want three “preview” paths — use intro + first temas.
        Audience::Adult => {
            let media = adult_media_dir(study, root);
            Deliverables {
                json: base.join(format!("{study}.json")),
                img1: media.join("intro-header.png"),
                img2: media.join("tema-1.png"),
                img3: media.join("tema-2.png"),
            }
        }
    }
}

pub fn verify_deliverables(study: u32, audience: Audience, root: &Path) -> Result<Deliverables> {
    match audience {
        Audience::Youth => verify_youth_deliverables(study, root),
        Audience::Adult => verify_adult_deliverables(study, root),
    }
}

/// If a previous run already left valid JSON + images on disk, return them
/// so prepare can skip the cloud agent (the expensive step).
pub fn try_resume_deliverables(
    study: u32,
    audience: Audience,
    root: &Path,
) -> Option<Deliverables> {
    match verify_deliverables(study, audience, root) {
        Ok(d) => Some(d),
        Err(_) => None,
    }
}

/// Valid study JSON on disk (schema OK) — images may still be missing.
pub fn try_resume_json_only(
    study: u32,
    audience: Audience,
    root: &Path,
) -> Option<PathBuf> {
    match verify_json_only(study, audience, root) {
        Ok(p) => Some(p),
        Err(_) => None,
    }
}

/// True when required scenic/section images are present (definición optional for adult).
pub fn required_images_present(study: u32, audience: Audience, root: &Path) -> bool {
    match audience {
        Audience::Youth => {
            let p = expected_paths(study, audience, root);
            p.img1.is_file() && p.img2.is_file() && p.img3.is_file()
        }
        Audience::Adult => {
            let media = adult_media_dir(study, root);
            adult_scenic_basenames()
                .iter()
                .all(|n| media.join(n).is_file())
        }
    }
}

/// Validate JSON schema only (no image check).
pub fn verify_json_only(study: u32, audience: Audience, root: &Path) -> Result<PathBuf> {
    match audience {
        Audience::Youth => {
            let paths = expected_paths(study, Audience::Youth, root);
            if !paths.json.is_file() {
                bail!("deliverable missing: {}", paths.json.display());
            }
            let data: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&paths.json)?)?;
            let puntos = data
                .get("puntos")
                .and_then(|p| p.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if puntos != 3 {
                bail!("{} must have 3 puntos, got {puntos}", paths.json.display());
            }
            Ok(paths.json)
        }
        Audience::Adult => {
            let paths = expected_paths(study, Audience::Adult, root);
            if !paths.json.is_file() {
                bail!("deliverable missing: {}", paths.json.display());
            }
            let text = std::fs::read_to_string(&paths.json)?;
            let study_model: crate::model::adult_study::AdultStudy = serde_json::from_str(&text)
                .map_err(|e| {
                    anyhow::anyhow!("{} is not valid adult study JSON ({e})", paths.json.display())
                })?;
            if study_model.temas.len() != 3 {
                bail!(
                    "{} must have exactly 3 temas, got {}",
                    paths.json.display(),
                    study_model.temas.len()
                );
            }
            Ok(paths.json)
        }
    }
}

/// Human summary of what is already on disk (for resume / missing messages).
pub fn existing_inventory_summary(study: u32, audience: Audience, root: &Path) -> String {
    let paths = expected_paths(study, audience, root);
    let json_ok = paths.json.is_file();
    match audience {
        Audience::Youth => {
            let imgs = [&paths.img1, &paths.img2, &paths.img3]
                .iter()
                .filter(|p| p.is_file())
                .count();
            format!(
                "JSON={} · section images={imgs}/3 ({})",
                if json_ok { "yes" } else { "no" },
                paths.json.display()
            )
        }
        Audience::Adult => {
            let media = adult_media_dir(study, root);
            let scenic = adult_scenic_basenames()
                .iter()
                .filter(|n| media.join(n).is_file())
                .count();
            let defs = adult_definicion_basenames()
                .iter()
                .filter(|n| media.join(n).is_file())
                .count();
            format!(
                "JSON={} · scenic={scenic}/{} · definición={defs}/{} ({})",
                if json_ok { "yes" } else { "no" },
                adult_scenic_basenames().len(),
                adult_definicion_basenames().len(),
                paths.json.display()
            )
        }
    }
}

fn verify_youth_deliverables(study: u32, root: &Path) -> Result<Deliverables> {
    let paths = expected_paths(study, Audience::Youth, root);
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

fn verify_adult_deliverables(study: u32, root: &Path) -> Result<Deliverables> {
    let paths = expected_paths(study, Audience::Adult, root);
    if !paths.json.exists() {
        bail!("deliverable missing: {}", paths.json.display());
    }
    let text = std::fs::read_to_string(&paths.json)?;
    let study_model: crate::model::adult_study::AdultStudy = serde_json::from_str(&text)
        .map_err(|e| {
            anyhow::anyhow!(
                "{} is not valid adult study JSON ({e}). \
                 Need lectura_antifonal, objetivos, pensamiento_central, texto_aureo, \
                 ensenanza, datos_generales, introduccion_slides, temas[3] with A/B, proximo.",
                paths.json.display()
            )
        })?;
    if study_model.numero.to_string() != study.to_string()
        && !study_model.numero.to_string().contains(&study.to_string())
    {
        bail!(
            "{} numero is {:?}, expected {study}",
            paths.json.display(),
            study_model.numero
        );
    }
    if study_model.temas.len() != 3 {
        bail!(
            "{} must have exactly 3 temas, got {}",
            paths.json.display(),
            study_model.temas.len()
        );
    }
    if study_model.objetivos.len() != 3 {
        bail!(
            "{} must have exactly 3 objetivos, got {}",
            paths.json.display(),
            study_model.objetivos.len()
        );
    }

    let media = adult_media_dir(study, root);
    let mut missing_scenic = Vec::new();
    for name in adult_scenic_basenames() {
        let p = media.join(name);
        if !p.exists() {
            missing_scenic.push(p.display().to_string());
        }
    }
    if !missing_scenic.is_empty() {
        bail!(
            "adult estudio {study} missing required scenic images ({}):\n  {}",
            missing_scenic.len(),
            missing_scenic.join("\n  ")
        );
    }
    let mut missing_def = Vec::new();
    for name in adult_definicion_basenames() {
        let p = media.join(name);
        if !p.exists() {
            missing_def.push(name.to_string());
        }
    }
    if !missing_def.is_empty() {
        crate::progress::warn(format!(
            "adult estudio {study}: missing definición cards ({}) — build will use text fallback",
            missing_def.join(", ")
        ));
    }
    Ok(paths)
}

/// Write `scenic_images` + `definicion_images` paths into adult JSON after PNGs land.
pub fn stamp_adult_image_paths(json_path: &Path, study: u32, root: &Path) -> Result<()> {
    let text = std::fs::read_to_string(json_path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("adult JSON root must be an object"))?;

    let rel = |name: &str| -> String {
        format!("studies/adult/{study}/{name}")
    };
    obj.insert(
        "scenic_images".into(),
        serde_json::json!({
            "intro_header": rel("intro-header.png"),
            "tema": [rel("tema-1.png"), rel("tema-2.png"), rel("tema-3.png")],
            "ab": [
                rel("ab-1A.png"), rel("ab-1B.png"),
                rel("ab-2A.png"), rel("ab-2B.png"),
                rel("ab-3A.png"), rel("ab-3B.png")
            ]
        }),
    );
    let media = adult_media_dir(study, root);
    let def_paths: Vec<String> = adult_definicion_basenames()
        .iter()
        .filter(|name| media.join(name).exists())
        .map(|name| rel(name))
        .collect();
    if def_paths.len() == adult_definicion_basenames().len() {
        obj.insert(
            "definicion_images".into(),
            serde_json::Value::Array(def_paths.into_iter().map(serde_json::Value::String).collect()),
        );
    } else {
        obj.remove("definicion_images");
    }
    std::fs::write(json_path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// Map one artifact path to a local destination. Prefers exact names:
/// `{study}.json`, `{study}-section1.png`, …
///
/// Rules (checked in order):
/// 1. basename equals `{study}.json`, or the path ends with `/{study}.json`.
/// 2. basename equals `{study}-section{1,2,3}.{png,jpg,jpeg}` (also accept the
///    bare `section{1,2,3}.{png,jpg,jpeg}`, without the study-number prefix).
///
/// Returns `None` when neither rule matches. Callers should then fall back
/// to positional matching (see [`resolve_artifacts`]) — never guess a JSON
/// artifact positionally, only images.
fn map_artifact(study: u32, art_path: &str, dest: &Deliverables) -> Option<PathBuf> {
    let basename = art_path.rsplit('/').next().unwrap_or(art_path);

    let json_name = format!("{study}.json");
    if basename.eq_ignore_ascii_case(&json_name) || art_path.ends_with(&format!("/{json_name}")) {
        return Some(dest.json.clone());
    }

    for (n, slot) in [(1, &dest.img1), (2, &dest.img2), (3, &dest.img3)] {
        for ext in ["png", "jpg", "jpeg", "webp"] {
            let named = format!("{study}-section{n}.{ext}");
            let bare = format!("section{n}.{ext}");
            if basename.eq_ignore_ascii_case(&named) || basename.eq_ignore_ascii_case(&bare) {
                return Some(slot.clone());
            }
        }
    }

    None
}

fn is_section_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
}

/// Assign every downloaded artifact to its local destination, per
/// AGENTS.md's "never double-count" rule.
///
/// - **Pass 1 (exact names):** each artifact is matched via
///   [`map_artifact`]; first match wins a slot, so one artifact can't fill
///   two slots and one slot can't be filled twice.
/// - **Pass 2 (positional fallback):** any of the 3 section-image slots
///   still empty after pass 1 are filled from the *remaining* (unmatched)
///   image artifacts (png/jpg/jpeg/webp), taken in sorted-path order,
///   lowest-numbered missing slot first. The JSON slot is never guessed
///   positionally — an unresolved JSON is a hard error.
///
/// Returns `(artifact_path, destination)` pairs for every filled slot, or a
/// clear error listing what was found vs. what's still missing.
fn resolve_artifacts(
    study: u32,
    artifacts: &[Artifact],
    dest: &Deliverables,
) -> Result<Vec<(String, PathBuf)>> {
    let slot_names = [
        format!("{study}.json"),
        "section1.png".to_string(),
        "section2.png".to_string(),
        "section3.png".to_string(),
    ];
    let mut slots: [Option<(String, PathBuf)>; 4] = [None, None, None, None];
    let mut used = vec![false; artifacts.len()];

    // Pass 1 — exact-name matches.
    for (idx, art) in artifacts.iter().enumerate() {
        let Some(dest_path) = map_artifact(study, &art.path, dest) else {
            continue;
        };
        let slot = if dest_path == dest.json {
            0
        } else if dest_path == dest.img1 {
            1
        } else if dest_path == dest.img2 {
            2
        } else {
            3
        };
        if slots[slot].is_none() {
            slots[slot] = Some((art.path.clone(), dest_path));
            used[idx] = true;
        }
    }

    // Pass 2 — fill remaining section-image slots from leftover images.
    let mut leftover_imgs: Vec<&Artifact> = artifacts
        .iter()
        .enumerate()
        .filter(|(idx, art)| !used[*idx] && is_section_image_path(&art.path))
        .map(|(_, art)| art)
        .collect();
    leftover_imgs.sort_by(|a, b| a.path.cmp(&b.path));
    let mut leftover = leftover_imgs.into_iter();

    for (slot, dest_slot) in [(1, &dest.img1), (2, &dest.img2), (3, &dest.img3)] {
        if slots[slot].is_some() {
            continue;
        }
        if let Some(art) = leftover.next() {
            slots[slot] = Some((art.path.clone(), dest_slot.clone()));
        }
    }

    let missing: Vec<&str> = slots
        .iter()
        .zip(slot_names.iter())
        .filter(|(a, _)| a.is_none())
        .map(|(_, name)| name.as_str())
        .collect();
    if !missing.is_empty() {
        let found: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        bail!(
            "could not map artifacts to deliverables for estudio {study} — missing: {}. \
             Artifacts found ({}): {:?}. Expected {study}.json + 3 section images \
             (png/jpg) under artifacts/.",
            missing.join(", "),
            found.len(),
            found
        );
    }

    Ok(slots.into_iter().map(|s| s.unwrap()).collect())
}

/// If `path` is not already a PNG (by magic bytes), decode and rewrite as RGB PNG.
/// Cloud agents sometimes emit `.jpg` while destinations are always `*-sectionN.png`.
fn ensure_png_file(path: &Path) -> Result<()> {
    let raw = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    if raw.len() >= 8 && raw.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
        return Ok(());
    }
    let img = image::load_from_memory(&raw)
        .map_err(|e| anyhow::anyhow!("decode image {}: {e}", path.display()))?;
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let mut out = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Png,
    )
    .map_err(|e| anyhow::anyhow!("encode PNG {}: {e}", path.display()))?;
    std::fs::write(path, out).map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    Ok(())
}

pub async fn run_prepare_agent(req: PrepareRequest) -> Result<Deliverables> {
    match req.provider {
        AgentProvider::Cursor => run_prepare_cursor(req).await,
        AgentProvider::ChatGpt => crate::agent::prepare_openai::run_prepare_openai(&req).await,
    }
}

async fn run_prepare_cursor(req: PrepareRequest) -> Result<Deliverables> {
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

    crate::progress::now_step(
        2,
        4,
        "Upload + start agent",
        "Rasterizing pages, uploading scans, creating the Cursor cloud agent (~1 min)…",
    );
    let rasters = rasterize_pages(
        &req.pdf,
        &page_nums,
        &pages_dir,
        req.pdftoppm_path.as_deref(),
    )?;
    crate::progress::phase("encoding page images for the agent…");
    let mut images = Vec::new();
    for r in &rasters {
        images.push(client::image_from_path(r)?);
    }

    let host_images = req.host_images();
    if host_images {
        crate::progress::phase(
            "fast path ON — agent writes JSON only; host will generate images in parallel (OpenAI)",
        );
    } else {
        crate::progress::warn(
            "openai_api_key not set — Cursor will also generate images (slow). \
             Set openai_api_key / OPENAI_API_KEY for the fast path.",
        );
    }

    let prompt = build_cloud_prepare_prompt(&req, host_images)?;
    let client = CursorClient::new(&req.api_key)?;
    let create_spin = crate::progress::Spinner::start(
        "Step 2/4 — Starting Cursor cloud agent (upload + create)… usually ~1 min",
    );
    let created = match client
        .create_agent(
            &prompt,
            &images,
            &req.model,
            &format!("lamad-prepare-{}", req.study),
        )
        .await
    {
        Ok(c) => {
            create_spin.succeed(format!(
                "agent {} · run {}",
                c.agent.id, c.run.id
            ));
            c
        }
        Err(e) => {
            create_spin.fail("failed to start Cursor agent");
            return Err(e);
        }
    };
    crate::progress::ok("Step 2/4 done — agent created");
    crate::progress::now_step(
        3,
        4,
        if host_images {
            "Agent (JSON only)"
        } else {
            "Agent work (longest)"
        },
        if host_images {
            "Cloud agent is writing study JSON only (~1–3 min)…"
        } else {
            match req.audience {
                Audience::Youth => {
                    "Cloud agent is writing JSON + 3 section images (~3–8 min)…"
                }
                Audience::Adult => {
                    "Cloud agent is writing JSON + scenic/definición images (~5–15 min)…"
                }
            }
        },
    );
    let mut agent_progress = crate::agent::wait_progress::CloudAgentProgress::new(
        req.study,
        req.audience,
        host_images,
    );
    client
        .wait_run_with_progress(
            &created.agent.id,
            &created.run.id,
            req.stream,
            Some(&mut agent_progress),
        )
        .await?;

    crate::progress::ok(if host_images {
        "Step 3/4 — agent finished (JSON)"
    } else {
        "Step 3/4 done — agent finished"
    });

    let mut artifacts = client.list_artifacts(&created.agent.id).await?;
    if artifacts.is_empty() {
        crate::progress::warn(if host_images {
            "no artifacts yet — sending follow-up to write JSON…"
        } else {
            "no artifacts yet — sending follow-up to write JSON + images…"
        });
        let nudge = json_followup_nudge(req.study, req.audience, host_images);
        let follow = client
            .create_followup(&created.agent.id, &nudge)
            .await?;
        println!("  follow-up run {}", follow.run.id);
        client
            .wait_run(&created.agent.id, &follow.run.id, req.stream)
            .await?;
        artifacts = client.list_artifacts(&created.agent.id).await?;
    }
    if artifacts.is_empty() {
        bail!(
            "cloud agent finished but listed no artifacts (after follow-up). \
             Agent id: {}",
            created.agent.id
        );
    }

    if !artifact_list_has_study_json(req.study, &artifacts) {
        artifacts = ensure_study_json_artifact(
            &client,
            &created.agent.id,
            req.study,
            &artifacts,
            req.stream,
        )
        .await?;
    }

    let dest = expected_paths(req.study, req.audience, &req.root);
    std::fs::create_dir_all(dest.json.parent().unwrap())?;
    if let Some(p) = dest.img1.parent() {
        std::fs::create_dir_all(p)?;
    }

    let resolved = if host_images {
        vec![resolve_json_artifact(req.study, &artifacts, &dest)?]
    } else {
        match req.audience {
            Audience::Youth => resolve_artifacts(req.study, &artifacts, &dest)?,
            Audience::Adult => resolve_adult_artifacts(req.study, &artifacts, &req.root)?,
        }
    };

    crate::progress::now_step(
        4,
        4,
        "Finish",
        if host_images {
            "Downloading JSON, then host parallel images, then PowerPoint…"
        } else {
            "Downloading artifacts, then building PowerPoint (+ PDF if enabled)…"
        },
    );
    crate::progress::phase(format!(
        "downloading {} artifact{} in parallel…",
        resolved.len(),
        if resolved.len() == 1 { "" } else { "s" }
    ));
    download_resolved_artifacts(&client, &created.agent.id, resolved).await?;

    stamp_audience(&dest.json, req.audience)?;
    if req.audience == Audience::Adult {
        stamp_adult_image_paths(&dest.json, req.study, &req.root)?;
    }

    if host_images {
        verify_json_only(req.study, req.audience, &req.root)?;
        crate::agent::prepare_openai::generate_host_images(&req).await?;
    }

    let _ = pdf_page_count;
    verify_deliverables(req.study, req.audience, &req.root)
}

fn json_followup_nudge(study: u32, audience: Audience, host_images: bool) -> String {
    if host_images {
        return format!(
            "STOP. Zero artifacts listed. Write **only** `artifacts/{study}.json` \
             (valid {aud} schema). Do **not** generate images — the host CLI does that. \
             Use Write/Shell tools; chat text does not count.",
            aud = audience.as_str()
        );
    }
    match audience {
        Audience::Youth => format!(
            "STOP. You finished without writing any files under `artifacts/`.\n\n\
             Create these exact paths:\n\
             1. artifacts/{study}.json\n\
             2. artifacts/{study}-section1.png\n\
             3. artifacts/{study}-section2.png\n\
             4. artifacts/{study}-section3.png\n\n\
             Use Write/Shell/image tools — chat text does not count."
        ),
        Audience::Adult => format!(
            "STOP. Zero artifacts listed. Write `artifacts/{study}.json` (adult schema) \
             plus these exact PNG basenames under `artifacts/`: {names}. \
             Use Write/Shell/image tools — chat text does not count.",
            names = adult_image_basenames().join(", ")
        ),
    }
}

fn resolve_json_artifact(
    study: u32,
    artifacts: &[Artifact],
    dest: &Deliverables,
) -> Result<(String, PathBuf)> {
    let json_name = format!("{study}.json");
    for art in artifacts {
        let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
        if basename.eq_ignore_ascii_case(&json_name) || art.path.ends_with(&format!("/{json_name}"))
        {
            return Ok((art.path.clone(), dest.json.clone()));
        }
    }
    bail!(
        "missing artifacts/{study}.json (found {} artifacts)",
        artifacts.len()
    )
}

async fn download_resolved_artifacts(
    client: &CursorClient,
    agent_id: &str,
    resolved: Vec<(String, PathBuf)>,
) -> Result<()> {
    let dl_bar = crate::progress::bar(resolved.len() as u64, "Downloading artifacts");
    {
        use futures_util::stream::{self, StreamExt};
        let results: Vec<Result<()>> = stream::iter(resolved.into_iter())
            .map(|(art_path, dest_path)| {
                let client = client;
                let agent_id = agent_id.to_string();
                let bar = dl_bar.clone();
                async move {
                    if let Some(parent) = dest_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    client
                        .download_artifact_quiet(&agent_id, &art_path, &dest_path)
                        .await?;
                    if dest_path.extension().and_then(|e| e.to_str()) == Some("png") {
                        ensure_png_file(&dest_path)?;
                    }
                    bar.inc(1);
                    Ok(())
                }
            })
            .buffer_unordered(8)
            .collect()
            .await;
        for r in results {
            r?;
        }
    }
    dl_bar.finish_with_message("Downloads complete");
    Ok(())
}

/// Map adult cloud artifacts: `{N}.json` + named scenic/definición images.
/// Accepts `.png`/`.jpg`/`.jpeg`/`.webp` under `artifacts/` or `artifacts/assets/`
/// (host converts to PNG on download). Matching is case-insensitive on the stem
/// (`ab-1A.jpg` → `ab-1A.png`).
fn resolve_adult_artifacts(
    study: u32,
    artifacts: &[Artifact],
    root: &Path,
) -> Result<Vec<(String, PathBuf)>> {
    let dest_json = expected_paths(study, Audience::Adult, root).json;
    let media = adult_media_dir(study, root);
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut used = vec![false; artifacts.len()];

    let json_name = format!("{study}.json");
    for (idx, art) in artifacts.iter().enumerate() {
        let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
        if basename.eq_ignore_ascii_case(&json_name) || art.path.ends_with(&format!("/{json_name}"))
        {
            out.push((art.path.clone(), dest_json.clone()));
            used[idx] = true;
            break;
        }
    }
    if out.is_empty() {
        bail!(
            "adult prepare missing artifacts/{study}.json (found {} artifacts)",
            artifacts.len()
        );
    }

    let mut missing_scenic = Vec::new();
    let mut missing_def = Vec::new();
    for name in adult_image_basenames() {
        let stem = name.trim_end_matches(".png");
        let stem_l = stem.to_lowercase();
        let mut found = None;
        for (idx, art) in artifacts.iter().enumerate() {
            if used[idx] {
                continue;
            }
            let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
            let lower = basename.to_lowercase();
            let (file_stem, ext) = match lower.rsplit_once('.') {
                Some(pair) => pair,
                None => continue,
            };
            if !matches!(ext, "png" | "jpg" | "jpeg" | "webp") {
                continue;
            }
            // Exact stem, or `{study}-stem` prefix from some agents.
            let ok = file_stem == stem_l
                || file_stem == format!("{study}-{stem_l}")
                || file_stem.ends_with(&format!("-{stem_l}"));
            if ok {
                found = Some((idx, art.path.clone()));
                break;
            }
        }
        if let Some((idx, path)) = found {
            used[idx] = true;
            out.push((path, media.join(name)));
        } else if adult_scenic_basenames().contains(&name) {
            missing_scenic.push(name.to_string());
        } else {
            missing_def.push(name.to_string());
        }
    }

    if !missing_scenic.is_empty() {
        let found: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        bail!(
            "adult prepare missing required scenic images for estudio {study}: {}. \
             Artifacts found ({}): {:?}. \
             Tip: JPG under artifacts/assets/ is OK (converted to PNG).",
            missing_scenic.join(", "),
            found.len(),
            found
        );
    }
    if !missing_def.is_empty() {
        crate::progress::warn(format!(
            "adult prepare: no definición cards yet ({}) — continuing; build uses text fallback",
            missing_def.join(", ")
        ));
    }
    Ok(out)
}

pub(crate) fn stamp_audience(json_path: &Path, audience: Audience) -> Result<()> {
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

fn artifact_list_has_study_json(study: u32, artifacts: &[Artifact]) -> bool {
    let json_name = format!("{study}.json");
    artifacts.iter().any(|a| {
        let base = a.path.rsplit('/').next().unwrap_or(a.path.as_str());
        base.eq_ignore_ascii_case(&json_name)
    })
}

fn json_artifact_candidate_paths(study: u32) -> Vec<String> {
    vec![
        format!("artifacts/{study}.json"),
        format!("artifacts/assets/{study}.json"),
    ]
}

/// List artifacts, then probe download for JSON paths the API sometimes omits from List.
async fn refresh_artifacts_with_json_probe(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
) -> Result<Vec<Artifact>> {
    let mut artifacts = client.list_artifacts(agent_id).await?;
    if artifact_list_has_study_json(study, &artifacts) {
        return Ok(artifacts);
    }
    for path in json_artifact_candidate_paths(study) {
        if artifacts.iter().any(|a| a.path == path) {
            continue;
        }
        if client.artifact_downloadable(agent_id, &path).await {
            eprintln!("  found {path} via download probe (omitted from list)");
            artifacts.push(Artifact {
                path,
                size_bytes: None,
            });
            break;
        }
    }
    Ok(artifacts)
}

async fn poll_artifacts_for_json(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
    attempts: usize,
    delay_secs: u64,
) -> Result<Vec<Artifact>> {
    for i in 0..attempts {
        let artifacts = refresh_artifacts_with_json_probe(client, agent_id, study).await?;
        if artifact_list_has_study_json(study, &artifacts) {
            return Ok(artifacts);
        }
        if i + 1 < attempts {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
    }
    refresh_artifacts_with_json_probe(client, agent_id, study).await
}

fn json_followup_prompt(study: u32, artifacts: &[Artifact], attempt: u32) -> String {
    let listed: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    match attempt {
        1 => format!(
            "The Cloud Agents artifact list currently has: {listed:?}\n\n\
             There is **no** `artifacts/{study}.json`. Write that file NOW with the complete \
             study JSON (audience, section_style, section_images, packed slides). \
             Path must be exactly `artifacts/{study}.json` (not under assets/). \
             Do not regenerate PNGs unless missing.",
        ),
        2 => format!(
            "List Artifacts still has no `artifacts/{study}.json` (only: {listed:?}).\n\n\
             The host CLI downloads via the Cloud Agents API — files not in the list cannot be \
             retrieved even if they exist elsewhere in the VM.\n\n\
             Run shell commands NOW to register the JSON:\n\
             ```\n\
             mkdir -p artifacts\n\
             test -f artifacts/{study}.json || cp /agent/artifacts/{study}.json artifacts/{study}.json 2>/dev/null || true\n\
             test -f artifacts/{study}.json || cp /opt/cursor/artifacts/{study}.json artifacts/{study}.json 2>/dev/null || true\n\
             ls -la artifacts/{study}.json\n\
             ```\n\
             If copy fails, use Write to create `artifacts/{study}.json` with the full study JSON. \
             Do not regenerate PNGs.",
        ),
        _ => format!(
            "CRITICAL: `artifacts/{study}.json` is STILL missing from List Artifacts ({listed:?}).\n\n\
             Use the Write tool to overwrite `artifacts/{study}.json` with the complete study JSON. \
             After writing, run `ls -la artifacts/{study}.json` in shell. \
             The file must appear in the artifact API. Do not end until it does.",
        ),
    }
}

/// Follow-up until `{study}.json` is listable or downloadable (up to 3 nudges + polling).
async fn ensure_study_json_artifact(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
    artifacts: &[Artifact],
    stream: bool,
) -> Result<Vec<Artifact>> {
    let mut artifacts = artifacts.to_vec();
    for attempt in 1..=3u32 {
        if artifact_list_has_study_json(study, &artifacts) {
            return Ok(artifacts);
        }
        eprintln!(
            "  Artifacts present but missing {study}.json — JSON follow-up {attempt}/3…"
        );
        let nudge = json_followup_prompt(study, &artifacts, attempt);
        let follow = client.create_followup(agent_id, &nudge).await?;
        println!("  JSON follow-up run {}", follow.run.id);
        client.wait_run(agent_id, &follow.run.id, stream).await?;
        artifacts = poll_artifacts_for_json(client, agent_id, study, 8, 3).await?;
    }
    Ok(artifacts)
}

fn build_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    match req.audience {
        Audience::Youth => build_youth_cloud_prepare_prompt(req, host_images),
        Audience::Adult => build_adult_cloud_prepare_prompt(req, host_images),
    }
}

fn proximo_prompt_block(req: &PrepareRequest) -> Result<String> {
    if req.omit_proximo {
        Ok("Próximo: **omit** (last study — no próximo object, or null).".to_string())
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

fn build_youth_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    let aud = req.audience.as_str();
    let study = req.study;
    let proximo_block = proximo_prompt_block(req)?;
    let style_block = style_prompt_block(study);
    let base = format!("studies/{aud}");

    if host_images {
        return Ok(format!(
            r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church Bible study.

Follow PREPARE_STUDY.md and AGENTS.md (full text below). Do **NOT** build a .pptx.
Do **NOT** generate images — the host CLI generates section PNGs after you finish.

## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Deliverable — ONE file only
Use Write/Shell to create:
1. `artifacts/{study}.json`  ← **required**; not under `artifacts/assets/`

JSON must include `"audience": "{aud}"`, three `puntos`, and the assigned `section_style`.
You may omit `section_images` (host stamps paths). Include `section_style` from the recipe below.

## Section style (for JSON only — do not render images)
{style_block}

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- End as soon as `artifacts/{study}.json` exists. Speed matters — JSON only.

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
        ));
    }

    Ok(format!(
        r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church Bible study.

Follow PREPARE_STUDY.md and AGENTS.md (full text below). Do **NOT** build a .pptx.

## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Deliverables — write under `artifacts/` (mandatory)
You are in **agent mode**. Do **not** only plan or narrate. Use Write / Shell / image tools
to create real files. The host CLI downloads via List Artifacts — if the directory is empty,
the job **fails**. Pasting JSON in chat is not enough.

Exact paths (mkdir -p `artifacts` first if needed):
1. `artifacts/{study}.json`  ← **required**; do **not** put this under `artifacts/assets/`
2. `artifacts/{study}-section1.png` (or `artifacts/assets/{study}-section1.png`)
3. `artifacts/{study}-section2.png` (or `artifacts/assets/{study}-section2.png`)
4. `artifacts/{study}-section3.png` (or `artifacts/assets/{study}-section3.png`)

JSON must include `"audience": "{aud}"`, `section_images` pointing at the three PNG paths
(as `{base}/media/{study}-section{{1,2,3}}.png`), and the assigned `section_style`.

## Section images — HARD
{style_block}

Also: 16:9 ≈1408×768; no text/logos/watermarks/yellow dashed arcs; calm left for title.
Generate the three PNGs with the image tool (or equivalent) and save them to the paths above.

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- Pack body ~360–400 chars; Lectura/Texto = whole verses only.
- End only after artifact list would show `{study}.json` plus three section PNGs.

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

fn build_adult_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    let study = req.study;
    let proximo_block = proximo_prompt_block(req)?;
    let img_list = adult_image_basenames()
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}. `artifacts/{n}`", i + 2))
        .collect::<Vec<_>>()
        .join("\n");

    let schema = r#"## Adult JSON schema (required keys)
```json
{
  "audience": "adult",
  "numero": STUDY,
  "titulo": "…",
  "base_biblica": ["Cita 1;", "Cita 2;", "Cita 3"],
  "lectura_antifonal": [{"cita":"…","versiculos":["1 …","2 …"]}],
  "objetivos": ["…","…","…"],
  "pensamiento_central": "…",
  "texto_aureo": {"texto":"…","cita":"Libro N:N"},
  "ensenanza": "…",
  "datos_generales": {"autor":"…","personajes":"…","fecha":"…","lugar":"…"},
  "introduccion_slides": ["para 1","para 2"],
  "temas": [
    {
      "titulo": "…",
      "rango": "Libro N:N-N",
      "definiciones": [{"termino":"…","texto":"…","referencia":"(Libro N:N)"}],
      "A": {"titulo":"…","texto_slides":["…"],"texto_biblico":{"cita":"…","versiculos":["…"]}},
      "B": {"titulo":"…","texto_slides":["…"],"texto_biblico":{"cita":"…","versiculos":["…"]}}
    }
  ],
  "proximo": {"numero":0,"titulo":"…","base_biblica":["…"]}
}
```
Exactly **3** `temas`, each with A and B. Host stamps `scenic_images` / `definicion_images`."#;
    let schema = schema.replace("STUDY", &study.to_string());

    if host_images {
        return Ok(format!(
            r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church **adult** Bible study.

Follow AGENTS.md adult HARD rules. Do **NOT** build a .pptx.
Do **NOT** generate images — the host CLI generates all scenic/definición PNGs after you finish.
Adult JSON is **not** the youth schema (no `puntos` / `section_images` / `section_style`).

## Inputs
- Estudio **{study}** (audience=adult)
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Deliverable — ONE file only
Use Write/Shell to create:
1. `artifacts/{study}.json`  ← required adult schema

{schema}

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; skip Ideas para el maestro / Preguntas.
- Lectura/Texto = whole verses.
- End as soon as `artifacts/{study}.json` exists. Speed matters — JSON only, no images.

---
# AGENTS.md (rules)

{agents}
"#,
            p0 = req.pages.0,
            p1 = req.pages.1,
            agents = AGENTS_MD,
        ));
    }

    Ok(format!(
        r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church **adult** Bible study.

Follow AGENTS.md adult HARD rules and LAYOUT_GUIDE ideas below. Do **NOT** build a .pptx.
Adult JSON is **not** the youth schema (no `puntos` / `section_images` / `section_style`).

## Inputs
- Estudio **{study}** (audience=adult)
- Content pages **{p0}–{p1}** are attached as images (in order).
- {proximo_block}

## Deliverables — write under `artifacts/` (mandatory)
Use Write / Shell / image tools. Host CLI downloads via List Artifacts.

1. `artifacts/{study}.json`  ← required adult schema (see below)
{img_list}

Image paths may also be `artifacts/assets/<name>.png` **or `.jpg`** (host converts to PNG).
Prefer PNG; JPG is accepted. **Scenic images (intro/tema/ab) are required**; definición cards are strongly preferred.

{schema}

## Images — HARD (13 PNGs, exact basenames)
16:9 ≈1408×768. No text/letters/logos/watermarks/yellow dashed arcs.
- `intro-header.png` — cinematic intro full-bleed
- `tema-1.png` … `tema-3.png` — one scenic per tema (different scenes)
- `ab-1A.png` … `ab-3B.png` — six scenic A/B title slides
- `definicion-1.png` … `definicion-3.png` — DEFINICIÓN Y ETIMOLOGÍA card graphics (header + term rows; readable Spanish text **is** allowed on these three definición cards only)

## Rules
- Faithful Spanish from the scans; merge cross-page cuts; skip Ideas para el maestro / Preguntas.
- Lectura/Texto = whole verses; body slides ~360–400 chars when packing.
- End only after `{study}.json` + all 13 PNGs exist under `artifacts/`.

---
# AGENTS.md (rules)

{agents}
"#,
        p0 = req.pages.0,
        p1 = req.pages.1,
        agents = AGENTS_MD,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_dest() -> Deliverables {
        Deliverables {
            json: PathBuf::from("/root/studies/youth/17.json"),
            img1: PathBuf::from("/root/studies/youth/media/17-section1.png"),
            img2: PathBuf::from("/root/studies/youth/media/17-section2.png"),
            img3: PathBuf::from("/root/studies/youth/media/17-section3.png"),
        }
    }

    fn art(path: &str) -> Artifact {
        Artifact {
            path: path.to_string(),
            size_bytes: None,
        }
    }

    #[test]
    fn maps_adult_jpg_assets_case_insensitive() {
        let root = PathBuf::from("/repo");
        let arts = vec![
            art("artifacts/4.json"),
            art("artifacts/assets/intro-header.jpg"),
            art("artifacts/assets/tema-1.jpg"),
            art("artifacts/assets/tema-2.jpg"),
            art("artifacts/assets/tema-3.jpg"),
            art("artifacts/assets/ab-1A.jpg"),
            art("artifacts/assets/ab-1B.jpg"),
            art("artifacts/assets/ab-2A.jpg"),
            art("artifacts/assets/ab-2B.jpg"),
            art("artifacts/assets/ab-3A.jpg"),
            art("artifacts/assets/ab-3B.jpg"),
            // no definición — should warn but succeed
        ];
        let resolved = resolve_adult_artifacts(4, &arts, &root).expect("map");
        assert_eq!(resolved.len(), 1 + adult_scenic_basenames().len());
        assert!(resolved.iter().any(|(_, d)| d.ends_with("ab-1A.png")));
        assert!(resolved.iter().any(|(src, _)| src.ends_with("ab-1A.jpg")));
    }

    #[test]
    fn parses_v1_artifact_list_items_and_size_bytes() {
        let raw = r#"{
          "items": [
            {"path": "artifacts/assets/23-section1.png", "sizeBytes": 12, "updatedAt": "2026-01-01T00:00:00.000Z"},
            {"path": "artifacts/23.json", "sizeBytes": 99}
          ]
        }"#;
        let list: crate::agent::client::ArtifactList =
            serde_json::from_str(raw).expect("parse");
        assert_eq!(list.artifacts.len(), 2);
        assert_eq!(list.artifacts[0].path, "artifacts/assets/23-section1.png");
        assert_eq!(list.artifacts[0].size_bytes, Some(12));
        assert_eq!(list.artifacts[1].path, "artifacts/23.json");
        assert_eq!(list.artifacts[1].size_bytes, Some(99));
    }

    #[test]
    fn map_artifact_matches_assets_subdir_pngs() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/assets/17-section2.png", &dest),
            Some(dest.img2.clone())
        );
    }

    #[test]
    fn artifact_list_detects_json_basename() {
        assert!(artifact_list_has_study_json(
            23,
            &[art("artifacts/23.json")]
        ));
        assert!(artifact_list_has_study_json(
            23,
            &[art("artifacts/assets/23.json")]
        ));
        assert!(!artifact_list_has_study_json(
            23,
            &[art("artifacts/assets/23-section1.png")]
        ));
    }

    #[test]
    fn map_artifact_matches_exact_json_name() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/17.json", &dest),
            Some(dest.json.clone())
        );
        assert_eq!(
            map_artifact(17, "17.json", &dest),
            Some(dest.json.clone())
        );
    }

    #[test]
    fn map_artifact_rejects_json_for_a_different_study() {
        let dest = fake_dest();
        assert_eq!(map_artifact(17, "artifacts/18.json", &dest), None);
    }

    #[test]
    fn map_artifact_matches_exact_and_bare_section_names() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/17-section1.png", &dest),
            Some(dest.img1.clone())
        );
        assert_eq!(
            map_artifact(17, "artifacts/section2.png", &dest),
            Some(dest.img2.clone())
        );
        assert_eq!(
            map_artifact(17, "artifacts/17-section3.png", &dest),
            Some(dest.img3.clone())
        );
        // JPG (common agent miss) maps to the same PNG destinations.
        assert_eq!(
            map_artifact(1, "artifacts/assets/1-section1.jpg", &dest),
            Some(dest.img1.clone())
        );
        assert_eq!(
            map_artifact(1, "artifacts/assets/1-section2.jpeg", &dest),
            Some(dest.img2.clone())
        );
    }

    #[test]
    fn map_artifact_rejects_unrelated_names() {
        let dest = fake_dest();
        assert_eq!(map_artifact(17, "artifacts/random.png", &dest), None);
        assert_eq!(map_artifact(17, "artifacts/notes.txt", &dest), None);
    }

    #[test]
    fn resolve_artifacts_prefers_exact_names() {
        let dest = fake_dest();
        let artifacts = vec![
            art("artifacts/17-section2.png"),
            art("artifacts/17.json"),
            art("artifacts/17-section1.png"),
            art("artifacts/17-section3.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        assert_eq!(resolved.len(), 4);
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/17.json"], dest.json);
        assert_eq!(by_dest["artifacts/17-section1.png"], dest.img1);
        assert_eq!(by_dest["artifacts/17-section2.png"], dest.img2);
        assert_eq!(by_dest["artifacts/17-section3.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_falls_back_positionally_for_unnamed_pngs() {
        let dest = fake_dest();
        // No study-number prefix anywhere — must fall back to sorted order.
        let artifacts = vec![
            art("artifacts/17.json"),
            art("artifacts/b.png"),
            art("artifacts/a.png"),
            art("artifacts/c.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/a.png"], dest.img1);
        assert_eq!(by_dest["artifacts/b.png"], dest.img2);
        assert_eq!(by_dest["artifacts/c.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_never_double_counts_an_exact_match() {
        let dest = fake_dest();
        // `17-section1.png` should fill img1 exactly once; the leftover
        // unnamed PNG should NOT also be assigned to img1.
        let artifacts = vec![
            art("artifacts/17.json"),
            art("artifacts/17-section1.png"),
            art("artifacts/leftover.png"),
            art("artifacts/17-section3.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/17-section1.png"], dest.img1);
        assert_eq!(by_dest["artifacts/leftover.png"], dest.img2);
        assert_eq!(by_dest["artifacts/17-section3.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_accepts_jpg_section_images() {
        let dest = fake_dest();
        let artifacts = vec![
            art("artifacts/1.json"),
            art("artifacts/assets/1-section1.jpg"),
            art("artifacts/assets/1-section2.jpg"),
            art("artifacts/assets/1-section3.jpg"),
        ];
        // Remap fake_dest study number: map_artifact uses study arg, not dest paths.
        let resolved = resolve_artifacts(1, &artifacts, &dest).expect("resolves jpg");
        assert_eq!(resolved.len(), 4);
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/assets/1-section1.jpg"], dest.img1);
        assert_eq!(by_dest["artifacts/assets/1-section2.jpg"], dest.img2);
        assert_eq!(by_dest["artifacts/assets/1-section3.jpg"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_errors_with_clear_found_vs_expected() {
        let dest = fake_dest();
        let artifacts = vec![art("artifacts/17.json"), art("artifacts/17-section1.png")];
        let err = resolve_artifacts(17, &artifacts, &dest).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("section2.png"), "{msg}");
        assert!(msg.contains("section3.png"), "{msg}");
        assert!(msg.contains("17-section1.png"), "{msg}");
    }
}
