//! Scan tray: inbox in `scans/`, archive successes to `pending/` or `complete/`, log failures.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;

/// Subfolders under `scans/` — not treated as inbox PDFs.
const TRAY_DIRS: &[&str] = &["complete", "error", "pending"];

/// List `*.pdf` directly in `scans/` (inbox only — not `pending/`, `complete/`, or `error/`).
pub fn list_inbox_pdfs() -> Result<Vec<PathBuf>> {
    list_pdfs_in_dir(&paths::scans_dir()?, TRAY_DIRS)
}

/// Back-compat alias — lists the **inbox**, not `scans/pending/`.
pub fn list_pending_pdfs() -> Result<Vec<PathBuf>> {
    list_inbox_pdfs()
}

fn list_pdfs_in_dir(dir: &Path, skip_subdirs: &[&str]) -> Result<Vec<PathBuf>> {
    ensure_tray(dir)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if skip_subdirs
            .iter()
            .any(|d| name.eq_ignore_ascii_case(d))
        {
            continue;
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

pub fn ensure_tray(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir.join("complete"))?;
    fs::create_dir_all(dir.join("error"))?;
    fs::create_dir_all(dir.join("pending"))?;
    Ok(())
}

/// After a **successful** prepare when more estudios remain in the PDF.
pub fn move_to_pending(pdf: &Path) -> Result<PathBuf> {
    move_pdf_to_tray(pdf, "pending")
}

pub fn move_to_complete(pdf: &Path) -> Result<PathBuf> {
    move_pdf_to_tray(pdf, "complete")
}

fn move_pdf_to_tray(pdf: &Path, tray: &str) -> Result<PathBuf> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let dest = unique_dest(dir.join(tray), pdf)?;
    fs::rename(pdf, &dest)
        .or_else(|_| {
            fs::copy(pdf, &dest)?;
            fs::remove_file(pdf)?;
            Ok::<(), anyhow::Error>(())
        })
        .with_context(|| format!("move {} → {}", pdf.display(), dest.display()))?;
    println!("Moved to {}", dest.display());
    Ok(dest)
}

/// Move a PDF into `scans/error/`.
///
/// **Do not call from prepare on build/agent failure.** An unfinished estudio
/// must leave the PDF in `scans/` for retry. Use only for explicit operator
/// archive (corrupt scan, abandoned job) — never mid-prepare.
#[allow(dead_code)]
pub fn move_to_error_tray(pdf: &Path) -> Result<PathBuf> {
    move_pdf_to_tray(pdf, "error")
}

/// On prepare failure: write a `.log` under `scans/error/`. The PDF stays in
/// `scans/` — prepare must not call [`move_to_error_tray`] while the estudio
/// is incomplete.
pub fn write_error_log(pdf: &Path, log: &str) -> Result<PathBuf> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let log_path = unique_log_dest(&dir.join("error"), pdf)?;
    fs::write(&log_path, log)?;
    eprintln!("Error log: {}", log_path.display());
    Ok(log_path)
}

/// Resolve a scan PDF path: exact file, inbox under `scans/`, then `pending/` / `complete/` by basename.
pub fn resolve_scan_pdf(requested: &Path) -> Option<PathBuf> {
    if requested.is_file() {
        return Some(requested.to_path_buf());
    }
    let scans_dir = paths::scans_dir().ok()?;
    let under_scans = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        scans_dir.join(requested)
    };
    if under_scans.is_file() {
        return Some(under_scans);
    }
    let file_name = requested
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if file_name.is_empty() {
        return None;
    }
    find_in_pending_tray(file_name).or_else(|| find_in_complete_tray(file_name))
}

/// Locate a PDF archived under `scans/pending/` (same basename / `-N` variants).
pub fn find_in_pending_tray(file_name: &str) -> Option<PathBuf> {
    find_in_tray("pending", file_name)
}

/// Locate a PDF archived under `scans/complete/`.
pub fn find_in_complete_tray(file_name: &str) -> Option<PathBuf> {
    find_in_tray("complete", file_name)
}

fn find_in_tray(tray: &str, file_name: &str) -> Option<PathBuf> {
    let dir = paths::scans_dir().ok()?;
    let tray_dir = dir.join(tray);
    if !tray_dir.is_dir() {
        return None;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    let mut matches: Vec<PathBuf> = Vec::new();
    let rd = fs::read_dir(&tray_dir).ok()?;
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

fn unique_log_dest(error_dir: &Path, pdf: &Path) -> Result<PathBuf> {
    let stem = pdf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scan");
    let mut dest = error_dir.join(format!("{stem}.log"));
    if !dest.exists() {
        return Ok(dest);
    }
    for i in 1..1000 {
        dest = error_dir.join(format!("{stem}-{i}.log"));
        if !dest.exists() {
            return Ok(dest);
        }
    }
    anyhow::bail!("could not find unique log name in {}", error_dir.display())
}

fn unique_dest(dir: PathBuf, pdf: &Path) -> Result<PathBuf> {
    let name = pdf.file_name().context("pdf has no file name")?;
    let mut dest = dir.join(name);
    if !dest.exists() {
        return Ok(dest);
    }
    let stem = pdf.file_stem().and_then(|s| s.to_str()).unwrap_or("scan");
    let ext = pdf.extension().and_then(|s| s.to_str()).unwrap_or("pdf");
    for i in 1..1000 {
        dest = dir.join(format!("{stem}-{i}.{ext}"));
        if !dest.exists() {
            return Ok(dest);
        }
    }
    anyhow::bail!("could not find unique name in {}", dir.display())
}
