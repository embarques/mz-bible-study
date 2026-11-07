//! Locate an estudio inside a multi-study scan PDF.
//!
//! Strategy:
//! 1. Prefer `pdftotext` when the PDF has a text layer.
//! 2. Else rasterize pages + `tesseract` OCR.
//! 3. Detect title pages (`Base bíblica` + `Idea principal` / adult Pensamiento).
//! 4. Read study numbers (`ESTUDIO N`, `N LECTURA BÍBLICA`, …).
//! 5. Fill gaps along the ordered title-page sequence from any anchors.
//!
//! Returns absolute 1-based PDF page ranges.
//!
//! **Youth:** usually 3 pages (title → next title − 1, else start+2).
//!
//! **Adult (HARD — not page-count math):**
//! - **Start** = title page with `ESTUDIO BÍBLICO N` (Base bíblica / Objetivos / …).
//! - **End** = PDF page that contains **`RESUMEN GENERAL`** (fence after
//!   DESARROLLO DEL ESTUDIO). We do **not** extract RESUMEN body; the marker
//!   only closes the content window. A PDF page may be 1-up or a 2-up book
//!   spread — page count is irrelevant.
//! - **Próximo** = next study’s title page when present; if none, omit
//!   (never invent a próximo page).

use anyhow::{bail, Context, Result};
use image::imageops::FilterType;
use image::GenericImageView;
use indicatif::ProgressBar;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::job::Audience;
use crate::pdf_page_count;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyPages {
    pub study: u32,
    /// Inclusive content PDF pages (1-based). Youth ≈3; adult = title→RESUMEN page.
    pub pages: (u32, u32),
    /// First page of the next study’s title block, when present in the PDF.
    /// Adult: only a real next title — never invented from page math.
    pub proximo_title_page: Option<u32>,
}

/// Find absolute PDF pages for `study` inside `pdf`.
pub fn discover_study_pages(
    pdf: &Path,
    study: u32,
    audience: Audience,
    pdftoppm_path: Option<&Path>,
) -> Result<StudyPages> {
    let page_count = pdf_page_count(pdf)?;
    if page_count == 0 {
        bail!("PDF has no pages: {}", pdf.display());
    }
    let texts = page_texts(pdf, page_count, pdftoppm_path)?;
    study_pages_from_texts(pdf, study, audience, &texts, page_count)
}

