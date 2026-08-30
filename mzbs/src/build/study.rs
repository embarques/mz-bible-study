//! `apply_study` + `build_study` — port of `python/mz_bible_study/build/study.py`
//! (fill logic) and the youth builder
//! (`python/mz_bible_study/build/youth/builder.py`).
//!
//! **Edit `model/study.rs` for the JSON shape** (fields, either-shaped
//! fallbacks, packing budgets). **Edit this file for slide order** (which
//! prototype slide gets which content, and in what sequence).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::build::ooxml::{self, VerseKind};
use crate::build::proto::PROTO;
use crate::job::Audience;
use crate::model::study::Study;
use crate::paths;

/// Build a study deck: unzip `template` (or the audience master template),
/// fix any `ns0:` corruption, fill it from the study JSON, pack+order
/// slides, zip, validate, and (optionally) export a PDF.
pub fn build_study(
    study_path: &Path,
    output: &Path,
    template: Option<&Path>,
    audience: Audience,
    export_pdf: bool,
) -> Result<PathBuf> {
    if audience == Audience::Adult {
        bail!(
            "audience=adult is not implemented yet. Use --audience youth (default), or wait \
             until the adult template and fill rules are wired. (study={}, output={})",
            study_path.display(),
            output.display()
        );
    }

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
    let study: Study = serde_json::from_str(&text)
        .with_context(|| format!("parse study JSON: {}", study_path.display()))?;

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
        paths::master_template(audience)?
    };
    if !base_pptx.is_file() {
        bail!("template not found: {}", base_pptx.display());
    }

    let numero = study.numero_string();
    let work = paths::generated_dir()?.join(format!("build-{numero}"));

    ooxml::unzip_pptx(&base_pptx, &work)?;
    ooxml::fix_package_ns0(&work)?;
    let order = apply_study(&work, &study)?;
    ooxml::set_active_order(&work, &order)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    ooxml::zip_pptx(&work, &output)?;
    println!("Wrote {}", output.display());

    crate::validate::validate_pptx(&output)
        .context("validation failed — not exporting PDF")?;

    if export_pdf {
        crate::export::export_one(&output)?;
    }

    Ok(output)
}

