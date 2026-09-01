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

/// Split commentary body at teaching markers `(1)`, `(2)`, … so each
/// marker starts a new paragraph (matches gold adult template layout).
pub(crate) fn split_teaching_paragraphs(text: &str) -> Vec<String> {
    let re = Regex::new(r"\(\d+\)").unwrap();
    let mut starts = vec![0usize];
    for m in re.find_iter(text) {
        if m.start() > 0 {
            starts.push(m.start());
        }
    }
    if starts.len() == 1 {
        return vec![text.trim().to_string()];
    }
    let mut out = Vec::new();
    for pair in starts.windows(2) {
        let chunk = text[pair[0]..pair[1]].trim();
        if !chunk.is_empty() {
            out.push(chunk.to_string());
        }
    }
    let tail = text[starts[starts.len() - 1]..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

fn build_ref_runs(
    text: &str,
    sample: Option<&str>,
    force_tnr_42: bool,
) -> Result<String> {
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
        runs.push_str(&clone_run(sample, &chunk, Some(is_ref), None, force)?);
    }
    Ok(runs)
}

/// Body text with inline scripture references (`(Efesios 2:1)`, `Josué
/// 10:7-14`, ...) bolded; everything else regular weight. When
/// `force_tnr_42` is set, every run is forced to Times New Roman 42pt
/// (Conclusión — see AGENTS.md hard rule).
pub(crate) fn set_body_with_refs_block(shape_xml: &str, text: &str, force_tnr_42: bool) -> Result<String> {
    set_body_with_refs_paragraphs(shape_xml, &[text.to_string()], force_tnr_42)
}

/// Like [`set_body_with_refs_block`] but emits one `<a:p>` per paragraph
/// (preserves per-paragraph `pPr` from the template when available).
pub(crate) fn set_body_with_refs_paragraphs(
    shape_xml: &str,
    paragraphs: &[String],
    force_tnr_42: bool,
) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = xml::next_element(content, "a:p", 0)
        .map(|e| e.start)
        .unwrap_or(content.len());
    let head = &content[..p_start];
    let sample = first_run(content);
    let default_ppr = first_para_ppr(content).unwrap_or_default();
    let template_paras: Vec<String> = xml::all_elements(content, "a:p")
        .into_iter()
        .map(|e| e.whole(content).to_string())
        .collect();

    let end_sz = if force_tnr_42 { r#" sz="4200""# } else { "" };
    let mut body = String::new();
    for (i, para_text) in paragraphs.iter().enumerate() {
        let ppr = template_paras
            .get(i)
            .and_then(|p| first_para_ppr(p))
            .unwrap_or_else(|| default_ppr.clone());
        let runs = build_ref_runs(para_text, sample.as_deref(), force_tnr_42)?;
        body.push_str(&format!(
            r#"<a:p>{ppr}{runs}<a:endParaRPr lang="es-DO"{end_sz}/></a:p>"#
        ));
    }

    let new_content = format!("{head}{body}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

/// Replace text while keeping the template's run/paragraph colours and
/// layout — distributes `new_text` across existing `<a:t>` slots by the
/// original per-slot character proportions.
pub(crate) fn replace_text_preserving_runs(shape_xml: &str, new_text: &str) -> Result<String> {
    let nodes = xml::all_elements(shape_xml, "a:t");
    if nodes.is_empty() {
        bail!("shape has no a:t nodes");
    }

    let old_lens: Vec<usize> = nodes
        .iter()
        .map(|e| e.inner(shape_xml, "a:t").chars().count())
        .collect();
    let total_old: usize = old_lens.iter().sum();
    let new_chars: Vec<char> = new_text.chars().collect();
    let total_new = new_chars.len();

    let mut assignments = vec![String::new(); nodes.len()];
    let mut char_idx = 0usize;
    for (i, &old_len) in old_lens.iter().enumerate() {
        let share = if total_old == 0 {
            if i == 0 { total_new } else { 0 }
        } else if i == nodes.len() - 1 {
            total_new.saturating_sub(char_idx)
        } else {
            (total_new * old_len) / total_old
        };
        let end = (char_idx + share).min(total_new);
        assignments[i] = new_chars[char_idx..end].iter().collect();
        char_idx = end;
    }

    let mut out = shape_xml.to_string();
    for (i, el) in nodes.iter().enumerate().rev() {
        let new_t = xml::build_t(&assignments[i]);
        out = replace_range(&out, el.start, el.end, &new_t);
    }
    Ok(out)
}

/// Keep the first `<a:t>` slot exact; distribute `rest` across the remaining
/// slots by their original character proportions (preserves accent colours).
pub(crate) fn replace_text_with_leading_run(
    shape_xml: &str,
    first_run_text: &str,
    rest_text: &str,
) -> Result<String> {
    let nodes = xml::all_elements(shape_xml, "a:t");
    if nodes.is_empty() {
        bail!("shape has no a:t nodes");
    }

    let mut assignments = vec![String::new(); nodes.len()];
    assignments[0] = first_run_text.to_string();

    if nodes.len() == 1 {
        assignments[0].push_str(rest_text);
    } else {
        let old_lens: Vec<usize> = nodes[1..]
            .iter()
            .map(|e| e.inner(shape_xml, "a:t").chars().count())
            .collect();
        let total_old: usize = old_lens.iter().sum();
        let new_chars: Vec<char> = rest_text.chars().collect();
        let total_new = new_chars.len();
        let mut char_idx = 0usize;
        for (i, &old_len) in old_lens.iter().enumerate() {
            let slot = i + 1;
            let share = if total_old == 0 {
                if i == old_lens.len() - 1 {
                    total_new.saturating_sub(char_idx)
                } else {
                    0
                }
            } else if i == old_lens.len() - 1 {
                total_new.saturating_sub(char_idx)
            } else {
                (total_new * old_len) / total_old
            };
            let end = (char_idx + share).min(total_new);
            assignments[slot] = new_chars[char_idx..end].iter().collect();
            char_idx = end;
        }
    }

    let mut out = shape_xml.to_string();
    for (i, el) in nodes.iter().enumerate().rev() {
        let new_t = xml::build_t(&assignments[i]);
        out = replace_range(&out, el.start, el.end, &new_t);
    }
    Ok(out)
}

/// Format a citation for a reference shape, preserving whether the
/// template used parentheses.
pub(crate) fn format_ref_citation_from_template(shape_xml: &str, cita: &str) -> Result<String> {
    let nodes = xml::all_elements(shape_xml, "a:t");
    let template = nodes
        .iter()
        .map(|e| e.inner(shape_xml, "a:t"))
        .find(|t| !t.trim().is_empty())
        .unwrap_or("");
    let cite = cita.trim();
    if template.starts_with('(') && !cite.starts_with('(') {
        Ok(format!("({cite})"))
    } else if !template.starts_with('(') && cite.starts_with('(') {
        Ok(cite.trim_start_matches('(').trim_end_matches(')').to_string())
    } else {
        Ok(cite.to_string())
    }
}
