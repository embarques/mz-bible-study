//! Verify adult section-folder text covers the **printed** study body on the scan.
//!
//! After generate/expand we OCR the content window (title → RESUMEN page) and
//! check that substantial scan sentences appear in `studies/adult/{N}/`. This
//! catches truncated extracts before a deck is built.
//!
//! Excluded from the scan side (not deck content): Ideas para el maestro,
//! Afianzamiento, Preguntas, RESUMEN GENERAL body, lecturas devocionales,
//! bosquejo-only lines, footers/copyright.

use anyhow::{bail, Context, Result};
use image::GenericImageView;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::content::adult_sections::adult_study_folder;
use crate::paths;
use crate::progress;

/// Minimum normalized length for a scan phrase to be required in the extract.
const MIN_SNIPPET_CHARS: usize = 40;
/// Word-window size pulled from column-OCR'd scan text.
const WINDOW_WORDS: usize = 10;
const WINDOW_STRIDE: usize = 5;
/// Fraction of required snippets that must match (OCR noise → not 100%).
const MIN_COVERAGE: f64 = 0.80;
/// Soft Jaccard for fuzzy OCR vs clean extract.
const MIN_TOKEN_OVERLAP: f64 = 0.70;

#[derive(Debug, Clone)]
pub struct MissingSnippet {
    pub preview: String,
    pub printed_hint: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CoverageReport {
    pub study: u32,
    pub required: usize,
    pub matched: usize,
    pub missing: Vec<MissingSnippet>,
    pub pages: (u32, u32),
}

impl CoverageReport {
    pub fn coverage_ratio(&self) -> f64 {
        if self.required == 0 {
            1.0
        } else {
            self.matched as f64 / self.required as f64
        }
    }

