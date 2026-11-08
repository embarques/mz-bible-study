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
pub mod content;
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
pub async fn run(mut job: PrepareJob, cfg: &Config) -> Result<()> {
    let host_images = cfg
        .openai_api_key
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    job.echo_plan(host_images);

    if job.audience == Audience::Adult && (job.generate || job.force_prepare) {
        if host_images {
            progress::phase(
                "images: host OpenAI (fast path) after Cursor JSON — runs in parallel with build",
            );
        } else {
            progress::phase(
                "images: Cursor cloud agent (your cursor_api_key) — \
                 missing slots get placeholders so the deck still builds",
            );
        }
    }

    let root = paths::project_root()?;
    // Clear finished scans out of pending/ before auto-pick (otherwise old
    // singles clutter the tray forever after a successful prepare).
    archive_ready_pending_pdfs(job.audience, &root);

    let pdf = scans::normalize_pending_inbox(&job.resolve_pdf()?)?;
    let page_count = pdf_page_count(&pdf)?;
    let mut errors: Vec<String> = Vec::new();
    let mut outputs: Vec<StudyOutput> = Vec::new();

    // Discover-all: OCR once, learn from/to, then same prepare loop.
    let slices = if job.discover_all {
        progress::now_step(
            1,
            4,
            "Find pages (OCR)",
            "Discovering every estudio in the PDF (one pass)…",
        );
        let found = discover::discover_all_study_pages(
            &pdf,
            job.audience,
            cfg.pdftoppm_path.as_deref(),
        )?;
        if found.is_empty() {
            bail!("No estudios discovered in {}", pdf.display());
        }
        job.from = found.first().map(|s| s.study).unwrap();
        job.to = found.last().map(|s| s.study).unwrap();
        job.pdf_from = job.from;
        job.discover = true;
        progress::ok(format!(
            "Step 1/4 done — {} estudio{} mapped ({}–{})",
            found.len(),
            if found.len() == 1 { "" } else { "s" },
            job.from,
            job.to
        ));
        found
            .into_iter()
            .map(|s| StudySlice {
                study: s.study,
                pages: s.pages,
                proximo_title_page: s.proximo_title_page,
            })
            .collect::<Vec<_>>()
    } else {
        let studies = (job.from..=job.to).collect::<Vec<_>>();
        // If every estudio in range already has complete JSON + images, skip OCR
        // and the cloud agent — only rebuild PPTX/PDF.
        let all_resumable = !job.force_prepare
            && !job.gen_images
            && studies.iter().all(|n| {
                agent::prepare::try_resume_deliverables(*n, job.audience, &root).is_some()
            });

        if all_resumable {
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
            if job.discover || job.audience == Audience::Adult {
                progress::now_step(
                    1,
                    4,
                    "Find pages (OCR)",
                    "Looking for this estudio inside the PDF (parallel OCR, usually under ~40s)…",
                );
                progress::info(format!(
                    "Step 1/4 — finding estudios {}–{} in PDF ({page_count} pages; OCR can take ~30–90s)…",
                    job.from, job.to
                ));
            } else {
                progress::now_step(
                    1,
                    4,
                    "Find pages",
                    "Using page math from --from (no OCR)…",
                );
            }
            let slices =
                resolve_study_slices(&job, &pdf, page_count, cfg.pdftoppm_path.as_deref())?;
            progress::ok(format!(
                "Step 1/4 done — {} estudio{} mapped",
                slices.len(),
                if slices.len() == 1 { "" } else { "s" }
            ));
            slices
        }
    };

    let total = slices.len();
    // HARD: collect every mapping / on-disk conflict *before* extract or build.
    preflight_prepare_batch(&job, &slices, &pdf, &root)?;

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
        // Adult próximo is a single title page. Youth kept the old 3-page window
        // for the next block's first page only (agent still uses `na`).
        let next_pages = slice.proximo_title_page.map(|p| match job.audience {
            Audience::Adult => (p, p),
            Audience::Youth => (p, p + 2),
        });
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
        let study_nums: Vec<u32> = slices.iter().map(|s| s.study).collect();
        run_review(&study_nums, &pdf, !job.prepare_only, &job, cfg, &root).await?;
    }

    // Archive when every estudio *named by the PDF* is now ready on disk.
    // Do NOT require "started at PDF page 1" — that blocked `--from 8` on a
    // Study 7–11 PDF and left leftover Próximo title pages keeping singles in pending/.
    let _ = page_count;
    match pdf_ready_to_archive(&pdf, job.audience, &root) {
        Ok(true) => {
            scans::move_to_complete(&pdf)?;
            progress::info(format!(
                "done — {} prepared; PDF moved to scans/complete/",
                slices.len()
            ));
        }
        Ok(false) => {
            let remaining = pdf_named_estudios_missing(&pdf, job.audience, &root);
            progress::info(format!(
                "done — {} prepared; PDF stays in scans/pending/ ({})",
                slices.len(),
                if remaining.is_empty() {
                    "filename has no estudio span — pass a full-span name or --force archive later"
                        .to_string()
                } else {
                    format!(
                        "still need estudio(s) {} on disk",
                        remaining
                            .iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ));
        }
        Err(e) => progress::warn(format!(
            "done — {} prepared; could not decide archive ({e:#}) — PDF left in pending/",
            slices.len()
        )),
    }
    Ok(())
}

/// Estudios implied by the PDF filename (`Study 7-11` → 7..=11, `Study 5` → [5]).
fn pdf_named_estudios(pdf: &Path) -> Option<Vec<u32>> {
    if let Some((s, e)) = discover::estudio_span_from_filename(pdf) {
        return Some((s..=e).collect());
    }
    discover::estudio_number_from_filename(pdf).map(|n| vec![n])
}

fn estudio_ready_on_disk(study: u32, audience: Audience, root: &Path) -> bool {
    agent::prepare::try_resume_deliverables(study, audience, root).is_some()
        || (audience == Audience::Adult && content::has_adult_sections(study))
}

/// True when every filename-named estudio is ready (section tree / JSON+images).
fn pdf_ready_to_archive(pdf: &Path, audience: Audience, root: &Path) -> Result<bool> {
    let Some(named) = pdf_named_estudios(pdf) else {
        return Ok(false);
    };
    Ok(named
        .iter()
        .all(|&n| estudio_ready_on_disk(n, audience, root)))
}

fn pdf_named_estudios_missing(pdf: &Path, audience: Audience, root: &Path) -> Vec<u32> {
    pdf_named_estudios(pdf)
        .unwrap_or_default()
        .into_iter()
        .filter(|&n| !estudio_ready_on_disk(n, audience, root))
        .collect()
}

/// Move any pending PDF whose filename-named estudios are already prepared.
fn archive_ready_pending_pdfs(audience: Audience, root: &Path) {
    let Ok(list) = scans::list_pending_pdfs() else {
        return;
    };
    for pdf in list {
        match pdf_ready_to_archive(&pdf, audience, root) {
            Ok(true) => match scans::move_to_complete(&pdf) {
                Ok(dest) => progress::ok(format!(
                    "archived {} → scans/complete/ (estudios already prepared)",
                    dest.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("pdf")
                )),
                Err(e) => progress::warn(format!(
                    "could not archive {}: {e:#}",
                    pdf.display()
                )),
            },
            Ok(false) => {}
            Err(_) => {}
        }
    }
}

/// Print the full prepare plan and fail on any on-disk conflict **before**
/// cloud extract / image gen / build starts.
fn preflight_prepare_batch(
    job: &PrepareJob,
    slices: &[StudySlice],
    pdf: &Path,
    root: &Path,
) -> Result<()> {
    progress::phase("preflight — collect plan; fail before extract if anything is wrong");
    let mut problems: Vec<String> = Vec::new();

    if slices.is_empty() {
        problems.push(format!(
            "no estudios mapped for {}–{} in {}",
            job.from,
            job.to,
            pdf.display()
        ));
    }

    let expected: Vec<u32> = (job.from..=job.to).collect();
    let mapped: Vec<u32> = slices.iter().map(|s| s.study).collect();
    if mapped != expected {
        problems.push(format!(
            "mapped estudios {:?} do not match requested {}–{} ({:?})",
            mapped, job.from, job.to, expected
        ));
    }

    println!("  Preflight plan ({} estudio{}):", slices.len(), if slices.len() == 1 { "" } else { "s" });
    for s in slices {
        let json = agent::prepare::expected_paths(s.study, job.audience, root).json;
        let (json_note, json_problem) = match read_existing_json_numero(&json) {
            None => ("no JSON yet".to_string(), None),
            Some(Ok(n)) if n == s.study => (format!("JSON ok (numero={n})"), None),
            Some(Ok(n)) => {
                let msg = format!(
                    "{} has numero={n}, expected {} — delete that JSON (or --force-prepare) \
                     so prepare does not resume the wrong estudio",
                    json.display(),
                    s.study
                );
                (format!("JSON CONFLICT numero={n}"), Some(msg))
            }
            Some(Err(e)) => {
                let msg = format!("{}: {e}", json.display());
                ("JSON unreadable".to_string(), Some(msg))
            }
        };
        if let Some(p) = json_problem {
            if !job.force_prepare {
                problems.push(p);
            }
        }
        let prox = s
            .proximo_title_page
            .map(|p| format!("próximo sheet {p}"))
            .unwrap_or_else(|| "no Próximo".into());
        println!(
            "    · estudio {} · PDF sheets {}–{} · {prox} · {json_note}",
            s.study, s.pages.0, s.pages.1
        );
    }

    if !problems.is_empty() {
        eprintln!("Preflight failed — not starting extract/build:");
        for (i, p) in problems.iter().enumerate() {
            eprintln!("  {}. {p}", i + 1);
        }
        bail!(
            "Preflight failed with {} problem(s). Fix the issues above, then re-run the same command.",
            problems.len()
        );
    }
    progress::ok(format!(
        "preflight OK — {} estudio{} ready to prepare",
        slices.len(),
        if slices.len() == 1 { "" } else { "s" }
    ));
    Ok(())
}

fn read_existing_json_numero(path: &Path) -> Option<Result<u32, String>> {
    if !path.is_file() {
        return None;
    }
    Some((|| {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("invalid JSON ({e})"))?;
        let n = v
            .get("numero")
            .or_else(|| v.get("estudio"))
            .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
            .ok_or_else(|| "missing numero".to_string())?;
        Ok(n as u32)
    })())
}

