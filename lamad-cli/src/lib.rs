//! lamad — Mount Zion Church Bible-study engine (Hebrew לָמַד: learn by instruction, practice, or experience).
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
pub mod discover;
pub mod export;
pub mod job;
pub mod model;
pub mod pack;
pub mod paths;
pub mod pdftoppm;
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

    if job.audience == Audience::Adult {
        bail!(
            "audience=adult is not implemented yet. Use --audience youth (default)."
        );
    }

    let root = paths::project_root()?;
    let studies = (job.from..=job.to).collect::<Vec<_>>();
    let mut errors: Vec<String> = Vec::new();

    // Pre-resolve page slices (discover once for -n, or page-math for --from/--to).
    let slices = resolve_study_slices(&job, &pdf, page_count, cfg.pdftoppm_path.as_deref())?;

    for slice in &slices {
        let n = slice.study;
        let pages = slice.pages;
        let omit_proximo = slice.proximo_title_page.is_none();
        let next_pages = slice.proximo_title_page.map(|p| (p, p + 2));

        println!(
            "\n=== Estudio {n} pages {}–{}{} ===",
            pages.0,
            pages.1,
            if omit_proximo {
                " (no Próximo — no following title page in PDF)".to_string()
            } else {
                let np = next_pages.unwrap();
                format!("; próximo from page {}", np.0)
            }
        );

        let style = section_styles::section_style_for_study(n);
        println!("  Section style: {} — {}", style.id, style.name);

        let one = PrepareOne {
            study: n,
            pdf: &pdf,
            pages,
            omit_proximo,
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
                    finish_with_prepare_errors(&pdf, &errors, &slices, page_count)?;
                }
            }
        }
    }

    if !errors.is_empty() {
        finish_with_prepare_errors(&pdf, &errors, &slices, page_count)?;
    }

    if job.review {
        run_review(&studies, &pdf, !job.prepare_only, &job, cfg, &root).await?;
    }

    // Archive only when this job prepared the full contiguous span from
    // PDF page 1 through EOF (no leftover studies for a later run).
    let covered_through = slices
        .last()
        .map(|s| s.pages.1 as usize)
        .unwrap_or(0);
    let started_at_page_one = slices.first().map(|s| s.pages.0 == 1).unwrap_or(false);
    if started_at_page_one && covered_through >= page_count {
        scans::move_to_complete(&pdf)?;
        println!("\nDone: {} prepared.", studies.len());
    } else {
        println!(
            "\nDone: {} prepared. PDF left in scans/ (more pages remain for other estudios).",
            studies.len()
        );
    }
    Ok(())
}

/// True when this job did not consume the PDF through the last page (more estudios may remain).
fn more_studies_pending_in_pdf(slices: &[StudySlice], page_count: usize) -> bool {
    let covered_through = slices
        .last()
        .map(|s| s.pages.1 as usize)
        .unwrap_or(0);
    covered_through < page_count
}

/// Log failure; keep PDF in `scans/` for retry unless more estudios remain in the PDF.
fn finish_with_prepare_errors(
    pdf: &Path,
    errors: &[String],
    slices: &[StudySlice],
    page_count: usize,
) -> Result<()> {
    scans::write_error_log(pdf, &errors.join("\n"))?;
    if more_studies_pending_in_pdf(slices, page_count) {
        scans::move_to_error_tray(pdf)?;
    } else {
        eprintln!("PDF left in {} for retry.", pdf.display());
    }
    if errors.len() == 1 {
        bail!("{}", errors[0]);
    }
    bail!("Finished with {} error(s).", errors.len());
}

#[derive(Debug, Clone)]
struct StudySlice {
    study: u32,
    pages: (u32, u32),
    proximo_title_page: Option<u32>,
}

