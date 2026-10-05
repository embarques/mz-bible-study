//! Clap CLI definitions.
//!
//! ## Bool flags that default to "on"
//!
//! For a flag that's on by default (`export_pdf`, `stream`,
//! `stop_on_error`), clap only needs **one** real command-line flag: the
//! negation (`--no-export-pdf`, `--no-stream`,
//! `--continue-on-error`). There's no separate `--export-pdf` etc. flag to
//! pass — the field already defaults to `true`, so simply *not* passing the
//! negation flag keeps it on. The pattern:
//!
//! ```ignore
//! #[arg(long = "export-pdf", default_value_t = true)]
//! #[arg(long = "no-export-pdf", action = ArgAction::SetFalse)]
//! pub export_pdf: bool,
//! ```
//!
//! Stacking two `#[arg(...)]` attributes on one field does **not** create
//! two clap arguments (clap derive is one `Arg` per field) — the second
//! attribute's `long` simply wins. Keep the first attribute anyway; it
//! documents the "on" name and default for readers, even though the only
//! argument clap registers is `--no-export-pdf`.

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::agent::AgentProvider;
use crate::job::Audience;

#[derive(Debug, Parser)]
#[command(
    name = "lamad",
    version,
    about = "lamad (Hebrew לָמַד: learn by instruction, practice, or experience) — Mount Zion Church Bible-study CLI",
    long_about = "lamad (Hebrew לָמַד) means to learn by instruction, practice, or experience — \
                  to become trained or accustomed; not mere information, but formation. \
                  Related Piel form לִמֵּד (limmed): to teach.\n\n\
                  Mount Zion Church Bible-study CLI — prepare, build, validate, export.\n\n\
                  Put a scan PDF in scans/, then:\n  lamad prepare --from N --to M\n\
                  Or run `lamad prepare` (TTY) for the interactive form.\n\
                  See LEEME.md / README.md.",
    after_help = "Name: lamad — Hebrew לָמַד, to learn by instruction, practice, or experience."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to config.toml
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Increase output (-v plan/ok, -vv steps/ETA, -vvv agent detail, -vvvv build debug)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan PDF → study JSON + images → PowerPoint + PDF (default full run).
    ///
    /// **Default:** writes `bible-studies/{N} - {TITLE}.pptx` (+ `.pdf` on macOS).
    /// **Optional:** `--prepare-only` (JSON/images only), `--review` (cloud QA).
    /// QA later without re-preparing: `lamad review -n N`.
    ///
    /// Looks for `*.pdf` directly under `scans/` (not `scans/complete/` or `scans/error/`).
    /// Batch: `--from N --to M`. Single: `-n N` (OCR discover) or `--from F -n N` (page math).
    /// Without `--from`/`--to`/`-n` on a TTY, opens the interactive form.
    Prepare(PrepareArgs),

    /// Build a PPTX from study JSON + section images.
    Build(BuildArgs),

    /// Validate a PPTX package (must print OK).
    Validate {
        /// Path to `.pptx`
        pptx: PathBuf,
    },

    /// Export PPTX → sibling PDF via Microsoft PowerPoint (macOS).
    #[command(name = "export-pdf")]
    ExportPdf {
        /// Path to `.pptx`
        pptx: PathBuf,
    },

    /// Re-run agent QA without prepare.
    Review(ReviewArgs),

    /// Check binary, template, API key, PowerPoint, scans/ writability.
    Doctor,
}

#[derive(Debug, Clone, Parser)]
pub struct PrepareArgs {
    /// Source PDF (optional if exactly one `*.pdf` sits in `scans/`)
    #[arg(long)]
    pub pdf: Option<PathBuf>,

    /// First estudio number (batch; required with --to)
    #[arg(long = "from")]
    pub from: Option<u32>,

    /// Last estudio number (batch; required with --from)
    #[arg(long = "to")]
    pub to: Option<u32>,

    /// Single study alias. Alone: discover pages via OCR. With `--from F`:
    /// PDF page 1 = estudio F, prepare only this study (page math).
    #[arg(short = 'n', long = "study")]
    pub study: Option<u32>,

    /// JSON + section images only — skips PowerPoint and PDF (power-user / debug)
    #[arg(long)]
    pub prepare_only: bool,

    /// Re-run the cloud agent even when JSON + images already exist locally
    /// (default: auto-resume — skip the agent and rebuild from disk)
    #[arg(long = "force-prepare")]
    pub force_prepare: bool,

    /// (Re)generate section/scenic images from existing study JSON via OpenAI.
    /// Overwrites images if they already exist. Requires openai_api_key / OPENAI_API_KEY
    /// and a ready `{N}.json`. Skips the cloud agent.
    #[arg(long = "gen-images")]
    pub gen_images: bool,

    /// PDF export is on by default; pass --no-export-pdf to skip it
    #[arg(long = "export-pdf", default_value_t = true)]
    #[arg(long = "no-export-pdf", action = ArgAction::SetFalse)]
    pub export_pdf: bool,

    /// Run cloud QA after build (off by default — use `lamad review` separately)
    #[arg(long, default_value_t = false)]
    pub review: bool,

    #[arg(long, value_enum, default_value_t = AudienceCli::Youth)]
    pub audience: AudienceCli,

    /// Master template PPTX override
    #[arg(long)]
    pub template: Option<PathBuf>,

    /// Legacy alias for --template
    #[arg(long = "base")]
    pub base: Option<PathBuf>,

    #[arg(long)]
    pub model: Option<String>,

