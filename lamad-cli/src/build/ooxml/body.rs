//! Section chrome, A/B titles, and body text slides.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::shape::{
    force_run_sz, set_body_with_refs_block, set_body_with_refs_paragraphs, set_simple_text_block,
    split_teaching_paragraphs, transform_shape,
};

pub fn set_section_chrome(path: &Path, title: &str, rango: &str, n: u32) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let title_text = format!("{n}- {title}");
    let title_sz = section_title_font_sz_hundredths(&title_text);
    let xml = transform_shape(&xml, "CuadroTexto 8", |b| {
        let s = set_simple_text_block(b, &title_text, Some(true))?;
        Ok(force_run_sz(&s, title_sz))
    })?;
    let xml = transform_shape(&xml, "CuadroTexto 3", |b| set_simple_text_block(b, rango, Some(true)))?;
    let mut xml = strip_arc_shapes(&xml);
    // Template ships a full-bleed black rect under the photo — it paints over the scenic image.
    xml = strip_named_shape(&xml, "!!Rectangle");
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Hundredths of a point — long section titles must shrink to pass QC and stay above the gray line.
fn section_title_font_sz_hundredths(title: &str) -> u32 {
    let n = title.chars().count();
    if n > 70 {
        2200
    } else if n > 55 {
        2400
    } else if n > 42 {
        2800
    } else if n > 32 {
        3200
    } else {
        3600
    }
}

/// Remove the first `<p:sp>…</p:sp>` whose `cNvPr` name matches.
fn strip_named_shape(xml: &str, name: &str) -> String {
    let marker = format!(r#"name="{name}""#);
    let Some(name_pos) = xml.find(&marker) else {
        return xml.to_string();
    };
    let Some(sp_start) = xml[..name_pos].rfind("<p:sp") else {
        return xml.to_string();
    };
    let after = &xml[sp_start..];
    let Some(end_rel) = after.find("</p:sp>") else {
        return xml.to_string();
    };
    let sp_end = sp_start + end_rel + "</p:sp>".len();
    format!("{}{}", &xml[..sp_start], &xml[sp_end..])
}

/// Remove any `<p:sp>...</p:sp>` block whose shape name starts with `Arc`
/// (retired yellow dashed/dotted chrome — see AGENTS.md).
pub(crate) fn strip_arc_shapes(xml: &str) -> String {
    let mut s = xml.to_string();
    while let Some(pos) = s.find(r#"name="Arc"#) {
        let Some(start) = s[..pos].rfind("<p:sp") else { break };
        let Some(rel_end) = s[pos..].find("</p:sp>") else { break };
        let end = pos + rel_end + "</p:sp>".len();
        s.replace_range(start..end, "");
    }
    s
}

// ---------------------------------------------------------------------------
// A/B point bodies
// ---------------------------------------------------------------------------

/// Set the `N.A- TITLE` / `N.B- TITLE` heading (bold Times New Roman ~40pt
/// — preserve the template's bold; we only ever set bold=true here).
pub fn set_ab_title(path: &Path, title: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "Título 1", |b| set_simple_text_block(b, title, Some(true)))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

// ---------------------------------------------------------------------------
// Comentario / Introducción / A-B / Conclusión bodies
// ---------------------------------------------------------------------------

/// Set a content-body shape's text, regular weight with bold inline
/// scripture references. `shape` defaults to `Marcador de contenido 2`;
/// pass `Some("CuadroTexto 5")` for Conclusión. `conclusion=true` forces
/// Times New Roman 42pt on every run (AGENTS.md hard rule — the template
/// often leaves Verlag Light here, which is wrong).
pub fn set_content_body(path: &Path, text: &str, shape: Option<&str>, conclusion: bool) -> Result<()> {
    let shape_name = shape.unwrap_or("Marcador de contenido 2");
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let fallback_conclusion_shape = "CuadroTexto 5";
    let has_primary = xml.contains(&format!(r#"name="{shape_name}""#));
    let effective_name = if !has_primary && conclusion {
        fallback_conclusion_shape
    } else {
        shape_name
    };
    let xml = transform_shape(&xml, effective_name, |b| {
        set_body_with_refs_block(b, text, conclusion)
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Adult / multi-paragraph body: split at `(1)`, `(2)`, … teaching markers
/// before writing (matches gold adult template paragraph breaks).
pub fn set_content_body_teaching(path: &Path, text: &str, shape: Option<&str>) -> Result<()> {
    let shape_name = shape.unwrap_or("Marcador de contenido 2");
    let paragraphs = split_teaching_paragraphs(text);
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape_name, |b| {
        set_body_with_refs_paragraphs(b, &paragraphs, false)
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}
