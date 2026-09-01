//! Adult study builder — fixed 62-slide template (no slide allocation).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::build::adult::ooxml::{
    set_ab_body_slide, set_ab_title_header, set_definicion, set_ensenanza_datos,
    set_intro_body, set_tema_header_image, set_tema_header_video, set_title_or_proximo,
    AbRefShape,
};
use crate::build::ooxml::{self, VerseKind};
use crate::job::Audience;
use crate::model::adult_study::{AdultStudy, TemaAdult};
use crate::paths;

const ADULT_SLIDE_COUNT: u32 = 62;

struct TemaLayout {
    header: u32,
    video_header: bool,
    definicion: u32,
    a_title: u32,
    a_ref: AbRefShape,
    texto_a: u32,
    a_bodies: &'static [u32],
    b_title: u32,
    b_ref: AbRefShape,
    texto_b: u32,
    b_bodies: &'static [u32],
}

const TEMA_LAYOUTS: [TemaLayout; 3] = [
    TemaLayout {
        header: 14,
        video_header: false,
        definicion: 15,
        a_title: 16,
        a_ref: AbRefShape::CuadroTexto6,
        texto_a: 17,
        a_bodies: &[18, 19, 20, 21],
        b_title: 22,
        b_ref: AbRefShape::CuadroTexto4,
        texto_b: 23,
        b_bodies: &[24, 25, 26, 27, 28],
    },
    TemaLayout {
        header: 29,
        video_header: true,
        definicion: 30,
        a_title: 31,
        a_ref: AbRefShape::Titulo6,
        texto_a: 32,
        a_bodies: &[33, 34, 35, 36],
        b_title: 37,
        b_ref: AbRefShape::Titulo7,
        texto_b: 38,
        b_bodies: &[39, 40, 41, 42, 43],
    },
    TemaLayout {
        header: 44,
        video_header: true,
        definicion: 45,
        a_title: 46,
        a_ref: AbRefShape::Titulo6,
        texto_a: 47,
        a_bodies: &[48, 49, 50, 51, 52],
        b_title: 53,
        b_ref: AbRefShape::Titulo6,
        texto_b: 54,
        b_bodies: &[55, 56, 57, 58, 59, 60, 61],
    },
];

/// Build an adult study deck from JSON + the adult master template.
pub fn build_study(
    study_path: &Path,
    output: &Path,
    template: Option<&Path>,
    export_pdf: bool,
) -> Result<PathBuf> {
    let study_path = study_path
        .canonicalize()
        .with_context(|| format!("study JSON not found: {}", study_path.display()))?;
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };

    let text = std::fs::read_to_string(&study_path)
        .with_context(|| format!("read {}", study_path.display()))?;
    let study: AdultStudy = serde_json::from_str(&text)
        .with_context(|| format!("parse adult study JSON: {}", study_path.display()))?;

    let root = paths::project_root()?;
    let base_pptx = if let Some(t) = template {
        t.to_path_buf()
    } else if let Some(t) = &study.template {
        let p = PathBuf::from(t);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    } else {
        paths::master_template(Audience::Adult)?
    };
    if !base_pptx.is_file() {
        bail!("adult template not found: {}", base_pptx.display());
    }

    let numero = study.numero_string();
    let work = paths::generated_dir()?.join(format!("build-adult-{numero}"));

    ooxml::unzip_pptx(&base_pptx, &work)?;
    ooxml::fix_package_ns0(&work)?;
    ooxml::fix_notes_slide_rels(&work)?;
    let order = apply_adult_study(&work, &study)?;
    ooxml::set_active_order(&work, &order)?;
    ooxml::fix_package_ns_prefixes(&work)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    ooxml::zip_pptx(&work, &output)?;
    crate::progress::phase(format!("wrote {}", output.display()));

    crate::progress::phase("validating PPTX…");
    crate::validate::validate_pptx(&output)
        .context("validation failed — not exporting PDF")?;

    if export_pdf {
        if cfg!(target_os = "macos") {
            let spin = crate::progress::Spinner::start(
                "Exporting PDF via PowerPoint (this can take a minute)…",
            );
            match crate::export::export_one(&output) {
                Ok(pdf) => spin.succeed(format!(
                    "PDF {}",
                    pdf.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("exported")
                )),
                Err(e) => {
                    spin.fail("PDF export failed");
                    return Err(e);
                }
            }
        } else {
            eprintln!(
                "note: skipping PDF export (requires macOS + PowerPoint). \
                 PPTX is ready; export later with: lamad export-pdf \"{}\"",
                output.display()
            );
        }
    }

    Ok(output)
}

