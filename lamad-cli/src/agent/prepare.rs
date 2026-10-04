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
    /// OpenAI Images model; ignored for Cursor.
    pub image_model: String,
    pub api_key: String,
    pub stream: bool,
    pub root: PathBuf,
    /// From `config.toml` `pdftoppm_path` (optional).
    pub pdftoppm_path: Option<PathBuf>,
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

    let prompt = build_cloud_prepare_prompt(&req)?;
    let client = CursorClient::new(&req.api_key)?;
    let create_spin = crate::progress::Spinner::start(
        "Starting Cursor cloud agent (upload + create)…",
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
    client
        .wait_run(&created.agent.id, &created.run.id, req.stream)
        .await?;

    // Download artifacts into studies/{aud}/
    let mut artifacts = client.list_artifacts(&created.agent.id).await?;
    if artifacts.is_empty() {
        // Agents sometimes narrate a plan and finish without writing files.
        // One follow-up in agent mode usually recovers.
        crate::progress::warn(
            "no artifacts yet — sending follow-up to write JSON + 3 PNGs…",
        );
        let nudge = format!(
            "STOP. You finished without writing any files under `artifacts/`.\n\n\
             The host CLI lists artifacts via the Cloud Agents API and found **zero**. \
             Narrating the JSON or describing images does **not** count.\n\n\
             Right now, using Write / Shell / image tools, create these exact paths \
             (mkdir -p artifacts first if needed):\n\
             1. artifacts/{study}.json   ← must be a real .json file at this path\n\
             2. artifacts/{study}-section1.png\n\
             3. artifacts/{study}-section2.png\n\
             4. artifacts/{study}-section3.png\n\n\
             Do not put the JSON only under artifacts/assets/. \
             Do not end the turn until `ls artifacts/` shows the .json and three PNGs. \
             Confirm the four paths when done.",
            study = req.study
        );
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
             The agent must write JSON + 3 PNGs under artifacts/. \
             Do not fall back to the Python CLI. Agent id: {}",
            created.agent.id
        );
    }

    // PNGs without JSON is a common miss — retry until listed or downloadable.
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
    std::fs::create_dir_all(dest.img1.parent().unwrap())?;

    let resolved = resolve_artifacts(req.study, &artifacts, &dest)?;
    crate::progress::phase(format!(
        "downloading {} artifact{}…",
        resolved.len(),
        if resolved.len() == 1 { "" } else { "s" }
    ));
    for (art_path, dest_path) in &resolved {
        client
            .download_artifact(&created.agent.id, art_path, dest_path)
            .await?;
        // Agents sometimes write .jpg; destinations are always .png — normalize.
        if dest_path.extension().and_then(|e| e.to_str()) == Some("png") {
            ensure_png_file(dest_path)?;
        }
    }

    // Stamp audience
    stamp_audience(&dest.json, req.audience)?;

    let _ = pdf_page_count; // silence if unused in some builds
    verify_deliverables(req.study, req.audience, &req.root)
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
