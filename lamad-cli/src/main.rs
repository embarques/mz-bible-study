//! lamad — Mount Zion Church Bible-study CLI (Hebrew לָמַד: learn by instruction, practice, or experience).

use anyhow::{bail, Result};
use clap::Parser;
use std::io::IsTerminal;
use std::process::ExitCode;

use lamad::cli::{Cli, Commands};
use lamad::config::Config;
use lamad::job::{Audience, PrepareJob};
use lamad::paths;
use lamad::{build, export, validate};

/// Known long flags that volunteers sometimes glue together
/// (e.g. `--prepare-only--no-review` → `--prepare-only` `--no-review`).
const SPLITTABLE_LONG_FLAGS: &[&str] = &[
    "prepare-only",
    "no-build",
    "no-review",
    "review",
    "no-export-pdf",
    "export-pdf",
    "no-stream",
    "stream",
    "stop-on-error",
    "last",
];

fn normalize_argv(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(raw.len());
    for arg in raw {
        if let Some(rest) = arg.strip_prefix("--") {
            if !rest.is_empty() && !rest.contains('=') {
                if let Some(parts) = split_glued_long_flags(rest) {
                    out.extend(parts.into_iter().map(|p| format!("--{p}")));
                    continue;
                }
            }
        }
        out.push(arg);
    }
    // Obsolete flags — drop so older scripts/docs keep working.
    out.retain(|a| a != "--no-stream" && a != "--no-review");
    out
}

fn split_glued_long_flags(rest: &str) -> Option<Vec<String>> {
    // Longest-first so `no-review` wins over `review`.
    let mut flags: Vec<&str> = SPLITTABLE_LONG_FLAGS.to_vec();
    flags.sort_by_key(|f| std::cmp::Reverse(f.len()));

    let mut remaining = rest;
    let mut parts = Vec::new();
    while !remaining.is_empty() {
        let mut matched = None;
        for flag in &flags {
            if remaining == *flag {
                matched = Some((*flag, ""));
                break;
            }
            let prefix = format!("{flag}--");
            if let Some(tail) = remaining.strip_prefix(&prefix) {
                matched = Some((*flag, tail));
                break;
            }
        }
        let (flag, tail) = matched?;
        parts.push(flag.to_string());
        remaining = tail;
    }
    if parts.len() >= 2 {
        Some(parts)
    } else {
        None
    }
}

#[cfg(test)]
mod argv_tests {
    use super::*;

    #[test]
    fn splits_glued_prepare_flags() {
        let out = normalize_argv(vec![
            "lamad".into(),
            "prepare".into(),
            "-n".into(),
            "23".into(),
            "--prepare-only--no-review".into(),
        ]);
        assert_eq!(
            out,
            vec![
                "lamad",
                "prepare",
                "-n",
                "23",
                "--prepare-only",
            ]
        );
    }