    pub fn ok(&self) -> bool {
        self.required == 0 || self.coverage_ratio() + f64::EPSILON >= MIN_COVERAGE
    }
}

/// OCR the estudio content window and require section-folder text to cover it.
///
/// `pages` = inclusive PDF sheet range (title → RESUMEN), same as discover.
pub fn verify_adult_scan_coverage(
    study: u32,
    pdf: &Path,
    pages: (u32, u32),
    pdftoppm_path: Option<&Path>,
) -> Result<CoverageReport> {
    let study_dir = adult_study_folder(study)?;
    if !study_dir.join("manifest.toml").is_file() {
        bail!(
            "scan coverage: no section tree at {} — expand/generate first",
            study_dir.display()
        );
    }

    progress::phase(format!(
        "scan coverage: OCR PDF sheets {}–{} vs studies/adult/{study}/ …",
        pages.0, pages.1
    ));

    let page_pngs = ensure_page_rasters(study, pdf, pages, pdftoppm_path)?;
    let mut scan_chunks: Vec<(Option<u32>, String)> = Vec::new();
    for png in &page_pngs {
        let raw = ocr_png(png)?;
        let printed = printed_footer_page(&raw);
        let cleaned = strip_excluded_regions(&raw);
        scan_chunks.push((printed, cleaned));
    }

    let extract = load_section_text(&study_dir)?;
    let extract_norm = normalize(&extract);
    if extract_norm.chars().count() < 200 {
        bail!(
            "scan coverage: studies/adult/{study}/ text is almost empty \
             ({} chars normalized) — extract looks incomplete",
            extract_norm.chars().count()
        );
    }

    let mut matched = 0usize;
    let mut missing = Vec::new();
    let mut required_n = 0usize;

    for (printed, chunk) in &scan_chunks {
        for phrase in content_phrases(chunk) {
            let norm = normalize(&phrase);
            if norm.chars().count() < MIN_SNIPPET_CHARS {
                continue;
            }
            if !is_plausible_prose(&norm) || looks_like_non_body(&norm) {
                continue;
            }
            required_n += 1;
            if snippet_covered(&norm, &extract_norm) {
                matched += 1;
            } else {
                missing.push(MissingSnippet {
                    preview: truncate(&phrase, 140),
                    printed_hint: *printed,
                });
            }
        }
    }

    let report = CoverageReport {
        study,
        required: required_n,
        matched,
        missing,
        pages,
    };

    // Always print the gate result (even at default verbosity) — this is how
    // we catch truncated extracts before a deck ships.
    if report.ok() {
        progress::info(format!(
            "scan coverage OK — {}/{} body snippets matched ({:.0}%) for estudio {study}",
            report.matched,
            report.required,
            report.coverage_ratio() * 100.0
        ));
    } else {
        progress::warn(format!(
            "scan coverage LOW — {}/{} body snippets matched ({:.0}%) for estudio {study} \
             (need ≥{:.0}%)",
            report.matched,
            report.required,
            report.coverage_ratio() * 100.0,
            MIN_COVERAGE * 100.0
        ));
        for (i, m) in report.missing.iter().take(12).enumerate() {
            let where_ = m
                .printed_hint
                .map(|p| format!("printed p.{p}"))
                .unwrap_or_else(|| "scan".into());
            eprintln!("      missing [{}] ({where_}): {}", i + 1, m.preview);
        }
        if report.missing.len() > 12 {
            eprintln!("      … +{} more", report.missing.len() - 12);
        }
    }

    Ok(report)
}

/// Same as [`verify_adult_scan_coverage`] but **fails** when coverage is low.
pub fn assert_adult_scan_coverage(
    study: u32,
    pdf: &Path,
    pages: (u32, u32),
    pdftoppm_path: Option<&Path>,
) -> Result<CoverageReport> {
    let report = verify_adult_scan_coverage(study, pdf, pages, pdftoppm_path)?;
    if report.ok() {
        return Ok(report);
    }
    bail!(
        "estudio {study}: extracted section text does not cover the scan \
         ({}/{} snippets, {:.0}% < {:.0}% required). \
         Missing examples:\n  - {}\n\
         Re-run prepare with -g against the correct printed pages, or fill the \
         gaps under studies/adult/{study}/ from the scan (verbatim).",
        report.matched,
        report.required,
        report.coverage_ratio() * 100.0,
        MIN_COVERAGE * 100.0,
        report
            .missing
            .iter()
            .take(5)
            .map(|m| m.preview.as_str())
            .collect::<Vec<_>>()
            .join("\n  - ")
    )
}

fn ensure_page_rasters(
    study: u32,
    pdf: &Path,
    pages: (u32, u32),
    pdftoppm_path: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    let dir = paths::generated_dir()?.join("pages").join(format!("study{study}"));
    fs::create_dir_all(&dir)?;
    let want: Vec<u32> = (pages.0..=pages.1).collect();
    let mut missing = Vec::new();
    let mut outs = Vec::new();
    for &p in &want {
        let png = dir.join(format!("page-{p}.png"));
        if png.is_file() {
            outs.push(png);
        } else {
            missing.push(p);
        }
    }
    if !missing.is_empty() {
        let fresh = crate::rasterize_pages(pdf, &missing, &dir, pdftoppm_path)?;
        // Re-collect in page order.
        outs.clear();
        for &p in &want {
            let png = dir.join(format!("page-{p}.png"));
            if !png.is_file() {
                // rasterize_pages may have written them; also accept fresh list
                if let Some(f) = fresh.iter().find(|f| {
                    f.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == format!("page-{p}.png"))
                        .unwrap_or(false)
                }) {
                    outs.push(f.clone());
                } else {
                    bail!("scan coverage: missing raster {}", png.display());
                }
            } else {
                outs.push(png);
            }
        }
    }
    Ok(outs)
}

fn ocr_png(png: &Path) -> Result<String> {
    // Adult Senda pages are 2-column: OCR left then right so we don't mash
    // adjacent columns into fake "missing" sentences.
    let img = image::open(png)
        .with_context(|| format!("open raster {}", png.display()))?;
    let (w, h) = img.dimensions();
    let mid = w / 2;
    let gap = (w as f32 * 0.02) as u32; // small gutter skip
    let left = img.crop_imm(0, 0, mid.saturating_sub(gap), h);
    let right = img.crop_imm(mid.saturating_add(gap), 0, w.saturating_sub(mid + gap), h);

    let tmp = png
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            ".cov-{}",
            png.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("page")
        ));
    fs::create_dir_all(&tmp)?;
    let left_path = tmp.join("left.png");
    let right_path = tmp.join("right.png");
    left.save(&left_path)
        .with_context(|| format!("write {}", left_path.display()))?;
    right
        .save(&right_path)
        .with_context(|| format!("write {}", right_path.display()))?;

    let mut text = ocr_single(&left_path)?;
    text.push_str("\n\n");
    text.push_str(&ocr_single(&right_path)?);
    let _ = fs::remove_dir_all(&tmp);
    Ok(text)
}

