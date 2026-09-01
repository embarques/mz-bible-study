//! Locate an estudio inside a multi-study scan PDF.
//!
//! Strategy:
//! 1. Prefer `pdftotext` when the PDF has a text layer.
//! 2. Else rasterize pages + `tesseract` OCR.
//! 3. Detect title pages (`Base bíblica` + `Idea principal`).
//! 4. Read study numbers (`ESTUDIO N`, `N LECTURA BÍBLICA`, …).
//! 5. Fill gaps along the ordered title-page sequence from any anchors.
//!
//! Returns absolute 1-based PDF page ranges (3 pages per estudio).

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pdf_page_count;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyPages {
    pub study: u32,
    /// Inclusive content pages (always 3 for Senda de Vida).
    pub pages: (u32, u32),
    /// First page of the next study’s title block, when present in the PDF.
    pub proximo_title_page: Option<u32>,
}

/// Find absolute PDF pages for `study` inside `pdf`.
pub fn discover_study_pages(
    pdf: &Path,
    study: u32,
    pdftoppm_path: Option<&Path>,
) -> Result<StudyPages> {
    let page_count = pdf_page_count(pdf)?;
    if page_count == 0 {
        bail!("PDF has no pages: {}", pdf.display());
    }

    let texts = page_texts(pdf, page_count, pdftoppm_path)?;
    let mut title_pages = find_title_pages(&texts);
    // Also accept pages that clearly show "N LECTURA B…" as title anchors
    // even when Base/Idea OCR is weak.
    for (i, t) in texts.iter().enumerate() {
        let p = (i as u32) + 1;
        if extract_study_number(t).is_some() && !title_pages.contains(&p) {
            // Only treat as title if it looks like a study open (not mid-body).
            let low = t.to_lowercase();
            if low.contains("lectura") || low.contains("base b") {
                title_pages.push(p);
            }
        }
    }
    title_pages.sort_unstable();
    title_pages.dedup();

    if title_pages.is_empty() {
        bail!(
            "Could not find any estudio title pages in {}. \
             Title pages usually contain “Base bíblica” and “Idea principal”. \
             Pass --from F -n {study} if you know the first estudio on page 1 \
             (e.g. --from 23 -n {study}).",
            pdf.display()
        );
    }

    let mut numbered: BTreeMap<u32, u32> = BTreeMap::new(); // page -> study
    for &p in &title_pages {
        let idx = (p as usize).saturating_sub(1);
        if let Some(t) = texts.get(idx) {
            if let Some(n) = extract_study_number(t) {
                numbered.insert(p, n);
            }
        }
    }
    // Fallback: scan every page head for a number and snap to nearest title page.
    if numbered.is_empty() {
        for (i, t) in texts.iter().enumerate() {
            let p = (i as u32) + 1;
            if let Some(n) = extract_study_number(t) {
                let snap = title_pages
                    .iter()
                    .min_by_key(|tp| (**tp as i64 - p as i64).unsigned_abs())
                    .copied()
                    .unwrap_or(p);
                numbered.entry(snap).or_insert(n);
            }
        }
    }

    let filled = fill_sequence(&title_pages, &numbered)?;
    let start = filled
        .iter()
        .find(|(_, n)| **n == study)
        .map(|(p, _)| *p)
        .ok_or_else(|| {
            let known: Vec<String> = filled
                .iter()
                .map(|(p, n)| format!("estudio {n} @ page {p}"))
                .collect();
            anyhow::anyhow!(
                "Estudio {study} not found in {}. Detected: {}. \
                 Pass --from F -n {study} if page math is known.",
                pdf.display(),
                if known.is_empty() {
                    "(none)".into()
                } else {
                    known.join(", ")
                }
            )
        })?;

    let end = start + 2;
    if end as usize > page_count {
        bail!(
            "Estudio {study} starts at page {start}, but PDF only has {page_count} pages \
             (need 3 content pages)."
        );
    }

    let next_title = start + 3;
    let proximo_title_page = if (next_title as usize) <= page_count {
        Some(next_title)
    } else {
        None
    };

    println!(
        "  Discovered estudio {study} at PDF pages {start}–{end}{}",
        match proximo_title_page {
            Some(p) => format!(" (próximo title page {p})"),
            None => " (no Próximo in PDF)".into(),
        }
    );

    Ok(StudyPages {
        study,
        pages: (start, end),
        proximo_title_page,
    })
}

