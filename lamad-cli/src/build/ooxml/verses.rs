//! Lectura / Texto Bíblico verse slides.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

use super::shape::{
    all_runs, clone_run, find_txbody, first_para_ppr, replace_range, transform_shape,
};
use super::xml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerseKind {
    Lectura,
    Texto,
}

/// Split `"18 Y el niño creció..."` into `("18", " Y el niño creció...")`.
pub(crate) fn split_verse(v: &str) -> Result<(String, String)> {
    let re = Regex::new(r"(?s)^(\d+)\s+(.*)$").unwrap();
    let caps = re
        .captures(v.trim_start())
        .ok_or_else(|| anyhow!("verse must start with number: {:.60}", v))?;
    Ok((caps[1].to_string(), format!(" {}", &caps[2])))
}

struct VerseSamples {
    cite: Option<String>,
    num: Option<String>,
    body: Option<String>,
}

/// Pick cite/number/body run samples by content, not fixed index —
/// continuation Lectura/Texto slides start with a number run, so
/// index-based picking would swap colors (port of `pick_verse_samples`).
fn pick_verse_samples(runs: &[String]) -> VerseSamples {
    let mut sample_num: Option<String> = None;
    let mut sample_body: Option<String> = None;
    let mut sample_cite: Option<String> = None;

    for r in runs {
        let text = xml::run_text(r);
        let stripped = text.trim();
        if stripped.is_empty() {
            continue;
        }
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            if sample_num.is_none() {
                sample_num = Some(r.clone());
            }
            continue;
        }
        if sample_cite.is_none() && stripped.contains(':') && stripped.chars().count() < 40 {
            sample_cite = Some(r.clone());
            continue;
        }
        if sample_body.is_none() && (text.starts_with(' ') || stripped.chars().count() > 15) {
            sample_body = Some(r.clone());
        }
    }

    if sample_num.is_none() {
        sample_num = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
            })
            .cloned()
            .or_else(|| runs.first().cloned());
    }
    if sample_body.is_none() {
        sample_body = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            })
            .cloned()
            .or_else(|| runs.last().cloned());
    }
    if sample_cite.is_none() {
        sample_cite = sample_num.clone();
    }

    VerseSamples {
        cite: sample_cite,
        num: sample_num,
        body: sample_body,
    }
}

/// Expand verse lines where multiple verses were glued into one string
/// (`…pronto; 3 no…`, `…derecha, 4 para…`, poetry after `porque:`).
pub fn expand_glued_verses(verses: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for v in verses {
        out.extend(split_glued_verse_line(v));
    }
    out
}

fn split_glued_verse_line(line: &str) -> Vec<String> {
    let re = Regex::new(r"(?:;\s*|\,\s*)(\d+)\s+").unwrap();
    let mut parts: Vec<String> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(line) {
        if m.start() == 0 {
            continue;
        }
        let before = line[last..m.start()]
            .trim_end_matches(';')
            .trim_end_matches(',')
            .trim();
        if !before.is_empty() {
            parts.push(before.to_string());
        }
        last = m.start() + m.as_str().find(|c: char| c.is_ascii_digit()).unwrap_or(0);
    }
    if last < line.len() {
        parts.extend(split_poetry_lines(line[last..].trim()));
    }
    if parts.is_empty() && !line.trim().is_empty() {
        parts.push(line.trim().to_string());
    }
    parts
}

fn split_poetry_lines(line: &str) -> Vec<String> {
    if let Some(idx) = line.find("porque:") {
        let (head, tail) = line.split_at(idx);
        let head = head.trim();
        let mut out = Vec::new();
        if !head.is_empty() {
            out.push(head.to_string());
        }
        let poetry = tail.trim_start_matches("porque:").trim();
        if poetry.contains(",Y ") {
            let pieces: Vec<&str> = poetry.split(",Y ").collect();
            if let Some(first) = pieces.first() {
                out.push(first.trim().to_string());
            }
            for piece in pieces.iter().skip(1) {
                out.push(format!("Y {piece}"));
            }
        } else if !poetry.is_empty() {
            out.push(poetry.to_string());
        }
        return out;
    }
    vec![line.trim().to_string()]
}