/// OCR/text-extract once, then resolve many estudios (batch `--from`/`--to` with discover).
///
/// **HARD:** `--from 8 --to 11` on a PDF that also contains estudio 7 **skips 7**
/// and starts at 8. Never map the first N title pages onto the requested range
/// when the PDF holds earlier estudios (filename span / OCR catalog wins).
pub fn discover_study_pages_batch(
    pdf: &Path,
    studies: &[u32],
    audience: Audience,
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<StudyPages>> {
    let page_count = pdf_page_count(pdf)?;
    if page_count == 0 {
        bail!("PDF has no pages: {}", pdf.display());
    }
    if studies.is_empty() {
        return Ok(Vec::new());
    }
    let texts = page_texts(pdf, page_count, pdftoppm_path)?;
    resolve_batch_from_texts(pdf, studies, audience, &texts, page_count)
}

/// Resolve requested estudios from already-extracted page texts (testable).
pub(crate) fn resolve_batch_from_texts(
    pdf: &Path,
    studies: &[u32],
    audience: Audience,
    texts: &[String],
    page_count: usize,
) -> Result<Vec<StudyPages>> {
    // Full PDF catalog first — then select the requested subset by number.
    if let Some(selected) =
        select_batch_from_catalog(pdf, studies, audience, texts, page_count)?
    {
        return Ok(selected);
    }

    let mut out = Vec::with_capacity(studies.len());
    for &study in studies {
        out.push(study_pages_from_texts(
            pdf,
            study,
            audience,
            texts,
            page_count,
        )?);
    }
    Ok(out)
}

/// Build a page→estudio catalog for the whole PDF, then return only `studies`.
///
/// Returns `Ok(None)` when the catalog already matches OCR and the per-study
/// path can resolve each number (no remapping needed).
/// Returns `Err` when requested estudios are missing — **before** any extract.
fn select_batch_from_catalog(
    pdf: &Path,
    studies: &[u32],
    audience: Audience,
    texts: &[String],
    page_count: usize,
) -> Result<Option<Vec<StudyPages>>> {
    if studies.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let from = studies[0];
    let to = *studies.last().unwrap();
    let hint = estudio_span_from_filename(pdf)
        .map(|(s, _)| s)
        .or_else(|| estudio_number_from_filename(pdf))
        .or(Some(from));

    let ocr_filled = build_filled_title_map(texts, page_count, hint)?;
    let title_pages: Vec<u32> = ocr_filled.keys().copied().collect();
    if title_pages.is_empty() {
        return Ok(None);
    }

    // page → estudio number for every title in the PDF
    let catalog = catalog_title_estudios(pdf, &title_pages, &ocr_filled);
    let found: Vec<u32> = {
        let mut v: Vec<u32> = catalog.values().copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let ocr_nums: Vec<u32> = title_pages
        .iter()
        .filter_map(|p| ocr_filled.get(p).copied())
        .collect();

    let missing: Vec<u32> = studies
        .iter()
        .copied()
        .filter(|s| !catalog.values().any(|n| n == s))
        .collect();

    if missing.is_empty() {
        // Requested estudios are in the PDF — select by number (skip earlier).
        let skipped: Vec<u32> = found
            .iter()
            .copied()
            .filter(|n| *n < from || *n > to)
            .collect();
        if catalog.values().any(|n| ocr_filled.values().all(|o| o != n))
            || ocr_nums != found
            || !skipped.is_empty()
        {
            eprintln!(
                "  info: PDF catalog estudios {:?} (OCR badges {:?}) — \
                 preparing {}–{}{}",
                found,
                ocr_nums,
                from,
                to,
                if skipped.is_empty() {
                    String::new()
                } else {
                    format!("; skipping {:?}", skipped)
                }
            );
        }
        let mut out = Vec::with_capacity(studies.len());
        for &study in studies {
            let start = *catalog
                .iter()
                .find(|(_, n)| **n == study)
                .map(|(p, _)| p)
                .expect("missing checked above");
            let ordered_next = title_pages
                .iter()
                .position(|&p| p == start)
                .and_then(|i| title_pages.get(i + 1).copied());
            let next_title = real_next_title_page(
                audience,
                start,
                ordered_next,
                texts,
                page_count,
                false,
            );
            let end = content_end_page(audience, start, next_title, texts, page_count);
            if end < start {
                bail!("Estudio {study}: invalid page range {start}–{end}");
            }
            let proximo_title_page =
                next_title.filter(|p| (*p as usize) <= page_count);
            print_discovered(study, audience, start, end, proximo_title_page, texts);
            out.push(StudyPages {
                study,
                pages: (start, end),
                proximo_title_page,
            });
        }
        return Ok(Some(out));
    }

    // Exact title count == requested count: PDF is only this batch; OCR digits
    // are wrong — map title order onto --from/--to (legacy volunteer path).
    if title_pages.len() == studies.len() {
        eprintln!(
            "  warn: OCR badges {:?} disagree with requested estudios {}–{} — \
             PDF has exactly {} title page(s); mapping them in order to {}–{} \
             (no earlier estudio to skip)",
            ocr_nums,
            from,
            to,
            title_pages.len(),
            from,
            to
        );
        let mut out = Vec::with_capacity(studies.len());
        for (i, &study) in studies.iter().enumerate() {
            let start = title_pages[i];
            let ordered_next = title_pages.get(i + 1).copied();
            let next_title = real_next_title_page(
                audience,
                start,
                ordered_next,
                texts,
                page_count,
                false,
            );
            let end = content_end_page(audience, start, next_title, texts, page_count);
            if end < start {
                bail!("Estudio {study}: invalid page range {start}–{end}");
            }
            let proximo_title_page =
                next_title.filter(|p| (*p as usize) <= page_count);
            print_discovered(study, audience, start, end, proximo_title_page, texts);
            out.push(StudyPages {
                study,
                pages: (start, end),
                proximo_title_page,
            });
        }
        return Ok(Some(out));
    }

    // More titles than requested, but catalog does not contain the numbers —
    // refuse to pretend the first pages are estudio `from` (that mapped 7→8).
    bail!(
        "Preflight failed — cannot start extract/build.\n\
         Requested: estudios {}–{}.\n\
         Found in PDF ({}): {:?}.\n\
         Missing: {:?}.\n\
         OCR badges on title pages: {:?}.\n\
         Hint: rename the PDF to the full span (e.g. Study 7-11 Adult.pdf) so \
         title-page order can be numbered correctly, then re-run \
         --from {} --to {}. Or prepare the estudios that are actually in the file \
         (--from {} --to {}).",
        from,
        to,
        pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(pdf)"),
        found,
        missing,
        ocr_nums,
        from,
        to,
        found.first().copied().unwrap_or(from),
        found.last().copied().unwrap_or(to),
    );
}

/// Assign estudio numbers to every title page (filename span preferred).
fn catalog_title_estudios(
    pdf: &Path,
    title_pages: &[u32],
    ocr_filled: &BTreeMap<u32, u32>,
) -> BTreeMap<u32, u32> {
    // Filename span matching title count: Study 7-11 Adult.pdf + 5 titles → 7…11.
    if let Some((start, end)) = estudio_range_from_filename(pdf, title_pages.len()) {
        let mut map = BTreeMap::new();
        for (i, &p) in title_pages.iter().enumerate() {
            let n = start + i as u32;
            if n <= end {
                map.insert(p, n);
            }
        }
        return map;
    }
    // Filename has a span but title count differs (e.g. only part of the book
    // was scanned). Still number from the span start in title order when the
    // span covers at least as many estudios as title pages.
    if let Some((start, end)) = estudio_span_from_filename(pdf) {
        let span_len = (end - start + 1) as usize;
        if title_pages.len() <= span_len {
            let mut map = BTreeMap::new();
            for (i, &p) in title_pages.iter().enumerate() {
                map.insert(p, start + i as u32);
            }
            return map;
        }
    }
    ocr_filled.clone()
}

/// Discover **every** estudio in the PDF (one OCR/text pass).
///
/// Used by `lamad prepare --pdf …` with no `--from`/`--to`/`-n` so volunteers
/// can drop a multi-study scan and prepare the whole file.
pub fn discover_all_study_pages(
    pdf: &Path,
    audience: Audience,
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<StudyPages>> {
    let page_count = pdf_page_count(pdf)?;
    if page_count == 0 {
        bail!("PDF has no pages: {}", pdf.display());
    }
    let texts = page_texts(pdf, page_count, pdftoppm_path)?;
    let hint = estudio_number_from_filename(pdf);
    let filled = build_filled_title_map(&texts, page_count, hint)?;
    if filled.is_empty() {
        bail!(
            "Could not find any estudios in {}. \
             Title pages usually contain “Base bíblica” and “Idea principal” \
             (adult: Pensamiento central / Texto áureo). \
             Pass --from F --to T if you know the range.",
            pdf.display()
        );
    }

    let mut ordered: Vec<(u32, u32)> = filled.into_iter().collect(); // (page, study)
    ordered.sort_by_key(|(p, _)| *p);

    // Single-study adult scans often include the next title page only for Próximo
    // (`Study 5 - Adult.pdf`). Do not also enqueue a stub estudio for that page.
    let single = estudio_number_from_filename(pdf);
    if let Some(n) = single {
        if ordered.len() > 1 {
            eprintln!(
                "  warn: PDF filename is estudio {n}; keeping first title page only \
                 (extra title page used as Próximo, not a second prepare)"
            );
            ordered.truncate(1);
            ordered[0].1 = n;
        }
    }

    let mut out = Vec::with_capacity(ordered.len());
    for (i, &(start, study)) in ordered.iter().enumerate() {
        let next_title = real_next_title_page(
            audience,
            start,
            ordered.get(i + 1).map(|(p, _)| *p),
            &texts,
            page_count,
            single.is_some() && i == 0,
        );
        let end = content_end_page(audience, start, next_title, &texts, page_count);
        if end < start {
            bail!("Estudio {study}: invalid page range {start}–{end}");
        }
        // Adult: only attach próximo when a real next title page exists.
        // Youth: keep prior behaviour (may use start+3 guess).
        let proximo_title_page = match audience {
            Audience::Adult => next_title.filter(|p| (*p as usize) <= page_count),
            Audience::Youth => next_title.filter(|p| (*p as usize) <= page_count),
        };
        out.push(StudyPages {
            study,
            pages: (start, end),
            proximo_title_page,
        });
    }

    // Title-page order (not OCR badge digits — tesseract often misreads “1” as “57”, etc.).
    out.sort_by_key(|s| s.pages.0);
    renumber_discovered_estudios(pdf, &mut out);
    for s in out.iter() {
        print_discovered(
            s.study,
            audience,
            s.pages.0,
            s.pages.1,
            s.proximo_title_page,
            &texts,
        );
    }

    crate::progress::ok(format!(
        "Discovered {} estudio{} in {} ({})",
        out.len(),
        if out.len() == 1 { "" } else { "s" },
        pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("PDF"),
        audience.as_str()
    ));
    Ok(out)
}

/// Next study title page — adult never invents from `start+3`.
fn real_next_title_page(
    audience: Audience,
    start: u32,
    ordered_next: Option<u32>,
    texts: &[String],
    page_count: usize,
    allow_scan_after_single: bool,
) -> Option<u32> {
    if let Some(p) = ordered_next {
        return Some(p);
    }
    // Real OCR title after `start` (Próximo-only trailing title, etc.).
    if let Some(p) = find_title_pages(texts).into_iter().find(|p| *p > start) {
        return Some(p);
    }
    match audience {
        Audience::Adult => None, // never invent
        Audience::Youth => {
            if allow_scan_after_single {
                return None;
            }
            let guess = start + 3;
            if (guess as usize) <= page_count {
                Some(guess)
            } else {
                None
            }
        }
    }
}

/// Inclusive last content PDF page for one estudio.
///
/// Adult: prefer the page that carries **`RESUMEN GENERAL`** (DESARROLLO fence).
/// Fall back to day-before next title, else last PDF page. Never assume 3 pages.
fn content_end_page(
    audience: Audience,
    start: u32,
    next_title: Option<u32>,
    texts: &[String],
    page_count: usize,
) -> u32 {
    let last = page_count as u32;
    match audience {
        Audience::Adult => {
            let search_until = next_title.unwrap_or(last + 1);
            if let Some(resumen) = find_resumen_page_after(texts, start, search_until) {
                return resumen;
            }
            match next_title {
                Some(n) if n > start => n - 1,
                _ => last.max(start),
            }
        }
        Audience::Youth => match next_title {
            Some(n) if n > start => n - 1,
            _ => (start + 2).min(last),
        },
    }
}

/// First PDF page after `start` (and before `search_until`) whose OCR/text
/// contains **RESUMEN GENERAL**.
fn find_resumen_page_after(texts: &[String], start: u32, search_until: u32) -> Option<u32> {
    for (i, t) in texts.iter().enumerate() {
        let p = (i as u32) + 1;
        if p <= start {
            continue;
        }
        if p >= search_until {
            break;
        }
        if has_resumen_general(t) {
            return Some(p);
        }
    }
    None
}

fn has_resumen_general(t: &str) -> bool {
    Regex::new(r"(?is)resumen\s+general")
        .map(|re| re.is_match(t))
        .unwrap_or(false)
}

/// Build page→estudio map from title pages (shared by single + all discover).
///
/// `hint_study` (from `-n N` or a single-study PDF filename) seeds the first
/// title page when OCR cannot read the stylized ESTUDIO badge.
fn build_filled_title_map(
    texts: &[String],
    _page_count: usize,
    hint_study: Option<u32>,
) -> Result<BTreeMap<u32, u32>> {
    let mut title_pages = find_title_pages(texts);
    for (i, t) in texts.iter().enumerate() {
        let p = (i as u32) + 1;
        if extract_study_number(t).is_some() && !title_pages.contains(&p) {
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
            "Could not find any estudio title pages. \
             Title pages usually contain “Base bíblica” and “Idea principal”."
        );
    }

    let mut numbered: BTreeMap<u32, u32> = BTreeMap::new();
    for &p in &title_pages {
        let idx = (p as usize).saturating_sub(1);
        if let Some(t) = texts.get(idx) {
            if let Some(n) = extract_study_number(t) {
                numbered.insert(p, n);
            }
        }
    }
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
    if numbered.is_empty() {
        if let Some(n) = hint_study {
            eprintln!(
                "  warn: OCR could not read ESTUDIO badge — using estudio {n} \
                 for first title page (from -n / PDF filename)"
            );
            numbered.insert(title_pages[0], n);
        }
    }

    fill_sequence(&title_pages, &numbered)
}

fn study_pages_from_texts(
    pdf: &Path,
    study: u32,
    audience: Audience,
    texts: &[String],
    page_count: usize,
) -> Result<StudyPages> {
    let mut filled = build_filled_title_map(texts, page_count, Some(study)).map_err(|e| {
        anyhow::anyhow!(
            "{e} Pass --from F -n {study} if you know the first estudio on page 1 \
             (e.g. --from 23 -n {study}). PDF: {}",
            pdf.display()
        )
    })?;

    // Filename span wins over misread OCR badges: `Study 7-11 Adult.pdf` + `-n 7`
    // → 1st title page (printed p.59), never a later sheet whose badge OCR said "7".
    if let Some((from, to)) = estudio_span_from_filename(pdf) {
        if (from..=to).contains(&study) {
            let title_pages: Vec<u32> = filled.keys().copied().collect();
            let idx = (study - from) as usize;
            if idx < title_pages.len() {
                let aligned = title_pages[idx];
                let ocr_at = filled.iter().find(|(_, n)| **n == study).map(|(p, _)| *p);
                if ocr_at != Some(aligned) {
                    eprintln!(
                        "  warn: aligning estudio {study} to title page order from \
                         filename span {from}–{to} → PDF sheet {aligned} \
                         (OCR badge had mapped it elsewhere: {ocr_at:?})"
                    );
                    filled.clear();
                    for (i, &p) in title_pages.iter().enumerate() {
                        let n = from + i as u32;
                        if n <= to {
                            filled.insert(p, n);
                        }
                    }
                }
            }
        }
    }

    let start = match filled.iter().find(|(_, n)| **n == study).map(|(p, _)| *p) {
        Some(p) => p,
        None => {
            // Adult badges are often stylized — OCR may misread a *single* study.
            // Never invent estudio N when OCR already found a clear multi-study map
            // that does not include N (that remapped 10/11 onto estudio 4's pages).
            let known: Vec<String> = filled
                .iter()
                .map(|(p, n)| format!("estudio {n} @ PDF sheet {p}"))
                .collect();
            let detected: std::collections::BTreeSet<u32> =
                filled.values().copied().collect();
            let title_pages: Vec<u32> = filled.keys().copied().collect();
            let single_misread = detected.len() == 1 && title_pages.len() <= 2;
            if title_pages.first() == Some(&1) && (detected.is_empty() || single_misread) {
                eprintln!(
                    "  warn: OCR did not see estudio {study} (saw: {}). \
                     Using first title page in the PDF as estudio {study} \
                     (check the printed page # on that scan — e.g. 59 for estudio 7)",
                    if known.is_empty() {
                        "none".into()
                    } else {
                        known.join(", ")
                    },
                );
                filled.clear();
                for (i, &p) in title_pages.iter().enumerate() {
                    filled.insert(p, study + i as u32);
                }
                title_pages[0]
            } else {
                bail!(
                    "Estudio {study} not found in {}. Detected: {}. \
                     This PDF does not appear to contain estudio {study} — \
                     prepare the studies that OCR found (e.g. --from {} --to {}), \
                     or pass --from F -n {study} only if you know page math.",
                    pdf.display(),
                    if known.is_empty() {
                        "(none)".into()
                    } else {
                        known.join(", ")
                    },
                    detected.iter().next().copied().unwrap_or(study),
                    detected.iter().next_back().copied().unwrap_or(study),
                );
            }
        }
    };

    let ordered_next = filled.keys().copied().find(|p| *p > start);
    let next_title = real_next_title_page(
        audience,
        start,
        ordered_next,
        texts,
        page_count,
        false,
    );
    let end = content_end_page(audience, start, next_title, texts, page_count);
    if end < start {
        bail!("Estudio {study}: invalid page range {start}–{end}");
    }
    if end as usize > page_count {
        bail!(
            "Estudio {study} starts at page {start}, but PDF only has {page_count} pages."
        );
    }

    let proximo_title_page = next_title.filter(|p| (*p as usize) <= page_count);

    print_discovered(study, audience, start, end, proximo_title_page, texts);

    Ok(StudyPages {
        study,
        pages: (start, end),
        proximo_title_page,
    })
}

/// Printed footer page # from scan OCR (book page, e.g. 59) — not the PDF sheet index.
fn printed_page_number(text: &str) -> Option<u32> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // Footer page is usually a lone 2–3 digit number near the bottom.
    let start = lines.len().saturating_sub(20);
    for ln in lines[start..].iter().rev() {
        if let Ok(n) = ln.parse::<u32>() {
            if (10..=999).contains(&n) {
                return Some(n);
            }
        }
    }
    None
}

fn print_discovered(
    study: u32,
    audience: Audience,
    start: u32,
    end: u32,
    proximo_title_page: Option<u32>,
    texts: &[String],
) {
    let printed_start = texts
        .get(start.saturating_sub(1) as usize)
        .and_then(|t| printed_page_number(t));
    let printed_end = texts
        .get(end.saturating_sub(1) as usize)
        .and_then(|t| printed_page_number(t));
    let prox = match proximo_title_page {
        Some(p) => {
            let pp = texts
                .get(p.saturating_sub(1) as usize)
                .and_then(|t| printed_page_number(t));
            match pp {
                Some(n) => format!(" (próximo title ≈ printed p.{n})"),
                None => format!(" (próximo title PDF sheet {p})"),
            }
        }
        None => " (no Próximo in PDF)".into(),
    };
    match (printed_start, printed_end) {
        (Some(a), Some(b)) => println!(
            "  Discovered estudio {study} ({}) at printed pages {a}–{b} \
             (PDF sheets {start}–{end}){prox}",
            audience.as_str()
        ),
        (Some(a), None) => println!(
            "  Discovered estudio {study} ({}) at printed page {a}→… \
             (PDF sheets {start}–{end}){prox}",
            audience.as_str()
        ),
        _ => println!(
            "  Discovered estudio {study} ({}) at PDF sheets {start}–{end}{prox} \
             — could not read printed footer page #s; use the page number printed \
             on the scan (not the PDF sheet index)",
            audience.as_str()
        ),
    }
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
    // One pdftoppm for the whole PDF (much faster than per-page) + parallel OCR.
    // 150dpi is enough to read ESTUDIO / Base bíblica badges.
    crate::progress::phase(format!(
        "Discover: rasterizing {page_count} pages @150dpi (one shot) + parallel tesseract ({})",
        tesseract.display()
    ));

    let prefix = root.join("page");
    let mut cmd = Command::new(&bin);
    crate::pdftoppm::configure_command(&mut cmd, &bin);
    let status = cmd
        .args([
            "-png",
            "-r",
            "150",
            pdf.to_str().unwrap_or(""),
            prefix.to_str().unwrap_or(""),
        ])
        .status()
        .context("pdftoppm (all pages)")?;
    if !status.success() {
        bail!("pdftoppm failed while rasterizing {}", pdf.display());
    }

    // pdftoppm writes page-1.png … page-N.png (1-based, zero-padded sometimes).
    let mut pngs: Vec<PathBuf> = Vec::with_capacity(page_count);
    for p in 1..=page_count {
        let candidates = [
            root.join(format!("page-{p}.png")),
            root.join(format!("page-{p:02}.png")),
            root.join(format!("page-{p:03}.png")),
        ];
        let png = candidates
            .into_iter()
            .find(|c| c.is_file())
            .with_context(|| format!("missing raster for page {p} under {}", root.display()))?;
        pngs.push(png);
    }

    let pb = crate::progress::bar(page_count as u64, "OCR pages");
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8);
    let texts = ocr_pages_parallel(&tesseract, &pngs, workers, &pb)?;
    pb.finish_with_message("OCR complete");
    Ok(texts)
}

/// Run tesseract on many page PNGs with a small thread pool.
fn ocr_pages_parallel(
    tesseract: &Path,
    pngs: &[PathBuf],
    workers: usize,
    pb: &ProgressBar,
) -> Result<Vec<String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let page_count = pngs.len();
    let next = AtomicUsize::new(0);
    let slot: Mutex<Vec<Option<Result<String>>>> = Mutex::new((0..page_count).map(|_| None).collect());
    let err_flag = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        for _ in 0..workers.min(page_count.max(1)) {
            scope.spawn(|| {
                loop {
                    if err_flag.load(Ordering::Relaxed) != 0 {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= page_count {
                        break;
                    }
                    let result = tesseract_ocr(tesseract, &pngs[i]);
                    if result.is_err() {
                        err_flag.store(1, Ordering::Relaxed);
                    }
                    if let Ok(mut guard) = slot.lock() {
                        guard[i] = Some(result);
                    }
                    pb.inc(1);
                }
            });
        }
    });

    let guard = slot.into_inner().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::with_capacity(page_count);
    for (i, cell) in guard.into_iter().enumerate() {
        match cell {
            Some(Ok(t)) => out.push(t),
            Some(Err(e)) => return Err(e).with_context(|| format!("tesseract page {}", i + 1)),
            None => bail!("OCR worker skipped page {}", i + 1),
        }
    }
    Ok(out)
}

