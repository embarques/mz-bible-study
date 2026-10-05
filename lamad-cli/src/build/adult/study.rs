//! Adult study builder — master template with dynamic slide allocation
//! for Lectura, Texto Bíblico, and A/B body packs (duplicate prototypes
//! when JSON has more slides than the gold layout).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::build::adult::ooxml::{
    apply_scenic_image, convert_video_tema_to_image_layout, ensure_intro_body_logo,
    remove_orphan_videos, set_ab_body_slide, set_ab_title_header,
    set_definicion_image, set_ensenanza_datos, set_intro_body, set_tema_header_image,
    set_title_or_proximo, AbRefShape,
};
use crate::build::ooxml::{self, VerseKind};
use crate::build::ooxml::verses::expand_glued_verses;
use crate::job::Audience;
use crate::model::adult_study::{AdultStudy, TemaAdult};
use crate::model::study::VERSE_BUDGET;
use crate::pack;
use crate::paths;

const ADULT_SLIDE_COUNT: u32 = 62;
/// Prototype lectura antifonal slides in the adult master template.
const LECTURA_PROTOS: [u32; 4] = [2, 3, 4, 5];
/// First non-lectura slide after the template lectura block.
const AFTER_LECTURA: u32 = 6;
const INTRO_HEADER_SLIDE: u32 = 9;
/// Intro body prototypes (template slides 10–13). Extra packs duplicate slide 10.
const INTRO_BODY_PROTOS: [u32; 4] = [10, 11, 12, 13];
/// Lectura Antifonal pack budget — **same as youth** (`VERSE_BUDGET` = 280).
/// Whole verses only; if the next verse would overflow the panel, start a
/// new slide. Do not raise this — 400 dumped Mateo 6:1–4 onto one slide and
/// clipped mid-line at the bottom (HARD: never overflow).
const ADULT_LECTURA_BUDGET: usize = VERSE_BUDGET;

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

    let diagrams = build.join("ppt").join("diagrams");

    set_title_or_proximo(
        &ooxml::slide_path(build, 1),
        &study.numero_string(),
        &study.titulo,
        &study.base_biblica_lines(),
    )?;

    let lectura_packs = lectura_antifonal_packs(study);
    if lectura_packs.is_empty() {
        bail!("lectura_antifonal produced no verse packs");
    }
    let lectura_nums = allocate_lectura_slides(build, lectura_packs.len())?;
    for ((verses, cite), &slide_n) in lectura_packs.iter().zip(lectura_nums.iter()) {
        ooxml::set_verses(
            &ooxml::slide_path(build, slide_n),
            verses,
            cite.as_deref(),
            VerseKind::Lectura,
        )?;
    }

    // Title → lectura → fixed mid block through intro header → dynamic
    // intro bodies → temas (dynamic Texto + A/B bodies) → próximo.
    let mut order = vec![1u32];
    order.extend(&lectura_nums);
    order.extend(AFTER_LECTURA..=INTRO_HEADER_SLIDE);

    if study.objetivos.len() != 3 {
        bail!("objetivos must have 3 entries, got {}", study.objetivos.len());
    }
    ooxml::replace_diagram_texts(
        &diagrams.join("data1.xml"),
        Some(&diagrams.join("drawing1.xml")),
        &study.objetivos,
        None,
    )?;

    super::diagrams::set_pensamiento_aureo(
        &diagrams.join("data2.xml"),
        &diagrams.join("drawing2.xml"),
        study.pensamiento_central.trim(),
        &study.texto_aureo_quote(),
        &study.texto_aureo_cita_plain(),
    )?;

    set_ensenanza_datos(
        &ooxml::slide_path(build, 8),
        &study.ensenanza,
        &study.datos_generales,
    )?;

    if study.introduccion_slides.is_empty() {
        bail!("introduccion_slides must have at least one paragraph");
    }
    let intro_nums =
        allocate_body_slides(build, &INTRO_BODY_PROTOS, study.introduccion_slides.len())?;
    for (&slide_num, text) in intro_nums.iter().zip(study.introduccion_slides.iter()) {
        set_intro_body(&ooxml::slide_path(build, slide_num), text)?;
        // Adult intro body prototypes lack the Mount Zion logo; youth has it.
        // Copy youth bottom-right logo onto intro body slides only.
        ensure_intro_body_logo(build, slide_num, &youth_intro_logo_path()?)?;
    }
    order.extend(&intro_nums);

    // Tema II/III templates are video posters — convert to the still-image
    // tema layout (slide 14) BEFORE scenic swaps / text fill. Stripping
    // video XML in place triggers PowerPoint "Repair".
    for layout in &TEMA_LAYOUTS {
        if layout.video_header {
            convert_video_tema_to_image_layout(build, layout.header, 14)?;
        }
    }

    apply_scenic_images(build, study)?;
    remove_orphan_videos(build)?;

    if study.temas.len() != 3 {
        bail!("expected 3 temas, got {}", study.temas.len());
    }
    for (idx, tema) in study.temas.iter().enumerate() {
        fill_tema(build, idx, tema, study, &mut order)?;
    }

    let prox_base = study
        .proximo
        .base_biblica
        .as_ref()
        .map(|b| b.as_lines())
        .unwrap_or_default();
    set_title_or_proximo(
        &ooxml::slide_path(build, ADULT_SLIDE_COUNT),
        &study.proximo.numero.to_string(),
        &study.proximo.titulo,
        &prox_base,
    )?;
    order.push(ADULT_SLIDE_COUNT);

    // Image-chrome slides (TEMA headers + A/B titles) for QC.
    let mut image_slides = Vec::new();
    for layout in &TEMA_LAYOUTS {
        image_slides.push(layout.header);
        image_slides.push(layout.a_title);
        image_slides.push(layout.b_title);
    }
    crate::build::qc_deck(build, &order, &image_slides)?;

    crate::progress::ok(format!("Filled adult deck with {} slides", order.len()));
    Ok(order)
}

