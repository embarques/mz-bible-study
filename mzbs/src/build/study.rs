//! `apply_study` + `build_study` — port of `python/mz_bible_study/build/study.py`
//! (fill logic) and the youth builder
//! (`python/mz_bible_study/build/youth/builder.py`).
//!
//! Study JSON is kept as `serde_json::Value` for flexibility (many fields
//! are optional / either-shaped, e.g. `comentario` vs `comentario_slides`,
//! `base_biblica` string-or-array) — see `crate::model::study::Study` for a
//! typed reference of the same shape.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::build::ooxml::{self, VerseKind};
use crate::build::proto::PROTO;
use crate::job::Audience;
use crate::pack;
use crate::paths;

const LECTURA_BUDGET: usize = 280;
const TEXTO_BUDGET: usize = 280;
const SENTENCE_BUDGET: usize = 380;

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
    let study: Value = serde_json::from_str(&text)
        .with_context(|| format!("parse study JSON: {}", study_path.display()))?;

    let root = paths::project_root()?;
    let base_pptx = if let Some(t) = template {
        t.to_path_buf()
    } else if let Some(t) = study.get("template").and_then(|v| v.as_str()) {
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

    let numero = numero_to_string(&study);
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
pub fn apply_study(build: &Path, study: &Value) -> Result<Vec<u32>> {
    let mut order: Vec<u32> = Vec::new();
    let base_lines = as_lines(study.get("base_biblica"));

    // Title (reuse prototype in-place).
    let title_n = PROTO.title;
    order.push(title_n);
    ooxml::set_title_slide(
        &ooxml::slide_path(build, title_n),
        &numero_to_string(study),
        get_str(study, "titulo")?,
        &base_lines,
    )?;

    // Lectura — pack + allocate.
    let lectura = study
        .get("lectura")
        .ok_or_else(|| anyhow!("study JSON missing `lectura`"))?;
    let lectura_cita = get_str(lectura, "cita")?;
    let lectura_versiculos = str_vec(
        lectura
            .get("versiculos")
            .ok_or_else(|| anyhow!("lectura missing `versiculos`"))?,
    )?;
    let lectura_packs = match study.get("lectura_slides") {
        Some(v) if !v.is_null() => str_vec_vec(v)?,
        _ => pack::pack_verses(&lectura_versiculos, LECTURA_BUDGET),
    };
    let lectura_nums = ooxml::allocate_slides(build, PROTO.lectura, lectura_packs.len())?;
    order.extend(&lectura_nums);
    for (i, &n) in lectura_nums.iter().enumerate() {
        ooxml::set_verses(
            &ooxml::slide_path(build, n),
            &lectura_packs[i],
            if i == 0 { Some(lectura_cita) } else { None },
            VerseKind::Lectura,
        )?;
    }

    // Propósitos / Idea (in-place).
    order.push(PROTO.propositos);
    order.push(PROTO.idea);
    let propositos = str_vec(
        study
            .get("propositos")
            .ok_or_else(|| anyhow!("study JSON missing `propositos`"))?,
    )?;
    if propositos.len() != 3 {
        bail!("propositos must have exactly 3 entries, got {}", propositos.len());
    }
    let diagrams = build.join("ppt").join("diagrams");
    ooxml::replace_diagram_texts(
        &diagrams.join("data2.xml"),
        Some(&diagrams.join("drawing2.xml")),
        &propositos,
        Some(3600),
    )?;
    ooxml::force_propositos_36pt(&diagrams.join("drawing2.xml"))?;

    let idea = get_str(study, "idea_principal")?.to_string();
    let para_memorizar = study
        .get("para_memorizar")
        .ok_or_else(|| anyhow!("study JSON missing `para_memorizar`"))?;
    let memo = get_str(para_memorizar, "texto")?.to_string();
    ooxml::replace_diagram_texts(
        &diagrams.join("data3.xml"),
        Some(&diagrams.join("drawing3.xml")),
        &[idea, memo],
        None,
    )?;
    if let Some(cite) = para_memorizar.get("cita").and_then(|v| v.as_str()) {
        if !cite.is_empty() {
            ooxml::set_diagram_citation(&diagrams.join("data3.xml"), cite)?;
        }
    }

    // Comentario.
    let comentario = packed_or_slides(study, "comentario", "comentario_slides")?;
    let com_nums = ooxml::allocate_slides(build, PROTO.comentario, comentario.len())?;
    order.extend(&com_nums);
    for (&n, text) in com_nums.iter().zip(comentario.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, None, false)?;
    }

    // Introducción header + bodies.
    order.push(PROTO.intro_header);
    let intro = packed_or_slides(study, "introduccion", "introduccion_slides")?;
    let intro_nums = ooxml::allocate_slides(build, PROTO.intro, intro.len())?;
    order.extend(&intro_nums);
    for (&n, text) in intro_nums.iter().zip(intro.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, None, false)?;
    }

    // Points 1–3.
    let puntos = study
        .get("puntos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("study JSON missing `puntos`"))?;
    if puntos.len() != 3 {
        bail!("expected 3 puntos, got {}", puntos.len());
    }
    for (idx, punto) in puntos.iter().enumerate() {
        let sec_n = PROTO.section[idx];
        order.push(sec_n);
        let n_val = punto
            .get("n")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("punto[{idx}] missing numeric `n`"))? as u32;
        let titulo = get_str(punto, "titulo")?;
        let rango = get_str(punto, "rango")?;
        ooxml::set_section_chrome(&ooxml::slide_path(build, sec_n), titulo, rango, n_val)?;

        let texto_biblico = str_vec(
            punto
                .get("texto_biblico")
                .ok_or_else(|| anyhow!("punto[{idx}] missing `texto_biblico`"))?,
        )?;
        let tpacks = match punto.get("texto_slides") {
            Some(v) if !v.is_null() => str_vec_vec(v)?,
            _ => pack::pack_verses(&texto_biblico, TEXTO_BUDGET),
        };
        let texto_nums = ooxml::allocate_slides(build, PROTO.texto, tpacks.len())?;
        order.extend(&texto_nums);
        for (j, &n) in texto_nums.iter().enumerate() {
            ooxml::set_verses(
                &ooxml::slide_path(build, n),
                &tpacks[j],
                if j == 0 { Some(rango) } else { None },
                VerseKind::Texto,
            )?;
        }

        for letter in ["A", "B"] {
            let block = punto
                .get(letter)
                .ok_or_else(|| anyhow!("punto[{idx}] missing `{letter}`"))?;
            let block_titulo = get_str(block, "titulo")?;
            let ab_title = format!("{n_val}.{letter}- {block_titulo}");
            let texts = match block.get("slides") {
                Some(v) if !v.is_null() => str_vec(v)?,
                _ => {
                    let cuerpo = block
                        .get("cuerpo")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    pack::pack_sentences(cuerpo, SENTENCE_BUDGET)
                }
            };
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
    let conclusion = packed_or_slides(study, "conclusion", "conclusion_slides")?;
    let conc_nums = ooxml::allocate_slides(build, PROTO.conclusion, conclusion.len())?;
    order.extend(&conc_nums);
    for (&n, text) in conc_nums.iter().zip(conclusion.iter()) {
        ooxml::set_content_body(&ooxml::slide_path(build, n), text, Some("CuadroTexto 5"), true)?;
    }

    // Próximo (omit entirely if the study says so — last study in a batch).
    if let Some(prox) = study.get("proximo").filter(|v| !v.is_null()) {
        let prox_n = PROTO.proximo;
        order.push(prox_n);
        ooxml::set_title_slide(
            &ooxml::slide_path(build, prox_n),
            &numero_to_string(prox),
            get_str(prox, "titulo")?,
            &as_lines(prox.get("base_biblica")),
        )?;
    }

    // Section images.
    let raw_images = study
        .get("section_images")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("study JSON must include section_images: [3 paths]"))?;
    let root = paths::project_root()?;
    let images: Vec<PathBuf> = raw_images
        .iter()
        .map(|v| {
            let s = v
                .as_str()
                .ok_or_else(|| anyhow!("section_images entries must be strings"))?;
            let p = PathBuf::from(s);
            Ok(if p.is_absolute() { p } else { root.join(p) })
        })
        .collect::<Result<Vec<_>>>()?;
    if images.len() != 3 {
        bail!("study JSON must include section_images: [3 paths]");
    }
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

// ---------------------------------------------------------------------------
// Small JSON helpers
// ---------------------------------------------------------------------------

/// `base_biblica`-style field: a `;`-joined string, or an array of strings.
/// Missing/null → empty.
fn as_lines(value: Option<&Value>) -> Vec<String> {
    match value {
        None => Vec::new(),
        Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => s
            .split(';')
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(other) => vec![other.to_string()],
    }
}

fn get_str<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("study JSON missing string field `{key}`"))
}