fn pdftotext_pages(pdf: &Path, page_count: usize) -> Result<Vec<String>> {
    let bin = which("pdftotext").context(
        "pdftotext not on PATH (install poppler; Apple Silicon: /opt/homebrew/bin/pdftotext)",
    )?;
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
    // Try upright first; landscape scans (common adult phone/scanner dumps)
    // need 90°/270° or Base bíblica / ESTUDIO never appear.
    let mut paths: Vec<PathBuf> = vec![png.to_path_buf()];
    if let Ok(extra) = landscape_ocr_variants(png) {
        paths.extend(extra);
    }
    let mut best = String::new();
    let mut best_score = i32::MIN;
    for path in &paths {
        let text = tesseract_ocr_one(bin, path)?;
        let score = ocr_discover_score(&text);
        if score > best_score {
            best_score = score;
            best = text;
        }
        // Good enough: title page markers + study number.
        if score >= 250 {
            break;
        }
    }
    Ok(best)
}

/// When the raster is landscape (w > h), write 90° and 270° siblings for OCR.
fn landscape_ocr_variants(png: &Path) -> Result<Vec<PathBuf>> {
    let raw = fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let img = image::load_from_memory(&raw).context("decode discover PNG")?;
    let (w, h) = img.dimensions();
    if w <= h {
        return Ok(Vec::new());
    }
    let parent = png.parent().unwrap_or(Path::new("."));
    let stem = png
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("page");
    let mut out = Vec::new();
    for (deg, rotated) in [(90u32, img.rotate90()), (270, img.rotate270())] {
        // Downscale huge pages slightly so OCR stays fast.
        let (rw, rh) = rotated.dimensions();
        let rotated = if rw > 2000 {
            rotated.resize(2000, 2000 * rh / rw.max(1), FilterType::Triangle)
        } else {
            rotated
        };
        let dest = parent.join(format!("{stem}-rot{deg}.png"));
        rotated
            .save(&dest)
            .with_context(|| format!("write {}", dest.display()))?;
        out.push(dest);
    }
    Ok(out)
}