/// Fill content and return the final active slide-number order (mirrors
/// Python's `apply_study`): title → lectura → propósitos/idea → comentario
/// → intro → puntos (section/texto/A/B) → conclusión → próximo → images.
///
/// Primary path — takes a typed [`Study`]. See [`apply_study_value`] for
/// callers that still only have a raw `serde_json::Value`.
pub fn apply_study(build: &Path, study: &Study) -> Result<Vec<u32>> {
    let mut order: Vec<u32> = Vec::new();
    let base_lines = study.base_biblica_lines();

    // Title (reuse prototype in-place).
    let title_n = PROTO.title;
    order.push(title_n);
    ooxml::set_title_slide(
        &ooxml::slide_path(build, title_n),
        &study.numero_string(),
        &study.titulo,
        &base_lines,
    )?;

    // Lectura — pack + allocate.
    let lectura_packs = study.lectura_packs();
    let lectura_nums = ooxml::allocate_slides(build, PROTO.lectura, lectura_packs.len())?;
    order.extend(&lectura_nums);
    for (i, &n) in lectura_nums.iter().enumerate() {
        ooxml::set_verses(
            &ooxml::slide_path(build, n),
            &lectura_packs[i],
            if i == 0 {
                Some(study.lectura.cita.as_str())
            } else {
                None
            },
            VerseKind::Lectura,
        )?;
    }

    // Propósitos / Idea (in-place).
    order.push(PROTO.propositos);
    order.push(PROTO.idea);
    if study.propositos.len() != 3 {
        bail!(
            "propositos must have exactly 3 entries, got {}",
            study.propositos.len()
        );
    }
    let diagrams = build.join("ppt").join("diagrams");
    ooxml::replace_diagram_texts(
        &diagrams.join("data2.xml"),
        Some(&diagrams.join("drawing2.xml")),
        &study.propositos,
        Some(3600),
    )?;
    ooxml::force_propositos_36pt(&diagrams.join("drawing2.xml"))?;

    ooxml::replace_diagram_texts(
        &diagrams.join("data3.xml"),
        Some(&diagrams.join("drawing3.xml")),
        &[
            study.idea_principal.clone(),
            study.para_memorizar.texto.clone(),
        ],
        None,
    )?;
    if let Some(cite) = study
        .para_memorizar
        .cita
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        ooxml::set_diagram_citation(&diagrams.join("data3.xml"), cite)?;
    }

    // Comentario.
    let comentario = study.comentario_packs();
    let com_nums = ooxml::allocate_slides(build, PROTO.comentario, comentario.len())?;
    order.extend(&com_nums);
    for (&n, text) in com_nums.iter().zip(comentario.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, None, false)?;
    }

    // Introducción header + bodies.
    order.push(PROTO.intro_header);
    let intro = study.intro_packs();
    let intro_nums = ooxml::allocate_slides(build, PROTO.intro, intro.len())?;
    order.extend(&intro_nums);
    for (&n, text) in intro_nums.iter().zip(intro.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, None, false)?;
    }

    // Points 1–3.
    if study.puntos.len() != 3 {
        bail!("expected 3 puntos, got {}", study.puntos.len());
    }
    for (idx, punto) in study.puntos.iter().enumerate() {
        let sec_n = PROTO.section[idx];
        order.push(sec_n);
        ooxml::set_section_chrome(
            &ooxml::slide_path(build, sec_n),
            &punto.titulo,
            &punto.rango,
            punto.n,
        )?;

        let tpacks = punto.punto_texto_packs();
        let texto_nums = ooxml::allocate_slides(build, PROTO.texto, tpacks.len())?;
        order.extend(&texto_nums);
        for (j, &n) in texto_nums.iter().enumerate() {
            ooxml::set_verses(
                &ooxml::slide_path(build, n),
                &tpacks[j],
                if j == 0 {
                    Some(punto.rango.as_str())
                } else {
                    None
                },
                VerseKind::Texto,
            )?;
        }

        for (letter, block) in [("A", &punto.a), ("B", &punto.b)] {
            let ab_title = format!("{}.{letter}- {}", punto.n, block.titulo);
            let texts = block.ab_packs();
            let ab_nums = ooxml::allocate_slides(build, PROTO.ab, texts.len())?;
            order.extend(&ab_nums);
            for (&n, text) in ab_nums.iter().zip(texts.iter()) {
                let path = ooxml::slide_path(build, n);
                ooxml::set_ab_title(&path, &ab_title)?;
                ooxml::set_content_body(&path, text, None, false)?;
            }
        }
    }

    // Conclusión.
    let conclusion = study.conclusion_packs();
    let conc_nums = ooxml::allocate_slides(build, PROTO.conclusion, conclusion.len())?;
    order.extend(&conc_nums);
    for (&n, text) in conc_nums.iter().zip(conclusion.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, Some("CuadroTexto 5"), true)?;
    }

    // Próximo (omit entirely if the study says so — last study in a batch).
    if let Some(prox) = &study.proximo {
        let prox_n = PROTO.proximo;
        order.push(prox_n);
        let prox_base = prox
            .base_biblica
            .as_ref()
            .map(|b| b.as_lines())
            .unwrap_or_default();
        ooxml::set_title_slide(
            &ooxml::slide_path(build, prox_n),
            &prox.numero.to_string(),
            &prox.titulo,
            &prox_base,
        )?;
    }

    // Section images.
    if study.section_images.len() != 3 {
        bail!("study JSON must include section_images: [3 paths]");
    }
    let root = paths::project_root()?;
    let images: Vec<PathBuf> = study
        .section_images
        .iter()
        .map(|s| {
            let p = PathBuf::from(s);
            if p.is_absolute() {
                p
            } else {
                root.join(&p)
            }
        })
        .collect();
    ooxml::replace_section_images(build, &images)?;

    println!(
        "Packed {} active slides (lectura={}, comentario={}, intro={}, conclusion={})",
        order.len(),
        lectura_nums.len(),
        com_nums.len(),
        intro_nums.len(),
        conc_nums.len()
    );

    Ok(order)
}

