//! Adult-template OOXML fill helpers (shape names differ from youth).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::build::ooxml::body::set_content_body;
use crate::build::ooxml::shape::{set_simple_text_block, transform_shape};
use crate::build::ooxml::title::set_adult_title_slide;

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

pub fn set_shape_text(path: &Path, shape: &str, text: &str, bold: Option<bool>) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape, |b| set_simple_text_block(b, text, bold))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

pub fn set_ensenanza_datos(
    path: &Path,
    ensenanza: &str,
    datos: &crate::model::adult_study::DatosGenerales,
) -> Result<()> {
    set_shape_text(path, "CuadroTexto 13", ensenanza, None)?;
    set_shape_text(path, "CuadroTexto 9", &datos.personajes, None)?;
    set_shape_text(path, "CuadroTexto 21", &datos.fecha, None)?;
    set_shape_text(path, "CuadroTexto 32", &datos.lugar, None)?;
    set_shape_text(path, "CuadroTexto 38", &datos.autor, None)?;
    Ok(())
}

pub fn set_tema_header_image(path: &Path, tema_label: &str, titulo: &str, rango: &str) -> Result<()> {
    let top = format!("{tema_label}\n{titulo}");
    set_shape_text(path, "CuadroTexto 3", &top, Some(true))?;
    set_shape_text(path, "CuadroTexto 5", rango, Some(true))?;
    Ok(())
}

pub fn set_tema_header_video(path: &Path, tema_label: &str, titulo: &str, rango: &str) -> Result<()> {
    let top = format!("{tema_label}   \n{titulo}");
    set_shape_text(path, "Título 1", &top, Some(true))?;
    let cite = rango.trim().trim_start_matches('(').trim_end_matches(')');
    set_shape_text(path, "CuadroTexto 4", cite, Some(true))?;
    Ok(())
}

pub fn set_ab_title_header(
    path: &Path,
    point_label: &str,
    titulo: &str,
    cita: &str,
    reference: AbRefShape,
) -> Result<()> {
    let heading = format!("{point_label} {titulo}");
    set_shape_text(path, "Título 1", &heading, Some(true))?;
    set_shape_text(path, reference.name(), &format!("({cita})"), Some(true))?;
    Ok(())
}

pub fn set_ab_body_slide(path: &Path, point_title: &str, body: &str) -> Result<()> {
    set_shape_text(path, "Título 1", point_title, Some(true))?;
    set_content_body(path, body, None, false)
}

pub fn set_definicion(path: &Path, text: &str) -> Result<()> {
    set_content_body(path, text, None, false)
}

pub fn set_intro_body(path: &Path, text: &str) -> Result<()> {
    set_content_body(path, text, None, false)
}