fn ocr_discover_score(t: &str) -> i32 {
    if t.trim().len() < 20 {
        return 0;
    }
    let mut s = (t.chars().count() as i32) / 40;
    if extract_study_number(t).is_some() {
        s += 100;
    }
    if is_title_page(t) {
        s += 200;
    }
    let low = t.to_lowercase();
    if low.contains("base b") {
        s += 40;
    }
    if low.contains("pensamiento") || low.contains("texto") {
        s += 20;
    }
    if low.contains("objetivo") {
        s += 15;
    }
    s
}

fn tesseract_ocr_one(bin: &Path, png: &Path) -> Result<String> {
    // Adult stylized “ESTUDIO BÍBLICO N” badges often vanish under PSM 6
    // (column/block mode) while PSM 3/4 still see them. Prefer any pass that
    // yields an estudio number; otherwise keep the longest non-empty text.
    let attempts = [
        ("spa+eng", "6"),
        ("spa+eng", "4"),
        ("spa+eng", "3"),
        ("eng", "6"),
        ("eng", "4"),
    ];
    let mut best_plain = String::new();
    let mut best_with_number = String::new();
    for (lang, psm) in attempts {
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
        let t = String::from_utf8_lossy(&output.stdout).into_owned();
        if t.trim().len() < 20 {
            continue;
        }
        if extract_study_number(&t).is_some() && t.len() > best_with_number.len() {
            best_with_number = t.clone();
        }
        if t.len() > best_plain.len() {
            best_plain = t;
        }
        // Early exit once we have a numbered pass — enough for discover.
        if !best_with_number.is_empty() && (psm == "4" || psm == "3") {
            break;
        }
    }
    if !best_with_number.is_empty() {
        Ok(best_with_number)
    } else {
        Ok(best_plain)
    }
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
    // Adult title: Base bíblica + Pensamiento / Texto áureo (+ often Lectura Antifonal).
    let has_aureo = Regex::new(r"texto\s+[aá]ureo").unwrap().is_match(&low);
    let has_pensamiento = low.contains("pensamiento");
    let has_objetivos = low.contains("objetivo");
    let has_estudio_badge = Regex::new(r"\bestudio\b").unwrap().is_match(&low)
        && extract_study_number(t).is_some();
    (has_base && has_idea)
        || (has_base && has_lectura && !has_maestro)
        || (has_base && has_aureo && has_pensamiento)
        // Adult landscape OCR sometimes drops “Base bíblica” but keeps badge + box.
        || (has_estudio_badge && has_pensamiento && has_aureo && has_objetivos)
}

