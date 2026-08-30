//! Section chrome, A/B titles, and body text slides.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::shape::{
    set_body_with_refs_block, set_simple_text_block, transform_shape,
};

pub fn set_section_chrome(path: &Path, title: &str, rango: &str, n: u32) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let title_text = format!("{n}- {title}");
    let xml = transform_shape(&xml, "CuadroTexto 8", |b| {
        set_simple_text_block(b, &title_text, Some(true))
    })?;
    let xml = transform_shape(&xml, "CuadroTexto 3", |b| set_simple_text_block(b, rango, Some(true)))?;
    let xml = strip_arc_shapes(&xml);
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Remove any `<p:sp>...</p:sp>` block whose shape name starts with `Arc`
/// (retired yellow dashed/dotted chrome — see AGENTS.md).
pub(crate) fn strip_arc_shapes(xml: &str) -> String {
    let mut s = xml.to_string();
    loop {
        let Some(pos) = s.find(r#"name="Arc"#) else { break };
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
