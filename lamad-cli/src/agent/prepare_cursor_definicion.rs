//! Cursor-only DEFINICIÓN cards: edit the locked gold PNG (no OpenAI required).
//!
//! Default path when `openai_api_key` is empty. Mirrors OpenAI `images/edits`:
//! attach the gold reference + locked-layout prompt; agent writes
//! `definicion-N.png` into artifacts. Host QC + `.terms` sidecar required.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::agent::client::{self, CursorClient};
use crate::agent::prepare::{adult_media_dir, PrepareRequest};
use crate::build::adult::definicion_card::{
    assert_study_definicion_card, locked_layout_edit_prompt, primary_gold_reference,
    terms_fingerprint, write_terms_sidecar,
};
use crate::content;
use crate::model::adult_study::AdultStudy;
use crate::Audience;

/// Force-regenerate DEFINICIÓN cards via a Cursor Cloud Agent that **edits**
/// the locked gold PNG (same intent as OpenAI images/edits).
pub async fn regenerate_adult_definicion_cards_cursor(req: &PrepareRequest) -> Result<()> {
    if req.audience != Audience::Adult {
        bail!("Cursor DEFINICIÓN gold-edit is adult-only");
    }
    let model = load_adult_model(req)?;
    let needed: Vec<(usize, String, Vec<(String, String)>)> = model
        .temas
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.definiciones.is_empty())
        .map(|(i, t)| {
            let rows: Vec<(String, String)> = t
                .definiciones
                .iter()
                .map(|d| (d.termino.clone(), d.texto.clone()))
                .collect();
            (i + 1, t.titulo.clone(), rows)
        })
        .collect();
    if needed.is_empty() {
        crate::progress::ok("no Definiciones boxes in this estudio — nothing to bake");
        return Ok(());
    }

    let gold = primary_gold_reference()?;
    let gold_img = client::image_from_path(&gold)
        .with_context(|| format!("encode gold {}", gold.display()))?;

    let prompt = build_cursor_gold_edit_prompt(req.study, &needed);
    crate::progress::phase(format!(
        "DEFINICIÓN: Cursor gold-edit (estudio {}, {} card(s)) — no openai_api_key",
        req.study,
        needed.len()
    ));

    let client = CursorClient::new(req.api_key.clone())?;
    let create_spin = crate::progress::Spinner::start(
        "Step — Starting Cursor agent to edit DEFINICIÓN gold PNG…",
    );
    let created = match client
        .create_agent(
            &prompt,
            &[gold_img],
            &req.model,
            &format!("lamad-definicion-{}", req.study),
        )
        .await
    {
        Ok(c) => {
            create_spin.succeed(format!("agent {} · run {}", c.agent.id, c.run.id));
            c
        }
        Err(e) => {
            create_spin.fail("failed to start Cursor DEFINICIÓN agent");
            return Err(e);
        }
    };

    client
        .wait_run(&created.agent.id, &created.run.id, req.stream)
        .await?;

    let mut artifacts = client.list_artifacts(&created.agent.id).await?;
    if !has_any_definicion_artifact(&artifacts, &needed) {
        crate::progress::warn(
            "no definición PNGs in artifacts yet — sending follow-up nudge…",
        );
        let nudge = format!(
            "You still have not written the DEFINICIÓN card PNG(s). \
             The gold reference image was attached on the first message — EDIT that layout. \
             Write under `artifacts/{study}/images/` (or `artifacts/`):\n{list}\n\
             Each file ≈1408×768 PNG. Change ONLY chevron terms, definition bodies, and icons. \
             Do not invent a new layout.",
            study = req.study,
            list = needed
                .iter()
                .map(|(n, _, _)| format!("- definicion-{n}.png"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let follow = client
            .create_followup(&created.agent.id, &nudge)
            .await?;
        client
            .wait_run(&created.agent.id, &follow.run.id, req.stream)
            .await?;
        artifacts = client.list_artifacts(&created.agent.id).await?;
    }

    let media = adult_media_dir(req.study, &req.root);
    std::fs::create_dir_all(&media)?;
    let mut written = 0usize;
    for (n, _titulo, rows) in &needed {
        let dest = media.join(format!("definicion-{n}.png"));
        let art_path = find_definicion_artifact(&artifacts, *n).ok_or_else(|| {
            anyhow::anyhow!(
                "Cursor agent did not produce definicion-{n}.png — artifacts: {:?}",
                artifacts.iter().map(|a| a.path.as_str()).collect::<Vec<_>>()
            )
        })?;
        client
            .download_artifact(&created.agent.id, art_path.as_str(), &dest)
            .await
            .with_context(|| format!("download {art_path}"))?;
        // Drop stale sidecars / placeholder marks before QC.
        let _ = std::fs::remove_file(PathBuf::from(format!("{}.placeholder", dest.display())));
        let terms_side = dest.with_extension("terms");
        let _ = std::fs::remove_file(&terms_side);
        let row_refs: Vec<(&str, &str)> = rows
            .iter()
            .map(|(t, b)| (t.as_str(), b.as_str()))
            .collect();
        // Layout first — then stamp THIS estudio's terms fingerprint.
        if let Err(layout_e) = crate::build::adult::assert_locked_definicion_card(&dest) {
            let _ = std::fs::remove_file(&dest);
            return Err(layout_e).context(format!(
                "Cursor DEFINICIÓN definicion-{n}.png failed locked-layout QC \
                 (must edit the attached gold PNG, not invent a new card)"
            ));
        }
        let fp = terms_fingerprint(row_refs.iter().copied());
        write_terms_sidecar(&dest, &fp)?;
        assert_study_definicion_card(&dest, row_refs.iter().copied()).with_context(|| {
            format!("Cursor DEFINICIÓN definicion-{n}.png failed study-terms QC")
        })?;
        content::note_real_image(&dest);
        written += 1;
        crate::progress::ok(format!("DEFINICIÓN card ready → {}", dest.display()));
    }

    crate::progress::ok(format!(
        "DEFINICIÓN: {written} card(s) via Cursor gold-edit (estudio {})",
        req.study
    ));
    Ok(())
}

fn load_adult_model(req: &PrepareRequest) -> Result<AdultStudy> {
    if content::has_adult_sections(req.study) {
        return content::load_adult_sections(req.study);
    }
    let json = crate::agent::prepare::expected_paths(req.study, req.audience, &req.root).json;
    let text = std::fs::read_to_string(&json)
        .with_context(|| format!("read {}", json.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", json.display()))
}

fn build_cursor_gold_edit_prompt(
    study: u32,
    cards: &[(usize, String, Vec<(String, String)>)],
) -> String {
    let mut body = String::new();
    for (n, titulo, rows) in cards {
        let block: String = rows
            .iter()
            .enumerate()
            .map(|(i, (t, x))| format!("  {}. «{t}» — {x}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        body.push_str(&format!(
            "\n### Card definicion-{n}.png — Tema: {titulo}\n{edit}\n",
            edit = locked_layout_edit_prompt(titulo, &block)
        ));
        body.push_str(&format!(
            "Write the finished PNG to BOTH of these if possible:\n\
             - `artifacts/{study}/images/definicion-{n}.png`\n\
             - `artifacts/definicion-{n}.png`\n"
        ));
    }
    format!(
        r#"You are editing Mount Zion Church DEFINICIÓN Y ETIMOLOGÍA cards.

HARD — the FIRST attached image is the locked GOLD reference card.
You must EDIT that image (same layout / chrome / header / white rows / chevrons).
Change ONLY: chevron term text, definition body text, and left/right icons so they
match THIS estudio's printed Definiciones (below). Spanish verbatim — no paraphrase.

FORBIDDEN: inventing a new layout, centered title pill, cartoon hills, play triangles,
parchment essay, cream title-only, copying Amalec/Justicia/Agag leftover text from the
gold when this estudio has different terms.

Estudio {study}. Produce one PNG per card listed (~1408×768).
Do NOT build a .pptx. Do NOT write study body text folders.
{body}
When finished, list the artifact paths you wrote.
"#
    )
}

fn has_any_definicion_artifact(
    artifacts: &[client::Artifact],
    needed: &[(usize, String, Vec<(String, String)>)],
) -> bool {
    needed
        .iter()
        .any(|(n, _, _)| find_definicion_artifact(artifacts, *n).is_some())
}

fn find_definicion_artifact(artifacts: &[client::Artifact], n: usize) -> Option<String> {
    let want = format!("definicion-{n}.png");
    let want_l = want.to_ascii_lowercase();
    artifacts.iter().find_map(|a| {
        let p = a.path.replace('\\', "/");
        let base = Path::new(&p)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if base == want_l || p.to_ascii_lowercase().ends_with(&want_l) {
            Some(a.path.clone())
        } else {
            None
        }
    })
}