/// Parse `Study 7-11 Adult.pdf` → `(7, 11)` regardless of how many titles OCR found.
fn estudio_span_from_filename(pdf: &Path) -> Option<(u32, u32)> {
    let name = pdf.file_name()?.to_str()?;
    let re = Regex::new(r"(?i)(?:estudio|study|bible\s+study)?\s*(\d+)\s*[-–]\s*(\d+)").ok()?;
    let caps = re.captures(name)?;
    let start: u32 = caps.get(1)?.as_str().parse().ok()?;
    let end: u32 = caps.get(2)?.as_str().parse().ok()?;
    (end >= start).then_some((start, end))
}

/// Parse `Bible study 1-8.pdf`, `Estudio 23-26.pdf`, etc. when the span matches
/// how many title pages OCR found.
fn estudio_range_from_filename(pdf: &Path, discovered_count: usize) -> Option<(u32, u32)> {
    let (start, end) = estudio_span_from_filename(pdf)?;
    if (end - start + 1) as usize == discovered_count {
        Some((start, end))
    } else {
        None
    }
}

/// Single-study scan names: `Study 5 - Adult.pdf`, `Bible Study 4 - Adult.pdf`,
/// `Estudio 24 - Adulto.pdf`.
fn estudio_number_from_filename(pdf: &Path) -> Option<u32> {
    let name = pdf.file_name()?.to_str()?;
    // Prefer explicit “Study/Estudio N” (not a range N-M).
    let re = Regex::new(
        r"(?i)(?:^|[^a-z0-9])(?:bible\s+study|estudio|study)\s*(\d{1,3})(?:\s*[-–]\s*(?:adult[oa]?)?(?:\.|$)|(?:\s|$))",
    )
    .ok()?;
    let caps = re.captures(name)?;
    // Reject if this is actually a range like "Study 1-8".
    if Regex::new(r"(?i)(?:study|estudio)\s*\d+\s*[-–]\s*\d+")
        .ok()?
        .is_match(name)
    {
        return None;
    }
    let n: u32 = caps.get(1)?.as_str().parse().ok()?;
    (1..=200).contains(&n).then_some(n)
}

