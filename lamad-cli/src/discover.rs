//! Locate an estudio inside a multi-study scan PDF.
//!
//! Strategy:
//! 1. Prefer `pdftotext` when the PDF has a text layer.
//! 2. Else rasterize pages + `tesseract` OCR.
//! 3. Detect title pages (`Base bíblica` + `Idea principal` / adult Pensamiento).
//! 4. Read study numbers from the **printed** `ESTUDIO BÍBLICO N` badge on the scan.
//! 5. Folder / `-n` number = that printed badge (gaps allowed — e.g. 7,8,10,11).
//! 6. Filename spans are a **hint only** when no badge can be read — never remap
//!    title-page order onto `Study 7-11.pdf` when the scan badges say otherwise.
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
/// **HARD:** select by **printed** `ESTUDIO BÍBLICO N` badge. `--from 8 --to 11`
/// skips estudios whose scan badge is outside that range (e.g. skips 7). Never
/// remap title-page order onto the filename span when badges differ (a PDF named
/// `Study 7-11` may actually hold 7,8,10,11 + próximo 12).
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
    let catalog = catalog_title_estudios(pdf, &title_pages, &ocr_filled, texts);
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
                "  info: PDF catalog estudios {:?} (title-page order {:?}) — \
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
            let catalog_pages: Vec<u32> = catalog.keys().copied().collect();
            let ordered_next = catalog_pages
                .iter()
                .position(|&p| p == start)
                .and_then(|i| catalog_pages.get(i + 1).copied());
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

    // Never invent estudio numbers from --from/--to or the filename. The scan
    // badge is the only source of truth for folder / `-n` numbers.
    bail!(
        "Preflight failed — cannot start extract/build.\n\
         Requested: estudios {}–{}.\n\
         Found in PDF ({}): {:?}.\n\
         Missing: {:?}.\n\
         Printed ESTUDIO badges on title pages: {:?}.\n\
         Prepare the estudios that are actually printed in the scan \
         (e.g. --from {} --to {}), or fix OCR so the badge digits are readable.",
        from,
        to,
        pdf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(pdf)"),
        found,
        missing,
        ocr_nums,
        found.first().copied().unwrap_or(from),
        found.last().copied().unwrap_or(to),
    );
}

/// Assign estudio numbers to every title page from the **printed scan badge**.
///
/// Filename spans are ignored here. Drop only weak false-positive title pages
/// that lack strong title markers. Printed badges (incl. gaps only when the
/// scan truly skips an estudio) come from `ocr_filled`.
fn catalog_title_estudios(
    _pdf: &Path,
    title_pages: &[u32],
    ocr_filled: &BTreeMap<u32, u32>,
    texts: &[String],
) -> BTreeMap<u32, u32> {
    let mut map = BTreeMap::new();
    for &p in title_pages {
        let idx = (p as usize).saturating_sub(1);
        let strength = texts.get(idx).map(|t| title_page_strength(t)).unwrap_or(0);
        let Some(&n) = ocr_filled.get(&p) else {
            continue;
        };
        // Drop weak ghosts (Base+Lectura only) that lack a readable badge cue.
        if strength < 6 {
            continue;
        }
        map.insert(p, n);
    }
    if map.is_empty() {
        return ocr_filled.clone();
    }
    map
}