/// Log failure; keep the PDF in `scans/pending/` so the user can retry.
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
        "PDF left in scans/pending/ for retry (estudio incomplete — not moved to error/). Path: {}",
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
    // Adult: never use youth ×3 page math. Boundaries are title → RESUMEN
    // GENERAL (+ optional next title for Próximo). Always OCR/text discover.
    let use_discover = job.discover || job.audience == Audience::Adult;
    if job.audience == Audience::Adult && !job.discover {
        crate::progress::info(
            "adult audience — discovering pages by section markers \
             (title → RESUMEN GENERAL; page count ignored)",
        );
    }

    if use_discover {
        let studies: Vec<u32> = (job.from..=job.to).collect();
        // One OCR/raster pass for the whole PDF — never re-OCR per estudio.
        let found = discover::discover_study_pages_batch(
            pdf,
            &studies,
            job.audience,
            pdftoppm_path,
        )?;
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
    if job.gen_definicion && (job.gen_images || job.force_prepare) {
        bail!("use --gen-definicion alone (not with --gen-images / --force-prepare)");
    }
    if job.gen_definicion && job.audience != Audience::Adult {
        bail!("--gen-definicion is adult-only");
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
        openai_model: cfg.openai_model.clone(),
        image_model: creds.image_model,
        api_key: creds.api_key,
        image_api_key: creds.image_api_key,
        stream: job.stream,
        root: root.to_path_buf(),
        pdftoppm_path: cfg.pdftoppm_path.clone(),
    };

    // Adult: if section tree (or salvable JSON) is already on disk, build it.
    // `-g` means "generate when missing" — NOT "always re-run the cloud agent".
    // Only `--force-prepare` forces a fresh agent.
    if job.audience == Audience::Adult
        && !job.force_prepare
        && !job.gen_images
        && !job.gen_definicion
    {
        let json_path = agent::prepare::expected_paths(study, job.audience, root).json;
        if !content::has_adult_sections(study) && json_path.is_file() {
            match content::expand_json_to_sections(&json_path, study) {
                Ok(dir) => progress::ok(format!(
                    "Expanded salvaged JSON → section tree {}",
                    rel_from_root(root, &dir)
                )),
                Err(e) => progress::info(format!(
                    "note: could not expand studies/adult/{study}.json ({e:#})"
                )),
            }
        }
        if content::has_adult_sections(study) {
            content::assert_adult_scan_coverage(
                study,
                pdf,
                pages,
                cfg.pdftoppm_path.as_deref(),
            )?;
            let model = content::load_adult_sections(study)?;
            let _ = content::ensure_adult_placeholders(study, &model, root)?;

            // Text is ready but scenic stubs are identical → uniqueness QC fails.
            // Fill real art before building; do not skip the agent when images are stubs.
            if content::any_placeholders(study, &model, root) {
                if host_image_key {
                    progress::ok(format!(
                        "Section text ready for estudio {study}; scenic are placeholders — \
                         generating host images…"
                    ));
                    let creds = cfg.provider_creds_for(job.audience)?;
                    agent::prepare_openai::generate_host_images(&make_req(creds)).await?;
                } else if job.generate {
                    progress::ok(format!(
                        "Section text ready for estudio {study}; scenic are placeholders — \
                         Cursor agent will generate unique images (set openai_api_key for host fast path)"
                    ));
                    // Fall through to cloud agent below (images), keep section tree on disk.
                } else {
                    bail!(
                        "adult estudio {study}: section folders exist but scenic images are \
                         PLACEHOLDERS (identical stubs). Re-run with -g so the Cursor agent \
                         generates unique scenes, or set openai_api_key and use --gen-images."
                    );
                }
            }

            if !content::any_placeholders(study, &model, root) {
                progress::ok(format!(
                    "Section content + scenic art ready (studies/adult/{study}/) — building \
                     (skipping cloud extract; use --force-prepare to re-extract text)"
                ));
                content::verify_adult_section_images(study, &model, root)?;
                let study_dir = content::adult_study_folder(study)?;
                if job.prepare_only {
                    progress::ok("prepare-only — section tree OK (no PowerPoint)");
                    return Ok(StudyOutput {
                        study,
                        json: study_dir,
                        pptx: None,
                        pdf: None,
                    });
                }
                let out = paths::study_pptx_path(job.audience, study, &model.titulo)?;
                let build_spin = progress::Spinner::start(format!(
                    "Step 4/4 — Building PPTX{}… usually ~1–2 min",
                    if job.export_pdf {
                        " + exporting PDF"
                    } else {
                        ""
                    }
                ));
                let template = job.template.clone().unwrap_or_else(|| {
                    paths::master_template(job.audience).expect("template")
                });
                let build_result = tokio::task::spawn_blocking({
                    let model = model.clone();
                    let out = out.clone();
                    let template = template.clone();
                    let export_pdf = job.export_pdf;
                    move || build::build_adult_from_model(&model, &out, Some(&template), export_pdf)
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
                    let p = export::default_pdf_path(&out);
                    if p.exists() {
                        Some(p)
                    } else {
                        None
                    }
                } else {
                    None
                };
                progress::info(format!("  PPTX → {}", rel_from_root(root, &out)));
                if let Some(ref pdf_path) = pdf {
                    progress::info(format!("  PDF  → {}", rel_from_root(root, pdf_path)));
                }
                return Ok(StudyOutput {
                    study,
                    json: study_dir,
                    pptx: Some(out),
                    pdf,
                });
            }
            // Placeholders remain + -g → continue into Cursor prepare for images.
        }
        if !job.generate {
            bail!(
                "No section content at studies/adult/{study}/ (need manifest.toml).\n\
                 Either:\n\
                   • drop generated section files there (see GENERATE_STUDY_AGENT.md), then re-run\n\
                   • or pass --generate / -g to extract from the scan"
            );
        }
        // -g and nothing on disk → fall through to cloud agent below.
    }

    // Resume complete deliverables unless --force-prepare (or image-only flags).
    // `-g` alone does NOT wipe resume — that was forcing useless re-agents.
    let resumed = if job.force_prepare || job.gen_images || job.gen_definicion {
        None
    } else {
        agent::prepare::try_resume_deliverables(study, job.audience, root)
    };

    let deliverables = if job.gen_definicion {
        if !host_image_key {
            bail!(
                "--gen-definicion needs openai_api_key / OPENAI_API_KEY \
                 (reference-edit of the locked DEFINICIÓN gold card)"
            );
        }
        let json = agent::prepare::try_resume_json_only(study, job.audience, root)
            .with_context(|| {
                format!(
                    "--gen-definicion needs studies/{}/{}.json first.",
                    job.audience.as_str(),
                    study
                )
            })?;
        progress::ok(format!(
            "--gen-definicion — using {} (force-rebuild DEFINICIÓN cards from gold)",
            json.display()
        ));
        let creds = cfg.provider_creds()?;
        agent::prepare_openai::regenerate_adult_definicion_cards(&make_req(creds)).await?;
        agent::prepare::verify_deliverables(study, job.audience, root)?
    } else if job.gen_images {
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

        let creds = cfg.provider_creds_for(job.audience)?;
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

    // Adult: prefer section folders downloaded from the agent; else expand JSON.
    let mut content_dir = deliverables.json.clone();
    if job.audience == Audience::Adult && (job.generate || job.force_prepare) {
        if content::has_adult_sections(study) {
            let dir = content::adult_study_folder(study)?;
            progress::ok(format!(
                "Section tree ready → {}",
                rel_from_root(root, &dir)
            ));
            let _ = content::print_section_inventory(&dir);
            content_dir = dir;
            content::assert_adult_scan_coverage(
                study,
                pdf,
                pages,
                cfg.pdftoppm_path.as_deref(),
            )?;
        } else if deliverables.json.is_file() {
            match content::expand_json_to_sections(&deliverables.json, study) {
                Ok(dir) => {
                    progress::ok(format!(
                        "Expanded JSON → section tree {}",
                        rel_from_root(root, &dir)
                    ));
                    content::print_section_inventory(&dir)?;
                    content_dir = dir;
                    content::assert_adult_scan_coverage(
                        study,
                        pdf,
                        pages,
                        cfg.pdftoppm_path.as_deref(),
                    )?;
                }
                Err(e) => progress::info(format!(
                    "note: could not expand section tree ({e:#}) — keeping agent JSON"
                )),
            }
        }
    }

    if job.prepare_only {
        // Best-effort images in background of prepare-only when key present.
        if job.audience == Audience::Adult && host_image_key {
            let creds = cfg.provider_creds()?;
            let req = make_req(creds);
            progress::phase("prepare-only: generating host images (best-effort)…");
            if let Err(e) = agent::prepare_openai::generate_host_images(&req).await {
                progress::warn(format!(
                    "images not ready ({e:#}) — section text is saved; run --gen-images later"
                ));
            }
        }
        progress::ok(
            "prepare-only — content saved (no PowerPoint). \
             Re-run without --prepare-only to build the deck from the folder.",
        );
        return Ok(StudyOutput {
            study,
            json: content_dir,
            pptx: None,
            pdf: None,
        });
    }

    // Adult: build NOW (placeholders OK), refresh art in parallel, rebuild if art lands.
    if job.audience == Audience::Adult && content::has_adult_sections(study) {
        let model = content::load_adult_sections(study)?;
        let _ = content::ensure_adult_placeholders(study, &model, root)?;
        content::verify_adult_section_images(study, &model, root)?;

        let out = paths::study_pptx_path(job.audience, study, &model.titulo)?;
        let template = job
            .template
            .clone()
            .unwrap_or_else(|| paths::master_template(job.audience).expect("template"));

        // Kick off host images while we build the first deck.
        let image_join = if host_image_key && content::any_placeholders(study, &model, root)
        {
            let creds = cfg.provider_creds()?;
            let req = make_req(creds);
            progress::phase(
                "building deck with placeholders while host images generate in parallel…",
            );
            Some(tokio::spawn(async move {
                agent::prepare_openai::generate_host_images(&req).await
            }))
        } else {
            None
        };

        let build_spin = progress::Spinner::start(format!(
            "Building PPTX{} (text ready; art may refresh next)…",
            if job.export_pdf {
                " + PDF"
            } else {
                ""
            }
        ));
        let build_result = tokio::task::spawn_blocking({
            let model = model.clone();
            let out = out.clone();
            let template = template.clone();
            let export_pdf = job.export_pdf;
            move || build::build_adult_from_model(&model, &out, Some(&template), export_pdf)
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

        if let Some(join) = image_join {
            match join.await {
                Ok(Ok(())) => {
                    if !content::any_placeholders(study, &model, root) {
                        progress::phase("real images ready — rebuilding deck…");
                        let rebuild = tokio::task::spawn_blocking({
                            let model = model.clone();
                            let out = out.clone();
                            let template = template.clone();
                            let export_pdf = job.export_pdf;
                            move || {
                                build::build_adult_from_model(
                                    &model,
                                    &out,
                                    Some(&template),
                                    export_pdf,
                                )
                            }
                        })
                        .await?;
                        match rebuild {
                            Ok(_) => progress::ok("deck refreshed with real images"),
                            Err(e) => progress::warn(format!(
                                "rebuild after images failed ({e:#}) — first deck kept"
                            )),
                        }
                    } else {
                        progress::warn(
                            "some placeholders remain — drop PNGs into images/ or --gen-images",
                        );
                    }
                }
                Ok(Err(e)) => progress::warn(format!(
                    "host images failed ({e:#}) — deck kept with placeholders"
                )),
                Err(e) => progress::warn(format!(
                    "image task join failed ({e}) — deck kept with placeholders"
                )),
            }
        } else if content::any_placeholders(study, &model, root) {
            progress::warn(format!(
                "deck uses PLACEHOLDER images — re-run with -g so Cursor \
                 (cursor_api_key) regenerates scenic art, or drop PNGs under \
                 studies/adult/{study}/images/ (optional: openai_api_key + --gen-images)"
            ));
        }

        let pdf = if job.export_pdf {
            let p = export::default_pdf_path(&out);
            if p.exists() {
                Some(p)
            } else {
                None
            }
        } else {
            None
        };
        progress::info(format!("  PPTX → {}", rel_from_root(root, &out)));
        if let Some(ref pdf_path) = pdf {
            progress::info(format!("  PDF  → {}", rel_from_root(root, pdf_path)));
        }
        return Ok(StudyOutput {
            study,
            json: content_dir,
            pptx: Some(out),
            pdf,
        });
    }

    let json_path = &deliverables.json;
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(json_path)?)?;
    let title = data
        .get("titulo")
        .and_then(|v| v.as_str())
        .unwrap_or("TITLE");
    let out = paths::study_pptx_path(job.audience, study, title)?;

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
        let p = export::default_pdf_path(&out);
        if p.exists() {
            Some(p)
        } else {
            None
        }
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
    let creds = cfg.provider_creds_for(job.audience)?;

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
