//! Monte de Sion Bible-study engine — shared by CLI and TUI.
//!
//! **New here?** Read [`../CONTRIBUTING.md`](../../CONTRIBUTING.md) for the
//! module map (“where do I change X?”). Entry points:
//! - [`run`] — prepare → build → review orchestration
//! - [`build::build_study`] — JSON + images → PPTX
//! - [`validate::validate_pptx`] — package integrity

pub mod agent;
pub mod build;
pub mod cli;
pub mod config;
pub mod export;
pub mod job;
pub mod model;
pub mod pack;
pub mod paths;
pub mod scans;
pub mod section_styles;
pub mod tui;
pub mod validate;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::job::{Audience, PrepareJob};

/// Run a full prepare job (agent → optional build → optional review).
pub async fn run(job: PrepareJob, cfg: &Config) -> Result<()> {
    job.echo_plan();

    let pdf = job.resolve_pdf()?;
    let page_count = pdf_page_count(&pdf)?;
    let expected = ((job.to - job.from) + 1) * 3;
    if page_count != expected as usize {
        bail!(
            "PDF page count is {page_count}, but --from {} --to {} needs exactly {expected} pages \
             (3 content pages × {} studies). Check the PDF or the study range.",
            job.from,
            job.to,
            job.to - job.from + 1
        );
    }

    if job.audience == Audience::Adult {
        bail!(
            "audience=adult is not implemented yet. Use --audience youth (default)."
        );
    }

    let root = paths::project_root()?;
    let studies = (job.from..=job.to).collect::<Vec<_>>();
    let mut errors: Vec<String> = Vec::new();

    for n in &studies {
        let is_last = *n == job.to;
        let pages = job::default_pages(*n, job.from);
        let next_pages = if is_last {
            None
        } else {
            Some(job::default_pages(*n + 1, job.from))
        };

        println!(
            "\n=== Estudio {n} pages {}–{}{} ===",
            pages.0,
            pages.1,
            if is_last {
                " (last, no Próximo)".to_string()
            } else {
                let np = next_pages.unwrap();
                format!("; próximo from pages {}–{}", np.0, np.1)
            }
        );

        let style = section_styles::section_style_for_study(*n);
        println!("  Section style: {} — {}", style.id, style.name);

        let one = PrepareOne {
            study: *n,
            pdf: &pdf,
            pages,
            omit_proximo: is_last,
            next_pages,
            job: &job,
            cfg,
            root: &root,
        };
        match prepare_one_study(one).await {
            Ok(()) => {}
            Err(e) => {
                let msg = format!("estudio {n}: {e:#}");
                eprintln!("FAIL: {msg}");
                errors.push(msg);
                if job.stop_on_error {
                    scans::move_to_error(&pdf, &errors.join("\n"))?;
                    bail!("{}", errors.last().unwrap());
                }
            }
        }
    }

    if !errors.is_empty() {
        scans::move_to_error(&pdf, &errors.join("\n"))?;
        bail!("Finished with {} error(s).", errors.len());
    }

    if job.review {
        run_review(&studies, &pdf, !job.prepare_only, &job, cfg, &root).await?;
    }

    scans::move_to_complete(&pdf)?;
    println!("\nDone: {} prepared.", studies.len());
    Ok(())
}

/// Everything one `prepare_one_study` call needs — grouped into a struct so
/// the function signature stays readable (see clippy's `too_many_arguments`).
struct PrepareOne<'a> {
    study: u32,
    pdf: &'a Path,
    pages: (u32, u32),
    /// True on the last study in a batch — no Próximo slide, no next title
    /// page to read from.
    omit_proximo: bool,
    /// Page range of the *next* study's title page, when there is one.
    next_pages: Option<(u32, u32)>,
    job: &'a PrepareJob,
    cfg: &'a Config,
    root: &'a Path,
}

