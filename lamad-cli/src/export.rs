//! Export PPTX → PDF via Microsoft PowerPoint AppleScript (macOS).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Export one pptx to a sibling PDF. Returns the PDF path.
pub fn export_one(pptx: &Path) -> Result<PathBuf> {
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

    let pdf = pptx.with_extension("pdf");
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

    let meta = std::fs::metadata(&pdf).with_context(|| format!("PDF missing: {}", pdf.display()))?;
    if meta.len() < 1000 {
        bail!("PDF missing or too small: {}", pdf.display());
    }
    // Caller (build_study) owns the spinner; keep a quiet OK for standalone export-pdf.
    Ok(pdf)
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