fn ocr_single(png: &Path) -> Result<String> {
    let tess = crate::discover::tesseract_bin().ok_or_else(|| {
        anyhow::anyhow!(
            "scan coverage needs `tesseract` on PATH \
             (brew install tesseract tesseract-lang)"
        )
    })?;
    let name = png.file_name().context("OCR image has no filename")?;
    let dir = png.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for (lang, psm) in [("spa", "4"), ("spa+eng", "4"), ("spa", "6")] {
        let output = Command::new(&tess)
            .current_dir(dir)
            .arg(name)
            .args([
                "stdout",
                "-l",
                lang,
                "--psm",
                psm,
            ])
            .output()
            .with_context(|| format!("tesseract failed on {}", png.display()))?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).into_owned();
            if s.chars().filter(|c| c.is_alphabetic()).count() > 40 {
                return Ok(s);
            }
        }
    }
    bail!("tesseract produced no usable text for {}", png.display());
}

fn load_section_text(study_dir: &Path) -> Result<String> {
    let mut buf = String::new();
    for entry in walkdir::WalkDir::new(study_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.ends_with(".txt") || name == "manifest.toml") {
            continue;
        }
        // Skip image sidecars / noise.
        if name.ends_with(".placeholder") {
            continue;
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        buf.push_str(&text);
        buf.push('\n');
    }
    Ok(buf)
}

