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
pub mod progress;
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
    let host_images = cfg
        .openai_api_key
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    job.echo_plan(host_images);

    let pdf = job.resolve_pdf()?;
    let page_count = pdf_page_count(&pdf)?;

    let root = paths::project_root()?;
    let studies = (job.from..=job.to).collect::<Vec<_>>();
    let mut errors: Vec<String> = Vec::new();
    let mut outputs: Vec<StudyOutput> = Vec::new();

    // If every estudio in range already has complete JSON + images, skip OCR
    // and the cloud agent — only rebuild PPTX/PDF.
    let all_resumable = !job.force_prepare
        && !job.gen_images
        && studies.iter().all(|n| {
            agent::prepare::try_resume_deliverables(*n, job.audience, &root).is_some()
        });

    let slices = if all_resumable {
        progress::now_step(
            1,
            4,
            "Find pages",
            "Skipped — complete JSON + images already on disk (resume)",
        );
        progress::ok(format!(
            "RESUME — {} estudio{} already prepared locally; skipping OCR + agent",
            studies.len(),
            if studies.len() == 1 { "" } else { "s" }
        ));
        // Placeholder page ranges (unused when agent is skipped).
        studies
            .iter()
            .map(|&study| StudySlice {
                study,
                pages: job::default_pages(study, job.pdf_from),
                proximo_title_page: None,
            })
            .collect::<Vec<_>>()
    } else {
        // Pre-resolve page slices (discover once for -n, or page-math for --from/--to).
        if job.discover {
            progress::now_step(
                1,
                4,
                "Find pages (OCR)",
                "Looking for this estudio inside the PDF (parallel OCR, usually under ~40s)…",
            );
        } else {
            progress::now_step(
                1,
                4,
                "Find pages",
                "Using page math from --from (no OCR)…",
            );
        }
        let slices = resolve_study_slices(&job, &pdf, page_count, cfg.pdftoppm_path.as_deref())?;
        progress::ok(format!(
            "Step 1/4 done — {} estudio{} mapped",
            slices.len(),
            if slices.len() == 1 { "" } else { "s" }
        ));
        slices
    };

    let total = slices.len();
    progress::info(format!(
        "preparing {} estudio{} ({}–{}) from {}",
        total,
        if total == 1 { "" } else { "s" },
        job.from,
        job.to,
        pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(pdf)")
    ));

    for (i, slice) in slices.iter().enumerate() {
        let n = slice.study;
        let pages = slice.pages;
        let omit_proximo = slice.proximo_title_page.is_none();
        let next_pages = slice.proximo_title_page.map(|p| (p, p + 2));
        let can_resume = !job.force_prepare
            && !job.gen_images
            && agent::prepare::try_resume_deliverables(n, job.audience, &root).is_some();

        let eta = progress::prepare_eta(job.audience, job.prepare_only, host_images);
        let eta_label = if can_resume {
            "ETA ~1–2 min (resume — rebuild only)".to_string()
        } else {
            format!("ETA ~{}–{} min", eta.per_study_min.0, eta.per_study_min.1)
        };
        progress::step(
            i + 1,
            total,
            format!(
                "Estudio {n} · pages {}–{}{} · {eta_label}",
                pages.0,
                pages.1,
                if omit_proximo {
                    " · no Próximo".to_string()
                } else {
                    format!(" · próximo p.{}", next_pages.unwrap().0)
                }
            ),
        );

        let style = section_styles::section_style_for_study(n);
        progress::phase(format!("section style: {} — {}", style.id, style.name));

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
        let study_started = std::time::Instant::now();
        match prepare_one_study(one).await {
            Ok(out) => {
                outputs.push(out);
                progress::ok(format!(
                    "Estudio {n} ready ({})",
                    progress::fmt_elapsed(study_started.elapsed())
                ));
            }
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

    print_run_summary(&outputs, &job, &root);

    if job.review {
        run_review(&studies, &pdf, !job.prepare_only, &job, cfg, &root).await?;
    }

    // Archive the scan PDF only after a fully successful prepare.
    let covered_through = slices
        .last()
        .map(|s| s.pages.1 as usize)
        .unwrap_or(0);
    let started_at_page_one = slices.first().map(|s| s.pages.0 == 1).unwrap_or(false);
    let pdf_fully_prepared = started_at_page_one && covered_through >= page_count;
    if pdf_fully_prepared {
        scans::move_to_complete(&pdf)?;
        progress::info(format!(
            "done — {} prepared; PDF archived to scans/complete/",
            studies.len()
        ));
    } else {
        scans::move_to_pending(&pdf)?;
        progress::info(format!(
            "done — {} prepared; PDF archived to scans/pending/ (more estudios remain — use --pdf for the next run)",
            studies.len()
        ));
    }
    Ok(())
}

/// True when this job did not consume the PDF through the last page (more estudios may remain).
#[allow(dead_code)] // kept for future batch archive heuristics
fn more_studies_pending_in_pdf(slices: &[StudySlice], page_count: usize) -> bool {
    let covered_through = slices
        .last()
        .map(|s| s.pages.1 as usize)
        .unwrap_or(0);
    covered_through < page_count
}

/// Log failure; keep the PDF in `scans/` so the user can retry.
///
/// **Never** move the PDF to `error/` here. The study is unfinished — the scan
/// is still the source for the next prepare. Moving it caused:
/// 1) retry → “document missing”
/// 2) second fail → `move → error: No such file` and the *real* build error
///    (Marcador, etc.) got buried under the tray error.
fn finish_with_prepare_errors(
    pdf: &Path,
    errors: &[String],
    _slices: &[StudySlice],
    _page_count: usize,
) -> Result<()> {
    if let Err(e) = scans::write_error_log(pdf, &errors.join("\n")) {
        eprintln!("warning: could not write error log: {e:#}");
    }
    eprintln!(
        "PDF left in {} for retry (estudio incomplete — not moved to error/).",
        pdf.display()
    );
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
        let studies: Vec<u32> = (job.from..=job.to).collect();
        // One OCR/raster pass for the whole PDF — never re-OCR per estudio.
        let found = discover::discover_study_pages_batch(pdf, &studies, pdftoppm_path)?;
        return Ok(found
            .into_iter()
            .map(|s| StudySlice {
                study: s.study,
                pages: s.pages,
                proximo_title_page: s.proximo_title_page,
            })
            .collect());
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

#[derive(Debug, Clone)]
struct StudyOutput {
    study: u32,
    json: PathBuf,
    pptx: Option<PathBuf>,
    pdf: Option<PathBuf>,
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

async fn prepare_one_study(one: PrepareOne<'_>) -> Result<StudyOutput> {
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

    // Resume: if a prior run already left valid JSON + images, skip the cloud
    // agent (the ~5–15 min step). Rebuild PPTX from those files instead.
    progress::phase(format!(
        "Checking for previous work… {}",
        agent::prepare::existing_inventory_summary(study, job.audience, root)
    ));

    let host_image_key = cfg
        .openai_api_key
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    if job.gen_images && job.force_prepare {
        bail!("use either --gen-images or --force-prepare, not both");
    }

    let make_req = |creds: crate::config::ProviderCreds| agent::prepare::PrepareRequest {
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
        image_api_key: creds.image_api_key,
        stream: job.stream,
        root: root.to_path_buf(),
        pdftoppm_path: cfg.pdftoppm_path.clone(),
    };

    let resumed = if job.force_prepare || job.gen_images {
        None
    } else {
        agent::prepare::try_resume_deliverables(study, job.audience, root)
    };

    let deliverables = if job.gen_images {
        // Keep JSON; always (re)generate images via OpenAI (overwrite if present).
        if !host_image_key {
            bail!(
                "--gen-images needs openai_api_key / OPENAI_API_KEY \
                 (host generates images; see lamad-cli/config.example.toml)"
            );
        }
        let json = agent::prepare::try_resume_json_only(study, job.audience, root)
            .with_context(|| {
                format!(
                    "--gen-images needs studies/{}/{}.json first. \
                     Run prepare without --gen-images to create the JSON, then retry.",
                    job.audience.as_str(),
                    study
                )
            })?;
        let had_images = agent::prepare::required_images_present(study, job.audience, root);
        progress::ok(format!(
            "--gen-images — using {} ({})",
            json.display(),
            if had_images {
                "overwriting existing images"
            } else {
                "no images yet — generating"
            }
        ));
        let creds = cfg.provider_creds()?;
        agent::prepare_openai::generate_host_images(&make_req(creds)).await?;
        agent::prepare::verify_deliverables(study, job.audience, root)?
    } else if let Some(existing) = resumed {
        if job.audience == Audience::Adult {
            let _ = agent::prepare::stamp_audience(&existing.json, job.audience);
            let _ = agent::prepare::stamp_adult_image_paths(&existing.json, study, root);
        }
        progress::ok(format!(
            "RESUME — complete JSON + images already on disk; \
             skipping Cursor agent (saves the long step). \
             Inventory: {}",
            agent::prepare::existing_inventory_summary(study, job.audience, root)
        ));
        progress::now_step(
            4,
            4,
            "Finish",
            "Reusing previous prepare output — building PowerPoint (+ PDF)…",
        );
        existing
    } else if !job.force_prepare
        && host_image_key
        && agent::prepare::try_resume_json_only(study, job.audience, root).is_some()
        && !agent::prepare::required_images_present(study, job.audience, root)
    {
        // Partial resume: JSON exists, images missing → only generate images.
        progress::ok(format!(
            "RESUME (partial) — JSON on disk, images missing; \
             generating host images only ({})",
            agent::prepare::existing_inventory_summary(study, job.audience, root)
        ));
        let creds = cfg.provider_creds()?;
        agent::prepare_openai::generate_host_images(&make_req(creds)).await?;
        agent::prepare::verify_deliverables(study, job.audience, root)?
    } else {
        if job.force_prepare {
            progress::phase("--force-prepare: ignoring local files and re-running agent…");
        }
        progress::phase(format!(
            "Nothing complete to resume ({}) — running prepare agent…",
            agent::prepare::existing_inventory_summary(study, job.audience, root)
        ));

        let creds = cfg.provider_creds()?;
        progress::phase(format!(
            "backend: {} (model={}){}",
            creds.provider.display_name(),
            creds.model,
            if creds.host_images() {
                " · host images ON"
            } else {
                ""
            }
        ));

        let deliverables = agent::prepare::run_prepare_agent(make_req(creds)).await?;

        agent::prepare::verify_deliverables(study, job.audience, root)?;
        deliverables
    };

    if job.prepare_only {
        progress::ok(
            "prepare-only — JSON + images saved (no PowerPoint). \
             Re-run without --prepare-only to build the deck.",
        );
        return Ok(StudyOutput {
            study,
            json: deliverables.json,
            pptx: None,
            pdf: None,
        });
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

    let build_spin = progress::Spinner::start(format!(
        "Step 4/4 — Building PPTX{}… usually ~1–2 min",
        if job.export_pdf {
            " + exporting PDF"
        } else {
            ""
        }
    ));
    let template = job
        .template
        .clone()
        .unwrap_or_else(|| paths::master_template(job.audience).expect("youth template"));

    let build_result = tokio::task::spawn_blocking({
        let json_path = json_path.clone();
        let out = out.clone();
        let template = template.clone();
        let export_pdf = job.export_pdf;
        let audience = job.audience;
        move || build::build_study(&json_path, &out, Some(&template), audience, export_pdf)
    })
    .await?;
    match build_result {
        Ok(_) => build_spin.succeed(format!(
            "built {}",
            out.file_name().unwrap_or_default().to_string_lossy()
        )),
        Err(e) => {
            build_spin.fail("build failed");
            return Err(e);
        }
    }

    let pdf = if job.export_pdf {
        let p = out.with_extension("pdf");
        if p.exists() { Some(p) } else { None }
    } else {
        None
    };

    progress::info(format!("  PPTX → {}", rel_from_root(root, &out)));
    if let Some(ref pdf_path) = pdf {
        progress::info(format!("  PDF  → {}", rel_from_root(root, pdf_path)));
    } else if job.export_pdf {
        progress::info("  PDF  → (not exported — needs macOS + Microsoft PowerPoint)");
    }

    Ok(StudyOutput {
        study,
        json: json_path.clone(),
        pptx: Some(out),
        pdf,
    })
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
        "lamad prepare --from {study} --to {study}   # or: lamad build studies/{aud}/{study}.json --export-pdf"
    )
}

fn rel_from_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn print_run_summary(outputs: &[StudyOutput], job: &PrepareJob, root: &Path) {
    if outputs.is_empty() {
        return;
    }

    if job.prepare_only {
        println!("\n=== Prepared (JSON + images only) ===");
        for o in outputs {
            println!("  Estudio {}: {}", o.study, rel_from_root(root, &o.json));
        }
        if outputs.len() == 1 {
            let n = outputs[0].study;
            println!(
                "\nTo build PowerPoint + PDF, run:\n  lamad prepare --from {n} --to {n}"
            );
        } else {
            println!(
                "\nTo build PowerPoint + PDF, run without --prepare-only:\n  \
                 lamad prepare --from {} --to {}",
                job.from, job.to
            );
        }
        return;
    }

    println!("\n=== Your files ===");
    for o in outputs {
        println!("Estudio {}", o.study);
        if let Some(p) = &o.pptx {
            println!("  PPTX: {}", rel_from_root(root, p));
        }
        if let Some(p) = &o.pdf {
            println!("  PDF:  {}", rel_from_root(root, p));
        } else if job.export_pdf {
            println!("  PDF:  (not exported — needs macOS + Microsoft PowerPoint)");
        }
    }

    if !job.review {
        let hint = if job.from == job.to {
            format!("lamad review -n {}", job.from)
        } else {
            format!("lamad review --from {} --to {}", job.from, job.to)
        };
        println!("\nOptional QA checklist: {hint}");
    }
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
    let pb = progress::bar(pages.len() as u64, "Rasterizing PDF pages");
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
            pb.abandon_with_message("Rasterizing PDF pages failed");
            bail!("pdftoppm exited nonzero for page {p} ({})", bin.display());
        }
        let png = out_dir.join(format!("page-{p}.png"));
        if !png.exists() {
            pb.abandon_with_message("Rasterizing PDF pages failed");
            bail!("expected raster missing: {}", png.display());
        }
        outs.push(png);
        pb.inc(1);
    }
    pb.finish_with_message("Rasterized PDF pages");
    Ok(outs)
}
