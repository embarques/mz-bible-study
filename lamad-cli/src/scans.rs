//! Scan tray: unprocessed PDFs in `pending/`, finished in `complete/`, logs in `error/`.
//!
//! Do **not** keep PDFs in `scans/` root — drop new scans in `scans/pending/`.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;

/// List `*.pdf` in `scans/pending/` (the only inbox).
pub fn list_pending_pdfs() -> Result<Vec<PathBuf>> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    list_pdfs_in_dir(&dir.join("pending"))
}

fn list_pdfs_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path
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

pub fn pending_dir() -> Result<PathBuf> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    Ok(dir.join("pending"))
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

/// Move a legacy PDF from `scans/` root into `scans/pending/`.
pub fn relocate_root_pdf_to_pending(pdf: &Path) -> Result<PathBuf> {
    let scans_dir = paths::scans_dir()?;
    ensure_tray(&scans_dir)?;
    if !is_scans_root_pdf(pdf, &scans_dir) {
        return Ok(pdf.to_path_buf());
    }
    let dest = unique_dest(pending_dir()?, pdf)?;
    fs::rename(pdf, &dest)
        .or_else(|_| {
            fs::copy(pdf, &dest)?;
            fs::remove_file(pdf)?;
            Ok::<(), anyhow::Error>(())
        })
        .with_context(|| format!("move {} → pending/", pdf.display()))?;
    eprintln!(
        "Moved {} → {} (use scans/pending/ for new scans — not scans/ root)",
        pdf.display(),
        dest.display()
    );
    Ok(dest)
}

fn is_scans_root_pdf(pdf: &Path, scans_dir: &Path) -> bool {
    pdf.parent()
        .map(|p| p == scans_dir)
        .unwrap_or(false)
        && pdf.is_file()
}

/// Ensure the PDF is in `pending/` before prepare. Rejects `complete/` unless re-run is explicit.
pub fn normalize_pending_inbox(pdf: &Path) -> Result<PathBuf> {
    let scans_dir = paths::scans_dir()?;
    ensure_tray(&scans_dir)?;
    let pending = scans_dir.join("pending");
    let complete = scans_dir.join("complete");

    if pdf.starts_with(&pending) {
        return Ok(pdf.to_path_buf());
    }
    if pdf.starts_with(&complete) {
        bail!(
            "Este PDF ya está en scans/complete/ (todo el archivo se preparó):\n  {}\n\
             Muévelo de vuelta a scans/pending/ solo si quieres regenerar.",
            pdf.display()
        );
    }
    if is_scans_root_pdf(pdf, &scans_dir) {
        return relocate_root_pdf_to_pending(pdf);
    }
    Ok(pdf.to_path_buf())
}

/// Move a PDF into `scans/error/`.
///
/// **Do not call from prepare on build/agent failure.** Use only for explicit operator archive.
#[allow(dead_code)]
pub fn move_to_error_tray(pdf: &Path) -> Result<PathBuf> {
    move_pdf_to_tray(pdf, "error")
}

/// On prepare failure: write a `.log` under `scans/error/`. The PDF stays in `pending/`.
pub fn write_error_log(pdf: &Path, log: &str) -> Result<PathBuf> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let log_path = unique_log_dest(&dir.join("error"), pdf)?;
    fs::write(&log_path, log)?;
    eprintln!("Error log: {}", log_path.display());
    Ok(log_path)
}

/// Resolve `--pdf` / auto-pick: exact path, `scans/pending/name`, or basename in `pending/`.
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
    // `scans/Foo.pdf` → `scans/pending/Foo.pdf`
    if let Some(name) = requested.file_name() {
        let in_pending = scans_dir.join("pending").join(name);
        if in_pending.is_file() {
            return Some(in_pending);
        }
        if let Some(found) = find_in_pending_tray(name.to_str().unwrap_or("")) {
            return Some(found);
        }
    }
    None
}

pub fn find_in_pending_tray(file_name: &str) -> Option<PathBuf> {
    find_in_tray("pending", file_name)
}

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