/// Drop teacher notes / Q&A / resumen / devotionals from OCR before matching.
fn strip_excluded_regions(text: &str) -> String {
    let start_re = Regex::new(
        r"(?i)^(ideas\s+para\s+el\s+maestro|afianzamiento\s+y\s+aplicaci[oó]n|preguntas\s+y\s+respuestas|ejercicios\s+de\s+clausura|para\s+la\s+pr[oó]xima\s+semana|resumen\s+general|lecturas\s+devocionales\s+diarias|bosquejo\s+del\s+estudio)\b",
    )
    .unwrap();
    let resume_re = Regex::new(
        r"(?i)^(desarrollo\s+del\s+estudio|introducci[oó]n|[ivx]+\.\s+[A-ZÁÉÍÓÚÑ]|[A-D]\.\s+\S)",
    )
    .unwrap();
    // Blocks that always run to end of page once opened.
    let forever = Regex::new(
        r"(?i)^(preguntas\s+y\s+respuestas|ejercicios\s+de\s+clausura|para\s+la\s+pr[oó]xima\s+semana|resumen\s+general)\b",
    )
    .unwrap();

    let mut out = String::new();
    let mut skipping = false;
    let mut forever_skip = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if forever_skip {
            continue;
        }
        if skipping {
            if resume_re.is_match(trimmed) {
                skipping = false;
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if start_re.is_match(trimmed) {
            skipping = true;
            if forever.is_match(trimmed) {
                forever_skip = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    let footer = Regex::new(
        r"(?im)^(no se permite fotocopiar|todos los derechos|senda de vida|volumen\s+\d+|item\s+\d).*$",
    )
    .unwrap();
    footer.replace_all(&out, "").into_owned()
}

/// Sliding word windows from OCR (better than `.`-split on 2-up/2-column scans).
fn content_phrases(text: &str) -> Vec<String> {
    let collapsed = Regex::new(r"[ \t\n]+").unwrap().replace_all(text, " ");
    let words: Vec<&str> = collapsed.split_whitespace().collect();
    if words.len() < WINDOW_WORDS {
        return if collapsed.trim().chars().count() >= MIN_SNIPPET_CHARS {
            vec![collapsed.trim().to_string()]
        } else {
            vec![]
        };
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + WINDOW_WORDS <= words.len() {
        out.push(words[i..i + WINDOW_WORDS].join(" "));
        i += WINDOW_STRIDE;
    }
    out
}

fn is_plausible_prose(norm: &str) -> bool {
    let chars: Vec<char> = norm.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let letters = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();
    let digits = chars.iter().filter(|c| c.is_ascii_digit()).count();
    let letter_ratio = letters as f64 / chars.len() as f64;
    if letter_ratio < 0.78 || digits > letters / 4 {
        return false;
    }
    let vowels = chars
        .iter()
        .filter(|c| "aeiou".contains(**c))
        .count();
    let vowel_ratio = vowels as f64 / letters.max(1) as f64;
    if !(0.28..=0.58).contains(&vowel_ratio) {
        return false;
    }
    // Reject OCR chrome / mixed-column garbage tokens.
    let junk = [
        "volumen",
        "item",
        "fotocopiar",
        "senda",
        "tema trimestral",
        "tema trumps",
        "soberania divina",
        "received",
        "trumps",
        "terllsoin",
    ];
    if junk.iter().any(|j| norm.contains(j)) {
        return false;
    }
    // Too many ultra-short tokens → column bleed mush.
    let toks: Vec<&str> = norm.split_whitespace().collect();
    let short = toks.iter().filter(|t| t.len() <= 2).count();
    if toks.len() >= 8 && short as f64 / toks.len() as f64 > 0.35 {
        return false;
    }
    true
}

fn looks_like_non_body(norm: &str) -> bool {
    let skip = [
        "fecha sugerida",
        "base biblica",
        "pensamiento central",
        "texto aureo",
        "lectura antifonal",
        "datos generales acerca",
        "definiciones y etimologia",
        "no se permite",
    ];
    skip.iter().any(|k| norm.contains(k))
}

fn normalize(s: &str) -> String {
    let lower: String = s.chars().flat_map(|c| c.to_lowercase()).collect();
    let deacc = strip_accents(&lower);
    let re = Regex::new(r"[^a-z0-9]+").unwrap();
    re.replace_all(&deacc, " ").trim().to_string()
}

fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn snippet_covered(snippet_norm: &str, extract_norm: &str) -> bool {
    if snippet_norm.is_empty() {
        return true;
    }
    // Exact / near-exact after normalize.
    if extract_norm.contains(snippet_norm) {
        return true;
    }
    // Allow small OCR gaps: require a long contiguous core.
    let tokens: Vec<&str> = snippet_norm.split_whitespace().collect();
    if tokens.len() >= 8 {
        // Try middle window of 6 tokens as anchor.
        let mid = tokens.len() / 2;
        let start = mid.saturating_sub(3);
        let end = (start + 6).min(tokens.len());
        let anchor = tokens[start..end].join(" ");
        if anchor.chars().count() >= 24 && extract_norm.contains(&anchor) {
            return true;
        }
    }
    // Token overlap against best window in extract.
    let snip_set: std::collections::HashSet<&str> = tokens.iter().copied().collect();
    if snip_set.len() < 5 {
        return false;
    }
    let ext_toks: Vec<&str> = extract_norm.split_whitespace().collect();
    let window = tokens.len().clamp(8, 40);
    let mut best = 0.0;
    if ext_toks.len() < window {
        let inter = ext_toks.iter().filter(|t| snip_set.contains(*t)).count();
        best = inter as f64 / snip_set.len() as f64;
    } else {
        for i in 0..=(ext_toks.len() - window) {
            let slice = &ext_toks[i..i + window];
            let inter = slice.iter().filter(|t| snip_set.contains(*t)).count();
            let score = inter as f64 / snip_set.len() as f64;
            if score > best {
                best = score;
            }
            if best >= MIN_TOKEN_OVERLAP {
                break;
            }
        }
    }
    best >= MIN_TOKEN_OVERLAP
}

fn printed_footer_page(text: &str) -> Option<u32> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
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

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let take: String = t.chars().take(max.saturating_sub(1)).collect();
        format!("{take}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_accents_and_punct() {
        assert_eq!(
            normalize("¡Hola, José!"),
            "hola jose"
        );
    }

    #[test]
    fn snippet_covered_exact_and_fuzzy() {
        let extract = normalize(
            "La historia de David demuestra claramente que Dios transforma \
             la vida de todos los que El llama para que le sirvan.",
        );
        let snip = normalize(
            "La historia de David demuestra claramente que Dios transforma \
             la vida de todos los que Él llama para que le sirvan.",
        );
        assert!(snippet_covered(&snip, &extract));

        let missing = normalize(
            "Nabucodonosor destruyo Jerusalen en el ano quinientos ochenta y siete.",
        );
        assert!(!snippet_covered(&missing, &extract));
    }

    #[test]
    fn strip_excluded_drops_ideas_and_resumen() {
        let raw = "\
INTRODUCCIÓN\n\
Resulta interesante hacer un breve resumen de David.\n\
Ideas para el maestro o líder\n\
(1) Invite a alguien.\n\
(2) Expliquen esto.\n\
DESARROLLO DEL ESTUDIO\n\
I. EN EL MOMENTO DE DIOS\n\
La historia de David demuestra claramente.\n\
RESUMEN GENERAL\n\
Como se senalo anteriormente cuando Dios escoge.\n";
        let cleaned = strip_excluded_regions(raw);
        assert!(cleaned.contains("Resulta interesante"));
        assert!(cleaned.contains("La historia de David"));
        assert!(!cleaned.to_lowercase().contains("invite a alguien"));
        assert!(!cleaned.to_lowercase().contains("como se senalo anteriormente"));
    }

    #[test]
    fn coverage_report_ok_threshold() {
        let ok = CoverageReport {
            study: 7,
            required: 10,
            matched: 9,
            missing: vec![],
            pages: (1, 8),
        };
        assert!(ok.ok());
        let bad = CoverageReport {
            study: 7,
            required: 10,
            matched: 7,
            missing: vec![],
            pages: (1, 8),
        };
        assert!(!bad.ok());
    }
}