fn resolve_study_slices(
    job: &PrepareJob,
    pdf: &Path,
    page_count: usize,
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<StudySlice>> {
    if job.discover {
        let mut out = Vec::new();
        for n in job.from..=job.to {
            let found = discover::discover_study_pages(pdf, n, pdftoppm_path)?;
            out.push(StudySlice {
                study: found.study,
                pages: found.pages,
                proximo_title_page: found.proximo_title_page,
            });
        }
        return Ok(out);
    }

    let min_pages = job::min_pages_for_range(job.pdf_from, job.to) as usize;
    // Need pages through the last prepared study (relative to pdf_from).
    let need = job::default_pages(job.to, job.pdf_from).1 as usize;
    if page_count < need {
        bail!(
            "PDF page count is {page_count}, but preparing through estudio {} \
             (PDF starts at {}) needs at least {need} pages. Check the PDF or --from.",
            job.to,
            job.pdf_from
        );
    }

    let mut out = Vec::new();
    for n in job.from..=job.to {
        let pages = job::default_pages(n, job.pdf_from);
        let has_prox = job::pdf_has_proximo_pages(n, job.pdf_from, page_count);
        out.push(StudySlice {
            study: n,
            pages,
            proximo_title_page: if has_prox {
                Some(pages.1 + 1)
            } else {
                None
            },
        });
    }
    if page_count > need {
        println!(
            "  PDF has {page_count} pages (need {need} through estudio {}); \
             extra pages are OK.",
            job.to
        );
    }
    let _ = min_pages;
    Ok(out)
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

    let creds = cfg.provider_creds()?;
    println!(
        "  Backend: {} (model={})",
        creds.provider.display_name(),
        creds.model
    );

    let deliverables = agent::prepare::run_prepare_agent(
        agent::prepare::PrepareRequest {
            study,
            pdf: pdf.to_path_buf(),
            pages,
            omit_proximo,
            next_pages,
            audience: job.audience,
            provider: creds.provider,
            model: creds.model,
            image_model: creds.image_model,
            api_key: creds.api_key,
            stream: job.stream,
            root: root.to_path_buf(),
            pdftoppm_path: cfg.pdftoppm_path.clone(),
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
    let creds = cfg.provider_creds()?;

    println!("\n=== Agent review ({}) ===", creds.provider.display_name());
    let report = agent::review::run_review_agent(
        agent::review::ReviewRequest {
            studies: studies.to_vec(),
            source_pdf: Some(pdf.to_path_buf()),
            expect_pptx,
            audience: job.audience,
            provider: creds.provider,
            model: creds.model,
            api_key: creds.api_key,
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
        "lamad build studies/{aud}/{study}.json -o \"bible-studies/{study} - TITLE.pptx\" \
         --audience {aud} --export-pdf"
    )
}

/// Count pages in a PDF (lopdf).
pub fn pdf_page_count(path: &Path) -> Result<usize> {
    let doc = lopdf::Document::load(path)
        .with_context(|| format!("failed to open PDF: {}", path.display()))?;
    Ok(doc.get_pages().len())
}

/// Rasterize PDF pages to PNG via `pdftoppm`.
///
/// `pdftoppm_path` comes from `config.toml` (`pdftoppm_path`); when `None`,
/// resolves bundled `tools/pdftoppm` then PATH.
pub fn rasterize_pages(
    pdf: &Path,
    pages: &[u32],
    out_dir: &Path,
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let bin = pdftoppm::resolve(pdftoppm_path)?;
    let mut outs = Vec::new();
    for &p in pages {
        let prefix = out_dir.join(format!("page-{p}"));
        let mut cmd = std::process::Command::new(&bin);
        pdftoppm::configure_command(&mut cmd, &bin);
        let status = cmd
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
            .with_context(|| format!("pdftoppm failed to start ({})", bin.display()))?;
        if !status.success() {
            bail!("pdftoppm exited nonzero for page {p} ({})", bin.display());
        }
        let png = out_dir.join(format!("page-{p}.png"));
        if !png.exists() {
            bail!("expected raster missing: {}", png.display());
        }
        outs.push(png);
    }
    Ok(outs)
}
