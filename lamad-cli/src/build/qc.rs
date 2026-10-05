//! Per-slide quality control for the build CLI (no AI).
//!
//! Runs after content fill, before zip. Hard failures abort the build so a
//! broken deck is never delivered.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct QcIssue {
    pub slide: u32,
    pub hard: bool,
    pub message: String,
}

/// QC every slide in `order`. Returns Err if any hard issue is found.
pub fn qc_deck(build: &Path, order: &[u32], image_slides: &[u32]) -> Result<()> {
    let mut issues = Vec::new();
    for &n in order {
        issues.extend(qc_slide(build, n, image_slides.contains(&n))?);
    }
    let hard: Vec<_> = issues.iter().filter(|i| i.hard).collect();
    for i in &issues {
        if i.hard {
            eprintln!("  QC FAIL slide {}: {}", i.slide, i.message);
        } else {
            crate::progress::debug(format!("QC warn slide {}: {}", i.slide, i.message));
        }
    }
    if !hard.is_empty() {
        bail!(
            "slide QC failed ({} hard issue{}). Fix packing/chrome before delivery.",
            hard.len(),
            if hard.len() == 1 { "" } else { "s" }
        );
    }
    crate::progress::ok(format!("QC: {} slides checked — OK", order.len()));
    Ok(())
}

fn qc_slide(build: &Path, slide: u32, is_image_chrome: bool) -> Result<Vec<QcIssue>> {
    let path = build
        .join("ppt")
        .join("slides")
        .join(format!("slide{slide}.xml"));
    if !path.is_file() {
        return Ok(vec![QcIssue {
            slide,
            hard: true,
            message: format!("missing {}", path.display()),
        }]);
    }
    let xml = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut issues = Vec::new();

    if xml.contains(r#"name="Arc"#) {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "retired Arc chrome still present".into(),
        });
    }

    if is_image_chrome {
        issues.extend(qc_image_chrome(slide, &xml));
    } else if looks_like_verse_slide(&xml) {
        // Only Lectura / Texto Bíblico — title/próximo may reuse shape names.
        issues.extend(qc_verse_like(slide, &xml));
    }

    Ok(issues)
}

fn looks_like_verse_slide(xml: &str) -> bool {
    let texts = all_text_runs(xml).join(" ");
    let upper = texts.to_uppercase();
    upper.contains("LECTURA") || upper.contains("TEXTO BÍBLICO") || upper.contains("TEXTO BIBLICO")
}

fn all_text_runs(xml: &str) -> Vec<String> {
    let re = Regex::new(r#"<a:t[^>]*>([^<]*)</a:t>"#).unwrap();
    re.captures_iter(xml)
        .map(|c| c[1].to_string())
        .collect()
}

fn qc_image_chrome(slide: u32, xml: &str) -> Vec<QcIssue> {
    let mut issues = Vec::new();
    if !xml.contains(r#"name="OrangeBar""#) {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "image slide missing OrangeBar".into(),
        });
    }
    if xml.contains(r#"name="!!Rectangle""#) {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "black !!Rectangle still covering scenic image".into(),
        });
    }
    if !xml.contains("<p:pic") {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "image slide missing <p:pic> (scenic image absent)".into(),
        });
    }
    let title = shape_plain_text(xml, "CuadroTexto 8");
    let verse = shape_plain_text(xml, "CuadroTexto 3");
    if title.trim().is_empty() {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "image slide title (CuadroTexto 8) empty".into(),
        });
    } else if title.chars().count() > 42 {
        // Long titles on widened adult panel (~5.4") — cap by length ladder.
        if let Some(block) = shape_block(xml, "CuadroTexto 8") {
            if let Some(caps) = Regex::new(r#"sz="(\d+)""#)
                .unwrap()
                .captures(&block)
            {
                let sz: u32 = caps[1].parse().unwrap_or(0);
                let n = title.chars().count();
                let max = if n > 70 {
                    2200
                } else if n > 55 {
                    2400
                } else {
                    2800
                };
                if sz > max {
                    issues.push(QcIssue {
                        slide,
                        hard: true,
                        message: format!(
                            "title too large for length (sz={sz}, need ≤{max}): {}…",
                            title.chars().take(40).collect::<String>()
                        ),
                    });
                }
            }
        }
    }
    if verse.trim().is_empty() {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: "image slide verse (CuadroTexto 3) empty".into(),
        });
    } else if verse.trim().starts_with('(') || verse.trim().ends_with(')') {
        issues.push(QcIssue {
            slide,
            hard: true,
            message: format!(
                "image citation has parentheses (forbidden): {}",
                verse.trim()
            ),
        });
    }
    // Title/verse must have an explicit fill colour (contrast applied).
    if let Some(block) = shape_block(xml, "CuadroTexto 8") {
        if !block.contains("srgbClr") {
            issues.push(QcIssue {
                slide,
                hard: true,
                message: "title has no srgbClr (contrast not applied)".into(),
            });
        }
    }
    issues
}

