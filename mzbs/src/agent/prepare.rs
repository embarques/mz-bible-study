//! Cloud prepare: rasterize pages → no-repo agent → download artifacts.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::agent::client::{self, Artifact, CursorClient};
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

/// Map one artifact path to a local destination. Prefers exact names:
/// `{study}.json`, `{study}-section1.png`, …
///
/// Rules (checked in order):
/// 1. basename equals `{study}.json`, or the path ends with `/{study}.json`.
/// 2. basename equals `{study}-section{1,2,3}.png` (also accept the bare
///    `section{1,2,3}.png`, without the study-number prefix).
///
/// Returns `None` when neither rule matches. Callers should then fall back
/// to positional matching (see [`resolve_artifacts`]) — never guess a JSON
/// artifact positionally, only PNGs.
fn map_artifact(study: u32, art_path: &str, dest: &Deliverables) -> Option<PathBuf> {
    let basename = art_path.rsplit('/').next().unwrap_or(art_path);

    let json_name = format!("{study}.json");
    if basename.eq_ignore_ascii_case(&json_name) || art_path.ends_with(&format!("/{json_name}")) {
        return Some(dest.json.clone());
    }

    for (n, slot) in [(1, &dest.img1), (2, &dest.img2), (3, &dest.img3)] {
        let named = format!("{study}-section{n}.png");
        let bare = format!("section{n}.png");
        if basename.eq_ignore_ascii_case(&named) || basename.eq_ignore_ascii_case(&bare) {
            return Some(slot.clone());
        }
    }

    None
}

/// Assign every downloaded artifact to its local destination, per
/// AGENTS.md's "never double-count" rule.
///
/// - **Pass 1 (exact names):** each artifact is matched via
///   [`map_artifact`]; first match wins a slot, so one artifact can't fill
///   two slots and one slot can't be filled twice.
/// - **Pass 2 (positional fallback):** any of the 3 section-image slots
///   still empty after pass 1 are filled from the *remaining* (unmatched)
///   `.png` artifacts, taken in sorted-path order, lowest-numbered missing
///   slot first. The JSON slot is never guessed positionally — an
///   unresolved JSON is a hard error.
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

    // Pass 2 — fill remaining section-image slots from leftover PNGs.
    let mut leftover_pngs: Vec<&Artifact> = artifacts
        .iter()
        .enumerate()
        .filter(|(idx, art)| !used[*idx] && art.path.to_lowercase().ends_with(".png"))
        .map(|(_, art)| art)
        .collect();
    leftover_pngs.sort_by(|a, b| a.path.cmp(&b.path));
    let mut leftover = leftover_pngs.into_iter();

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
             Artifacts found ({}): {:?}. Expected {study}.json + 3 section PNGs under artifacts/.",
            missing.join(", "),
            found.len(),
            found
        );
    }

    Ok(slots.into_iter().map(|s| s.unwrap()).collect())
}

pub async fn run_prepare_agent(req: PrepareRequest) -> Result<Deliverables> {
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

    let resolved = resolve_artifacts(req.study, &artifacts, &dest)?;
    for (art_path, dest_path) in &resolved {
        println!("  artifact {art_path} → {}", dest_path.display());
        client
            .download_artifact(&created.agent.id, art_path, dest_path)
            .await?;
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