fn validate_counts(study: &AdultStudy) -> Result<()> {
    if study.lectura_antifonal.is_empty() {
        bail!("lectura_antifonal must have at least one passage");
    }
    if study.temas.len() != TEMA_LAYOUTS.len() {
        bail!("expected {} temas", TEMA_LAYOUTS.len());
    }
    for (idx, tema) in study.temas.iter().enumerate() {
        let n = idx + 1;
        if tema.a.texto_slides.is_empty() {
            bail!(
                "tema {n} A has empty texto_slides — agent must pack body paragraphs \
                 (builder will allocate as many A/B body slides as needed)"
            );
        }
        if tema.b.texto_slides.is_empty() {
            bail!(
                "tema {n} B has empty texto_slides — agent must pack body paragraphs \
                 (builder will allocate as many A/B body slides as needed)"
            );
        }
    }
    Ok(())
}

/// Same rules as youth Lectura Bíblica colours/structure, with an
/// adult denser pack budget (see [`ADULT_LECTURA_BUDGET`]). Consecutive
/// JSON passages that share the same `cita` are merged before packing so
/// continuation slides do not repeat the citation title.
fn lectura_antifonal_packs(study: &AdultStudy) -> Vec<(Vec<String>, Option<String>)> {
    let mut passages: Vec<(String, Vec<String>)> = Vec::new();
    for block in &study.lectura_antifonal {
        let cite_key = block.cita.trim().trim_end_matches(';').trim().to_string();
        let expanded = expand_glued_verses(&block.versiculos);
        if let Some(last) = passages.last_mut() {
            if last.0.eq_ignore_ascii_case(&cite_key) {
                last.1.extend(expanded);
                continue;
            }
        }
        passages.push((cite_key, expanded));
    }

    let mut out = Vec::new();
    for (cita, verses) in passages {
        let packs = pack::pack_verses(&verses, ADULT_LECTURA_BUDGET);
        for (i, pack) in packs.into_iter().enumerate() {
            let cite = if i == 0 { Some(cita.clone()) } else { None };
            out.push((pack, cite));
        }
    }
    out
}

/// Reuse template slides 2–5; duplicate slide 2 when more packs are needed
/// (youth allocate pattern). Unused prototype slides stay on disk but drop
/// out of `sldIdLst`.
fn allocate_lectura_slides(build: &Path, count: usize) -> Result<Vec<u32>> {
    let mut nums = Vec::with_capacity(count);
    for i in 0..count {
        if i < LECTURA_PROTOS.len() {
            nums.push(LECTURA_PROTOS[i]);
        } else {
            nums.push(ooxml::duplicate_slide(build, LECTURA_PROTOS[0])?);
        }
    }
    Ok(nums)
}