fn page_texts(
    pdf: &Path,
    page_count: usize,
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<String>> {
    // Fast path: native text layer.
    if let Ok(texts) = pdftotext_pages(pdf, page_count) {
        let nonempty = texts.iter().filter(|t| t.chars().count() > 40).count();
        if nonempty >= (page_count / 3).max(1) {
            println!("  Discover: using pdftotext ({nonempty}/{page_count} text pages)");
            return Ok(texts);
        }
    }

    let tesseract = which("tesseract").context(
        "Scan PDF has little/no text layer; need `tesseract` on PATH to discover -n studies \
         (brew install tesseract tesseract-lang). Or pass --from F -n N for page math.",
    )?;

    let root = crate::paths::generated_dir()?.join("discover");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;

    let bin = crate::pdftoppm::resolve(pdftoppm_path)?;
    println!(
        "  Discover: rasterizing {page_count} pages @200dpi + tesseract ({})",
        tesseract.display()
    );

    let mut texts = Vec::with_capacity(page_count);
    for p in 1..=page_count as u32 {
        let prefix = root.join(format!("page-{p}"));
        let mut cmd = Command::new(&bin);
        crate::pdftoppm::configure_command(&mut cmd, &bin);
        let status = cmd
            .args([
                "-png",
                "-r",
                "200",
                "-f",
                &p.to_string(),
                "-l",
                &p.to_string(),
                "-singlefile",
                pdf.to_str().unwrap_or(""),
                prefix.to_str().unwrap_or(""),
            ])
            .status()
            .with_context(|| format!("pdftoppm page {p}"))?;
        if !status.success() {
            bail!("pdftoppm failed for page {p}");
        }
        let png = root.join(format!("page-{p}.png"));
        let text = tesseract_ocr(&tesseract, &png)?;
        texts.push(text);
        if p % 3 == 0 || p as usize == page_count {
            print!("\r  Discover: OCR {p}/{page_count}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
    }
    println!();
    Ok(texts)
}

fn pdftotext_pages(pdf: &Path, page_count: usize) -> Result<Vec<String>> {
    let bin = which("pdftotext").context("pdftotext not on PATH")?;
    let mut out = Vec::with_capacity(page_count);
    for p in 1..=page_count {
        let output = Command::new(&bin)
            .args([
                "-f",
                &p.to_string(),
                "-l",
                &p.to_string(),
                "-layout",
                pdf.to_str().unwrap_or(""),
                "-",
            ])
            .output()
            .with_context(|| format!("pdftotext page {p}"))?;
        if !output.status.success() {
            bail!(
                "pdftotext failed page {p}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        out.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Ok(out)
}

fn tesseract_ocr(bin: &Path, png: &Path) -> Result<String> {
    // Merge a couple of PSM modes — badge/header layout varies.
    let mut merged = String::new();
    for lang in ["spa+eng", "eng"] {
        let mut lang_ok = false;
        for psm in ["6", "4"] {
            let output = Command::new(bin)
                .args([
                    png.to_str().unwrap_or(""),
                    "stdout",
                    "-l",
                    lang,
                    "--psm",
                    psm,
                ])
                .output();
            let Ok(output) = output else { continue };
            if !output.status.success() {
                continue;
            }
            let t = String::from_utf8_lossy(&output.stdout);
            if t.trim().is_empty() {
                continue;
            }
            lang_ok = true;
            if !merged.is_empty() {
                merged.push('\n');
            }
            merged.push_str(&t);
        }
        if lang_ok {
            break; // spa+eng or eng — don't double every page
        }
    }
    Ok(merged)
}

fn find_title_pages(texts: &[String]) -> Vec<u32> {
    texts
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            if is_title_page(t) {
                Some((i as u32) + 1)
            } else {
                None
            }
        })
        .collect()
}

fn is_title_page(t: &str) -> bool {
    let low = t.to_lowercase();
    let has_base = Regex::new(r"base\s*b[ií1l]bl")
        .unwrap()
        .is_match(&low);
    let has_idea = Regex::new(r"idea\s*p").unwrap().is_match(&low);
    let has_lectura = low.contains("lectura");
    let has_maestro = Regex::new(r"ideas\s+para\s+el\s+maestro")
        .unwrap()
        .is_match(&low);
    (has_base && has_idea) || (has_base && has_lectura && !has_maestro)
}

fn extract_study_number(t: &str) -> Option<u32> {
    // Drop common false friends.
    let cleaned = Regex::new(r"(?is)desarrollo\s+del\s+estudio")
        .unwrap()
        .replace_all(t, " ");
    let cleaned = Regex::new(r"(?is)ideas\s+para\s+el\s+maestro")
        .unwrap()
        .replace_all(&cleaned, " ");

    // Prefer matches in the first portion (badge / header).
    let head: String = cleaned.chars().take(800).collect();

    let patterns = [
        r"(?is)\bestudio\b\s*[:\-]?\s*(\d{1,3})\b",
        r"(?is)\bestudio\b[\s\|]*\r?\n\s*(\d{1,3})\b",
        // Senda de Vida: "24 LECTURA BÍBLICA" when badge OCR drops ESTUDIO
        r"(?is)(?:^|[^\d])(\d{1,3})\s+LECTURA\s+B",
        r"(?is)\bestudio\b[^\d]{0,12}(\d{1,3})\b",
    ];
    for pat in patterns {
        let re = Regex::new(pat).ok()?;
        if let Some(c) = re.captures(&head) {
            let n: u32 = c.get(1)?.as_str().parse().ok()?;
            if (1..=200).contains(&n) {
                return Some(n);
            }
        }
    }
    // Whole page fallback for ESTUDIO N only (avoid mid-page verse traps on LECTURA).
    let re = Regex::new(r"(?is)\bestudio\b\s*[:\-]?\s*(\d{1,3})\b").ok()?;
    if let Some(c) = re.captures(&cleaned) {
        let n: u32 = c.get(1)?.as_str().parse().ok()?;
        if (1..=200).contains(&n) {
            return Some(n);
        }
    }
    None
}

/// Given ordered title pages and any known page→study anchors, fill the rest
/// assuming consecutive estudios on consecutive title pages.
fn fill_sequence(
    title_pages: &[u32],
    numbered: &BTreeMap<u32, u32>,
) -> Result<BTreeMap<u32, u32>> {
    if title_pages.is_empty() {
        return Ok(BTreeMap::new());
    }

    // Pick a consistent anchor (first numbered title page).
    let Some((anchor_page, anchor_study)) = title_pages
        .iter()
        .find_map(|p| numbered.get(p).map(|n| (*p, *n)))
    else {
        bail!(
            "Found {} title page(s) but could not read any ESTUDIO number from OCR. \
             Install/update tesseract languages (spa) or pass --from F -n N \
             (F = first estudio on PDF page 1).",
            title_pages.len()
        );
    };

    let anchor_idx = title_pages
        .iter()
        .position(|p| *p == anchor_page)
        .unwrap();

    let mut out = BTreeMap::new();
    for (i, &page) in title_pages.iter().enumerate() {
        let study = if i >= anchor_idx {
            anchor_study + (i - anchor_idx) as u32
        } else {
            anchor_study.saturating_sub((anchor_idx - i) as u32)
        };
        // Prefer direct OCR when present and consistent-ish.
        if let Some(&ocr_n) = numbered.get(&page) {
            out.insert(page, ocr_n);
        } else {
            out.insert(page, study);
        }
    }

    // If OCR contradicts sequence badly, trust sequence from anchor.
    for (i, &page) in title_pages.iter().enumerate() {
        let expected = if i >= anchor_idx {
            anchor_study + (i - anchor_idx) as u32
        } else {
            anchor_study.saturating_sub((anchor_idx - i) as u32)
        };
        out.insert(page, expected);
    }

    Ok(out)
}

fn which(name: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(PathBuf::from(p))
    }
}

/// Human-readable line for `lamad doctor`.
pub fn doctor_line() -> String {
    match which("tesseract") {
        Some(p) => format!("tesseract: OK — {} (used for -n discover)", p.display()),
        None => "tesseract: MISSING — needed for -n discover on scan PDFs (brew install tesseract)"
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_title_and_number_from_sample_ocr() {
        let title = r#"
Unidad IV
SUPERA EL RECHAZO
24 LECTURA BÍBLICA
e Base bíblica
Génesis 37:3-28
e Idea principal
Algo
e Propósitos
1. Uno
"#;
        assert!(is_title_page(title));
        assert_eq!(extract_study_number(title), Some(24));

        let mid = r#"
IDEAS PARA EL MAESTRO
DESARROLLO DEL ESTUDIO
1. CAUSAS
"#;
        assert!(!is_title_page(mid));
    }

    #[test]
    fn fills_sequence_from_one_anchor() {
        let titles = vec![1, 4, 7, 10];
        let mut numbered = BTreeMap::new();
        numbered.insert(4, 24);
        let filled = fill_sequence(&titles, &numbered).unwrap();
        assert_eq!(filled.get(&1), Some(&23));
        assert_eq!(filled.get(&4), Some(&24));
        assert_eq!(filled.get(&7), Some(&25));
        assert_eq!(filled.get(&10), Some(&26));
    }

    #[test]
    #[ignore = "needs sample PDF + tesseract; run with --ignored"]
    fn discovers_estudio_25_in_sample_pdf() {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Bible Study 23-26.pdf");
        if !pdf.exists() {
            eprintln!("skip: sample PDF missing at {}", pdf.display());
            return;
        }
        let found = discover_study_pages(&pdf, 25, Some(Path::new("/usr/local/bin/pdftoppm")))
            .expect("discover 25");
        assert_eq!(found.pages, (7, 9), "estudio 25 should be pages 7-9");
        assert_eq!(found.proximo_title_page, Some(10));
    }
}