    #[test]
    fn leaves_normal_flags_alone() {
        let out = normalize_argv(vec![
            "lamad".into(),
            "--prepare-only".into(),
            "--no-review".into(),
        ]);
        assert_eq!(out, vec!["lamad", "--prepare-only"]);
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("lamad=info".parse().unwrap()),
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
    let cli = Cli::parse_from(normalize_argv(std::env::args().collect()));
    lamad::progress::set_verbosity(cli.verbose);
    let mut cfg = Config::load(cli.config.as_deref())?;

    match cli.command {
        None => {
            // Bare `lamad` → TUI if TTY
            if !std::io::stdin().is_terminal() {
                bail!("Need --from and --to (non-interactive). Or run in a TTY for the form.");
            }
            lamad::tui::run_prepare_tui(cfg).await
        }
        Some(Commands::Prepare(args)) => {
            cfg.apply_cli_overrides(
                args.provider.map(Into::into),
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
                    lamad::tui::run_prepare_tui(cfg).await
                }
                Some(range) => {
                    let job = PrepareJob {
                        pdf: args.pdf.clone(),
                        pdf_from: range.pdf_from,
                        from: range.from,
                        to: range.to,
                        discover: range.discover,
                        audience: args.audience.into(),
                        prepare_only: args.prepare_only,
                        force_prepare: args.force_prepare,
                        gen_images: args.gen_images,
                        export_pdf: args.export_pdf,
                        review: args.review,
                        template: args.template_effective(),
                        stream: args.stream,
                        stop_on_error: args.stop_on_error,
                    };
                    lamad::run(job, &cfg).await
                }
            }
        }
        Some(Commands::Build(args)) => {
            let audience: Audience = args.audience.into();
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
            let spin = lamad::progress::Spinner::start(
                "Exporting PDF via PowerPoint (this can take a minute)…",
            );
            match export::export_one(&pptx) {
                Ok(pdf) => {
                    spin.succeed(format!(
                        "{} ({} bytes)",
                        pdf.display(),
                        std::fs::metadata(&pdf).map(|m| m.len()).unwrap_or(0)
                    ));
                    Ok(())
                }
                Err(e) => {
                    spin.fail("PDF export failed");
                    Err(e)
                }
            }
        }
        Some(Commands::Review(args)) => {
            cfg.apply_cli_overrides(
                args.provider.map(Into::into),
                args.api_key.clone(),
                args.model.clone(),
                Some(args.audience.into()),
                None,
            );
            let (from, to) = args.range()?;
            let studies: Vec<u32> = (from..=to).collect();
            let root = paths::project_root()?;
            let pdf = args.pdf.clone();
            let creds = cfg.provider_creds()?;
            let report = lamad::agent::review::run_review_agent(
                lamad::agent::review::ReviewRequest {
                    studies,
                    source_pdf: pdf,
                    expect_pptx: true,
                    audience: args.audience.into(),
                    provider: creds.provider,
                    model: creds.model,
                    api_key: creds.api_key,
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
    println!("lamad doctor");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));

    let root = paths::project_root()?;
    println!("  project_root: {}", root.display());

    match paths::master_template(Audience::Youth) {
        Ok(p) => println!("  template (youth): OK — {}", p.display()),
        Err(e) => println!("  template (youth): FAIL — {e}"),
    }

    println!(
        "  agent_provider: {} ({})",
        cfg.agent_provider.as_str(),
        cfg.agent_provider.display_name()
    );

    match &cfg.cursor_api_key {
        Some(k) if !k.is_empty() => {
            let masked = mask_key(k);
            println!("  cursor_api_key: OK — {masked}");
        }
        _ => println!(
            "  cursor_api_key: MISSING — set in config.toml or CURSOR_API_KEY"
        ),
    }
    match &cfg.openai_api_key {
        Some(k) if !k.is_empty() => {
            let masked = mask_key(k);
            println!("  openai_api_key: OK — {masked}");
        }
        _ => println!(
            "  openai_api_key: MISSING — set in config.toml or OPENAI_API_KEY (needed for chatgpt)"
        ),
    }
    match cfg.agent_provider {
        lamad::agent::AgentProvider::Cursor => {
            println!("  active model: {}", cfg.cursor_model);
        }
        lamad::agent::AgentProvider::ChatGpt => {
            println!(
                "  active models: chat={} image={}",
                cfg.openai_model, cfg.openai_image_model
            );
        }
    }
    if let Err(e) = cfg.provider_creds() {
        println!("  active credentials: FAIL — {e:#}");
    } else {
        println!("  active credentials: OK");
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
        println!("  PowerPoint PDF export: n/a (macOS only) — use --no-export-pdf");
    }

    println!("  {}", lamad::pdftoppm::doctor_line(cfg.pdftoppm_path.as_deref()));
    if let Some(p) = &cfg.pdftoppm_path {
        println!("  pdftoppm_path (config): {}", p.display());
    }
    println!("  {}", lamad::discover::doctor_line());

    Ok(())
}

fn mask_key(k: &str) -> String {
    if k.len() > 8 {
        format!("{}…", &k[..4])
    } else {
        "(set)".into()
    }
}
