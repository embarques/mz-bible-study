//! Clap CLI definitions.

use clap::{Parser, Subcommand, ValueEnum};
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

    /// When building, also export PDF
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub export_pdf: bool,

    /// Disable PDF export
    #[arg(long = "no-export-pdf", overrides_with = "export_pdf")]
    pub no_export_pdf: bool,

    /// Run agent QA after prepare (default on)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub review: bool,

    #[arg(long = "no-review", overrides_with = "review")]
    pub no_review: bool,

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

    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub stream: bool,

    #[arg(long = "no-stream", overrides_with = "stream")]
    pub no_stream: bool,

    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub stop_on_error: bool,

    #[arg(long = "continue-on-error", overrides_with = "stop_on_error")]
    pub continue_on_error: bool,
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

    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub stream: bool,

    #[arg(long = "no-stream", overrides_with = "stream")]
    pub no_stream: bool,
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
    pub fn export_pdf_effective(&self) -> bool {
        if self.no_export_pdf {
            false
        } else {
            self.export_pdf
        }
    }

    pub fn review_effective(&self) -> bool {
        if self.no_review {
            false
        } else {
            self.review
        }
    }

    pub fn stream_effective(&self) -> bool {
        if self.no_stream {
            false
        } else {
            self.stream
        }
    }

    pub fn stop_on_error_effective(&self) -> bool {
        if self.continue_on_error {
            false
        } else {
            self.stop_on_error
        }
    }

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

    pub fn stream_effective(&self) -> bool {
        if self.no_stream {
            false
        } else {
            self.stream
        }
    }
}