fn fill_tema(
    build: &Path,
    idx: usize,
    tema: &TemaAdult,
    study: &AdultStudy,
    order: &mut Vec<u32>,
) -> Result<()> {
    let layout = &TEMA_LAYOUTS[idx];
    let tema_label = match idx {
        0 => "TEMA I",
        1 => "TEMA II",
        _ => "TEMA III",
    };
    let header_path = ooxml::slide_path(build, layout.header);
    // All tema headers use the image layout (video slides converted earlier).
    set_tema_header_image(&header_path, tema_label, &tema.titulo, &tema.rango)?;
    order.push(layout.header);

    // Only emit a Definición slide when the printed study has a
    // "Definiciones y etimología" box for this tema. Never invent terms.
    if !tema.definiciones.is_empty() {
        if let Some(imgs) = study.definicion_images.as_ref() {
            if imgs.len() != study.temas.len() {
                bail!(
                    "definicion_images must have {} paths (one per tema), got {}",
                    study.temas.len(),
                    imgs.len()
                );
            }
            let img_path = resolve_study_path(&imgs[idx])?;
            if img_path.is_file() {
                set_definicion_image(build, layout.definicion, &img_path)?;
            } else {
                // Adult definición slides are image-cards — there is no
                // `Marcador de contenido 2` text shape. Fail clearly.
                bail!(
                    "missing definición card for tema {} ({}). \
                     Generate studies/adult/.../definicion-{}.png from the printed \
                     «Definiciones y etimología» box — do not use text fallback.",
                    idx + 1,
                    img_path.display(),
                    idx + 1
                );
            }
        } else {
            bail!(
                "tema {} has definiciones but JSON has no definicion_images[{}]. \
                 Re-run prepare --gen-images or stamp definicion_images paths.",
                idx + 1,
                idx
            );
        }
        order.push(layout.definicion);
    }

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
        order,
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
        order,
    )
}

fn fill_ab_block(
    build: &Path,
    point: usize,
    letter: &str,
    block: &crate::model::adult_study::BloqueAdult,
    title_slide: u32,
    ref_shape: AbRefShape,
    texto_proto: u32,
    body_slides: &[u32],
    order: &mut Vec<u32>,
) -> Result<()> {
    let label = format!("{point}.{letter}");
    set_ab_title_header(
        &ooxml::slide_path(build, title_slide),
        &label,
        &block.titulo,
        &block.texto_biblico.cita,
        ref_shape,
    )?;
    order.push(title_slide);

    // Same rules as youth Texto Bíblico: expand → pack at VERSE_BUDGET →
    // allocate continuation slides; citation only on first pack.
    let packs = texto_biblico_packs(&block.texto_biblico);
    if packs.is_empty() {
        bail!("{label} texto_biblico produced no verse packs");
    }
    let texto_nums = allocate_from_proto(build, texto_proto, packs.len())?;
    for ((verses, cite), &slide_n) in packs.iter().zip(texto_nums.iter()) {
        ooxml::set_verses(
            &ooxml::slide_path(build, slide_n),
            verses,
            cite.as_deref(),
            VerseKind::Texto,
        )?;
    }
    order.extend(&texto_nums);

    let body_title = format!("{label} - {}", block.titulo);
    let body_nums = allocate_body_slides(build, body_slides, block.texto_slides.len())?;
    for (&slide_n, text) in body_nums.iter().zip(block.texto_slides.iter()) {
        set_ab_body_slide(&ooxml::slide_path(build, slide_n), &body_title, text)?;
        order.push(slide_n);
    }
    Ok(())
}

fn texto_biblico_packs(
    passage: &crate::model::study::Lectura,
) -> Vec<(Vec<String>, Option<String>)> {
    let expanded = expand_glued_verses(&passage.versiculos);
    let packs = pack::pack_verses(&expanded, VERSE_BUDGET);
    packs
        .into_iter()
        .enumerate()
        .map(|(i, pack)| {
            let cite = if i == 0 {
                Some(passage.cita.clone())
            } else {
                None
            };
            (pack, cite)
        })
        .collect()
}

fn allocate_from_proto(build: &Path, proto: u32, count: usize) -> Result<Vec<u32>> {
    if count == 0 {
        bail!("allocate_from_proto requires count >= 1");
    }
    let mut nums = Vec::with_capacity(count);
    nums.push(proto);
    for _ in 1..count {
        nums.push(ooxml::duplicate_slide(build, proto)?);
    }
    Ok(nums)
}

/// Reuse template A/B body prototypes in order; duplicate the first when JSON
/// has more slides than the gold layout (same idea as Lectura / Texto packs).
/// Unused prototype slides stay on disk but are omitted from `order`.
fn allocate_body_slides(build: &Path, protos: &[u32], count: usize) -> Result<Vec<u32>> {
    if count == 0 {
        bail!("allocate_body_slides requires count >= 1");
    }
    if protos.is_empty() {
        bail!("allocate_body_slides needs at least one prototype slide");
    }
    let mut nums = Vec::with_capacity(count);
    for i in 0..count {
        if i < protos.len() {
            nums.push(protos[i]);
        } else {
            nums.push(ooxml::duplicate_slide(build, protos[0])?);
        }
    }
    Ok(nums)
}