fn format_citation_from_template(
    shape_xml: &str,
    cite: &str,
    kind: VerseKind,
) -> Result<String> {
    match kind {
        VerseKind::Texto => super::shape::format_ref_citation_from_template(shape_xml, cite),
        VerseKind::Lectura => {
            let nodes = super::xml::all_elements(shape_xml, "a:t");
            let sample = nodes
                .iter()
                .map(|e| e.inner(shape_xml, "a:t"))
                .find(|t| !t.trim().is_empty())
                .unwrap_or("");
            let base = cite.trim().trim_end_matches(';').trim();
            if sample.contains(';') {
                Ok(format!("{base}; "))
            } else {
                Ok(base.to_string())
            }
        }
    }
}

/// Fill a Lectura/Texto Bíblico slide with whole verses (+ optional
/// citation on the first slide of a range). One paragraph per citation /
/// per verse — no soft `a:br` breaks (matches known-good decks).
pub fn set_verses(path: &Path, verses: &[String], citation: Option<&str>, kind: VerseKind) -> Result<()> {
    let expanded = expand_glued_verses(verses);
    set_verses_expanded(path, &expanded, citation, kind)
}

fn set_verses_expanded(
    path: &Path,
    verses: &[String],
    citation: Option<&str>,
    kind: VerseKind,
) -> Result<()> {
    let shape_name = match kind {
        VerseKind::Lectura => "CuadroTexto 5",
        VerseKind::Texto => "TextBox 4",
    };
    let accent = match kind {
        VerseKind::Lectura => "FF0000",
        VerseKind::Texto => "FFFF00",
    };
    let body_rgb: Option<&str> = match kind {
        VerseKind::Lectura => None, // default black — leave inherited
        VerseKind::Texto => Some("FFFFFF"),
    };

    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape_name, |shape_xml| {
        let (tb, tag) = find_txbody(shape_xml)?;
        let content = tb.inner(shape_xml, tag);
        let runs = all_runs(content);
        if runs.is_empty() {
            bail!("no sample runs in shape {shape_name}");
        }
        let samples = pick_verse_samples(&runs);

        let p_start = xml::next_element(content, "a:p", 0)
            .map(|e| e.start)
            .unwrap_or(content.len());
        let head = &content[..p_start];
        let ppr = first_para_ppr(content).unwrap_or_default();

        let mut body = String::new();
        if let Some(cite) = citation {
            let formatted = format_citation_from_template(shape_xml, cite, kind)?;
            let r = clone_run(samples.cite.as_deref(), &formatted, Some(true), Some(Some(accent)), None)?;
            body.push_str(&format!("<a:p>{ppr}{r}</a:p>"));
        }
        for v in verses {
            let trimmed = v.trim();
            match split_verse(trimmed) {
                Ok((num, verse_body)) => {
                    let r_num = clone_run(samples.num.as_deref(), &num, Some(true), Some(Some(accent)), None)?;
                    let color_opt = body_rgb.map(Some);
                    let r_body = clone_run(samples.body.as_deref(), &verse_body, Some(false), color_opt, None)?;
                    body.push_str(&format!("<a:p>{ppr}{r_num}{r_body}</a:p>"));
                }
                Err(_) => {
                    let body_text = if trimmed.starts_with(' ') {
                        trimmed.to_string()
                    } else {
                        format!(" {trimmed}")
                    };
                    let r_body = clone_run(samples.body.as_deref(), &body_text, Some(false), body_rgb.map(Some), None)?;
                    body.push_str(&format!("<a:p>{ppr}{r_body}</a:p>"));
                }
            }
        }

        let new_content = format!("{head}{body}");
        let open_tag = tb.open_tag(shape_xml);
        let new_txbody = format!("{open_tag}{new_content}</{tag}>");
        Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}
