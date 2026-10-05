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
    let lines = adult_title_lines(titulo);
    fill_title_slide_lines(path, numero, &lines, base, "CuadroTexto 1")?;
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = strip_text_highlights(&xml);
    let title_sz = adult_title_font_sz(titulo, lines.len());
    let max_bottom = ADULT_BASE_BIBLICA_Y - ADULT_TITLE_BASE_GAP;
    let xml = transform_shape(&xml, "CuadroTexto 1", |b| {
        let b = force_shape_run_sz(b, title_sz);
        let b = set_body_pr_anchor(&b, "t"); // top — wrap down, leave Base clear
        Ok(fit_title_box_above_base(&b, max_bottom))
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Split long titles into 2–3 centred lines so we can stay near gold size.
fn adult_title_lines(titulo: &str) -> Vec<String> {
    let words: Vec<&str> = titulo.split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.len() <= 5 {
        return vec![titulo.trim().to_string()];
    }
    if words.len() <= 8 {
        let mid = words.len() / 2;
        let break_at = if mid < words.len()
            && words[mid].chars().count() <= 2
            && mid + 1 < words.len()
        {
            mid + 1
        } else {
            mid
        };
        return vec![
            words[..break_at].join(" "),
            words[break_at..].join(" "),
        ];
    }
    let a = (words.len() + 2) / 3;
    let b = (words.len() + 2) / 3;
    vec![
        words[..a].join(" "),
        words[a..a + b].join(" "),
        words[a + b..].join(" "),
    ]
}

/// Hundredths of a point — stay grande; fewer lines ⇒ larger type.
fn adult_title_font_sz(titulo: &str, line_count: usize) -> u32 {
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