pub fn apply_adult_study(build: &Path, study: &AdultStudy) -> Result<Vec<u32>> {
    validate_counts(study)?;

    let order: Vec<u32> = (1..=ADULT_SLIDE_COUNT).collect();
    let diagrams = build.join("ppt").join("diagrams");

    set_title_or_proximo(
        &ooxml::slide_path(build, 1),
        &study.numero_string(),
        &study.titulo,
        &study.base_biblica_lines(),
    )?;

    if study.lectura_antifonal.len() != 4 {
        bail!(
            "lectura_antifonal must have 4 slides, got {}",
            study.lectura_antifonal.len()
        );
    }
    for (i, block) in study.lectura_antifonal.iter().enumerate() {
        ooxml::set_verses(
            &ooxml::slide_path(build, 2 + i as u32),
            &block.versiculos,
            Some(block.cita.as_str()),
            VerseKind::Lectura,
        )?;
    }

    if study.objetivos.len() != 3 {
        bail!("objetivos must have 3 entries, got {}", study.objetivos.len());
    }
    ooxml::replace_diagram_texts(
        &diagrams.join("data1.xml"),
        Some(&diagrams.join("drawing1.xml")),
        &study.objetivos,
        None,
    )?;

    ooxml::replace_diagram_texts(
        &diagrams.join("data2.xml"),
        Some(&diagrams.join("drawing2.xml")),
        &[
            study.pensamiento_central.clone(),
            study.texto_aureo_diagram_text(),
        ],
        None,
    )?;

    set_ensenanza_datos(
        &ooxml::slide_path(build, 8),
        &study.ensenanza,
        &study.datos_generales,
    )?;

    if study.introduccion_slides.len() != 4 {
        bail!(
            "introduccion_slides must have 4 entries, got {}",
            study.introduccion_slides.len()
        );
    }
    for (i, text) in study.introduccion_slides.iter().enumerate() {
        set_intro_body(&ooxml::slide_path(build, 10 + i as u32), text)?;
    }

    if study.temas.len() != 3 {
        bail!("expected 3 temas, got {}", study.temas.len());
    }
    for (idx, tema) in study.temas.iter().enumerate() {
        fill_tema(build, idx, tema, study)?;
    }

    let prox_base = study
        .proximo
        .base_biblica
        .as_ref()
        .map(|b| b.as_lines())
        .unwrap_or_default();
    set_title_or_proximo(
        &ooxml::slide_path(build, 62),
        &study.proximo.numero.to_string(),
        &study.proximo.titulo,
        &prox_base,
    )?;

    println!("Filled adult deck with {} slides", order.len());
    Ok(order)
}

fn validate_counts(study: &AdultStudy) -> Result<()> {
    if study.temas.len() != TEMA_LAYOUTS.len() {
        bail!("expected {} temas", TEMA_LAYOUTS.len());
    }
    for (idx, (tema, layout)) in study.temas.iter().zip(TEMA_LAYOUTS.iter()).enumerate() {
        if tema.a.texto_slides.len() != layout.a_bodies.len() {
            bail!(
                "tema {} A needs {} body slides, got {}",
                idx + 1,
                layout.a_bodies.len(),
                tema.a.texto_slides.len()
            );
        }
        if tema.b.texto_slides.len() != layout.b_bodies.len() {
            bail!(
                "tema {} B needs {} body slides, got {}",
                idx + 1,
                layout.b_bodies.len(),
                tema.b.texto_slides.len()
            );
        }
    }
    Ok(())
}

