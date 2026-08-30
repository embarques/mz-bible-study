//! Title / Próximo slide chrome (`Base Bíblica:` bold label only).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::shape::{
    clone_run, find_txbody, first_run, replace_range, set_simple_text_block, transform_shape,
};

pub fn set_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "TextBox 4", |b| set_simple_text_block(b, numero, Some(true)))?;
    // Verlag Black title — never force bold=true; leave template weight as-is.
    let xml = transform_shape(&xml, "CuadroTexto 13", |b| set_simple_text_block(b, titulo, Some(false)))?;
    let xml = transform_shape(&xml, "TextBox 7", |b| set_base_biblica_block(b, base))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

fn set_base_biblica_block(shape_xml: &str, base: &[String]) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let sample = first_run(content);

    let mut p = String::from(r#"<a:p><a:pPr algn="ctr"/>"#);
    p.push_str(&clone_run(sample.as_deref(), "Base Bíblica:", Some(true), None, None)?);
    for line in base {
        p.push_str("<a:br/>");
        p.push_str(&clone_run(sample.as_deref(), line, Some(false), None, None)?);
    }
    p.push_str("</a:p>");

    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{p}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}
