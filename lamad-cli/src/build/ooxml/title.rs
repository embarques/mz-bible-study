//! Title / Próximo slide chrome (`Base Bíblica:` bold label only).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;

use super::shape::{
    clone_run, find_txbody, first_para_ppr, first_run, replace_range, set_simple_text_block,
    transform_shape,
};

/// Adult `TextBox 7` (`Base Bíblica`) top edge in EMU — title box must end above this.
const ADULT_BASE_BIBLICA_Y: i64 = 5_755_697;
/// Gap between title box bottom and Base Bíblica top (~0.25").
const ADULT_TITLE_BASE_GAP: i64 = 230_000;

pub fn set_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
  fill_title_slide(path, numero, titulo, base, "CuadroTexto 13")
}

/// Adult title / próximo slides use `CuadroTexto 1` for the study title.
/// Adult gold sometimes paints a cyan `<a:highlight>` behind part of the
/// title (e.g. "LA MODESTIA") — strip that so the title matches youth
/// (plain black type on white, no background chip).
///
/// Keep the title **grande** (near gold 115pt). Long titles get explicit
/// line breaks (2–3 lines) + a slightly smaller size so every word fits
/// above Base Bíblica without covering it.
pub fn set_adult_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
    let max_bottom = ADULT_BASE_BIBLICA_Y - ADULT_TITLE_BASE_GAP;
    // Probe box geometry from the template before writing text.
    let probe = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let box_cx = shape_cx(&probe, "CuadroTexto 1").unwrap_or(ADULT_TITLE_BOX_CX);
    let box_y = shape_y(&probe, "CuadroTexto 1").unwrap_or(ADULT_TITLE_BOX_Y);
    let available = (max_bottom - box_y).max(500_000);

    // Try 2-line layout first; fall back to 3 lines if we must shrink below 72pt.
    let words: Vec<&str> = titulo.split_whitespace().filter(|w| !w.is_empty()).collect();
    let mut lines = adult_title_lines(titulo);
    let mut title_sz = adult_title_font_sz_fit(titulo, &lines, box_cx, available);
    if title_sz < 7200 && words.len() >= 6 {
        let three = split_words_balanced(&words, 3);
        let sz3 = adult_title_font_sz_fit(titulo, &three, box_cx, available);
        if sz3 > title_sz {
            lines = three;
            title_sz = sz3;
        }
    }

    fill_title_slide_lines(path, numero, &lines, base, "CuadroTexto 1")?;
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = strip_text_highlights(&xml);
    let xml = transform_shape(&xml, "CuadroTexto 1", |b| {
        let b = force_shape_run_sz(b, title_sz);
        let b = set_body_pr_anchor(&b, "t"); // top — wrap down, leave Base clear
        Ok(fit_title_box_above_base(&b, max_bottom))
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Default adult title box geometry (gold slide 1) when XML probe fails.
const ADULT_TITLE_BOX_CX: i64 = 12_529_326;
const ADULT_TITLE_BOX_Y: i64 = 1_502_300;

/// Split long titles into 2–3 centred lines. Prefer **2 balanced lines**;
/// only go to 3 when even a shrunk 2-line layout still overflows Base.
fn adult_title_lines(titulo: &str) -> Vec<String> {
    let words: Vec<&str> = titulo.split_whitespace().filter(|w| !w.is_empty()).collect();
    let chars = titulo.chars().count();
    if words.len() <= 4 && chars <= 32 {
        return vec![titulo.trim().to_string()];
    }
    if words.len() >= 5 || chars > 28 {
        return split_words_balanced(&words, 2);
    }
    vec![titulo.trim().to_string()]
}

/// Pick the cut that best balances character counts; keep short glue words
/// ("A", "DE", "SU") with the **previous** line so they are not orphans.
fn split_words_balanced(words: &[&str], n: usize) -> Vec<String> {
    if words.is_empty() {
        return vec![String::new()];
    }
    if n <= 1 || words.len() <= n {
        return vec![words.join(" ")];
    }
    if n == 2 {
        let mut best = 1usize;
        let mut best_score = i64::MAX;
        for cut in 1..words.len() {
            let left = words[..cut].join(" ").chars().count() as i64;
            let right = words[cut..].join(" ").chars().count() as i64;
            let mut score = (left - right).abs();
            // Prefer not starting the second line with a tiny glue word.
            if words[cut].chars().count() <= 2 {
                score += 8;
            }
            if score < best_score {
                best_score = score;
                best = cut;
            }
        }
        return vec![words[..best].join(" "), words[best..].join(" ")];
    }
    // 3 lines — equal char thirds.
    let total: usize = words.iter().map(|w| w.chars().count() + 1).sum();
    let mut cuts = Vec::new();
    let mut acc = 0usize;
    let mut next_target = total / 3;
    for (i, w) in words.iter().enumerate() {
        if cuts.len() >= 2 {
            break;
        }
        acc += w.chars().count() + 1;
        if acc >= next_target && i + 1 < words.len() {
            let mut cut = i + 1;
            if words[cut].chars().count() <= 2 && cut + 1 < words.len() {
                cut += 1;
            }
            if cuts.last().copied() != Some(cut) {
                cuts.push(cut);
                next_target = total.saturating_mul(cuts.len() + 1) / 3;
            }
        }
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for &cut in &cuts {
        out.push(words[start..cut].join(" "));
        start = cut;
    }
    out.push(words[start..].join(" "));
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Hundredths of a point — stay grande when it fits; shrink so wrapped height
/// never crosses Base Bíblica.
fn adult_title_font_sz_fit(
    titulo: &str,
    lines: &[String],
    box_cx: i64,
    available_cy: i64,
) -> u32 {
    let mut sz = adult_title_font_sz_start(titulo, lines.len());
    while sz > 6400 {
        let visual = estimate_visual_lines(lines, box_cx, sz);
        let need = (visual as i64) * line_height_emu(sz);
        if need <= available_cy {
            break;
        }
        sz -= 400; // step down 4pt
    }
    sz
}

fn adult_title_font_sz_start(titulo: &str, line_count: usize) -> u32 {
    let n = titulo.chars().count();
    if n <= 28 && line_count <= 2 {
        11500 // 115pt — gold short titles
    } else if line_count <= 2 {
        10400 // 104pt on 2 lines
    } else if line_count == 3 {
        9600 // 96pt on 3 lines
    } else {
        8800
    }
}

/// Rough visual wrap count for bold caps (Verlag Black) at `sz` hundredths.
fn estimate_visual_lines(lines: &[String], box_cx: i64, sz: u32) -> usize {
    // Average capital width ≈ 0.58 × font size (pt) for Verlag Black.
    let char_emu = ((sz as f64) / 100.0) * 0.58 * (914_400.0 / 72.0);
    let cpl = ((box_cx as f64) / char_emu).floor().max(8.0) as usize;
    let mut total = 0usize;
    for line in lines {
        let n = line.chars().count().max(1);
        total += n.div_ceil(cpl);
    }
    total.max(1)
}

fn line_height_emu(sz: u32) -> i64 {
    // ~1.12× leading — PowerPoint default for display titles.
    let pt = (sz as f64) / 100.0;
    ((pt * 1.12) * (914_400.0 / 72.0)) as i64
}

fn shape_cx(xml: &str, shape: &str) -> Option<i64> {
    let marker = format!(r#"name="{shape}""#);
    let pos = xml.find(&marker)?;
    let block = &xml[pos..xml[pos..].find("</p:sp>").map(|i| pos + i)?];
    let re = Regex::new(r#"<a:ext cx="(-?\d+)""#).ok()?;
    re.captures(block)?.get(1)?.as_str().parse().ok()
}

fn shape_y(xml: &str, shape: &str) -> Option<i64> {
    let marker = format!(r#"name="{shape}""#);
    let pos = xml.find(&marker)?;
    let block = &xml[pos..xml[pos..].find("</p:sp>").map(|i| pos + i)?];
    let re = Regex::new(r#"<a:off x="-?\d+" y="(-?\d+)""#).ok()?;
    re.captures(block)?.get(1)?.as_str().parse().ok()
}

/// Set every `<a:rPr … sz="…">` inside the shape to `sz`.
fn force_shape_run_sz(shape_xml: &str, sz: u32) -> String {
    let re = Regex::new(r#"(<a:rPr\b[^>]*\bsz=")(\d+)(")"#).expect("rPr sz regex");
    re.replace_all(shape_xml, |caps: &regex::Captures| {
        format!("{}{}{}", &caps[1], sz, &caps[3])
    })
    .into_owned()
}

fn set_body_pr_anchor(shape_xml: &str, anchor: &str) -> String {
    let re = Regex::new(r#"<a:bodyPr([^>]*)>"#).expect("bodyPr regex");
    re.replace(shape_xml, |caps: &regex::Captures| {
        let attrs = Regex::new(r#"\sanchor="[^"]*""#)
            .expect("anchor attr")
            .replace(&caps[1], "");
        format!(r#"<a:bodyPr{attrs} anchor="{anchor}">"#)
    })
    .into_owned()
}

/// Grow / clamp title box so its bottom sits just above Base Bíblica.
fn fit_title_box_above_base(shape_xml: &str, max_bottom: i64) -> String {
    let re = Regex::new(
        r#"<a:off x="(-?\d+)" y="(-?\d+)"\s*/>\s*<a:ext cx="(-?\d+)" cy="(-?\d+)"\s*/>"#,
    )
    .expect("xfrm regex");
    re.replace(shape_xml, |caps: &regex::Captures| {
        let x: i64 = caps[1].parse().unwrap_or(0);
        let y: i64 = caps[2].parse().unwrap_or(0);
        let cx: i64 = caps[3].parse().unwrap_or(0);
        let max_cy = (max_bottom - y).max(500_000);
        format!(r#"<a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{max_cy}"/>"#)
    })
    .into_owned()
}

fn fill_title_slide(
    path: &Path,
    numero: &str,
    titulo: &str,
    base: &[String],
    title_shape: &str,
) -> Result<()> {
    fill_title_slide_lines(path, numero, &[titulo.to_string()], base, title_shape)
}

fn fill_title_slide_lines(
    path: &Path,
    numero: &str,
    title_lines: &[String],
    base: &[String],
    title_shape: &str,
) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "TextBox 4", |b| set_simple_text_block(b, numero, Some(true)))?;
    let xml = transform_shape(&xml, title_shape, |b| {
        let clean = strip_text_highlights(b);
        set_multiline_centred_block(&clean, title_lines, Some(false))
    })?;
    let xml = transform_shape(&xml, "TextBox 7", |b| set_base_biblica_block(b, base))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// One centred paragraph with `<a:br/>` between lines (keeps title chrome).
fn set_multiline_centred_block(
    shape_xml: &str,
    lines: &[String],
    bold: Option<bool>,
) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = super::xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);
    let mut ppr = first_para_ppr(content).unwrap_or_else(|| r#"<a:pPr algn="ctr"/>"#.to_string());
    if !ppr.contains("algn=") {
        ppr = ppr.replacen("<a:pPr", r#"<a:pPr algn="ctr""#, 1);
    }
    let mut body = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            body.push_str(r#"<a:br/>"#);
        }
        body.push_str(&clone_run(sample.as_deref(), line, bold, None, None)?);
    }
    let new_paragraph = format!(r#"<a:p>{ppr}{body}<a:endParaRPr lang="es-DO"/></a:p>"#);
    let new_content = format!("{head}{new_paragraph}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

/// Remove PowerPoint text-highlight chips (`<a:highlight>…</a:highlight>`).
fn strip_text_highlights(xml: &str) -> String {
    let re = regex::Regex::new(r"<a:highlight\b[^>]*>[\s\S]*?</a:highlight>|<a:highlight\b[^/]*/>")
        .expect("highlight regex");
    re.replace_all(xml, "").into_owned()
}

fn set_base_biblica_block(shape_xml: &str, base: &[String]) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    // Keep <a:bodyPr> / <a:lstStyle> — PowerPoint treats a txBody that starts
    // with <a:p> (no bodyPr) as corrupt and offers Repair.
    let p_start = super::xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);

    let mut p = String::from(r#"<a:p><a:pPr algn="ctr"/>"#);
    p.push_str(&clone_run(sample.as_deref(), "Base Bíblica: ", Some(true), None, None)?);
    p.push_str("</a:p>");

    let cite_line: String = base
        .iter()
        .map(|line| {
            let t = line.trim();
            if t.ends_with(';') {
                format!("{t} ")
            } else {
                t.to_string()
            }
        })
        .collect();

    let mut p2 = String::from(r#"<a:p><a:pPr algn="ctr"/>"#);
    p2.push_str(&clone_run(sample.as_deref(), &cite_line, Some(false), None, None)?);
    p2.push_str("</a:p>");

    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{head}{p}{p2}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}