fn title_page_strength(t: &str) -> i32 {
    let low = t.to_lowercase();
    let mut s = 0i32;
    if extract_study_number(t).is_some() {
        s += 4;
    }
    if Regex::new(r"(?is)\bestudio\s+b[ií]blico\b")
        .unwrap()
        .is_match(&low)
    {
        s += 5;
    }
    if Regex::new(r"base\s*b[ií1l]bl").unwrap().is_match(&low) {
        s += 3;
    }
    if Regex::new(r"texto\s+[aá]ureo").unwrap().is_match(&low) {
        s += 2;
    }
    if low.contains("pensamiento") {
        s += 2;
    }
    if low.contains("objetivo") {
        s += 1;
    }
    if Regex::new(r"(?is)resumen\s+general")
        .unwrap()
        .is_match(&low)
    {
        s -= 8;
    }
    if Regex::new(r"(?is)ideas\s+para\s+el\s+maestro")
        .unwrap()
        .is_match(&low)
        && !Regex::new(r"base\s*b[ií1l]bl").unwrap().is_match(&low)
    {
        s -= 4;
    }
    s
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

    // Keep printed badge numbers from the scan (gaps OK). Filename is last-resort only.
    out.sort_by_key(|s| s.pages.0);
    renumber_discovered_estudios(pdf, &mut out);
    drop_trailing_title_only(audience, &mut out, &texts);
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

/// Adult: a last title page with no `RESUMEN GENERAL` after it is only the
/// Próximo for the previous estudio — not a study to prepare.
fn drop_trailing_title_only(audience: Audience, out: &mut Vec<StudyPages>, texts: &[String]) {
    if audience != Audience::Adult || out.len() < 2 {
        return;
    }
    let last = out.last().unwrap();
    let has_resumen = (last.pages.0..=last.pages.1).any(|p| {
        p > last.pages.0
            && texts
                .get((p - 1) as usize)
                .is_some_and(|t| has_resumen_general(t))
    });
    let prev = &out[out.len() - 2];
    if !has_resumen && prev.proximo_title_page == Some(last.pages.0) {
        eprintln!(
            "  info: estudio {} is a title page only (no RESUMEN GENERAL after it) — \
             used as Próximo for estudio {}, not prepared",
            last.study, prev.study
        );
        out.pop();
    }
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
            let has_base = Regex::new(r"base\s*b[ií1l]bl").unwrap().is_match(&low);
            // Youth badge fallback: "24 LECTURA BÍBLICA" + Base bíblica.
            if has_base && (low.contains("lectura") || low.contains("objetivo")) {
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
    // Adult outlined title badges often OCR blank; body sheets still print
    // "ESTUDIO 9" in the running header (e.g. sheet 19 after title 17).
    reinforce_title_numbers_from_body(&title_pages, texts, &mut numbered);
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

    let positions = title_positions(&title_pages, texts);
    let consensus = consensus_title_numbers(&title_pages, &positions, &numbered);
    if !consensus.is_empty() {
        return Ok(consensus);
    }
    fill_sequence(&title_pages, &numbered)
}

/// When the stylized title badge is unreadable, pull `ESTUDIO N` from the next
/// few body pages (before the next title). This recovers printed 9 on
/// *DIOS ENGRANDECE…* when the title sheet OCR drops the outlined digit.
fn reinforce_title_numbers_from_body(
    title_pages: &[u32],
    texts: &[String],
    numbered: &mut BTreeMap<u32, u32>,
) {
    let re = Regex::new(r"(?is)\bestudio\s+(\d{1,3})\b").unwrap();
    for (i, &title_p) in title_pages.iter().enumerate() {
        if numbered.contains_key(&title_p) {
            continue;
        }
        let next_title = title_pages
            .get(i + 1)
            .copied()
            .unwrap_or((texts.len() as u32).saturating_add(1));
        let last = (title_p + 4).min(next_title.saturating_sub(1));
        for p in (title_p + 1)..=last {
            let Some(t) = texts.get((p as usize).saturating_sub(1)) else {
                continue;
            };
            let head: String = t.chars().take(600).collect();
            let Some(caps) = re.captures(&head) else {
                continue;
            };
            let Ok(n) = caps[1].parse::<u32>() else {
                continue;
            };
            if !(1..=200).contains(&n) {
                continue;
            }
            eprintln!(
                "  info: title sheet {title_p}: printed ESTUDIO {n} recovered from body sheet {p}"
            );
            numbered.insert(title_p, n);
            break;
        }
    }
}

/// Sequence position of each title page. Adult estudios each close with one
/// `RESUMEN GENERAL`; two between consecutive titles means OCR missed a title
/// page, so the position advances by two instead of shifting every number.
fn title_positions(title_pages: &[u32], texts: &[String]) -> Vec<i64> {
    let mut out = Vec::with_capacity(title_pages.len());
    let mut pos = 0i64;
    for (i, &p) in title_pages.iter().enumerate() {
        if i > 0 {
            let prev = title_pages[i - 1];
            let resumenes = (prev + 1..p)
                .filter(|s| {
                    texts
                        .get((*s - 1) as usize)
                        .is_some_and(|t| has_resumen_general(t))
                })
                .count() as i64;
            pos += resumenes.max(1);
        }
        out.push(pos);
    }
    out
}

/// Number title pages from **agreeing** printed-badge reads.
///
/// Title pages in one book scan are consecutive estudios, so each badge read
/// `n` on the i-th title page votes for offset `n − i`. Stylized adult digits
/// misread often (outlined 7/11/12 → 1, 7, nothing), but the true reads agree
/// on one offset — the lone misreads do not. A real gap (PDF skips an
/// estudio) shows up as two offsets each backed by ≥2 reads, ordered by page;
/// then each side keeps its own offset and titles between them stay unnumbered.
///
/// Returns empty when no offset has a clear majority (caller falls back).
fn consensus_title_numbers(
    title_pages: &[u32],
    positions: &[i64],
    numbered: &BTreeMap<u32, u32>,
) -> BTreeMap<u32, u32> {
    let pos = |i: usize| positions.get(i).copied().unwrap_or(i as i64);
    let reads: Vec<(usize, i64)> = title_pages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| numbered.get(p).map(|&n| (i, n as i64 - pos(i))))
        .collect();
    if reads.is_empty() {
        return BTreeMap::new();
    }
    let mut votes: BTreeMap<i64, usize> = BTreeMap::new();
    for (_, o) in &reads {
        *votes.entry(*o).or_insert(0) += 1;
    }
    let max = *votes.values().max().unwrap();
    let winners: Vec<i64> = votes
        .iter()
        .filter(|(_, c)| **c == max)
        .map(|(o, _)| *o)
        .collect();
    let mut out = BTreeMap::new();

    // Strong offsets (≥2 agreeing reads) that occupy disjoint, ordered page runs.
    let strong: Vec<i64> = votes
        .iter()
        .filter(|(_, c)| **c >= 2)
        .map(|(o, _)| *o)
        .collect();
    if strong.len() >= 2 {
        let spans: Vec<(i64, usize, usize)> = strong
            .iter()
            .map(|o| {
                let idx: Vec<usize> =
                    reads.iter().filter(|(_, ro)| ro == o).map(|(i, _)| *i).collect();
                (*o, *idx.iter().min().unwrap(), *idx.iter().max().unwrap())
            })
            .collect();
        let mut sorted = spans.clone();
        sorted.sort_by_key(|s| s.1);
        let ordered = sorted
            .windows(2)
            .all(|w| w[0].2 < w[1].1 && w[0].0 < w[1].0);
        if !ordered {
            return BTreeMap::new();
        }
        for (k, (o, lo, hi)) in sorted.iter().enumerate() {
            let from = if k == 0 { 0 } else { *lo };
            let to = if k + 1 == sorted.len() {
                title_pages.len() - 1
            } else {
                *hi
            };
            for i in from..=to {
                let n = pos(i) + o;
                if (1..=200).contains(&n) {
                    out.insert(title_pages[i], n as u32);
                }
            }
        }
        return out;
    }

    if winners.len() != 1 {
        return BTreeMap::new();
    }
    let offset = winners[0];
    for (i, &p) in title_pages.iter().enumerate() {
        let n = pos(i) + offset;
        if (1..=200).contains(&n) {
            out.insert(p, n as u32);
        }
    }
    let ignored: Vec<String> = reads
        .iter()
        .filter(|(_, o)| *o != offset)
        .map(|(i, o)| format!("sheet {} read {}", title_pages[*i], pos(*i) + o))
        .collect();
    if !ignored.is_empty() {
        eprintln!(
            "  info: ignored badge misread(s) that disagree with the other title pages: {}",
            ignored.join(", ")
        );
    }
    out
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

    // HARD: match by printed ESTUDIO badge only — never remap via filename order.

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
    // Block-mode OCR merges the footer into one line:
    // `Todos los derechos reservados 91 ©2026-II - ITEM …`.
    let footer = Regex::new(r"(?:^|\s)(\d{2,3})\s+[©@]?\s*20\d\d\b").unwrap();
    for ln in lines.iter().rev().take(6) {
        if let Some(c) = footer.captures(ln) {
            if let Ok(n) = c[1].parse::<u32>() {
                if (10..=999).contains(&n) {
                    return Some(n);
                }
            }
        }
    }
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

/// Printed page # for a PDF sheet, cross-checked against neighbouring footers.
///
/// Footer OCR misses or misreads some sheets (e.g. `07`, `21`). When the
/// nearest readable footers before and after agree on `printed − sheet`, that
/// offset wins; a lone direct read that disagrees with both is discarded.
fn printed_page_for_sheet(texts: &[String], sheet: u32) -> Option<u32> {
    let read = |s: u32| -> Option<i64> {
        texts
            .get(s.checked_sub(1)? as usize)
            .and_then(|t| printed_page_number(t))
            .map(|n| n as i64 - s as i64)
    };
    let last = texts.len() as u32;
    let before = (1..sheet).rev().find_map(read);
    let after = (sheet + 1..=last).find_map(read);
    let offset = match (read(sheet), before, after) {
        (Some(d), Some(b), Some(a)) if b == a && d != b => b,
        (Some(d), _, _) => d,
        (None, Some(b), Some(a)) if b == a => b,
        (None, Some(o), None) | (None, None, Some(o)) => o,
        _ => return None,
    };
    let n = sheet as i64 + offset;
    (n > 0).then_some(n as u32)
}

fn print_discovered(
    study: u32,
    audience: Audience,
    start: u32,
    end: u32,
    proximo_title_page: Option<u32>,
    texts: &[String],
) {
    let printed_start = printed_page_for_sheet(texts, start);
    let printed_end = printed_page_for_sheet(texts, end);
    let prox = match proximo_title_page {
        Some(p) => {
            match printed_page_for_sheet(texts, p) {
                Some(n) => format!(" (próximo title printed p.{n})"),
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
    // Always show feedback at default verbosity — this wait used to look "stuck".
    crate::progress::info(format!(
        "Discover: rasterizing {page_count} pages @150dpi, then OCR (tesseract) — usually ~30–90s"
    ));
    let spin = crate::progress::Spinner::start(format!(
        "pdftoppm rasterizing {page_count} pages…"
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
        spin.fail("pdftoppm failed");
        bail!("pdftoppm failed while rasterizing {}", pdf.display());
    }
    spin.succeed(format!("rasterized {page_count} pages"));

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

    let pb = crate::progress::bar_always(page_count as u64, "OCR pages");
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
    // Adult outlined badge digits are easy to miss on full-page OCR — reinforce
    // from a crop next to the ESTUDIO/BÍBLICO words. Title pages only: body
    // pages yield random digits that used to turn into fake title pages.
    if is_title_page(&best) {
        if let Some(n) = ocr_badge_number_from_png(bin, png) {
            if extract_study_number(&best) != Some(n) {
                best = format!("ESTUDIO BÍBLICO {n}\n{best}");
            }
        }
    }
    Ok(best)
}

/// Pixel box `(left, top, width, height)` of the ESTUDIO / BÍBLICO badge words.
fn locate_badge_words(bin: &Path, png: &Path) -> Option<(u32, u32, u32, u32)> {
    let output = Command::new(bin)
        .args([
            png.to_str().unwrap_or(""),
            "stdout",
            "-l",
            "spa+eng",
            "--psm",
            "11",
            "tsv",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tsv = String::from_utf8_lossy(&output.stdout);
    let word = Regex::new(r"(?i)^(estudio|b[ií]blico)").ok()?;
    let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || !word.is_match(cols[11].trim()) {
            continue;
        }
        let nums: Vec<u32> = cols[6..10].iter().filter_map(|c| c.parse().ok()).collect();
        if nums.len() == 4 {
            boxes.push((nums[0], nums[1], nums[2], nums[3]));
        }
    }
    let (_, ih) = image::image_dimensions(png).ok()?;
    // Badge sits in the page header — ignore body mentions of "estudio".
    boxes.retain(|b| b.1 < ih / 4);
    if boxes.is_empty() {
        return None;
    }
    let left = boxes.iter().map(|b| b.0).min()?;
    let top = boxes.iter().map(|b| b.1).min()?;
    let right = boxes.iter().map(|b| b.0 + b.2).max()?;
    let bottom = boxes.iter().map(|b| b.1 + b.3).max()?;
    Some((left, top, right - left, bottom - top))
}

/// Binary close (dilate dark, then erode) so outlined badge digits read as solid.
fn close_dark(img: &image::GrayImage, radius: u32) -> image::GrayImage {
    let pass = |src: &image::GrayImage, dark_wins: bool| {
        let (w, h) = src.dimensions();
        let mut out = src.clone();
        let r = radius as i64;
        for y in 0..h {
            for x in 0..w {
                let mut hit = false;
                'k: for dy in -r..=r {
                    for dx in -r..=r {
                        if dx * dx + dy * dy > r * r {
                            continue;
                        }
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                            continue;
                        }
                        let dark = src.get_pixel(nx as u32, ny as u32)[0] < 128;
                        if dark == dark_wins {
                            hit = true;
                            break 'k;
                        }
                    }
                }
                let v = if hit == dark_wins { 0 } else { 255 };
                out.put_pixel(x, y, image::Luma([v]));
            }
        }
        out
    };
    let dilated = pass(img, true);
    pass(&dilated, false)
}

/// Crop the top-right `ESTUDIO BÍBLICO N` zone and OCR digit candidates.
fn ocr_badge_number_from_png(bin: &Path, png: &Path) -> Option<u32> {
    let img = image::open(png).ok()?;
    let (w, h) = img.dimensions();
    if w < 200 || h < 200 {
        return None;
    }
    let parent = png.parent().unwrap_or(Path::new("."));
    let stem = png
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("page");

    let digits_of = |path: &Path, psm: &str| -> Option<u32> {
        let out = Command::new(bin)
            .args([
                path.to_str().unwrap_or(""),
                "stdout",
                "--psm",
                psm,
                "-c",
                "tessedit_char_whitelist=0123456789",
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let d: String = String::from_utf8_lossy(&out.stdout)
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        d.parse::<u32>().ok().filter(|n| (1..=200).contains(n))
    };

    // Primary: the big digit sits just right of the ESTUDIO / BÍBLICO words.
    let mut votes: BTreeMap<u32, u32> = BTreeMap::new();
    if let Some((bx, by, bw, bh)) = locate_badge_words(bin, png) {
        let left = (bx + bw + 2).min(w.saturating_sub(1));
        // OCR may only find one of the two words — pad by ~one word height.
        let pad = h / 20;
        let top = by.saturating_sub(pad);
        let cw = (bw * 13 / 10).min(w - left).max(1);
        let ch = (by + bh + pad / 3 - top).min(h - top).max(1);
        let gray = img.crop_imm(left, top, cw, ch).to_luma8();
        let big = image::imageops::resize(&gray, cw * 3, ch * 3, FilterType::Triangle);
        let mut thresh = big.clone();
        for p in thresh.pixels_mut() {
            p[0] = if p[0] < 153 { 0 } else { 255 };
        }
        let closed = close_dark(&thresh, 6);
        for (i, variant) in [gray, thresh, closed].iter().enumerate() {
            let dest = parent.join(format!("{stem}-digit{i}.png"));
            if variant.save(&dest).is_err() {
                continue;
            }
            for psm in ["7", "10", "6"] {
                if let Some(n) = digits_of(&dest, psm) {
                    *votes.entry(n).or_insert(0) += 1;
                }
            }
        }
        if let Some((n, c)) = votes.iter().max_by_key(|(n, c)| (**c, **n)) {
            if *c >= 2 {
                return Some(*n);
            }
        }
    }

    // Fallback: (left%, top%, width%, height%) of full page.
    let crops = [
        (55u32, 0u32, 45u32, 28u32),
        (70, 2, 28, 18),
        (78, 3, 20, 16),
    ];
    let mut votes: BTreeMap<u32, u32> = BTreeMap::new();
    for (i, (lx, ty, ww, hh)) in crops.iter().enumerate() {
        let left = w.saturating_mul(*lx) / 100;
        let top = h.saturating_mul(*ty) / 100;
        let cw = (w.saturating_mul(*ww) / 100).max(1).min(w.saturating_sub(left));
        let ch = (h.saturating_mul(*hh) / 100).max(1).min(h.saturating_sub(top));
        let crop = img.crop_imm(left, top, cw, ch);
        let dest = parent.join(format!("{stem}-badge{i}.png"));
        if crop.save(&dest).is_err() {
            continue;
        }
        for (lang, psm) in [("spa+eng", "6"), ("spa+eng", "11"), ("eng", "8"), ("eng", "10")] {
            let Ok(output) = Command::new(bin)
                .args([
                    dest.to_str().unwrap_or(""),
                    "stdout",
                    "-l",
                    lang,
                    "--psm",
                    psm,
                ])
                .output()
            else {
                continue;
            };
            if output.status.success() {
                let t = String::from_utf8_lossy(&output.stdout);
                if let Some(n) = extract_study_number(&t) {
                    *votes.entry(n).or_insert(0) += 2;
                }
            }
            // Digits-only pass on the same crop.
            let Ok(output2) = Command::new(bin)
                .args([
                    dest.to_str().unwrap_or(""),
                    "stdout",
                    "--psm",
                    psm,
                    "-c",
                    "tessedit_char_whitelist=0123456789",
                ])
                .output()
            else {
                continue;
            };
            if output2.status.success() {
                let digits = String::from_utf8_lossy(&output2.stdout);
                if let Some(n) = digits
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .ok()
                {
                    if (1..=200).contains(&n) {
                        *votes.entry(n).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    votes
        .into_iter()
        .max_by_key(|(n, c)| (*c, *n))
        .filter(|(_, c)| *c >= 2)
        .map(|(n, _)| n)
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
        // Adult box OCR often drops one of Pensamiento / Texto áureo (estudio 9 scan).
        || (has_base && has_objetivos && (has_pensamiento || has_aureo))
        // Adult landscape OCR sometimes drops “Base bíblica” but keeps badge + box.
        || (has_estudio_badge && has_pensamiento && has_aureo && has_objetivos)
}

/// Parse `Study 7-11 Adult.pdf` → `(7, 11)` regardless of how many titles OCR found.
pub(crate) fn estudio_span_from_filename(pdf: &Path) -> Option<(u32, u32)> {
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
pub(crate) fn estudio_number_from_filename(pdf: &Path) -> Option<u32> {
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

/// Last-resort only: when printed badges are missing or clearly garbage, seed
/// from the PDF filename. **Never** overwrite a strictly increasing scan badge
/// sequence (gaps allowed — e.g. 7,8,10,11,12).
fn renumber_discovered_estudios(pdf: &Path, out: &mut [StudyPages]) {
    if out.is_empty() {
        return;
    }
    let old: Vec<u32> = out.iter().map(|s| s.study).collect();
    if printed_badges_look_usable(&old) {
        return;
    }
    let start = if let Some((s, _)) = estudio_range_from_filename(pdf, out.len()) {
        s
    } else if let Some(n) = estudio_number_from_filename(pdf) {
        n
    } else {
        return;
    };
    for (i, entry) in out.iter_mut().enumerate() {
        entry.study = start + i as u32;
    }
    if old != out.iter().map(|s| s.study).collect::<Vec<_>>() {
        eprintln!(
            "  warn: could not trust OCR estudio numbers {:?} — seeded {}–{} from PDF filename \
             (printed ESTUDIO badge preferred when readable)",
            old,
            start,
            start + out.len() as u32 - 1
        );
    }
}

/// Printed badges are usable when strictly increasing (gaps OK) in 1..=200.
fn printed_badges_look_usable(nums: &[u32]) -> bool {
    if nums.is_empty() {
        return false;
    }
    nums.iter().all(|n| (1..=200).contains(n))
        && nums.windows(2).all(|w| w[1] > w[0])
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
        // Printed adult badge first — folder numbers must match the scan.
        r"(?is)\bestudio\s+b[ií]blico\s*[:\-]?\s*(\d{1,3})\b",
        r"(?is)\bestudio\b\s*[:\-]?\s*(\d{1,3})\b",
        r"(?is)\bestudio\b[\s\|]*\r?\n\s*(\d{1,3})\b",
        r"(?is)\bestudio\b[^\d]{0,12}(\d{1,3})\b",
        // Adult OCR sometimes emits the stylized badge digit as "4 Texto áureo".
        r"(?is)\b(\d{1,3})\s+Texto\s+[aá]ureo\b",
        // Senda de Vida youth: "24 LECTURA BÍBLICA" when badge OCR drops ESTUDIO
        r"(?is)(?:^|[^\d])(\d{1,3})\s+LECTURA\s+B",
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

/// Keep each title page’s printed OCR badge. Only fill pages with **no** badge
/// when both neighbors are known and the gap is exactly one missing estudio.
/// Never invent a consecutive run that would invent missing estudios (e.g. do
/// not turn printed 7,8,10,11 into 7,8,9,10).
fn fill_sequence(
    title_pages: &[u32],
    numbered: &BTreeMap<u32, u32>,
) -> Result<BTreeMap<u32, u32>> {
    if title_pages.is_empty() {
        return Ok(BTreeMap::new());
    }

    if numbered.is_empty() {
        bail!(
            "Found {} title page(s) but could not read any ESTUDIO number from OCR. \
             Install/update tesseract languages (spa) or pass --from F -n N \
             (F = first estudio on PDF page 1).",
            title_pages.len()
        );
    }

    let mut out = BTreeMap::new();
    for &page in title_pages {
        if let Some(&ocr_n) = numbered.get(&page) {
            out.insert(page, ocr_n);
        }
    }

    // Fill a single-hole gap only: neighbors N and N+2 → middle is N+1.
    for (i, &page) in title_pages.iter().enumerate() {
        if out.contains_key(&page) {
            continue;
        }
        let prev = (0..i)
            .rev()
            .find_map(|j| out.get(&title_pages[j]).map(|&n| (j, n)));
        let next = ((i + 1)..title_pages.len())
            .find_map(|j| out.get(&title_pages[j]).map(|&n| (j, n)));
        if let (Some((pi, pn)), Some((ni, nn))) = (prev, next) {
            if ni == i + 1 && pi + 1 == i && nn == pn + 2 {
                out.insert(page, pn + 1);
            }
        }
    }

    // Still-missing title pages: if *every* page lacks a number except we already
    // bailed on empty `numbered`, leave them out of the map (not invent).
    if out.is_empty() {
        bail!(
            "Found {} title page(s) but could not assign printed ESTUDIO numbers.",
            title_pages.len()
        );
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

    /// OCR as seen on the real `Study 7-11 Adult.pdf` (41 sheets, printed p.59–99):
    /// titles printed 7,8,9,10,11 + próximo-only 12; RESUMEN after each of 7–11.
    /// Badge reads mimic live tesseract: 7 and 11 unreadable, 12 misread as 7.
    fn real_study_7_11_texts() -> Vec<String> {
        let mut texts: Vec<String> = (1..=41u32)
            .map(|s| format!("DESARROLLO\ncuerpo\n{}\n", s + 58))
            .collect();
        let titles: [(u32, Option<u32>, &str); 6] = [
            (1, None, "EL TIEMPO DE DIOS\nSIEMPRE ES MEJOR"),
            (9, Some(8), "HACIENDO LO BUENO DE\nMANERA INCORRECTA"),
            (17, Some(9), "DIOS ENGRANDECE CUANDO\nHAY OBEDIENCIA"),
            (25, Some(10), "TEMA TRIMESTRAL"),
            (33, None, "LA MEJOR HERENCIA DE UN\nPADRE A SU HIJO"),
            (41, Some(7), "LA ADORACIÓN EN LA\nEXPERIENCIA DEL CRISTIANO"),
        ];
        for (sheet, badge, title) in titles {
            let badge = badge
                .map(|n| format!("ESTUDIO BÍBLICO {n}\n"))
                .unwrap_or_default();
            // Sheet 17 OCR really drops "Texto áureo".
            let aureo = if sheet == 17 { "" } else { "Texto áureo\n" };
            texts[(sheet - 1) as usize] = format!(
                "{badge}{title}\nBase bíblica\n1 Crónicas 18:1-17\nObjetivos\n\
                 Pensamiento central\n{aureo}Fecha sugerida\n{}\n",
                sheet + 58
            );
        }
        for sheet in [7u32, 15, 24, 32, 39] {
            texts[(sheet - 1) as usize] = "RESUMEN GENERAL\nalgo\n".into();
        }
        // Footer misreads seen live.
        texts[8] = texts[8].replace("\n67\n", "\n07\n");
        texts[16] = texts[16].replace("\n75\n", "\n");
        texts
    }

    #[test]
    fn real_7_11_batch_maps_printed_badges_7_to_11_with_proximo_12() {
        let texts = real_study_7_11_texts();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let studies: Vec<u32> = (7..=11).collect();
        let got = resolve_batch_from_texts(&pdf, &studies, Audience::Adult, &texts, 41).unwrap();
        let summary: Vec<(u32, (u32, u32), Option<u32>)> = got
            .iter()
            .map(|s| (s.study, s.pages, s.proximo_title_page))
            .collect();
        assert_eq!(
            summary,
            vec![
                (7, (1, 7), Some(9)),
                (8, (9, 15), Some(17)),
                (9, (17, 24), Some(25)),
                (10, (25, 32), Some(33)),
                (11, (33, 39), Some(41)),
            ]
        );
    }

    #[test]
    fn estudio_9_recovered_from_body_header_when_title_badge_blank() {
        // Live OCR: title sheet 17 has Base/Pensamiento/Objetivos but no digit;
        // body sheet 19 prints "ESTUDIO 9" in the running header.
        let mut texts = real_study_7_11_texts();
        texts[16] = "DIOS ENGRANDECE CUANDO\nHAY OBEDIENCIA\nBase bíblica\n\
            1 Crónicas 18:1-17\nObjetivos\nPensamiento central\nFecha sugerida\n75\n"
            .into();
        texts[18] = "ESTUDIO 9 VOLUMEN\nun hombre realizado: Ya había sido\n\
            coronado por las doce tribus\n76\n"
            .into();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let nine = study_pages_from_texts(&pdf, 9, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(nine.pages.0, 17);
        assert_eq!(nine.proximo_title_page, Some(25));
    }

    #[test]
    fn real_7_11_single_n_9_and_n_11() {
        let texts = real_study_7_11_texts();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let nine = study_pages_from_texts(&pdf, 9, Audience::Adult, &texts, 41).unwrap();
        assert_eq!((nine.pages, nine.proximo_title_page), ((17, 24), Some(25)));
        let eleven = study_pages_from_texts(&pdf, 11, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(
            (eleven.pages, eleven.proximo_title_page),
            ((33, 39), Some(41)),
            "printed 11 = LA MEJOR HERENCIA, content through RESUMEN, próximo printed 12"
        );
        assert_eq!(printed_page_for_sheet(&texts, 33), Some(91));
        assert_eq!(printed_page_for_sheet(&texts, 41), Some(99));
        assert_eq!(printed_page_for_sheet(&texts, 17), Some(75), "inferred from neighbours");
        assert_eq!(printed_page_for_sheet(&texts, 9), Some(67), "07 misread discarded");
    }

    #[test]
    fn real_7_11_trailing_12_is_title_only() {
        let texts = real_study_7_11_texts();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let twelve = study_pages_from_texts(&pdf, 12, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(
            twelve.pages,
            (41, 41),
            "no RESUMEN after printed 12 → title-only window (preflight rejects it)"
        );
        assert_eq!(twelve.proximo_title_page, None);

        let mut all = vec![
            StudyPages { study: 11, pages: (33, 39), proximo_title_page: Some(41) },
            StudyPages { study: 12, pages: (41, 41), proximo_title_page: None },
        ];
        drop_trailing_title_only(Audience::Adult, &mut all, &texts);
        assert_eq!(all.iter().map(|s| s.study).collect::<Vec<_>>(), vec![11]);
    }

    #[test]
    fn consensus_ignores_lone_badge_misreads() {
        let titles = vec![1, 9, 17, 25, 33, 41];
        let mut numbered = BTreeMap::new();
        numbered.insert(1, 1); // outlined 7 read as 1
        numbered.insert(9, 8);
        numbered.insert(17, 9);
        numbered.insert(25, 10);
        numbered.insert(41, 7); // outlined 12 read as 7
        let positions: Vec<i64> = (0..6).collect();
        let got = consensus_title_numbers(&titles, &positions, &numbered);
        assert_eq!(got.values().copied().collect::<Vec<_>>(), vec![7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn missed_title_page_does_not_shift_other_estudios() {
        // OCR misses the estudio 9 title (sheet 17) entirely; two RESUMEN pages
        // between sheets 9 and 25 keep 10/11 on their printed numbers.
        let mut texts = real_study_7_11_texts();
        texts[16] = "DESARROLLO\ncuerpo\n75\n".into();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let got =
            resolve_batch_from_texts(&pdf, &[10, 11], Audience::Adult, &texts, 41).unwrap();
        assert_eq!(got[0].pages, (25, 32));
        assert_eq!(got[1].pages, (33, 39));
        let err = resolve_batch_from_texts(&pdf, &[9], Audience::Adult, &texts, 41)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Missing: [9]"), "{err}");
    }

    #[test]
    fn filename_span_never_remaps_printed_badges() {
        // Badges say 20–24; a misleading "Study 7-11" name must not renumber them.
        let mut texts = vec![String::new(); 10];
        for (i, sheet) in [1u32, 3, 5, 7, 9].iter().enumerate() {
            texts[(sheet - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {}\nBase bíblica\nObjetivos\nPensamiento central\nTexto áureo",
                20 + i
            );
            texts[*sheet as usize] = "RESUMEN GENERAL".into();
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let err = resolve_batch_from_texts(&pdf, &[7], Audience::Adult, &texts, 10)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[20, 21, 22, 23, 24]"), "{err}");
        let got = resolve_batch_from_texts(&pdf, &[22], Audience::Adult, &texts, 10).unwrap();
        assert_eq!(got[0].pages, (5, 6));
    }

    #[test]
    fn real_sheet_17_ocr_is_a_title_page() {
        let t = "DIOS ENGRANDECE CUANDO\n\nHAY OBEDIENCIA\n\nBase bíblica\n1 Crónicas 18:1-17\n\n\
                 Objetivos\n\n1. Analizar por qué Dios engrandeció\n\nPensamiento central\n";
        assert!(is_title_page(t));
    }

    #[test]
    fn printed_page_from_merged_footer_line() {
        let t = "y juzgaba con justicia\n| Modos lo derechos seseevis 75 ©2026-11- TAT ISH 8 SUL 1a\n";
        assert_eq!(printed_page_number(t), Some(75));
        let t2 = "Todos los derechos reservados 91 2026-11 - 1LEM 15018 15019 15025\n";
        assert_eq!(printed_page_number(t2), Some(91));
    }

    #[test]
    fn catalog_keeps_synthetic_printed_gap() {
        // Synthetic (not the real 7-11 scan): a PDF that truly skips estudio 9.
        // Two agreeing reads on each side keep the gap — never invent 9.
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 7u32), (9, 8), (25, 10), (33, 11), (41, 12)] {
            texts[(page - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Samuel 1:1\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL\nObjetivos"
            );
        }
        for page in [8u32, 16, 32, 40] {
            texts[(page - 1) as usize] = "RESUMEN GENERAL\nalgo".into();
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let studies = [10u32, 11];
        let got =
            resolve_batch_from_texts(&pdf, &studies, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].study, 10);
        assert_eq!(got[0].pages.0, 25);
        assert_eq!(got[1].study, 11);
        assert_eq!(got[1].pages.0, 33);
        assert_eq!(got[1].proximo_title_page, Some(41));
    }

    #[test]
    fn batch_rejects_estudio_absent_from_synthetic_gap_scan() {
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 7u32), (9, 8), (25, 10), (33, 11)] {
            texts[(page - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Samuel 1:1\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL\nObjetivos"
            );
        }
        for page in [8u32, 16, 32, 40] {
            texts[(page - 1) as usize] = "RESUMEN GENERAL\nalgo".into();
        }
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let err = resolve_batch_from_texts(&pdf, &[9u32], Audience::Adult, &texts, 41)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Missing") || err.contains("9"),
            "estudio 9 is not in this synthetic PDF: {err}"
        );
    }

    #[test]
    fn prepare_n_11_on_synthetic_gap_scan_uses_printed_badge() {
        let mut texts = vec![String::new(); 41];
        for (page, ocr_n) in [(1u32, 7u32), (9, 8), (25, 10), (33, 11), (41, 12)] {
            texts[(page - 1) as usize] = format!(
                "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n1 Crónicas 28:1-21\n\
                 Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL\nObjetivos\n91\n"
            );
            if page == 41 {
                texts[(page - 1) as usize] = format!(
                    "ESTUDIO BÍBLICO {ocr_n}\nBase bíblica\n2 Crónicas 6 y 7\n\
                     Pensamiento central\nTexto áureo\nLECTURA ANTIFONAL\nObjetivos\n99\n"
                );
            }
        }
        texts[39] = "RESUMEN GENERAL\n98\n".into();
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let got = study_pages_from_texts(&pdf, 11, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(got.study, 11);
        assert_eq!(got.pages.0, 33, "printed ESTUDIO 11 is LA MEJOR HERENCIA");
        assert_eq!(got.proximo_title_page, Some(41));
        assert_ne!(got.pages.0, 41, "must not treat ESTUDIO 12 title as 11");
    }

    #[test]
    fn renumber_keeps_printed_badges_with_gaps() {
        let pdf = PathBuf::from("Study 7-11 Adult.pdf");
        let mut out = vec![
            StudyPages {
                study: 7,
                pages: (1, 8),
                proximo_title_page: Some(9),
            },
            StudyPages {
                study: 8,
                pages: (9, 24),
                proximo_title_page: Some(25),
            },
            StudyPages {
                study: 10,
                pages: (25, 32),
                proximo_title_page: Some(33),
            },
            StudyPages {
                study: 11,
                pages: (33, 40),
                proximo_title_page: Some(41),
            },
        ];
        renumber_discovered_estudios(&pdf, &mut out);
        assert_eq!(
            out.iter().map(|s| s.study).collect::<Vec<_>>(),
            vec![7, 8, 10, 11],
            "filename must not flatten gaps in printed badges"
        );
    }

    #[test]
    fn renumber_discovered_estudios_uses_filename_when_ocr_garbage() {
        let pdf = PathBuf::from("Bible study 1-8.pdf");
        // Non-increasing garbage — filename may seed.
        let mut out: Vec<StudyPages> = (0..8)
            .map(|i| StudyPages {
                study: 57,
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
        // Usable increasing badges — keep them (do not force filename 5,6).
        let mut out = vec![
            StudyPages {
                study: 5,
                pages: (1, 4),
                proximo_title_page: Some(5),
            },
            StudyPages {
                study: 6,
                pages: (5, 5),
                proximo_title_page: None,
            },
        ];
        renumber_discovered_estudios(&pdf, &mut out);
        assert_eq!(out[0].study, 5);
        assert_eq!(out[1].study, 6);
    }

    #[test]
    fn fills_sequence_keeps_printed_gaps_and_single_holes() {
        let titles = vec![1, 4, 7, 10];
        let mut numbered = BTreeMap::new();
        numbered.insert(1, 7);
        numbered.insert(4, 8);
        numbered.insert(7, 10);
        numbered.insert(10, 11);
        let filled = fill_sequence(&titles, &numbered).unwrap();
        assert_eq!(filled.get(&1), Some(&7));
        assert_eq!(filled.get(&4), Some(&8));
        assert_eq!(filled.get(&7), Some(&10));
        assert_eq!(filled.get(&10), Some(&11));

        // Single hole between 23 and 25 → fill 24 only.
        let titles2 = vec![1, 4, 7];
        let mut numbered2 = BTreeMap::new();
        numbered2.insert(1, 23);
        numbered2.insert(7, 25);
        let filled2 = fill_sequence(&titles2, &numbered2).unwrap();
        assert_eq!(filled2.get(&4), Some(&24));
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
    #[ignore = "needs scans/pending/Study 7-11 Adult.pdf + tesseract; run with --ignored"]
    fn live_study_7_11_adult_pdf_printed_badges() {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../scans/pending/Study 7-11 Adult.pdf");
        if !pdf.exists() {
            eprintln!("skip: {}", pdf.display());
            return;
        }
        let ppm = which("pdftoppm").unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/pdftoppm"));
        let texts = page_texts(&pdf, 41, Some(ppm.as_path())).expect("OCR");
        let all: Vec<u32> = (7..=11).collect();
        let got = resolve_batch_from_texts(&pdf, &all, Audience::Adult, &texts, 41)
            .expect("printed 7–11 all present");
        let summary: Vec<(u32, (u32, u32), Option<u32>)> = got
            .iter()
            .map(|s| (s.study, s.pages, s.proximo_title_page))
            .collect();
        assert_eq!(
            summary,
            vec![
                (7, (1, 7), Some(9)),
                (8, (9, 15), Some(17)),
                (9, (17, 24), Some(25)),
                (10, (25, 32), Some(33)),
                (11, (33, 39), Some(41)),
            ]
        );
        let nine = study_pages_from_texts(&pdf, 9, Audience::Adult, &texts, 41).unwrap();
        assert_eq!((nine.pages, nine.proximo_title_page), ((17, 24), Some(25)));
        let eleven = study_pages_from_texts(&pdf, 11, Audience::Adult, &texts, 41).unwrap();
        assert_eq!((eleven.pages, eleven.proximo_title_page), ((33, 39), Some(41)));
        assert_eq!(printed_page_for_sheet(&texts, 17), Some(75));
        assert_eq!(printed_page_for_sheet(&texts, 33), Some(91));
        assert_eq!(printed_page_for_sheet(&texts, 41), Some(99));
        let twelve = study_pages_from_texts(&pdf, 12, Audience::Adult, &texts, 41).unwrap();
        assert_eq!(twelve.pages, (41, 41), "printed 12 is title-only (próximo)");
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