    /// Cloud backend: cursor (default) or chatgpt
    #[arg(long, value_enum)]
    pub provider: Option<ProviderCli>,

    /// API key for the active provider (`CURSOR_API_KEY` or `OPENAI_API_KEY`)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Live agent narration (noisy / often duplicated). Off by default — progress
    /// spinner + elapsed time is clearer. Pass `--stream` to dump agent text.
    #[arg(long = "stream", action = ArgAction::SetTrue)]
    pub stream: bool,

    /// Stops the batch on the first failed study by default; pass
    /// --continue-on-error to keep going and report all failures at the end
    #[arg(long = "stop-on-error", default_value_t = true)]
    #[arg(long = "continue-on-error", action = ArgAction::SetFalse)]
    pub stop_on_error: bool,
}

#[derive(Debug, Clone, Parser)]
pub struct BuildArgs {
    /// Path to study JSON
    pub json: PathBuf,

    /// Output PPTX path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = AudienceCli::Youth)]
    pub audience: AudienceCli,

    #[arg(long)]
    pub template: Option<PathBuf>,

    #[arg(long = "base")]
    pub base: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub export_pdf: bool,
}

#[derive(Debug, Clone, Parser)]
pub struct ReviewArgs {
    #[arg(long = "from")]
    pub from: Option<u32>,

    #[arg(long = "to")]
    pub to: Option<u32>,

    #[arg(short = 'n', long = "study")]
    pub study: Option<u32>,

    #[arg(long)]
    pub pdf: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = AudienceCli::Youth)]
    pub audience: AudienceCli,

    #[arg(long)]
    pub model: Option<String>,

    /// Cloud backend: cursor (default) or chatgpt
    #[arg(long, value_enum)]
    pub provider: Option<ProviderCli>,

    /// API key for the active provider (`CURSOR_API_KEY` or `OPENAI_API_KEY`)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Live agent narration (noisy / often duplicated). Off by default — progress
    /// spinner + elapsed time is clearer. Pass `--stream` to dump agent text.
    #[arg(long = "stream", action = ArgAction::SetTrue)]
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AudienceCli {
    #[default]
    Youth,
    Adult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderCli {
    Cursor,
    #[value(alias = "openai", alias = "gpt")]
    Chatgpt,
}

impl From<ProviderCli> for AgentProvider {
    fn from(v: ProviderCli) -> Self {
        match v {
            ProviderCli::Cursor => AgentProvider::Cursor,
            ProviderCli::Chatgpt => AgentProvider::ChatGpt,
        }
    }
}

impl From<AudienceCli> for Audience {
    fn from(v: AudienceCli) -> Self {
        match v {
            AudienceCli::Youth => Audience::Youth,
            AudienceCli::Adult => Audience::Adult,
        }
    }
}

impl PrepareArgs {
    pub fn template_effective(&self) -> Option<PathBuf> {
        self.template.clone().or_else(|| self.base.clone())
    }

    /// Resolve prepare range.
    ///
    /// Returns `None` if the interactive form should open.
    ///
    /// | Flags | Meaning |
    /// |-------|---------|
    /// | `--from F --to T` | PDF page 1 = estudio F; prepare F…T (page math) |
    /// | `-n N` | Discover estudio N inside the PDF via OCR |
    /// | `--from F -n N` | PDF page 1 = estudio F; prepare only N (page math) |
    pub fn range(&self) -> anyhow::Result<Option<PrepareRange>> {
        match (self.from, self.to, self.study) {
            (Some(f), Some(t), None) => {
                if t < f {
                    anyhow::bail!("--to must be >= --from");
                }
                Ok(Some(PrepareRange {
                    pdf_from: f,
                    from: f,
                    to: t,
                    discover: false,
                }))
            }
            (None, None, Some(n)) => Ok(Some(PrepareRange {
                pdf_from: n,
                from: n,
                to: n,
                discover: true,
            })),
            (Some(f), None, Some(n)) => {
                if n < f {
                    anyhow::bail!("-n {n} must be >= --from {f} (PDF start estudio)");
                }
                Ok(Some(PrepareRange {
                    pdf_from: f,
                    from: n,
                    to: n,
                    discover: false,
                }))
            }
            (Some(f), Some(t), Some(n)) => {
                if n < f || n > t {
                    anyhow::bail!("-n {n} must be within --from {f} --to {t}");
                }
                Ok(Some(PrepareRange {
                    pdf_from: f,
                    from: n,
                    to: n,
                    discover: false,
                }))
            }
            (None, Some(_), _) => {
                anyhow::bail!("--to requires --from (or use -n for one study)")
            }
            (Some(_), None, None) => {
                anyhow::bail!("--from alone needs --to (batch) or -n N (one study)")
            }
            (None, None, None) => Ok(None),
        }
    }
}

/// Resolved prepare targeting (from CLI flags or TUI).
#[derive(Debug, Clone, Copy)]
pub struct PrepareRange {
    pub pdf_from: u32,
    pub from: u32,
    pub to: u32,
    pub discover: bool,
}

impl ReviewArgs {
    pub fn range(&self) -> anyhow::Result<(u32, u32)> {
        match (self.from, self.to, self.study) {
            (Some(f), Some(t), None) if t >= f => Ok((f, t)),
            (None, None, Some(n)) => Ok((n, n)),
            (Some(f), Some(t), Some(n)) if f == t && f == n => Ok((n, n)),
            _ => anyhow::bail!("review requires --from/--to or -n"),
        }
    }
}