fn fill_tema(build: &Path, idx: usize, tema: &TemaAdult, study: &AdultStudy) -> Result<()> {
    let layout = &TEMA_LAYOUTS[idx];
    let tema_label = match idx {
        0 => "TEMA I",
        1 => "TEMA II",
        _ => "TEMA III",
    };
    let header_path = ooxml::slide_path(build, layout.header);
    if layout.video_header {
        set_tema_header_video(&header_path, tema_label, &tema.titulo, &tema.rango)?;
    } else {
        set_tema_header_image(&header_path, tema_label, &tema.titulo, &tema.rango)?;
    }

    let defs = study.definiciones_text(&tema.definiciones);
    set_definicion(&ooxml::slide_path(build, layout.definicion), &defs)?;

    let point = idx + 1;
    fill_ab_block(
        build,
        point,
        "A",
        &tema.a,
        layout.a_title,
        layout.a_ref,
        layout.texto_a,
        layout.a_bodies,
    )?;
    fill_ab_block(
        build,
        point,
        "B",
        &tema.b,
        layout.b_title,
        layout.b_ref,
        layout.texto_b,
        layout.b_bodies,
    )
}

fn fill_ab_block(
    build: &Path,
    point: usize,
    letter: &str,
    block: &crate::model::adult_study::BloqueAdult,
    title_slide: u32,
    ref_shape: AbRefShape,
    texto_slide: u32,
    body_slides: &[u32],
) -> Result<()> {
    let label = format!("{point}.{letter}");
    set_ab_title_header(
        &ooxml::slide_path(build, title_slide),
        &label,
        &block.titulo,
        &block.texto_biblico.cita,
        ref_shape,
    )?;
    let verses = normalize_verses(&block.texto_biblico.cita, &block.texto_biblico.versiculos);
    ooxml::set_verses(
        &ooxml::slide_path(build, texto_slide),
        &verses,
        Some(block.texto_biblico.cita.as_str()),
        VerseKind::Texto,
    )?;

    let body_title = format!("{label} - {}", block.titulo);
    for (&slide_n, text) in body_slides.iter().zip(block.texto_slides.iter()) {
        set_ab_body_slide(&ooxml::slide_path(build, slide_n), &body_title, text)?;
    }
    Ok(())
}

/// Gold-deck JSON sometimes omits the leading verse number on single-verse
/// Texto Bíblico slides — `set_verses` requires `N text…` lines.
fn normalize_verses(cita: &str, verses: &[String]) -> Vec<String> {
  let default_num = verse_num_from_cita(cita);
  verses
    .iter()
    .map(|v| {
      if v.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit()) {
        v.clone()
      } else if let Some(n) = &default_num {
        format!("{n} {}", v.trim())
      } else {
        v.clone()
      }
    })
    .collect()
}

fn verse_num_from_cita(cita: &str) -> Option<String> {
  let tail = cita.rsplit(':').next()?.trim();
  let num = tail.split([',', '-']).next()?.trim();
  if num.chars().all(|c| c.is_ascii_digit()) {
    Some(num.to_string())
  } else {
    None
  }
}

pub fn apply_adult_study_value(build: &Path, study: &Value) -> Result<Vec<u32>> {
    let study: AdultStudy = serde_json::from_value(study.clone())
        .context("study JSON does not match adult shape (see model::adult_study::AdultStudy)")?;
    apply_adult_study(build, &study)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::adult_study::AdultStudy;

    #[test]
    fn fixture_24_deserializes() {
        let root = paths::project_root().expect("root");
        let path = root.join("lamad-cli/tests/fixtures/adult/24.json");
        let text = std::fs::read_to_string(&path).expect("read fixture");
        let study: AdultStudy = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(study.numero_string(), "24");
        assert_eq!(study.temas.len(), 3);
        assert_eq!(study.lectura_antifonal.len(), 4);
    }
}
