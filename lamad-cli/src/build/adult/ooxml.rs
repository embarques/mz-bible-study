//! Adult-template OOXML fill helpers (shape names differ from youth).
//!
//! Fills preserve the corrected `master-template.pptx` run colours,
//! paragraph breaks, and layout geometry — only text is swapped.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::build::ooxml::body::set_content_body_teaching;
use crate::build::ooxml::shape::{
    format_ref_citation_from_template, replace_text_preserving_runs,
    replace_text_with_leading_run, transform_shape,
};
use crate::build::ooxml::title::set_adult_title_slide;

/// Soft cap per body paragraph on adult `Marcador de contenido 2` shapes.
/// See `LAYOUT_GUIDE.md` — navy footer bar is ~bottom 8–10% of slide;
/// content must stop above it (~360–400 chars per paragraph at ~42pt).
pub const ADULT_BODY_PARA_BUDGET: usize = 400;

#[derive(Debug, Clone, Copy)]
pub enum AbRefShape {
    CuadroTexto6,
    CuadroTexto4,
    Titulo6,
    Titulo7,
}

impl AbRefShape {
    pub fn name(self) -> &'static str {
        match self {
            Self::CuadroTexto6 => "CuadroTexto 6",
            Self::CuadroTexto4 => "CuadroTexto 4",
            Self::Titulo6 => "Título 6",
            Self::Titulo7 => "Título 7",
        }
    }
}

pub fn set_title_or_proximo(
    path: &Path,
    numero: &str,
    titulo: &str,
    base: &[String],
) -> Result<()> {
    set_adult_title_slide(path, numero, titulo, base)
}

fn fill_shape_preserving(path: &Path, shape: &str, text: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape, |b| replace_text_preserving_runs(b, text))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

fn fill_shape_leading(path: &Path, shape: &str, leading: &str, rest: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape, |b| replace_text_with_leading_run(b, leading, rest))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

pub fn set_ensenanza_datos(
    path: &Path,
    ensenanza: &str,
    datos: &crate::model::adult_study::DatosGenerales,
) -> Result<()> {
    fill_shape_preserving(path, "CuadroTexto 13", ensenanza)?;
    fill_shape_preserving(path, "CuadroTexto 9", &datos.personajes)?;
    fill_shape_preserving(path, "CuadroTexto 21", &datos.fecha)?;
    fill_shape_preserving(path, "CuadroTexto 32", &datos.lugar)?;
    fill_shape_preserving(path, "CuadroTexto 38", &datos.autor)?;
    Ok(())
}

pub fn set_tema_header_image(path: &Path, tema_label: &str, titulo: &str, rango: &str) -> Result<()> {
    fill_shape_leading(path, "CuadroTexto 3", &format!("{tema_label} "), &format!(" {titulo}"))?;
    let cite = rango.trim();
    let cite = if cite.starts_with('(') {
        cite.to_string()
    } else {
        format!("({cite})")
    };
    fill_shape_preserving(path, "CuadroTexto 5", &cite)?;
    Ok(())
}

pub fn set_tema_header_video(path: &Path, tema_label: &str, titulo: &str, rango: &str) -> Result<()> {
    fill_shape_leading(path, "Título 1", &format!("{tema_label}   "), &format!(" {titulo}"))?;
    let cite = rango.trim().trim_start_matches('(').trim_end_matches(')');
    fill_shape_preserving(path, "CuadroTexto 4", cite)?;
    Ok(())
}

pub fn set_ab_title_header(
    path: &Path,
    point_label: &str,
    titulo: &str,
    cita: &str,
    reference: AbRefShape,
) -> Result<()> {
    let heading = format!("{point_label} ");
    fill_shape_leading(path, "Título 1", &heading, &format!(" {titulo}"))?;

    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let ref_shape = reference.name();
    let xml = transform_shape(&xml, ref_shape, |b| {
        let cite = format_ref_citation_from_template(b, cita)?;
        replace_text_preserving_runs(b, &cite)
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

pub fn set_ab_body_slide(path: &Path, point_title: &str, body: &str) -> Result<()> {
    if let Some((label, rest)) = point_title.split_once(" - ") {
        fill_shape_leading(path, "Título 1", &format!("{label} - "), rest)?;
    } else {
        fill_shape_preserving(path, "Título 1", point_title)?;
    }
    warn_if_over_budget(body);
    set_content_body_teaching(path, body, None)
}

pub fn set_definicion(path: &Path, text: &str) -> Result<()> {
    warn_if_over_budget(text);
    set_content_body_teaching(path, text, None)
}

pub fn set_intro_body(path: &Path, text: &str) -> Result<()> {
    warn_if_over_budget(text);
    set_content_body_teaching(path, text, None)
}

fn warn_if_over_budget(text: &str) {
    use crate::build::ooxml::shape::split_teaching_paragraphs;
    for (i, para) in split_teaching_paragraphs(text).iter().enumerate() {
        if para.chars().count() > ADULT_BODY_PARA_BUDGET {
            eprintln!(
                "warning: adult body paragraph {} has {} chars (budget {}) — may overflow",
                i + 1,
                para.chars().count(),
                ADULT_BODY_PARA_BUDGET
            );
        }
    }
}
