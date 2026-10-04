//! Title / Próximo slide chrome (`Base Bíblica:` bold label only).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::shape::{
    clone_run, find_txbody, first_run, replace_range, set_simple_text_block, transform_shape,
};

pub fn set_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
  fill_title_slide(path, numero, titulo, base, "CuadroTexto 13")
}

/// Adult title / próximo slides use `CuadroTexto 1` for the study title.
/// Adult gold sometimes paints a cyan `<a:highlight>` behind part of the
/// title (e.g. "LA MODESTIA") — strip that so the title matches youth
/// (plain black type on white, no background chip).
pub fn set_adult_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
    fill_title_slide(path, numero, titulo, base, "CuadroTexto 1")?;
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = strip_text_highlights(&xml);
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

fn fill_title_slide(
    path: &Path,
    numero: &str,
    titulo: &str,
    base: &[String],
    title_shape: &str,
) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "TextBox 4", |b| set_simple_text_block(b, numero, Some(true)))?;
    // Verlag Black title — never force bold=true; leave template weight as-is.
    // Also drop highlight from the shape before cloning runs so a highlighted
    // second line in the gold adult title cannot leak into the new run.
    let xml = transform_shape(&xml, title_shape, |b| {
        let clean = strip_text_highlights(b);
        set_simple_text_block(&clean, titulo, Some(false))
    })?;
    let xml = transform_shape(&xml, "TextBox 7", |b| set_base_biblica_block(b, base))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
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
