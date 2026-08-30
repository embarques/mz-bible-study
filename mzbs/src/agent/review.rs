//! Cloud review agent → studies/{aud}/REVIEW.md or REVIEW-{N}.md

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

use crate::agent::client::CursorClient;
use crate::agent::prepare::expected_paths;
use crate::agent::{AGENTS_MD, REVIEW_MD};
use crate::job::Audience;
use crate::paths;

pub struct ReviewRequest {
    pub studies: Vec<u32>,
    pub source_pdf: Option<PathBuf>,
    pub expect_pptx: bool,
    pub audience: Audience,
    pub model: String,
    pub api_key: String,
    pub stream: bool,
    pub root: PathBuf,
}

pub async fn run_review_agent(req: ReviewRequest) -> Result<PathBuf> {
    let aud = req.audience.as_str();
    paths::studies_dir(req.audience)?;

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

    let prompt = build_review_prompt(&req, &report_path)?;
    let client = CursorClient::new(&req.api_key)?;
    let created = client
        .create_agent(
            &prompt,
            &[],
            &req.model,
            &format!("mzbs-review-{}", req.studies.first().unwrap_or(&0)),
        )
        .await?;

    println!("  Review agent {} run {}", created.agent.id, created.run.id);
    client
        .wait_run(&created.agent.id, &created.run.id, req.stream)
        .await?;

    // Prefer artifact report; else look on disk (no-repo agents write artifacts)
    let artifacts = client.list_artifacts(&created.agent.id).await.unwrap_or_default();
    for art in &artifacts {
        if art.path.to_lowercase().contains("review") && art.path.ends_with(".md") {
            client
                .download_artifact(&created.agent.id, &art.path, &report_path)
                .await?;
            break;
        }
    }

    if !report_path.exists() {
        // Write a minimal FAIL placeholder so the operator has a path
        std::fs::write(
            &report_path,
            format!(
                "# Review\n\nAgent finished but did not produce {report_name}.\n\nREVIEW_STATUS: FAIL\n"
            ),
        )?;
        bail!(
            "review reported FAIL (missing report artifact). See {}",
            report_path.display()
        );
    }

    let status = read_review_status(&report_path)?;
    if status != "PASS" {
        bail!(
            "review reported {status}. See {}",
            report_path.display()
        );
    }
    Ok(report_path)
}

fn build_review_prompt(req: &ReviewRequest, report_path: &Path) -> Result<String> {
    let aud = req.audience.as_str();
    let root = &req.root;
    let mut rows = String::new();
    for &n in &req.studies {
        let paths = expected_paths(n, req.audience, root);
        let (pptx, pdf) = find_built_deck(root, n);
        rows.push_str(&format!(
            "### Estudio {n}\n\
             - json: `{}`\n\
             - images: `{}`, `{}`, `{}`\n\
             - pptx: `{}`\n\
             - pdf: `{}`\n\n",
            rel(root, &paths.json),
            rel(root, &paths.img1),
            rel(root, &paths.img2),
            rel(root, &paths.img3),
            pptx.map(|p| rel(root, &p)).unwrap_or_else(|| "MISSING".into()),
            pdf.map(|p| rel(root, &p)).unwrap_or_else(|| "MISSING".into()),
        ));
    }

    let expect = if req.expect_pptx {
        "Expect finished `bible-studies/{N} - {TITLE}.pptx` (+ `.pdf` when present)."
    } else {
        "Prepare-only: JSON + section images required; pptx/pdf optional."
    };

    let pdf_line = req
        .source_pdf
        .as_ref()
        .map(|p| format!("- Source PDF: `{}`\n", p.display()))
        .unwrap_or_else(|| "- No source PDF path provided.\n".into());

    Ok(format!(
        r#"You are QA for Monte de Sion Bible-study CLI (audience={aud}).

{expect}
{pdf_line}

## Studies to review
{rows}

## Output
Write the report to artifact path matching `{report}` (also fine as `artifacts/REVIEW.md`).
End with: `REVIEW_STATUS: PASS` or `REVIEW_STATUS: FAIL`.

{review_guide}

---
# AGENTS.md (rules excerpt)

{agents}
"#,
        report = report_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("REVIEW.md"),
        review_guide = REVIEW_MD,
        agents = AGENTS_MD,
    ))
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
        // Prefer non-_v1
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
