//! Export PPTX → PDF via Microsoft PowerPoint AppleScript (macOS).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::job::Audience;
use crate::paths;

/// Locate the finished PPTX for a study under `bible-studies/{audience}/{N}/powerpoint/`.
///
/// Picks the newest non-lock `*.pptx` whose name starts with `{N} - ` (or any
/// single `.pptx` in that tray). Used by `lamad export-pdf -n N` after manual
/// PowerPoint edits — re-export PDF only, no rebuild.
pub fn find_study_pptx(audience: Audience, study: u32) -> Result<PathBuf> {
    let dir = paths::study_output_dir(audience, study)?.join("powerpoint");
    if !dir.is_dir() {
        bail!(
            "no powerpoint tray for {} estudio {}: expected {}",
            audience.as_str(),
            study,
            dir.display()
        );
    }
    let prefix = format!("{study} - ");
    let all: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("pptx")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| !n.starts_with("~$"))
                    .unwrap_or(false)
        })
        .collect();
    // Prefer `{N} - *.pptx`; fall back to any non-lock pptx in the tray.
    let mut matches: Vec<PathBuf> = all
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if matches.is_empty() {
        matches = all;
    }
    if matches.is_empty() {
        bail!(
            "no .pptx found in {} — build or place the study deck there first",
            dir.display()
        );
    }
    matches.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    // Newest if several.
    Ok(matches.pop().expect("non-empty"))
}

/// Default PDF destination for a PPTX.
///
/// If the PPTX lives under `…/powerpoint/`, the PDF goes to sibling `…/pdf/`
/// with the same basename. Otherwise the PDF is written beside the PPTX.
pub fn default_pdf_path(pptx: &Path) -> PathBuf {
    let parent = pptx.parent();
    let stem_pdf = pptx.with_extension("pdf");
    let Some(name) = stem_pdf.file_name() else {
        return stem_pdf;
    };
    if let Some(parent) = parent {
        if parent
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("powerpoint"))
            .unwrap_or(false)
        {
            let pdf_dir = parent
                .parent()
                .map(|p| p.join("pdf"))
                .unwrap_or_else(|| parent.join("pdf"));
            let _ = std::fs::create_dir_all(&pdf_dir);
            return pdf_dir.join(name);
        }
    }
    stem_pdf
}

/// Export one pptx to its default PDF path (see [`default_pdf_path`]).
pub fn export_one(pptx: &Path) -> Result<PathBuf> {
    let pdf = default_pdf_path(pptx);
    export_to(pptx, &pdf)
}

/// Export one pptx to an explicit PDF path.
pub fn export_to(pptx: &Path, pdf: &Path) -> Result<PathBuf> {
    let pptx = pptx.canonicalize().unwrap_or_else(|_| pptx.to_path_buf());
    if !pptx.exists() {
        bail!("file not found: {}", pptx.display());
    }
    if pptx.extension().and_then(|e| e.to_str()) != Some("pptx") {
        bail!("not a pptx: {}", pptx.display());
    }
    if pptx
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("~$"))
        .unwrap_or(false)
    {
        bail!("skip PowerPoint lock file: {}", pptx.display());
    }

    if !cfg!(target_os = "macos") {
        bail!(
            "PDF export requires macOS + Microsoft PowerPoint (osascript). \
             PPTX was written; export later on a Mac with: lamad export-pdf \"{}\"",
            pptx.display()
        );
    }

    if let Some(parent) = pdf.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }

    let src = pptx.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let dst = pdf.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");

    let script = format!(
        r#"
tell application "Microsoft PowerPoint"
  activate
  open POSIX file "{src}"
  set thePres to active presentation
  save thePres in POSIX file "{dst}" as save as PDF
  close thePres saving no
end tell
"#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .context("osascript failed to start")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        bail!(
            "PowerPoint PDF export failed for {}: {}",
            pptx.file_name().unwrap_or_default().to_string_lossy(),
            if err.is_empty() { out } else { err }.trim()
        );
    }

    let meta = std::fs::metadata(pdf).with_context(|| format!("PDF missing: {}", pdf.display()))?;
    if meta.len() < 1000 {
        bail!("PDF missing or too small: {}", pdf.display());
    }
    Ok(pdf.to_path_buf())
}

pub fn powerpoint_available() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to (name of processes) contains \"Microsoft PowerPoint\""])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Path::new("/Applications/Microsoft PowerPoint.app").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powerpoint_tray_maps_pdf_sibling() {
        let pptx = PathBuf::from("/tmp/bible-studies/adult/7/powerpoint/7 - TITLE.pptx");
        let pdf = default_pdf_path(&pptx);
        assert_eq!(
            pdf,
            PathBuf::from("/tmp/bible-studies/adult/7/pdf/7 - TITLE.pdf")
        );
    }

    #[test]
    fn flat_pptx_keeps_sibling_pdf() {
        let pptx = PathBuf::from("/tmp/bible-studies/7 - TITLE.pptx");
        assert_eq!(
            default_pdf_path(&pptx),
            PathBuf::from("/tmp/bible-studies/7 - TITLE.pdf")
        );
    }

}
