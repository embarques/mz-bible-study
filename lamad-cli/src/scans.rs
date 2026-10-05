//! Scan tray: list PDFs in scans/, archive successes to complete/, log failures.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;

/// List `*.pdf` directly in scans/ (not complete/ or error/, no recursion).
pub fn list_pending_pdfs() -> Result<Vec<PathBuf>> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.eq_ignore_ascii_case("complete") || name.eq_ignore_ascii_case("error") {
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
    Ok(())
}

pub fn move_to_complete(pdf: &Path) -> Result<PathBuf> {
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let dest = unique_dest(dir.join("complete"), pdf)?;
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
    let dir = paths::scans_dir()?;
    ensure_tray(&dir)?;
    let dest = unique_dest(dir.join("error"), pdf)?;
    fs::rename(pdf, &dest)
        .or_else(|_| {
            fs::copy(pdf, &dest)?;
            fs::remove_file(pdf)?;
            Ok::<(), anyhow::Error>(())
        })
        .with_context(|| format!("move {} → error", pdf.display()))?;
    println!("Moved to {}", dest.display());
    Ok(dest)
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