/// Thin adapter for any leftover caller that only has a raw
/// `serde_json::Value` (e.g. hand-built JSON in a test or script) instead
/// of a typed [`Study`]. Prefer calling [`apply_study`] directly.
pub fn apply_study_value(build: &Path, study: &Value) -> Result<Vec<u32>> {
    let study: Study = serde_json::from_value(study.clone())
        .context("study JSON does not match the expected shape (see model::study::Study)")?;
    apply_study(build, &study)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_study_json() -> Value {
        serde_json::json!({
            "numero": 17,
            "titulo": "UN ABRAZO QUE DA VIDA",
            "base_biblica": "2 Reyes 4:18-37",
            "lectura": {"cita": "2 Reyes 4:18-21", "versiculos": ["18 ...", "19 ..."]},
            "propositos": ["a", "b", "c"],
            "idea_principal": "idea",
            "para_memorizar": {"texto": "texto"},
            "comentario": "comentario",
            "introduccion": "intro",
            "puntos": [
                {
                    "n": 1, "titulo": "T1", "rango": "R1", "texto_biblico": ["1 x"],
                    "A": {"titulo": "A1", "cuerpo": "a body"},
                    "B": {"titulo": "B1", "cuerpo": "b body"}
                },
                {
                    "n": 2, "titulo": "T2", "rango": "R2", "texto_biblico": ["2 x"],
                    "A": {"titulo": "A2", "cuerpo": "a body"},
                    "B": {"titulo": "B2", "cuerpo": "b body"}
                },
                {
                    "n": 3, "titulo": "T3", "rango": "R3", "texto_biblico": ["3 x"],
                    "A": {"titulo": "A3", "cuerpo": "a body"},
                    "B": {"titulo": "B3", "cuerpo": "b body"}
                }
            ],
            "conclusion": "concl",
            "section_images": ["a.png", "b.png", "c.png"]
        })
    }

    #[test]
    fn sample_study_deserializes() {
        let study: Study = serde_json::from_value(sample_study_json()).expect("deserialize");
        assert_eq!(study.numero_string(), "17");
        assert_eq!(
            study.base_biblica_lines(),
            vec!["2 Reyes 4:18-37".to_string()]
        );
        assert_eq!(study.puntos.len(), 3);
    }

    #[test]
    fn packing_helpers_fall_back_to_raw_text() {
        let study: Study = serde_json::from_value(sample_study_json()).expect("deserialize");
        assert_eq!(study.comentario_packs(), vec!["comentario".to_string()]);
        assert_eq!(study.intro_packs(), vec!["intro".to_string()]);
        assert_eq!(study.conclusion_packs(), vec!["concl".to_string()]);
        assert_eq!(
            study.lectura_packs(),
            vec![vec!["18 ...".to_string(), "19 ...".to_string()]]
        );
        assert_eq!(
            study.puntos[0].punto_texto_packs(),
            vec![vec!["1 x".to_string()]]
        );
        assert_eq!(study.puntos[0].a.ab_packs(), vec!["a body".to_string()]);
    }

    #[test]
    fn packing_helpers_prefer_pre_packed_slides() {
        let mut json = sample_study_json();
        json["comentario_slides"] = serde_json::json!(["one", "two"]);
        json["lectura_slides"] = serde_json::json!([["18 ..."], ["19 ..."]]);
        let study: Study = serde_json::from_value(json).expect("deserialize");
        assert_eq!(study.comentario_packs(), vec!["one".to_string(), "two".to_string()]);
        assert_eq!(
            study.lectura_packs(),
            vec![vec!["18 ...".to_string()], vec!["19 ...".to_string()]]
        );
    }

    #[test]
    fn apply_study_value_rejects_shape_mismatch_before_touching_disk() {
        // `puntos` is a required field on `Study` — this should fail during
        // the Value→Study deserialize step, before any filesystem access.
        let mut json = sample_study_json();
        json.as_object_mut().unwrap().remove("puntos");

        let tmp = std::env::temp_dir().join("mzbs-apply-study-value-test");
        let err = apply_study_value(&tmp, &json).unwrap_err();
        assert!(
            err.to_string().contains("does not match the expected shape"),
            "unexpected error: {err}"
        );
    }
}