async fn prepare_one_study(one: PrepareOne<'_>) -> Result<()> {
    let PrepareOne {
        study,
        pdf,
        pages,
        omit_proximo,
        next_pages,
        job,
        cfg,
        root,
    } = one;

    let api_key = cfg
        .cursor_api_key
        .as_deref()
        .filter(|s| !s.is_empty())
        .context(
            "No Cursor API key. Set cursor_api_key in config.toml or CURSOR_API_KEY env \
             (see mzbs/config.example.toml).",
        )?;

    let deliverables = agent::prepare::run_prepare_agent(
        agent::prepare::PrepareRequest {
            study,
            pdf: pdf.to_path_buf(),
            pages,
            omit_proximo,
            next_pages,
            audience: job.audience,
            model: cfg.cursor_model.clone(),
            api_key: api_key.to_string(),
            stream: job.stream,
            root: root.to_path_buf(),
        },
    )
    .await?;

    agent::prepare::verify_deliverables(study, job.audience, root)?;

    if job.prepare_only {
        println!(
            "  Prepare OK (prepare-only). Next:\n  {}",
            suggested_build_command(study, job.audience)
        );
        return Ok(());
    }

    let json_path = &deliverables.json;
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(json_path)?)?;
    let title = data
        .get("titulo")
        .and_then(|v| v.as_str())
        .unwrap_or("TITLE");
    let out = root
        .join("bible-studies")
        .join(format!("{study} - {title}.pptx"));
    println!("  Building {}…", out.display());

    let template = job
        .template
        .clone()
        .unwrap_or_else(|| paths::master_template(job.audience).expect("youth template"));

    tokio::task::spawn_blocking({
        let json_path = json_path.clone();
        let out = out.clone();
        let template = template.clone();
        let export_pdf = job.export_pdf;
        let audience = job.audience;
        move || build::build_study(&json_path, &out, Some(&template), audience, export_pdf)
    })
    .await??;

    Ok(())
}

async fn run_review(
    studies: &[u32],
    pdf: &Path,
    expect_pptx: bool,
    job: &PrepareJob,
    cfg: &Config,
    root: &Path,
) -> Result<()> {
    let api_key = cfg
        .cursor_api_key
        .as_deref()
        .filter(|s| !s.is_empty())
        .context("No Cursor API key for review")?;

    println!("\n=== Agent review ===");
    let report = agent::review::run_review_agent(
        agent::review::ReviewRequest {
            studies: studies.to_vec(),
            source_pdf: Some(pdf.to_path_buf()),
            expect_pptx,
            audience: job.audience,
            model: cfg.cursor_model.clone(),
            api_key: api_key.to_string(),
            stream: job.stream,
            root: root.to_path_buf(),
        },
    )
    .await?;
    println!("Review PASS — {}", report.display());
    Ok(())
}

pub fn suggested_build_command(study: u32, audience: Audience) -> String {
    let aud = audience.as_str();
    format!(
        "mzbs build studies/{aud}/{study}.json -o \"bible-studies/{study} - TITLE.pptx\" \
         --audience {aud} --export-pdf"
    )
}

/// Count pages in a PDF (lopdf).
pub fn pdf_page_count(path: &Path) -> Result<usize> {
    let doc = lopdf::Document::load(path)
        .with_context(|| format!("failed to open PDF: {}", path.display()))?;
    Ok(doc.get_pages().len())
}

/// Rasterize PDF pages to PNG via `pdftoppm` when available.
pub fn rasterize_pages(pdf: &Path, pages: &[u32], out_dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let pdftoppm = which_pdftoppm();
    let Some(bin) = pdftoppm else {
        bail!(
            "pdftoppm not found (poppler-utils). Install it to rasterize PDF pages for the cloud agent."
        );
    };
    let mut outs = Vec::new();
    for &p in pages {
        let prefix = out_dir.join(format!("page-{p}"));
        let status = std::process::Command::new(&bin)
            .args([
                "-png",
                "-f",
                &p.to_string(),
                "-l",
                &p.to_string(),
                "-singlefile",
                pdf.to_str().unwrap_or(""),
                prefix.to_str().unwrap_or(""),
            ])
            .status()
            .context("pdftoppm failed to start")?;
        if !status.success() {
            bail!("pdftoppm exited nonzero for page {p}");
        }
        let png = out_dir.join(format!("page-{p}.png"));
        if !png.exists() {
            bail!("expected raster missing: {}", png.display());
        }
        outs.push(png);
    }
    Ok(outs)
}

fn which_pdftoppm() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("pdftoppm");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}
