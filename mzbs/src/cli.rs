//! Clap CLI definitions.
//!
//! ## Bool flags that default to "on"
//!
//! For a flag that's on by default (`export_pdf`, `review`, `stream`,
//! `stop_on_error`), clap only needs **one** real command-line flag: the
//! negation (`--no-export-pdf`, `--no-review`, `--no-stream`,
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

use crate::job::Audience;

#[derive(Debug, Parser)]
#[command(
    name = "mzbs",
    version,
    about = "Monte de Sion Bible-study CLI — prepare, build, validate, export",
    long_about = "Put a scan PDF in scans/, then:\n  mzbs prepare --from N --to M\n\
                  Or run `mzbs prepare` (TTY) for the interactive form.\n\
                  See LEEME.md / README.md."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to config.toml
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Prepare study JSON + section images from a PDF (single or batch).
    ///
    /// Looks for `*.pdf` directly under `scans/` (not `scans/complete/` or `scans/error/`).
    /// Batch: `--from N --to M`. Single: `--from N --to N` or `-n N`.
    /// Without `--from`/`--to` on a TTY, opens the interactive form.
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

    /// Single study alias for --from N --to N
    #[arg(short = 'n', long = "study")]
    pub study: Option<u32>,

    /// Skip build + PDF (JSON + section images only)
    #[arg(long)]
    pub prepare_only: bool,

    /// PDF export is on by default; pass --no-export-pdf to skip it
    #[arg(long = "export-pdf", default_value_t = true)]
    #[arg(long = "no-export-pdf", action = ArgAction::SetFalse)]
    pub export_pdf: bool,

    /// Agent QA review runs by default; pass --no-review to skip it
    #[arg(long = "review", default_value_t = true)]
    #[arg(long = "no-review", action = ArgAction::SetFalse)]
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

    #[arg(long, env = "CURSOR_API_KEY")]
    pub api_key: Option<String>,

    /// Streaming agent output is on by default; pass --no-stream to disable it
    #[arg(long = "stream", default_value_t = true)]
    #[arg(long = "no-stream", action = ArgAction::SetFalse)]
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

    #[arg(long, env = "CURSOR_API_KEY")]
    pub api_key: Option<String>,

    /// Streaming agent output is on by default; pass --no-stream to disable it
    #[arg(long = "stream", default_value_t = true)]
    #[arg(long = "no-stream", action = ArgAction::SetFalse)]
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AudienceCli {
    #[default]
    Youth,
    Adult,
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

    /// Resolve (from, to) from flags. Returns None if interactive form should open.
    pub fn range(&self) -> anyhow::Result<Option<(u32, u32)>> {
        match (self.from, self.to, self.study) {
            (Some(f), Some(t), None) => {
                if t < f {
                    anyhow::bail!("--to must be >= --from");
                }
                Ok(Some((f, t)))
            }
            (None, None, Some(n)) => Ok(Some((n, n))),
            (Some(f), Some(t), Some(n)) => {
                if f != t || f != n {
                    anyhow::bail!("Do not combine -n with a range where from ≠ to");
                }
                Ok(Some((n, n)))
            }
            (Some(_), None, _) | (None, Some(_), _) => {
                anyhow::bail!("Batch mode requires both --from and --to (or use -n for one study)")
            }
            (None, None, None) => Ok(None),
        }
    }
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