fn str_vec(v: &Value) -> Result<Vec<String>> {
    v.as_array()
        .ok_or_else(|| anyhow!("expected a JSON array of strings"))?
        .iter()
        .map(|x| {
            x.as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow!("expected a JSON array of strings"))
        })
        .collect()
}

fn str_vec_vec(v: &Value) -> Result<Vec<Vec<String>>> {
    v.as_array()
        .ok_or_else(|| anyhow!("expected a JSON array of arrays of strings"))?
        .iter()
        .map(str_vec)
        .collect()
}

fn numero_to_string(v: &Value) -> String {
    match v.get("numero") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
        None => "x".to_string(),
    }
}

/// `{field}_slides` (pre-packed array of strings) if present, else
/// `pack::pack_sentences({field}, 380)`.
fn packed_or_slides(study: &Value, field: &str, slides_field: &str) -> Result<Vec<String>> {
    if let Some(v) = study.get(slides_field).filter(|v| !v.is_null()) {
        return str_vec(v);
    }
    let text = study.get(field).and_then(|v| v.as_str()).unwrap_or_default();
    Ok(pack::pack_sentences(text, SENTENCE_BUDGET))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_lines_splits_semicolons() {
        let v = Value::String("Juan 3:16; Romanos 5:8".to_string());
        assert_eq!(
            as_lines(Some(&v)),
            vec!["Juan 3:16".to_string(), "Romanos 5:8".to_string()]
        );
    }

    #[test]
    fn as_lines_accepts_array() {
        let v = serde_json::json!(["Juan 3:16", "Romanos 5:8"]);
        assert_eq!(
            as_lines(Some(&v)),
            vec!["Juan 3:16".to_string(), "Romanos 5:8".to_string()]
        );
    }

    #[test]
    fn as_lines_missing_is_empty() {
        assert!(as_lines(None).is_empty());
    }

    #[test]
    fn numero_to_string_handles_number_and_string() {
        assert_eq!(numero_to_string(&serde_json::json!({"numero": 17})), "17");
        assert_eq!(
            numero_to_string(&serde_json::json!({"numero": "17B"})),
            "17B"
        );
    }
}
