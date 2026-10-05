//! Lectura / Texto Bíblico verse slides.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

use super::shape::{find_txbody, first_para_ppr, replace_range, transform_shape};
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

/// Strip `()`, trailing `;`, and extra spaces from a Lectura/Texto citation.
fn clean_verse_citation(cite: &str) -> String {
    cite.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string()
}

fn format_citation_from_template(
    shape_xml: &str,
    cite: &str,
    kind: VerseKind,
) -> Result<String> {
    match kind {
        // Texto Bíblico: never parentheses — adult gold prototypes wrap
        // cites in `()` and format_ref_citation_from_template would copy that.
        VerseKind::Texto => {
            let _ = shape_xml;
            Ok(clean_verse_citation(cite))
        }
        VerseKind::Lectura => {
            // Lectura / Lectura Antifonal citation title: clean book range only.
            // Never copy trailing `;` from dirty adult template samples
            // (`;` belongs on Base Bíblica multi-line lists, not here).
            let _ = shape_xml;
            Ok(clean_verse_citation(cite))
        }
    }
}

/// Youth-style verse run: minimal `rPr` so colour is never lost to polluted
/// adult template samples (Verlag + ln/effectLst, or a lone `"1"` from
/// `"1 Corintios"` mistaken as a verse number).
fn clean_verse_run(text: &str, bold: bool, color: Option<&str>, sz: u32) -> String {
    let bold_attr = if bold { r#" b="1""# } else { "" };
    let fill = match color {
        Some(rgb) => format!(r#"<a:solidFill><a:srgbClr val="{rgb}"/></a:solidFill>"#),
        None => String::new(),
    };
    format!(
        r#"<a:r><a:rPr lang="es-DO" sz="{sz}" dirty="0"{bold_attr}>{fill}</a:rPr>{}</a:r>"#,
        xml::build_t(text)
    )
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
    // Match youth Lectura / Texto colour + weight exactly.
    let (accent, body_rgb, sz): (&str, Option<&str>, u32) = match kind {
        VerseKind::Lectura => ("FF0000", None, 4400), // red cite/num; body inherits black
        VerseKind::Texto => ("FFFF00", Some("FFFFFF"), 4400), // yellow cite/num; white body
    };

    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape_name, |shape_xml| {
        let (tb, tag) = find_txbody(shape_xml)?;
        let content = tb.inner(shape_xml, tag);

        let p_start = xml::next_element(content, "a:p", 0)
            .map(|e| e.start)
            .unwrap_or(content.len());
        let head = &content[..p_start];
        let ppr = first_para_ppr(content).unwrap_or_default();

        let mut body = String::new();
        if let Some(cite) = citation {
            let formatted = format_citation_from_template(shape_xml, cite, kind)?;
            body.push_str(&format!(
                "<a:p>{ppr}{}</a:p>",
                clean_verse_run(&formatted, true, Some(accent), sz)
            ));
        }
        for v in verses {
            let trimmed = v.trim();
            match split_verse(trimmed) {
                Ok((num, verse_body)) => {
                    let r_num = clean_verse_run(&num, true, Some(accent), sz);
                    let r_body = clean_verse_run(&verse_body, false, body_rgb, sz);
                    body.push_str(&format!("<a:p>{ppr}{r_num}{r_body}</a:p>"));
                }
                Err(_) => {
                    let body_text = if trimmed.starts_with(' ') {
                        trimmed.to_string()
                    } else {
                        format!(" {trimmed}")
                    };
                    let r_body = clean_verse_run(&body_text, false, body_rgb, sz);
                    body.push_str(&format!("<a:p>{ppr}{r_body}</a:p>"));
                }
            }
        }

        let new_content = format!("{head}{body}");
        let open_tag = tb.open_tag(shape_xml);
        let new_txbody = format!("{open_tag}{new_content}</{tag}>");
        let shape_xml = replace_range(shape_xml, tb.start, tb.end, &new_txbody);
        let shape_xml = force_verse_no_autofit(&shape_xml);
        // Adult gold has broken TextBox 4 frames (y negative / under the
        // "Texto Bíblico" title). Clamp geometry so verses never overlap
        // the header.
        let shape_xml = if kind == VerseKind::Texto {
            fix_texto_verse_box_xfrm(&shape_xml)
        } else {
            shape_xml
        };
        Ok(shape_xml)
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Known-good adult Texto verse box (from template slide 17), EMUs.
const TEXTO_BOX_X: i64 = 294_515;
const TEXTO_BOX_Y: i64 = 856_357; // ~0.94" — clears "Texto Bíblico" title
const TEXTO_BOX_CX: i64 = 11_602_969;
const TEXTO_BOX_CY: i64 = 5_755_422;

fn fix_texto_verse_box_xfrm(shape_xml: &str) -> String {
    let re = Regex::new(
        r#"<a:xfrm>\s*<a:off x="(-?\d+)" y="(-?\d+)"\s*/>\s*<a:ext cx="(-?\d+)" cy="(-?\d+)"\s*/>\s*</a:xfrm>"#,
    )
    .unwrap();
    let Some(caps) = re.captures(shape_xml) else {
        return shape_xml.to_string();
    };
    let y: i64 = caps[2].parse().unwrap_or(0);
    // Only rewrite when the box is too high (overlaps title) or negative.
    if y >= TEXTO_BOX_Y {
        return shape_xml.to_string();
    }
    let new_xfrm = format!(
        r#"<a:xfrm><a:off x="{TEXTO_BOX_X}" y="{TEXTO_BOX_Y}"/><a:ext cx="{TEXTO_BOX_CX}" cy="{TEXTO_BOX_CY}"/></a:xfrm>"#
    );
    re.replace(shape_xml, new_xfrm.as_str()).into_owned()
}

fn force_verse_no_autofit(shape_xml: &str) -> String {
    let re = Regex::new(r"<a:(?:spAutoFit|normAutofit)\s*/>").unwrap();
    let replaced = re.replace_all(shape_xml, "<a:noAutofit/>");
    if replaced.contains("<a:noAutofit") {
        return replaced.into_owned();
    }
    // No autofit node yet — insert into bodyPr.
    let Some(el) = xml::next_element(shape_xml, "a:bodyPr", 0) else {
        return replaced.into_owned();
    };
    if el.self_closing {
        let open = el.open_tag(shape_xml);
        let attrs = open
            .trim_start_matches("<a:bodyPr")
            .trim_end_matches("/>")
            .trim_end_matches('>');
        let expanded = format!("<a:bodyPr{attrs}><a:noAutofit/></a:bodyPr>");
        return replace_range(shape_xml, el.start, el.end, &expanded);
    }
    let inner = el.inner(shape_xml, "a:bodyPr");
    let open = el.open_tag(shape_xml);
    replace_range(
        shape_xml,
        el.start,
        el.end,
        &format!("{open}{inner}<a:noAutofit/></a:bodyPr>"),
    )
}
