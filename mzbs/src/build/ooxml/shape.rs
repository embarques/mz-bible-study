//! Shape lookup and text-block rewriting helpers.

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

use super::xml::{self, Elem};

/// Locate the `<p:sp>...</p:sp>` block whose `cNvPr` has `name="name"`.
pub(crate) fn shape_block(slide_xml: &str, name: &str) -> Result<(usize, usize)> {
    let needle = format!(r#"name="{name}""#);
    let name_pos = slide_xml
        .find(&needle)
        .ok_or_else(|| anyhow!("shape not found: {name}"))?;
    let start = slide_xml[..name_pos]
        .rfind("<p:sp")
        .ok_or_else(|| anyhow!("no enclosing <p:sp> for shape: {name}"))?;
    let rel_end = slide_xml[name_pos..]
        .find("</p:sp>")
        .ok_or_else(|| anyhow!("no closing </p:sp> for shape: {name}"))?;
    let end = name_pos + rel_end + "</p:sp>".len();
    Ok((start, end))
}

pub(crate) fn replace_range(s: &str, start: usize, end: usize, with: &str) -> String {
    format!("{}{}{}", &s[..start], with, &s[end..])
}

/// Apply `f` to the shape block named `name` within `slide_xml`, splicing
/// the result back in.
pub(crate) fn transform_shape(
    slide_xml: &str,
    name: &str,
    f: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let (start, end) = shape_block(slide_xml, name)?;
    let new_block = f(&slide_xml[start..end])?;
    Ok(replace_range(slide_xml, start, end, &new_block))
}

/// Locate `<p:txBody>` (or, rarely, `<a:txBody>`) within a shape block.
pub(crate) fn find_txbody(shape_xml: &str) -> Result<(Elem, &'static str)> {
    if let Some(el) = xml::next_element(shape_xml, "p:txBody", 0) {
        return Ok((el, "p:txBody"));
    }
    if let Some(el) = xml::next_element(shape_xml, "a:txBody", 0) {
        return Ok((el, "a:txBody"));
    }
    bail!("shape has no txBody")
}

const DEFAULT_RUN: &str = r#"<a:r><a:rPr lang="es-MX"/><a:t></a:t></a:r>"#;

/// Build a new run from `sample` (an existing `<a:r>...</a:r>` string, or
/// `None` to fall back to a bare run), setting its text and — when
/// requested — bold / fill color / forced size+typeface.
pub(crate) fn clone_run(
    sample: Option<&str>,
    text: &str,
    bold: Option<bool>,
    color: Option<Option<&str>>,
    force_sz_typeface: Option<(u32, &str)>,
) -> Result<String> {
    let sample = sample.unwrap_or(DEFAULT_RUN);
    let rpr = xml::next_element(sample, "a:rPr", 0)
        .ok_or_else(|| anyhow!("run has no a:rPr"))?;
    let open = rpr.open_tag(sample);
    let mut new_open = xml::set_bold_attr(open, bold);
    let mut children = if rpr.self_closing {
        String::new()
    } else {
        rpr.inner(sample, "a:rPr").to_string()
    };

    if let Some(rgb) = color {
        children = xml::set_run_color(&children, rgb);
    }
    if let Some((sz, typeface)) = force_sz_typeface {
        new_open = xml::set_sz_attr(&new_open, sz);
        for tag in ["a:latin", "a:ea", "a:cs"] {
            children = xml::force_typeface(&children, tag, typeface);
        }
    }

    let new_rpr = if children.is_empty() && rpr.self_closing {
        // still self-closing (open tag already ends with "/>")
        new_open
    } else {
        // ensure open tag doesn't self-close now that we (may) have children
        let opening = if new_open.ends_with("/>") {
            format!("{}>", &new_open[..new_open.len() - 2])
        } else {
            new_open
        };
        format!("{opening}{children}</a:rPr>")
    };

    Ok(format!("<a:r>{new_rpr}{}</a:r>", xml::build_t(text)))
}

/// First `<a:r>...</a:r>` anywhere inside `s` (used as a style sample).
pub(crate) fn first_run(s: &str) -> Option<String> {
    xml::next_element(s, "a:r", 0).map(|e| e.whole(s).to_string())
}

/// All `<a:r>...</a:r>` runs anywhere inside `s`, in document order.
pub(crate) fn all_runs(s: &str) -> Vec<String> {
    xml::all_elements(s, "a:r")
        .into_iter()
        .map(|e| e.whole(s).to_string())
        .collect()
}

/// First `<a:pPr>...` (self-closing or paired) inside the first `<a:p>`
/// paragraph of `content`, if any.
pub(crate) fn first_para_ppr(content: &str) -> Option<String> {
    let p = xml::next_element(content, "a:p", 0)?;
    let inner = p.inner(content, "a:p");
    xml::next_element(inner, "a:pPr", 0).map(|e| e.whole(inner).to_string())
}

// ---------------------------------------------------------------------------
// Generic paragraph-body replacement (title/section/AB/content shapes)
// ---------------------------------------------------------------------------

/// Replace shape text with a single run, preserving first-run formatting
/// and the first paragraph's `pPr` (port of `set_simple_text`).
pub(crate) fn set_simple_text_block(shape_xml: &str, text: &str, bold: Option<bool>) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);
    let ppr = first_para_ppr(content).unwrap_or_default();
    let run = clone_run(sample.as_deref(), text, bold, None, None)?;
    let new_paragraph = format!(r#"<a:p>{ppr}{run}<a:endParaRPr lang="es-DO"/></a:p>"#);
    let new_content = format!("{head}{new_paragraph}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

const REF_PATTERN: &str = r"(\((?:v\.?\s*\d+|[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\s]+\s+\d+[^\)]*)\)|(?:[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.]*(?:\s+[A-Za-zÁÉÍÓÚáéíóúñÑ\.]+)*)\s+\d+:\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)";

/// Body text with inline scripture references (`(Efesios 2:1)`, `Josué
/// 10:7-14`, ...) bolded; everything else regular weight. When
/// `force_tnr_42` is set, every run is forced to Times New Roman 42pt
/// (Conclusión — see AGENTS.md hard rule).
pub(crate) fn set_body_with_refs_block(shape_xml: &str, text: &str, force_tnr_42: bool) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);
    let ppr = first_para_ppr(content).unwrap_or_default();

    let ref_re = Regex::new(REF_PATTERN).context("compile REF_RE")?;
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut pos = 0usize;
    for m in ref_re.find_iter(text) {
        if m.start() > pos {
            parts.push((text[pos..m.start()].to_string(), false));
        }
        parts.push((m.as_str().to_string(), true));
        pos = m.end();
    }
    if pos < text.len() {
        parts.push((text[pos..].to_string(), false));
    }
    if parts.is_empty() {
        parts.push((text.to_string(), false));
    }

    let force = force_tnr_42.then_some((4200u32, "Times New Roman"));
    let mut runs = String::new();
    for (chunk, is_ref) in parts {
        if chunk.is_empty() {
            continue;
        }
        let bold = Some(is_ref);
        runs.push_str(&clone_run(sample.as_deref(), &chunk, bold, None, force)?);
    }
    let end_sz = if force_tnr_42 { r#" sz="4200""# } else { "" };
    let new_paragraph = format!(r#"<a:p>{ppr}{runs}<a:endParaRPr lang="es-DO"{end_sz}/></a:p>"#);
    let new_content = format!("{head}{new_paragraph}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}
