//! OpenAI / ChatGPT review — local file checks + model QA report.
//! Separate from Cursor Cloud review in [`super::review`].

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

use crate::agent::openai::OpenAiClient;
use crate::agent::prepare::expected_paths;
use crate::agent::review::ReviewRequest;
use crate::agent::{AGENTS_MD, REVIEW_MD};
use crate::section_styles::section_style_for_study;

pub async fn run_review_openai(req: &ReviewRequest) -> Result<PathBuf> {
    let aud = req.audience.as_str();
    let report_name = if req.studies.len() == 1 {
        format!("REVIEW-{}.md", req.studies[0])
    } else {
        "REVIEW.md".into()
    };
    let report_path = req
        .root
        .join("studies")
        .join(aud)
        .join(&report_name);

    let mut local_findings = String::new();
    let mut json_blobs = String::new();

    for &n in &req.studies {
        let paths = expected_paths(n, req.audience, &req.root);
        let style = section_style_for_study(n);
        local_findings.push_str(&format!("### Estudio {n} — local checks\n"));

        for (label, p) in [
            ("json", &paths.json),
            ("section1", &paths.img1),
            ("section2", &paths.img2),
            ("section3", &paths.img3),
        ] {
            if p.exists() {
                local_findings.push_str(&format!("- {label}: OK `{}`\n", rel(&req.root, p)));
            } else {
                local_findings.push_str(&format!("- {label}: MISSING `{}`\n", rel(&req.root, p)));
            }
        }

        let (pptx, pdf) = find_built_deck(&req.root, n);
        if req.expect_pptx {
            match &pptx {
                Some(p) => local_findings.push_str(&format!("- pptx: OK `{}`\n", rel(&req.root, p))),
                None => local_findings.push_str("- pptx: MISSING\n"),
            }
            match &pdf {
                Some(p) => local_findings.push_str(&format!("- pdf: OK `{}`\n", rel(&req.root, p))),
                None => local_findings.push_str("- pdf: missing (optional on non-macOS)\n"),
            }
        } else {
            local_findings.push_str("- prepare-only: pptx/pdf not required\n");
        }

        local_findings.push_str(&format!(
            "- assigned section_style.id: `{}`\n\n",
            style.id
        ));

        if paths.json.exists() {
            let text = std::fs::read_to_string(&paths.json)?;
            // Keep payload bounded for the model.
            let clipped = if text.len() > 60_000 {
                format!("{}…\n[truncated]", &text[..60_000])
            } else {
                text
            };
            json_blobs.push_str(&format!("### `{n}.json`\n```json\n{clipped}\n```\n\n"));
        }
    }

    let system = "You are QA for Mount Zion Church Bible-study decks. \
                  Write a markdown review report. End with exactly one line: \
                  REVIEW_STATUS: PASS or REVIEW_STATUS: FAIL.";

    let user = format!(
        r#"Audience={aud}. Review the studies below using REVIEW.md rules and AGENTS.md.

{expect}
{pdf_line}

## Local file checks (authoritative for existence)
{local_findings}

## Study JSON contents
{json_blobs}

## Output
Write the full markdown report (checklist + notes).
End with: `REVIEW_STATUS: PASS` or `REVIEW_STATUS: FAIL`.
Fail if any required file is MISSING, section_style.id mismatches the assigned id,
JSON is missing required fields, or content clearly violates HARD rules.

---
# REVIEW.md

{review}

---
# AGENTS.md

{agents}
"#,
        expect = if req.expect_pptx {
            "Expect finished bible-studies PPTX (+ PDF when present)."
        } else {
            "Prepare-only: JSON + section images required; pptx/pdf optional."
        },
        pdf_line = req
            .source_pdf
            .as_ref()
            .map(|p| format!("Source PDF: `{}`", p.display()))
            .unwrap_or_else(|| "No source PDF path provided.".into()),
        review = REVIEW_MD,
        agents = AGENTS_MD,
    );

    println!("  ChatGPT review (model={})", req.model);
    let client = OpenAiClient::new(&req.api_key)?;
    let report = client
        .chat_text(&req.model, system, &user)
        .await
        .context("OpenAI review chat")?;

    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, &report)
        .with_context(|| format!("write {}", report_path.display()))?;

    let status = read_review_status(&report_path)?;
    if status != "PASS" {
        bail!(
            "review reported {status}. See {}",
            report_path.display()
        );
    }
    Ok(report_path)
}

fn find_built_deck(root: &Path, study: u32) -> (Option<PathBuf>, Option<PathBuf>) {
    let bs = root.join("bible-studies");
    if !bs.is_dir() {
        return (None, None);
    }
    let mut pptx = None;
    if let Ok(rd) = std::fs::read_dir(&bs) {
        let mut matches: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("pptx")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&format!("{study} - ")) && !n.starts_with("~$"))
                        .unwrap_or(false)
            })
            .collect();
        matches.sort();
        matches.sort_by_key(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.contains("_v1"))
                .unwrap_or(false)
        });
        pptx = matches.into_iter().next();
    }
    let pdf = pptx
        .as_ref()
        .map(|p| p.with_extension("pdf"))
        .filter(|p| p.exists());
    (pptx, pdf)
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn read_review_status(report: &Path) -> Result<String> {
    let text = std::fs::read_to_string(report).context("read review report")?;
    let re = Regex::new(r"(?i)REVIEW_STATUS:\s*(PASS|FAIL)").unwrap();
    if let Some(c) = re.captures(&text) {
        return Ok(c[1].to_uppercase());
    }
    if text.contains("FAIL") {
        return Ok("FAIL".into());
    }
    if text.contains("PASS") {
        return Ok("PASS".into());
    }
    Ok("UNKNOWN".into())
}
