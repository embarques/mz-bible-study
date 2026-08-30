//! Prepare job model + page math.

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::scans;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    #[default]
    Youth,
    Adult,
}

impl Audience {
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::Youth => "youth",
            Audience::Adult => "adult",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrepareJob {
    pub pdf: Option<PathBuf>,
    pub from: u32,
    pub to: u32,
    pub audience: Audience,
    pub prepare_only: bool,
    pub export_pdf: bool,
    pub review: bool,
    pub template: Option<PathBuf>,
    pub stream: bool,
    pub stop_on_error: bool,
}

impl PrepareJob {
    pub fn resolve_pdf(&self) -> Result<PathBuf> {
        if let Some(p) = &self.pdf {
            if !p.exists() {
                bail!("PDF not found: {}", p.display());
            }
            return Ok(p.canonicalize().unwrap_or_else(|_| p.clone()));
        }
        let list = scans::list_pending_pdfs()?;
        match list.len() {
            0 => bail!(
                "No PDF found. Put a scan PDF in scans/ (not scans/complete/ or scans/error/), \
                 or pass --pdf PATH."
            ),
            1 => Ok(list[0].clone()),
            _ => bail!(
                "Multiple PDFs in scans/ ({}). Pass --pdf PATH to choose one:\n{}",
                list.len(),
                list.iter()
                    .map(|p| format!("  - {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    pub fn echo_plan(&self) {
        let pdf_disp = self
            .pdf
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(auto from scans/)".into());
        println!("Plan:");
        println!("  PDF:     {pdf_disp}");
        println!("  Range:   {}–{}", self.from, self.to);
        println!(
            "  Last study ({}) has no Próximo",
            self.to
        );
        println!("  Audience: {}", self.audience.as_str());
        println!(
            "  Mode:     {}",
            if self.prepare_only {
                "prepare-only"
            } else {
                "prepare + build (+ PDF if enabled)"
            }
        );
        println!(
            "  Review:   {}",
            if self.review { "yes" } else { "no" }
        );
        println!("  Output:   studies/{}/  and  bible-studies/", self.audience.as_str());
    }
}

/// 3 content pages per estudio, sequential from `from`.
pub fn default_pages(study: u32, from: u32) -> (u32, u32) {
    let start = (study.saturating_sub(from)) * 3 + 1;
    (start, start + 2)
}

pub fn pages_for_images(study: u32, from: u32, omit_proximo: bool) -> Vec<u32> {
    let (a, b) = default_pages(study, from);
    let mut v: Vec<u32> = (a..=b).collect();
    if !omit_proximo {
        let (na, _) = default_pages(study + 1, from);
        // first page of next block for próximo title
        if !v.contains(&na) {
            v.push(na);
        }
    }
    v
}

/// Check path is under scans/ tray semantics.
pub fn is_under_scans(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "scans")
}
