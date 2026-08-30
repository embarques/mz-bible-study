//! mzbs — Monte de Sion Bible-study CLI.

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use mzbs::cli::{Cli, Commands};
use mzbs::config::Config;
use mzbs::job::{Audience, PrepareJob};
use mzbs::paths;
use mzbs::{build, export, validate};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mzbs=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load(cli.config.as_deref())?;

    match cli.command {
        None => {
            // Bare `mzbs` → TUI if TTY
            if !std::io::stdin().is_terminal() {
                bail!("Need --from and --to (non-interactive). Or run in a TTY for the form.");
            }
            mzbs::tui::run_prepare_tui(cfg).await
        }
        Some(Commands::Prepare(args)) => {
            cfg.apply_cli_overrides(
                args.api_key.clone(),
                args.model.clone(),
                Some(args.audience.into()),
                Some(args.export_pdf),
            );

            let range = args.range()?;
            match range {
                None => {
                    if !std::io::stdin().is_terminal() {
                        bail!(
                            "prepare without --from/--to requires a TTY. \
                             Pass --from N --to M (or -n N)."
                        );
                    }
                    mzbs::tui::run_prepare_tui(cfg).await
                }
                Some((from, to)) => {
                    let job = PrepareJob {
                        pdf: args.pdf.clone(),
                        from,
                        to,
                        audience: args.audience.into(),
                        prepare_only: args.prepare_only,
                        export_pdf: args.export_pdf,
                        review: args.review,
                        template: args.template_effective(),
                        stream: args.stream,
                        stop_on_error: args.stop_on_error,
                    };
                    mzbs::run(job, &cfg).await
                }
            }
        }
        Some(Commands::Build(args)) => {
            let audience: Audience = args.audience.into();
            if audience == Audience::Adult {
                bail!("audience=adult is not implemented yet. Use --audience youth.");
            }
            let template = args.template.clone().or(args.base.clone());
            let output = match args.output {
                Some(o) => o,
                None => {
                    let data: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(&args.json)?)?;
                    let n = data
                        .get("numero")
                        .map(|v| v.to_string().trim_matches('"').to_string())
                        .unwrap_or_else(|| "X".into());
                    let title = data
                        .get("titulo")
                        .and_then(|v| v.as_str())
                        .unwrap_or("TITLE");
                    paths::bible_studies_dir()?.join(format!("{n} - {title}.pptx"))
                }
            };
            let tmpl = template;
            let json = args.json.clone();
            let export_pdf = args.export_pdf;
            tokio::task::spawn_blocking(move || {
                build::build_study(
                    &json,
                    &output,
                    tmpl.as_deref(),
                    audience,
                    export_pdf,
                )
            })
            .await??;
            Ok(())
        }
        Some(Commands::Validate { pptx }) => {
            validate::validate_pptx(&pptx)?;
            Ok(())
        }
        Some(Commands::ExportPdf { pptx }) => {
            export::export_one(&pptx)?;
            Ok(())
        }
        Some(Commands::Review(args)) => {
            cfg.apply_cli_overrides(
                args.api_key.clone(),
                args.model.clone(),
                Some(args.audience.into()),
                None,
            );
            let (from, to) = args.range()?;
            let studies: Vec<u32> = (from..=to).collect();
            let root = paths::project_root()?;
            let pdf = args.pdf.clone();
            let api_key = cfg
                .cursor_api_key
                .clone()
                .filter(|s| !s.is_empty())
                .context("No Cursor API key for review")?;
            let report = mzbs::agent::review::run_review_agent(
                mzbs::agent::review::ReviewRequest {
                    studies,
                    source_pdf: pdf,
                    expect_pptx: true,
                    audience: args.audience.into(),
                    model: cfg.cursor_model.clone(),
                    api_key,
                    stream: args.stream,
                    root,
                },
            )
            .await?;
            println!("Review PASS — {}", report.display());
            Ok(())
        }
        Some(Commands::Doctor) => doctor(&cfg),
    }
}

fn doctor(cfg: &Config) -> Result<()> {
    println!("mzbs doctor");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));

    let root = paths::project_root()?;
    println!("  project_root: {}", root.display());

    match paths::master_template(Audience::Youth) {
        Ok(p) => println!("  template (youth): OK — {}", p.display()),
        Err(e) => println!("  template (youth): FAIL — {e}"),
    }

    match &cfg.cursor_api_key {
        Some(k) if !k.is_empty() => {
            let masked = if k.len() > 8 {
                format!("{}…", &k[..4])
            } else {
                "(set)".into()
            };
            println!("  cursor_api_key: OK — {masked}");
        }
        _ => println!(
            "  cursor_api_key: MISSING — set in config.toml or CURSOR_API_KEY"
        ),
    }

    let scans = paths::scans_dir()?;
    let writable = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(scans.join(".doctor-write-test"))
        .map(|_| {
            let _ = std::fs::remove_file(scans.join(".doctor-write-test"));
            true
        })
        .unwrap_or(false);
    println!(
        "  scans/: {} — {}",
        if writable { "OK writable" } else { "FAIL" },
        scans.display()
    );

    if cfg!(target_os = "macos") {
        if export::powerpoint_available() {
            println!("  PowerPoint (macOS): OK");
        } else {
            println!("  PowerPoint (macOS): not detected (PDF export will fail)");
        }
    } else {
        println!("  PowerPoint PDF export: n/a (macOS only)");
    }

    if which("pdftoppm").is_some() {
        println!("  pdftoppm: OK");
    } else {
        println!("  pdftoppm: MISSING — install poppler for prepare rasterization");
    }

    Ok(())
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
        }
        None
    })
}
