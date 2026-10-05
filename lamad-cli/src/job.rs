//! Prepare job model + page math.

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;
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
    /// When true, ignore local JSON/images and re-run the cloud agent.
    pub force_prepare: bool,
    /// When true, (re)generate images from existing JSON via OpenAI (overwrite).
    pub gen_images: bool,
    pub export_pdf: bool,
    pub review: bool,
    pub template: Option<PathBuf>,
    pub stream: bool,
    pub stop_on_error: bool,
}

impl PrepareJob {
    pub fn resolve_pdf(&self) -> Result<PathBuf> {
        if let Some(p) = &self.pdf {
            if let Some(found) = scans::resolve_scan_pdf(p) {
                if found != *p && crate::progress::at_least(1) {
                    crate::progress::info(format!(
                        "Using scan PDF from {} (archived after a previous prepare)",
                        found.display()
                    ));
                }
                return Ok(found.canonicalize().unwrap_or(found));
            }
            bail!("{}", missing_scan_pdf_message(p, self.from, self.to));
        }
        let list = scans::list_pending_pdfs()?;
        match list.len() {
            0 => bail!(
                "No hay PDF de scan en scans/pending/.\n\
                 Pon el documento en scans/pending/ (no en complete/ ni error/), \
                 o pasa --pdf con la ruta al PDF.\n\
                 Ejemplo: --pdf \"scans/pending/Bible Study 4 - Adult.pdf\""
            ),
            1 => Ok(list[0].clone()),
            _ => bail!(
                "Hay varios PDFs en scans/pending/ ({}). Pasa --pdf con el nombre del documento:\n{}",
                list.len(),
                list.iter()
                    .map(|p| {
                        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                        format!("  - {name}\n      ({})", p.display())
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    pub fn echo_plan(&self, host_images: bool) {
        // Default: one info line. Full plan at -v+.
        if !crate::progress::at_least(1) {
            crate::progress::info(format!(
                "prepare estudios {}–{} ({}){}",
                self.from,
                self.to,
                self.audience.as_str(),
                if host_images {
                    " · host images"
                } else {
                    ""
                }
            ));
            return;
        }
        let pdf_disp = self
            .pdf
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(auto from scans/pending/)".into());
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
        if self.force_prepare {
            println!("  Resume:   off (--force-prepare — agent will re-create JSON + images)");
        } else if self.gen_images {
            println!(
                "  Resume:   --gen-images — keep JSON, (re)generate images via OpenAI, then build"
            );
        } else {
            println!(
                "  Resume:   on — if studies/{}/{{N}} already has complete JSON + images, \
                 skip the agent and rebuild",
                self.audience.as_str()
            );
        }
        if host_images {
            println!("  Images:   host parallel (OpenAI) — fast path ON");
        } else {
            println!(
                "  Images:   cloud agent (slow) — set openai_api_key / OPENAI_API_KEY for fast path"
            );
        }
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
        let n = self.to.saturating_sub(self.from).saturating_add(1);
        crate::progress::print_prepare_eta(self.audience, n, self.prepare_only, host_images);
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

/// Clear error when `--pdf` points at a missing scan document.
fn missing_scan_pdf_message(path: &Path, from: u32, to: u32) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("documento.pdf");
    let estudio = if from == to {
        format!("estudio {from}")
    } else {
        format!("estudios {from}–{to}")
    };

    let mut msg = format!(
        "El documento de scan no está.\n\
         Documento: {name}\n\
         Para: {estudio}\n\
         Ruta pedida: {}",
        path.display()
    );

    if let Some(in_pending) = scans::find_in_pending_tray(name) {
        msg.push_str(&format!(
            "\n\
             Hallado en scans/pending/ (inbox — aún no procesado por completo):\n\
               {}\n\
             Usa:\n\
               --pdf \"{}\"",
            in_pending.display(),
            in_pending.display()
        ));
    } else if let Some(in_complete) = scans::find_in_complete_tray(name) {
        msg.push_str(&format!(
            "\n\
             Hallado en scans/complete/ (todo el PDF ya se preparó):\n\
               {}",
            in_complete.display()
        ));
    } else if let Some(in_error) = find_in_error_tray(name) {
        msg.push_str(&format!(
            "\n\
             Hallado en scans/error/ (falló un prepare anterior):\n\
               {}\n\
             Muévelo de vuelta a scans/pending/ o usa:\n\
               --pdf \"{}\"",
            in_error.display(),
            in_error.display()
        ));
    } else {
        msg.push_str(
            "\n\
             Pon el PDF en scans/pending/ (no en complete/ ni error/) \
             y vuelve a correr prepare.",
        );
    }
    msg
}

/// Same basename (or `stem-N.pdf` unique_dest variants) under `scans/error/`.
fn find_in_error_tray(file_name: &str) -> Option<PathBuf> {
    let dir = paths::scans_dir().ok()?;
    let error_dir = dir.join("error");
    if !error_dir.is_dir() {
        return None;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    let mut matches: Vec<PathBuf> = Vec::new();
    let rd = fs::read_dir(&error_dir).ok()?;
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let is_pdf = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);
        if !is_pdf {
            continue;
        }
        let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // Exact name, or unique_dest variants: "Bible Study 4 - Adult-1.pdf"
        let is_variant = n
            .strip_prefix(stem)
            .map(|rest| {
                rest.eq_ignore_ascii_case(".pdf")
                    || (rest.starts_with('-') && rest.to_ascii_lowercase().ends_with(".pdf"))
            })
            .unwrap_or(false);
        if n.eq_ignore_ascii_case(file_name) || is_variant {
            matches.push(p);
        }
    }
    matches.sort();
    // Prefer exact basename, else last sorted variant (highest -N)
    matches
        .iter()
        .find(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.eq_ignore_ascii_case(file_name))
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| matches.pop())
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

    #[test]
    fn missing_scan_message_includes_document_name() {
        let msg = missing_scan_pdf_message(
            Path::new("scans/Bible Study 4 - Adult.pdf"),
            4,
            4,
        );
        assert!(msg.contains("El documento de scan no está"));
        assert!(msg.contains("Bible Study 4 - Adult.pdf"));
        assert!(msg.contains("estudio 4"));
    }

    #[test]
    fn resolve_scan_pdf_finds_pending_by_basename() {
        let scans = paths::scans_dir().expect("scans dir");
        let _ = scans::ensure_tray(&scans);
        let pending = scans.join("pending");
        let name = "lamad-resolve-test.pdf";
        let pending_file = pending.join(name);
        std::fs::write(&pending_file, b"%PDF-test").unwrap();
        let pending_rel = Path::new("scans").join("pending").join(name);
        let found = scans::resolve_scan_pdf(&pending_rel)
            .or_else(|| scans::resolve_scan_pdf(Path::new(name)));
        assert_eq!(
            found.as_ref().and_then(|p| p.file_name()),
            pending_file.file_name()
        );
        let _ = std::fs::remove_file(&pending_file);
    }
}