/// Fix OCR badge digits when the PDF filename carries a reliable span/number.
///
/// - `Bible study 1-8.pdf` + 8 title pages → 1…8  
/// - `Study 5 - Adult.pdf` + 1–2 title pages (content + próximo) → 5, 6…  
/// - Otherwise **keep OCR numbers** (never force 1…N — that broke adult singles).
fn renumber_discovered_estudios(pdf: &Path, out: &mut [StudyPages]) {
    if out.is_empty() {
        return;
    }
    let start = if let Some((s, _)) = estudio_range_from_filename(pdf, out.len()) {
        s
    } else if let Some(n) = estudio_number_from_filename(pdf) {
        n
    } else {
        return;
    };
    let old: Vec<u32> = out.iter().map(|s| s.study).collect();
    for (i, entry) in out.iter_mut().enumerate() {
        entry.study = start + i as u32;
    }
    if old != out.iter().map(|s| s.study).collect::<Vec<_>>() {
        eprintln!(
            "  warn: OCR estudio numbers {:?} → renumbered {}–{} from PDF filename / title-page order",
            old,
            start,
            start + out.len() as u32 - 1
        );
    }
}

fn extract_study_number(t: &str) -> Option<u32> {
    // Drop common false friends.
    let cleaned = Regex::new(r"(?is)desarrollo\s+del\s+estudio")
        .unwrap()
        .replace_all(t, " ");
    let cleaned = Regex::new(r"(?is)bosquejo\s+del\s+estudio")
        .unwrap()
        .replace_all(&cleaned, " ");
    let cleaned = Regex::new(r"(?is)ideas\s+para\s+el\s+maestro")
        .unwrap()
        .replace_all(&cleaned, " ");

    // Prefer matches in the first portion (badge / header).
    let head: String = cleaned.chars().take(1200).collect();

    let patterns = [
        // Adult OCR often emits the stylized badge digit as "4 Texto áureo"
        // (must win before "1 Samuel" false positives).
        r"(?is)\b(\d{1,3})\s+Texto\s+[aá]ureo\b",
        // Adult badge: "ESTUDIO BÍBLICO 4" with number right after the label.
        r"(?is)\bestudio\s+b[ií]blico\s*[:\-]?\s*(\d{1,3})\b",
        r"(?is)\bestudio\b\s*[:\-]?\s*(\d{1,3})\b",
        r"(?is)\bestudio\b[\s\|]*\r?\n\s*(\d{1,3})\b",
        // Senda de Vida youth: "24 LECTURA BÍBLICA" when badge OCR drops ESTUDIO
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
    let re = Regex::new(r"(?is)\bestudio\s*(?:b[ií]blico\s*)?[:\-]?\s*(\d{1,3})\b").ok()?;
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

/// Resolve a CLI tool, preferring Apple Silicon Homebrew (`/opt/homebrew/bin`)
/// over Intel `/usr/local/bin` (which raises “Bad CPU type” / os error 86 on arm64).
/// Resolve `tesseract` on PATH (prefer Apple Silicon Homebrew over Intel).
pub fn tesseract_bin() -> Option<PathBuf> {
    which("tesseract")
}

fn which(name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for prefix in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
    ] {
        candidates.push(PathBuf::from(prefix).join(name));
    }
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !p.is_empty() {
                candidates.push(PathBuf::from(p));
            }
        }
    }
    for p in candidates {
        if !p.is_file() {
            continue;
        }
        // Reject binaries that cannot exec on this CPU (Intel brew on arm64).
        let probe = Command::new(&p).arg("--version").output();
        match probe {
            Ok(o) if o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty() => {
                return Some(p);
            }
            Ok(_) => continue, // exited weirdly with empty IO — try next
            Err(e) if e.raw_os_error() == Some(86) => continue, // Bad CPU type
            Err(_) => continue,
        }
    }
    None
}