/// Lectura / Texto: if the verse shape exists, require real body text (not
/// numbers-only). Soft on unrelated slides that lack those shapes.
fn qc_verse_like(slide: u32, xml: &str) -> Vec<QcIssue> {
    let mut issues = Vec::new();
    // Texto: verse box must sit below the "Texto Bíblico" title (~0.9"+).
    if let Some(y) = textbox4_y_emu(xml) {
        if y < 800_000 {
            issues.push(QcIssue {
                slide,
                hard: true,
                message: format!(
                    "Texto TextBox 4 y={y} EMU overlaps title (need y>=856357)"
                ),
            });
        }
    }
    for (shape, label) in [("CuadroTexto 5", "Lectura"), ("TextBox 4", "Texto")] {
        if !xml.contains(&format!(r#"name="{shape}""#)) {
            continue;
        }
        let texts = shape_text_runs(xml, shape);
        if texts.is_empty() {
            issues.push(QcIssue {
                slide,
                hard: true,
                message: format!("{label} shape {shape} has no text runs"),
            });
            continue;
        }
        // First non-empty run that looks like a book citation must not use ().
        if label == "Texto" {
            if let Some(cite) = texts.iter().map(|t| t.trim()).find(|t| {
                !t.is_empty()
                    && !t.chars().all(|c| c.is_ascii_digit())
                    && t.contains(':')
            }) {
                if cite.starts_with('(') || cite.ends_with(')') {
                    issues.push(QcIssue {
                        slide,
                        hard: true,
                        message: format!(
                            "Texto Bíblico citation has parentheses (forbidden): {cite}"
                        ),
                    });
                }
            }
        }
        let has_body = texts.iter().any(|t| {
            let s = t.trim();
            !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit()) && s.chars().count() > 8
        });
        if !has_body {
            issues.push(QcIssue {
                slide,
                hard: true,
                message: format!("{label} slide has numbers/citation only — verse body missing"),
            });
        }
        // Soft budget warn (280 chars of verse body-ish runs).
        let body_chars: usize = texts
            .iter()
            .filter(|t| {
                let s = t.trim();
                !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit()) && !s.contains(':')
            })
            .map(|t| t.chars().count())
            .sum();
        if body_chars > 320 {
            issues.push(QcIssue {
                slide,
                hard: false,
                message: format!(
                    "{label} body ~{body_chars} chars (soft cap 280) — may overflow"
                ),
            });
        }
    }
    issues
}

fn shape_block<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!(r#"name="{name}""#);
    let pos = xml.find(&marker)?;
    let start = xml[..pos].rfind("<p:sp")?;
    let end = xml[pos..].find("</p:sp>")? + pos + "</p:sp>".len();
    Some(&xml[start..end])
}

fn shape_plain_text(xml: &str, name: &str) -> String {
    shape_text_runs(xml, name).join("")
}

fn shape_text_runs(xml: &str, name: &str) -> Vec<String> {
    let Some(block) = shape_block(xml, name) else {
        return Vec::new();
    };
    let re = Regex::new(r#"<a:t[^>]*>([^<]*)</a:t>"#).unwrap();
    re.captures_iter(block)
        .map(|c| c[1].to_string())
        .collect()
}

fn textbox4_y_emu(xml: &str) -> Option<i64> {
    let block = shape_block(xml, "TextBox 4")?;
    let re = Regex::new(r#"<a:off x="(-?\d+)" y="(-?\d+)""#).unwrap();
    let caps = re.captures(block)?;
    caps[2].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qc_image_chrome_detects_missing_title() {
        let xml = r#"<p:sld><p:sp><p:nvSpPr><p:cNvPr name="OrangeBar"/></p:nvSpPr></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr name="CuadroTexto 8"/></p:nvSpPr><p:txBody><a:p><a:r><a:t></a:t></a:r></a:p></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr name="CuadroTexto 3"/></p:nvSpPr><p:txBody><a:p><a:r><a:t>verse</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#;
        let issues = qc_image_chrome(1, xml);
        assert!(issues.iter().any(|i| i.message.contains("title")));
    }

    #[test]
    fn qc_verse_detects_numbers_only() {
        let xml = r#"<p:sld><p:sp><p:nvSpPr><p:cNvPr name="CuadroTexto 5"/></p:nvSpPr>
        <p:txBody><a:p><a:r><a:t>1</a:t></a:r></a:p><a:p><a:r><a:t>2</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#;
        let issues = qc_verse_like(2, xml);
        assert!(issues.iter().any(|i| i.hard && i.message.contains("body missing")));
    }
}
