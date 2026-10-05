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
    /// Estudio number on PDF page 1 (page-math origin). Ignored when `discover`.
    pub pdf_from: u32,
    /// First estudio to prepare (inclusive).
    pub from: u32,
    /// Last estudio to prepare (inclusive).
    pub to: u32,
    /// When true (typical `-n N` alone), locate the estudio via OCR instead of page math.
    pub discover: bool,
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
        if self.discover {
            println!(
                "  Studies: {}–{} (discover pages via OCR)",
                self.from, self.to
            );
        } else {
            println!(
                "  Studies: {}–{} (PDF page 1 = estudio {})",
                self.from, self.to, self.pdf_from
            );
        }
        println!("  Audience: {}", self.audience.as_str());
        if self.prepare_only {
            println!("  Deliver:  JSON + images only (--prepare-only — no PowerPoint)");
        } else {
            println!(
                "  Deliver:  PowerPoint +{} in bible-studies/",
                if self.export_pdf { " PDF" } else { "" }
            );
            println!("            (JSON + images in studies/{}/)", self.audience.as_str());
        }
        println!(
            "  Review:   {}",
            if self.review {
                "yes (cloud QA after build)"
            } else {
                "no — run `lamad review` later if you want a checklist"
            }
        );
    }
}

/// 3 content pages per estudio, sequential from `from` (PDF page 1 = start of `from`).
pub fn default_pages(study: u32, from: u32) -> (u32, u32) {
    let start = (study.saturating_sub(from)) * 3 + 1;
    (start, start + 2)
}

/// Minimum PDF pages needed to prepare `from..=to` (3 pages each).
pub fn min_pages_for_range(from: u32, to: u32) -> u32 {
    ((to.saturating_sub(from)) + 1) * 3
}

/// True when the PDF has at least the first page of the study after `study`
/// (title page for Próximo). `from` is the study that starts on PDF page 1.
pub fn pdf_has_proximo_pages(study: u32, from: u32, page_count: usize) -> bool {
    let next_title = default_pages(study, from).1 + 1;
    (next_title as usize) <= page_count
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_math_and_proximo_on_long_pdf() {
        assert_eq!(default_pages(23, 23), (1, 3));
        assert_eq!(default_pages(24, 23), (4, 6));
        assert_eq!(min_pages_for_range(23, 23), 3);
        assert_eq!(min_pages_for_range(23, 26), 12);
        // 12-page PDF of 23–26: -n 23 can still read próximo from page 4
        assert!(pdf_has_proximo_pages(23, 23, 12));
        assert!(!pdf_has_proximo_pages(26, 23, 12));
        assert!(!pdf_has_proximo_pages(23, 23, 3));
    }
}