/// Human-readable line for `lamad doctor`.
pub fn doctor_line() -> String {
    match tesseract_bin() {
        Some(p) => format!("tesseract: OK — {} (used for -n discover + scan coverage)", p.display()),
        None => "tesseract: MISSING — needed for -n discover / scan coverage \
                 (`brew install tesseract tesseract-lang`; on Apple Silicon use /opt/homebrew)"
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
    fn detects_adult_estudio_biblico_badge_ocr() {
        // Real tesseract layout: badge words split; digit lands before Texto áureo.
        let title = r#"
TEMA TRIMESTRAL LA SOBERANÍA DIVINA EN EL REINO UNIDO:
DIOS BUSCA DISCÍPULOS ESTUDIO
CONFORME A SU CORAZÓN BÍBLICO
Pensamiento central
Base bíblica
1 Samuel 15-16:1-13
4 Texto áureo
LECTURA ANTIFONAL
"#;
        assert!(is_title_page(title));
        assert_eq!(extract_study_number(title), Some(4));
    }

    #[test]
    fn estudio_range_from_filename_matches_batch_pdf_name() {
        let pdf = PathBuf::from("Bible study 1-8.pdf");
        assert_eq!(estudio_range_from_filename(&pdf, 8), Some((1, 8)));
        let pdf2 = PathBuf::from("Estudio 23-26.pdf");
        assert_eq!(estudio_range_from_filename(&pdf2, 4), Some((23, 26)));
        assert_eq!(estudio_range_from_filename(&pdf, 4), None);
        // Adult multi-scan name the volunteer used for estudios 7–11.
        let pdf3 = PathBuf::from("Study 7-11 Adult.pdf");
        assert_eq!(estudio_range_from_filename(&pdf3, 5), Some((7, 11)));
    }

    #[test]
    fn align_batch_prefers_filename_span_over_wrong_ocr_badges() {
        // Five adult title pages; OCR wrongly read 4…8 instead of 7…11.
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 4u32), (9, 5), (17, 6), (25, 7), (33, 8)] {
            texts[(page - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Samuel 1:1\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL"
            );
        }
        // Put RESUMEN near end of each window so content_end works.
        for page in [8u32, 16, 24, 32, 40] {
            texts[(page - 1) as usize] = "RESUMEN GENERAL\nalgo".into();
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let studies = [7u32, 8, 9, 10, 11];
        let aligned =
            resolve_batch_from_texts(&pdf, &studies, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(aligned.len(), 5);
        assert_eq!(aligned[0].study, 7);
        assert_eq!(aligned[0].pages.0, 1);
        assert_eq!(aligned[4].study, 11);
        assert_eq!(aligned[1].study, 8);
        assert_eq!(aligned[1].pages.0, 9);
    }

    #[test]
    fn batch_from_8_to_11_skips_estudio_7_in_7_11_pdf() {
        // Same wrong OCR; user asked only 8–11 — must NOT treat sheet 1 as estudio 8.
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 4u32), (9, 5), (17, 6), (25, 7), (33, 8)] {
            texts[(page - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Samuel 1:1\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL"
            );
        }
        for page in [8u32, 16, 24, 32, 40] {
            texts[(page - 1) as usize] = "RESUMEN GENERAL\nalgo".into();
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let studies = [8u32, 9, 10, 11];
        let got =
            resolve_batch_from_texts(&pdf, &studies, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(got.len(), 4);
        assert_eq!(got[0].study, 8);
        assert_eq!(
            got[0].pages.0, 9,
            "estudio 8 must start at the 2nd title page, not sheet 1 (estudio 7)"
        );
        assert_eq!(got[3].study, 11);
        assert_eq!(got[3].pages.0, 33);
    }

    #[test]
    fn single_n_on_span_pdf_uses_title_order_not_misread_badge() {
        // Same wrong OCR as above: badge "7" lands on PDF sheet 25.
        // `-n 7` alone must still start at the first title (printed ~59), not 25.
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 4u32), (9, 5), (17, 6), (25, 7), (33, 8)] {
            let mut body = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Samuel 1:1\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL\n"
            );
            // Footer printed page roughly: sheet 1 → 59, sheet 25 → 83.
            let printed = 58 + page;
            body.push_str(&format!("\n{printed}\n"));
            texts[(page - 1) as usize] = body;
        }
        for page in [8u32, 16, 24, 32, 40] {
            texts[(page - 1) as usize] = format!("RESUMEN GENERAL\n{}\n", 58 + page);
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let got = study_pages_from_texts(&pdf, 7, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(got.study, 7);
        assert_eq!(got.pages.0, 1, "estudio 7 must start at first title page");
        assert_ne!(got.pages.0, 25);
        assert_eq!(estudio_span_from_filename(&pdf), Some((7, 11)));
        assert_eq!(printed_page_number("foo\n59\n"), Some(59));
    }

    #[test]
    fn renumber_discovered_estudios_uses_filename_span() {
        let pdf = PathBuf::from("Bible study 1-8.pdf");
        let mut out: Vec<StudyPages> = (0..8)
            .map(|i| StudyPages {
                study: 57 + i,
                pages: (1 + i * 3, 3 + i * 3),
                proximo_title_page: Some(4 + i * 3),
            })
            .collect();
        renumber_discovered_estudios(&pdf, &mut out);
        assert_eq!(out[0].study, 1);
        assert_eq!(out[7].study, 8);
        assert_eq!(out[0].pages, (1, 3));
    }

    #[test]
    fn estudio_number_from_filename_single_adult_scan() {
        assert_eq!(
            estudio_number_from_filename(Path::new("Study 5 - Adult.pdf")),
            Some(5)
        );
        assert_eq!(
            estudio_number_from_filename(Path::new("Bible Study 4 - Adult.pdf")),
            Some(4)
        );
        assert_eq!(
            estudio_number_from_filename(Path::new("Estudio 24 - Adulto.pdf")),
            Some(24)
        );
        // Ranges are handled by estudio_range_from_filename, not this helper.
        assert_eq!(
            estudio_number_from_filename(Path::new("Bible study 1-8.pdf")),
            None
        );
    }

    #[test]
    fn build_filled_map_uses_hint_when_badge_missing() {
        let texts = vec![
            "Base bíblica\n1 Samuel 1:1\nPensamiento central\nTexto áureo\nLECTURA ANTIFONAL"
                .into(),
            "body".into(),
        ];
        let filled = build_filled_title_map(&texts, 2, Some(5)).unwrap();
        assert_eq!(filled.get(&1), Some(&5));
    }

    #[test]
    fn renumber_single_adult_filename_with_proximo_title() {
        let pdf = PathBuf::from("Study 5 - Adult.pdf");
        let mut out = vec![
            StudyPages {
                study: 99,
                pages: (1, 4),
                proximo_title_page: Some(5),
            },
            StudyPages {
                study: 100,
                pages: (5, 5),
                proximo_title_page: None,
            },
        ];
        renumber_discovered_estudios(&pdf, &mut out);
        assert_eq!(out[0].study, 5);
        assert_eq!(out[1].study, 6);
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
    fn build_filled_map_finds_three_youth_titles() {
        // Minimal 9-page youth batch: titles on 1, 4, 7.
        let mut texts = vec![String::new(); 9];
        texts[0] = "ESTUDIO 20\nBase bíblica\nGénesis 1:1\nIdea principal\nAlgo\nLECTURA BÍBLICA".into();
        texts[3] = "ESTUDIO 21\nBase bíblica\nGénesis 2:1\nIdea principal\nAlgo\nLECTURA BÍBLICA".into();
        texts[6] = "ESTUDIO 22\nBase bíblica\nGénesis 3:1\nIdea principal\nAlgo\nLECTURA BÍBLICA".into();
        let filled = build_filled_title_map(&texts, 9, None).unwrap();
        assert_eq!(filled.len(), 3);
        assert_eq!(filled.get(&1), Some(&20));
        assert_eq!(filled.get(&4), Some(&21));
        assert_eq!(filled.get(&7), Some(&22));
    }

    #[test]
    fn adult_content_ends_at_resumen_general_not_page_math() {
        // Simulates Canon-style adult scan: title @1, body, RESUMEN @8, next title @9.
        // PDF page count is irrelevant — a page may be 1-up or 2-up.
        let mut texts = vec![String::new(); 9];
        texts[0] = "ESTUDIO BÍBLICO 12\nBase bíblica\n2 Crónicas 6 y 7\n\
            Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL"
            .into();
        texts[1] = "DATOS GENERALES ACERCA DEL TEMA\nINTRODUCCIÓN".into();
        texts[2] = "DESARROLLO DEL ESTUDIO\nI. LA HISTORIA".into();
        texts[3] = "A. Historia\nB. Adoración".into();
        texts[4] = "II. LA ADORACIÓN".into();
        texts[5] = "Definiciones y etimología\nIII. PODER".into();
        texts[6] = "Afianzamiento y aplicación\nB. Un Dios".into();
        texts[7] = "RESUMEN GENERAL\nPreguntas y respuestas".into();
        texts[8] = "ESTUDIO BÍBLICO 13\nBase bíblica\n1 Reyes 9-11\n\
            Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL"
            .into();

        let end = content_end_page(Audience::Adult, 1, Some(9), &texts, 9);
        assert_eq!(end, 8, "adult end must be RESUMEN page, not next_title-1 only");

        let next = real_next_title_page(Audience::Adult, 1, Some(9), &texts, 9, false);
        assert_eq!(next, Some(9));

        // Last study in PDF — no próximo invented.
        let mut last_only = texts.clone();
        last_only.truncate(8);
        let next_none = real_next_title_page(Audience::Adult, 1, None, &last_only, 8, false);
        assert_eq!(next_none, None);
        let end_last = content_end_page(Audience::Adult, 1, None, &last_only, 8);
        assert_eq!(end_last, 8);

        assert!(has_resumen_general(&texts[7]));
        assert!(!has_resumen_general(&texts[2]));
    }

    #[test]
    fn youth_still_uses_three_page_fallback() {
        let texts = vec!["ESTUDIO 20\nBase bíblica\nIdea principal\nLECTURA BÍBLICA".into()];
        let end = content_end_page(Audience::Youth, 1, None, &texts, 10);
        assert_eq!(end, 3);
        let guess = real_next_title_page(Audience::Youth, 1, None, &texts, 10, false);
        assert_eq!(guess, Some(4));
        let adult_no_guess = real_next_title_page(Audience::Adult, 1, None, &texts, 10, false);
        assert_eq!(adult_no_guess, None);
    }

    #[test]
    #[ignore = "needs adult scan PDF + tesseract; run with --ignored"]
    fn discovers_adult_estudio_4_in_scan_pdf() {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scans/Bible Study 4 - Adult.pdf");
        if !pdf.exists() {
            eprintln!("skip: {}", pdf.display());
            return;
        }
        let ppm = which("pdftoppm").unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/pdftoppm"));
        let found = discover_study_pages(&pdf, 4, Audience::Adult, Some(ppm.as_path()))
            .expect("discover estudio 4");
        assert_eq!(found.study, 4);
        assert_eq!(found.pages.0, 1);
        assert!(found.pages.1 >= 3, "adult study should span past 3 pages: {:?}", found.pages);
    }

    #[test]
    #[ignore = "needs sample PDF + tesseract; run with --ignored"]
    fn discovers_estudio_25_in_sample_pdf() {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Bible Study 23-26.pdf");
        if !pdf.exists() {
            eprintln!("skip: sample PDF missing at {}", pdf.display());
            return;
        }
        let ppm = which("pdftoppm").unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/pdftoppm"));
        let found = discover_study_pages(&pdf, 25, Audience::Youth, Some(ppm.as_path()))
            .expect("discover 25");
        assert_eq!(found.pages, (7, 9), "estudio 25 should be pages 7-9");
        assert_eq!(found.proximo_title_page, Some(10));
    }

    #[test]
    fn discovers_adult_estudio_5_in_scan_pdf() {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scans/pending/Study 5 - Adult.pdf");
        if !pdf.exists() {
            eprintln!("skip: {}", pdf.display());
            return;
        }
        let ppm = which("pdftoppm").unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/pdftoppm"));
        let found = discover_study_pages(&pdf, 5, Audience::Adult, Some(ppm.as_path()))
            .expect("discover estudio 5");
        assert_eq!(found.study, 5);
        assert_eq!(found.pages.0, 1, "estudio 5 should start at page 1: {:?}", found);
        assert!(found.pages.1 >= 3, "should span content pages: {:?}", found.pages);
    }
}