fn resolve_study_path(p: &str) -> Result<PathBuf> {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        return Ok(path);
    }
    let root = paths::project_root()?;
    Ok(root.join(path))
}

/// Mount Zion logo used on youth intro body slides (`ppt/media/image1.png`).
fn youth_intro_logo_path() -> Result<PathBuf> {
    let root = paths::project_root()?;
    // Prefer extracted copy next to adult assets; fall back to unpacking youth template.
    let cached = root.join("lamad-cli/template/adult/assets/mz-logo.png");
    if cached.is_file() {
        return Ok(cached);
    }
    let studies_copy = root.join("studies/adult/24/mz-logo.png");
    if studies_copy.is_file() {
        return Ok(studies_copy);
    }
    // Last resort: extract from youth master template into adult assets.
    let youth_pptx = root.join("template/youth/master-template.pptx");
    if !youth_pptx.is_file() {
        bail!(
            "youth logo not found (expected {} or {})",
            cached.display(),
            youth_pptx.display()
        );
    }
    if let Some(parent) = cached.parent() {
        std::fs::create_dir_all(parent)?;
    }
    extract_media_from_pptx(&youth_pptx, "ppt/media/image1.png", &cached)?;
    Ok(cached)
}

fn extract_media_from_pptx(pptx: &Path, inner: &str, dest: &Path) -> Result<()> {
    use std::io::Read;
    let file = std::fs::File::open(pptx).with_context(|| format!("open {}", pptx.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("zip {}", pptx.display()))?;
    let mut entry = archive
        .by_name(inner)
        .with_context(|| format!("missing {inner} in {}", pptx.display()))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .with_context(|| format!("read {inner}"))?;
    std::fs::write(dest, buf).with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}

/// Swap scenic art onto intro / tema / A-B title slides.
///
/// All image slides are **full-bleed**. TEMA / A-B chrome is rewritten later
/// to the youth section layout (orange bar + title + gray line + verse)
/// anchored at the bottom-left — see `set_tema_header_image` /
/// `set_ab_title_header`.
fn apply_scenic_images(build: &Path, study: &AdultStudy) -> Result<()> {
    let Some(scenic) = study.scenic_images.as_ref() else {
        eprintln!("warning: no scenic_images — image slides keep template art (may be inset)");
        return Ok(());
    };
    if scenic.tema.len() != study.temas.len() {
        bail!(
            "scenic_images.tema must have {} paths, got {}",
            study.temas.len(),
            scenic.tema.len()
        );
    }
    let ab_needed = study.temas.len() * 2;
    if scenic.ab.len() != ab_needed {
        bail!(
            "scenic_images.ab must have {ab_needed} paths (A+B per tema), got {}",
            scenic.ab.len()
        );
    }

    apply_scenic_image(
        build,
        INTRO_HEADER_SLIDE,
        &resolve_study_path(&scenic.intro_header)?,
        true,
    )?;

    for (idx, path) in scenic.tema.iter().enumerate() {
        apply_scenic_image(build, TEMA_LAYOUTS[idx].header, &resolve_study_path(path)?, true)?;
    }

    let mut ab_i = 0usize;
    for layout in &TEMA_LAYOUTS {
        for &slide in &[layout.a_title, layout.b_title] {
            apply_scenic_image(
                build,
                slide,
                &resolve_study_path(&scenic.ab[ab_i])?,
                true,
            )?;
            ab_i += 1;
        }
    }
    Ok(())
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
        assert!(!study.lectura_antifonal.is_empty());
        let packs = lectura_antifonal_packs(&study);
        // Denser adult budget still splits long Mateo 6:1-4 across slides.
        assert!(
            packs.len() >= study.lectura_antifonal.len(),
            "expected at least one pack per passage, got {} packs from {} passages",
            packs.len(),
            study.lectura_antifonal.len()
        );
        assert!(
            packs[0]
                .1
                .as_ref()
                .map(|c| c.to_lowercase().contains("mateo"))
                .unwrap_or(false),
            "first pack should be Mateo with citation"
        );
        let mateo_count = 1 + packs
            .iter()
            .skip(1)
            .take_while(|(_, cite)| cite.is_none())
            .count();
        assert!(
            mateo_count >= 2,
            "Mateo 6:1-4 must split across slides (budget {ADULT_LECTURA_BUDGET}), got {mateo_count} pack(s)"
        );
        for (verses, _) in &packs {
            if verses.len() <= 1 {
                continue;
            }
            let len: usize = verses.iter().map(|v| v.chars().count() + 1).sum::<usize>() - 1;
            assert!(
                len <= ADULT_LECTURA_BUDGET,
                "lectura pack over budget: {len} > {ADULT_LECTURA_BUDGET}"
            );
        }
    }
}
