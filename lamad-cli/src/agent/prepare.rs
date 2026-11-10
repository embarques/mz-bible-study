//! Cloud prepare: rasterize pages → agent backend → study JSON + section PNGs.
//!
//! Backends: Cursor Cloud Agents (default) or OpenAI ChatGPT (secondary / fallback)
//! ([`super::prepare_openai`]).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::client::{self, Artifact, CursorClient};
use crate::agent::provider::AgentProvider;
use crate::agent::{AGENTS_MD, PREPARE_STUDY_MD};
use crate::job::Audience;
use crate::paths;
use crate::section_styles::{section_style_for_study, style_prompt_block};
use crate::{pdf_page_count, rasterize_pages};

/// Injected into every prepare prompt — Bible-study wording must stay untouched.
const VERBATIM_HARD: &str = "\
**VERBATIM TEXT (HARD — non-negotiable):** Copy the printed Spanish **word-for-word** \
into JSON (`introduccion_slides`, adult `temas[].A/B/…texto_slides`, youth comentario / \
intro / A-B / conclusión, etc.). Fix only obvious OCR letter errors. \
**FORBIDDEN:** paraphrasing, summarizing, shortening, rewriting “in your own words,” \
inventing transitions, or improving theology. This is a Mount Zion Church Bible study — \
the author’s sentences must appear in the JSON. Skip only explicitly excluded sections \
(Ideas para el maestro, Preguntas, Afianzamiento, RESUMEN GENERAL body).";

#[derive(Clone)]
pub struct PrepareRequest {
    pub study: u32,
    pub pdf: PathBuf,
    pub pages: (u32, u32),
    pub omit_proximo: bool,
    pub next_pages: Option<(u32, u32)>,
    pub audience: Audience,
    pub provider: AgentProvider,
    pub model: String,
    /// ChatGPT model used when Cursor fails and we fall back (also primary for `--provider chatgpt`).
    pub openai_model: String,
    /// OpenAI Images model for host-side parallel generation.
    pub image_model: String,
    pub api_key: String,
    /// When set, Cursor/ChatGPT prepare uses JSON-only agent + host images.
    pub image_api_key: Option<String>,
    pub stream: bool,
    pub root: PathBuf,
    /// From `config.toml` `pdftoppm_path` (optional).
    pub pdftoppm_path: Option<PathBuf>,
}

impl PrepareRequest {
    /// Fast path: cloud agent writes JSON; host generates images in parallel.
    pub fn host_images(&self) -> bool {
        self.image_api_key
            .as_ref()
            .map(|k| !k.is_empty())
            .unwrap_or(false)
    }
}

pub struct Deliverables {
    pub json: PathBuf,
    pub img1: PathBuf,
    pub img2: PathBuf,
    pub img3: PathBuf,
}

/// Adult scenic / definición PNGs live under `studies/adult/{N}/`.
pub fn adult_media_dir(study: u32, root: &Path) -> PathBuf {
    let study_dir = root
        .join("studies")
        .join(Audience::Adult.as_str())
        .join(study.to_string());
    let images = study_dir.join("images");
    // New layout: studies/adult/{N}/images/. Keep legacy flat dir only when
    // images already live beside the folder (no images/ yet).
    let legacy_intro = study_dir.join("intro-header.png");
    if images.is_dir() || !legacy_intro.is_file() {
        let _ = std::fs::create_dir_all(&images);
        return images;
    }
    study_dir
}

/// Scenic + definición basenames derived from adult study shape (variable temas/points).
pub fn adult_basenames_for_study(study: &crate::model::adult_study::AdultStudy) -> Vec<String> {
    let mut out = vec!["intro-header.png".to_string()];
    for i in 0..study.temas.len() {
        out.push(format!("tema-{}.png", i + 1));
    }
    for (ti, tema) in study.temas.iter().enumerate() {
        for p in &tema.puntos {
            out.push(format!("ab-{}{}.png", ti + 1, p.letra));
        }
    }
    for i in 0..study.temas.len() {
        out.push(format!("definicion-{}.png", i + 1));
    }
    out
}

/// Scenic only (required before build).
pub fn adult_scenic_basenames_for(study: &crate::model::adult_study::AdultStudy) -> Vec<String> {
    adult_basenames_for_study(study)
        .into_iter()
        .filter(|n| !n.starts_with("definicion-"))
        .collect()
}

pub fn adult_definicion_basenames_for(n_temas: usize) -> Vec<String> {
    (1..=n_temas)
        .map(|i| format!("definicion-{i}.png"))
        .collect()
}

/// Legacy fixed list (3×A/B) — used when JSON is not yet available (progress UX).
pub fn adult_image_basenames() -> &'static [&'static str] {
    &[
        "intro-header.png",
        "tema-1.png",
        "tema-2.png",
        "tema-3.png",
        "ab-1A.png",
        "ab-1B.png",
        "ab-2A.png",
        "ab-2B.png",
        "ab-3A.png",
        "ab-3B.png",
        "definicion-1.png",
        "definicion-2.png",
        "definicion-3.png",
    ]
}

/// Legacy scenic-only subset of [`adult_image_basenames`].
pub fn adult_scenic_basenames() -> &'static [&'static str] {
    &[
        "intro-header.png",
        "tema-1.png",
        "tema-2.png",
        "tema-3.png",
        "ab-1A.png",
        "ab-1B.png",
        "ab-2A.png",
        "ab-2B.png",
        "ab-3A.png",
        "ab-3B.png",
    ]
}

/// Legacy definición slots.
pub fn adult_definicion_basenames() -> &'static [&'static str] {
    &["definicion-1.png", "definicion-2.png", "definicion-3.png"]
}

pub fn expected_paths(study: u32, audience: Audience, root: &Path) -> Deliverables {
    let base = root.join("studies").join(audience.as_str());
    match audience {
        Audience::Youth => Deliverables {
            json: base.join(format!("{study}.json")),
            img1: base.join("media").join(format!("{study}-section1.png")),
            img2: base.join("media").join(format!("{study}-section2.png")),
            img3: base.join("media").join(format!("{study}-section3.png")),
        },
        // Review / summary still want three “preview” paths — use intro + first temas.
        Audience::Adult => {
            let media = adult_media_dir(study, root);
            Deliverables {
                json: base.join(format!("{study}.json")),
                img1: media.join("intro-header.png"),
                img2: media.join("tema-1.png"),
                img3: media.join("tema-2.png"),
            }
        }
    }
}

pub fn verify_deliverables(study: u32, audience: Audience, root: &Path) -> Result<Deliverables> {
    match audience {
        Audience::Youth => verify_youth_deliverables(study, root),
        Audience::Adult => verify_adult_deliverables(study, root),
    }
}

/// If a previous run already left valid JSON + images on disk, return them
/// so prepare can skip the cloud agent (the expensive step).
pub fn try_resume_deliverables(
    study: u32,
    audience: Audience,
    root: &Path,
) -> Option<Deliverables> {
    match verify_deliverables(study, audience, root) {
        Ok(d) => Some(d),
        Err(_) => None,
    }
}

/// Valid study JSON on disk (schema OK) — images may still be missing.
pub fn try_resume_json_only(
    study: u32,
    audience: Audience,
    root: &Path,
) -> Option<PathBuf> {
    match verify_json_only(study, audience, root) {
        Ok(p) => Some(p),
        Err(_) => None,
    }
}

/// True when required scenic/section images are present (definición optional for adult).
pub fn required_images_present(study: u32, audience: Audience, root: &Path) -> bool {
    match audience {
        Audience::Youth => {
            let p = expected_paths(study, audience, root);
            p.img1.is_file() && p.img2.is_file() && p.img3.is_file()
        }
        Audience::Adult => {
            let model = if crate::content::has_adult_sections(study) {
                crate::content::load_adult_sections(study).ok()
            } else {
                let paths = expected_paths(study, Audience::Adult, root);
                let _ = normalize_adult_agent_json(&paths.json);
                std::fs::read_to_string(&paths.json)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
            };
            let Some(model) = model else {
                return false;
            };
            if crate::content::any_scenic_placeholders(study, &model, root) {
                return false;
            }
            let media = adult_media_dir(study, root);
            adult_scenic_basenames_for(&model)
                .iter()
                .all(|n| media.join(n).is_file())
        }
    }
}

/// Validate JSON schema only (no image check).
pub fn verify_json_only(study: u32, audience: Audience, root: &Path) -> Result<PathBuf> {
    match audience {
        Audience::Youth => {
            let paths = expected_paths(study, Audience::Youth, root);
            if !paths.json.is_file() {
                bail!("deliverable missing: {}", paths.json.display());
            }
            let data: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&paths.json)?)?;
            let puntos = data
                .get("puntos")
                .and_then(|p| p.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if puntos != 3 {
                bail!("{} must have 3 puntos, got {puntos}", paths.json.display());
            }
            Ok(paths.json)
        }
        Audience::Adult => {
            let paths = expected_paths(study, Audience::Adult, root);
            if !paths.json.is_file() {
                bail!("deliverable missing: {}", paths.json.display());
            }
            let _ = normalize_adult_agent_json(&paths.json)?;
            let text = std::fs::read_to_string(&paths.json)?;
            let study_model: crate::model::adult_study::AdultStudy = serde_json::from_str(&text)
                .map_err(|e| {
                    anyhow::anyhow!("{} is not valid adult study JSON ({e})", paths.json.display())
                })?;
            if study_model.temas.is_empty() {
                bail!("{} must have at least 1 tema", paths.json.display());
            }
            for (i, tema) in study_model.temas.iter().enumerate() {
                if tema.puntos.is_empty() {
                    bail!(
                        "{} tema {} has no puntos (need A…N)",
                        paths.json.display(),
                        i + 1
                    );
                }
            }
            Ok(paths.json)
        }
    }
}

/// Human summary of what is already on disk (for resume / missing messages).
pub fn existing_inventory_summary(study: u32, audience: Audience, root: &Path) -> String {
    let paths = expected_paths(study, audience, root);
    let json_ok = paths.json.is_file();
    match audience {
        Audience::Youth => {
            let imgs = [&paths.img1, &paths.img2, &paths.img3]
                .iter()
                .filter(|p| p.is_file())
                .count();
            format!(
                "JSON={} · section images={imgs}/3 ({})",
                if json_ok { "yes" } else { "no" },
                paths.json.display()
            )
        }
        Audience::Adult => {
            let media = adult_media_dir(study, root);
            let (scenic_need, def_need) = if json_ok {
                std::fs::read_to_string(&paths.json)
                    .ok()
                    .and_then(|t| {
                        serde_json::from_str::<crate::model::adult_study::AdultStudy>(&t).ok()
                    })
                    .map(|m| {
                        (
                            adult_scenic_basenames_for(&m).len(),
                            m.temas.len(),
                        )
                    })
                    .unwrap_or((adult_scenic_basenames().len(), 3))
            } else {
                (adult_scenic_basenames().len(), 3)
            };
            let scenic = if json_ok {
                std::fs::read_to_string(&paths.json)
                    .ok()
                    .and_then(|t| {
                        serde_json::from_str::<crate::model::adult_study::AdultStudy>(&t).ok()
                    })
                    .map(|m| {
                        adult_scenic_basenames_for(&m)
                            .iter()
                            .filter(|n| media.join(n).is_file())
                            .count()
                    })
                    .unwrap_or(0)
            } else {
                adult_scenic_basenames()
                    .iter()
                    .filter(|n| media.join(n).is_file())
                    .count()
            };
            let defs = (1..=def_need)
                .filter(|i| media.join(format!("definicion-{i}.png")).is_file())
                .count();
            format!(
                "JSON={} · scenic={scenic}/{scenic_need} · definición={defs}/{def_need} ({})",
                if json_ok { "yes" } else { "no" },
                paths.json.display()
            )
        }
    }
}

fn verify_youth_deliverables(study: u32, root: &Path) -> Result<Deliverables> {
    let paths = expected_paths(study, Audience::Youth, root);
    for p in [&paths.json, &paths.img1, &paths.img2, &paths.img3] {
        if !p.exists() {
            bail!("deliverable missing: {}", p.display());
        }
    }
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&paths.json)?)?;
    if data.get("numero").and_then(|v| v.as_u64()) != Some(study as u64)
        && data.get("numero").and_then(|v| v.as_i64()) != Some(study as i64)
    {
        let ok = data
            .get("numero")
            .map(|v| v.to_string().contains(&study.to_string()))
            .unwrap_or(false);
        if !ok {
            bail!(
                "{} numero is {:?}, expected {study}",
                paths.json.display(),
                data.get("numero")
            );
        }
    }
    let imgs = data
        .get("section_images")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if imgs != 3 {
        bail!(
            "{} must list 3 section_images, got {imgs}",
            paths.json.display()
        );
    }
    let expected = section_style_for_study(study);
    let id = data
        .pointer("/section_style/id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if id != expected.id {
        bail!(
            "{} must include section_style.id={:?} (assigned for estudio {study}); got {:?}",
            paths.json.display(),
            expected.id,
            id
        );
    }
    Ok(paths)
}

fn verify_adult_deliverables(study: u32, root: &Path) -> Result<Deliverables> {
    let paths = expected_paths(study, Audience::Adult, root);
    // Prefer section folder tree (incremental prepare); fall back to JSON.
    let study_model: crate::model::adult_study::AdultStudy =
        if crate::content::has_adult_sections(study) {
            crate::content::load_adult_sections(study)?
        } else if paths.json.exists() {
            let _ = normalize_adult_agent_json(&paths.json)?;
            let text = std::fs::read_to_string(&paths.json)?;
            serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!(
                    "{} is not valid adult study JSON ({e}). \
                     Need lectura_antifonal, objetivos, pensamiento_central, texto_aureo, \
                     ensenanza, datos_generales, introduccion_slides, temas[1+] with \
                     puntos A…N (or legacy A/B keys), proximo.",
                    paths.json.display()
                )
            })?
        } else {
            bail!(
                "adult estudio {study}: need studies/adult/{study}/ (section folders) \
                 or {}",
                paths.json.display()
            );
        };
    if study_model.numero.to_string() != study.to_string()
        && !study_model.numero.to_string().contains(&study.to_string())
    {
        bail!(
            "adult estudio {study}: numero is {:?}, expected {study}",
            study_model.numero
        );
    }
    if study_model.temas.is_empty() {
        bail!("{} must have at least 1 tema", paths.json.display());
    }
    if study_model.objetivos.len() != 3 {
        bail!(
            "{} must have exactly 3 objetivos, got {}",
            paths.json.display(),
            study_model.objetivos.len()
        );
    }
    for (idx, tema) in study_model.temas.iter().enumerate() {
        let n = idx + 1;
        if tema.puntos.is_empty() {
            bail!(
                "{} tema {n} has no puntos — need lettered points A…N until Afianzamiento",
                paths.json.display()
            );
        }
        for p in &tema.puntos {
            if p.texto_slides.is_empty() {
                bail!(
                    "{} tema {n} point {} has empty texto_slides — pack body paragraphs (~360–400 chars each)",
                    paths.json.display(),
                    p.letra
                );
            }
        }
    }

    let media = adult_media_dir(study, root);
    let scenic_needed = adult_scenic_basenames_for(&study_model);
    let mut missing_scenic = Vec::new();
    for name in &scenic_needed {
        let p = media.join(name);
        if !p.exists() {
            missing_scenic.push(p.display().to_string());
        }
    }
    if !missing_scenic.is_empty() {
        bail!(
            "adult estudio {study} missing required scenic images ({}):\n  {}",
            missing_scenic.len(),
            missing_scenic.join("\n  ")
        );
    }
    // Scenic stubs block resume (need Cursor/host art). Definición stubs do NOT —
    // local gold bake / --gen-definicion fills them (OpenAI only if key set).
    if crate::content::any_scenic_placeholders(study, &study_model, root) {
        bail!(
            "adult estudio {study}: PLACEHOLDER scenic image(s) remain under {} — \
             not ready to resume/build. Re-run with -g (Cursor generates unique scenes) \
             or set openai_api_key and --gen-images.",
            media.display()
        );
    }
    let mut missing_or_stub_def = Vec::new();
    for (i, tema) in study_model.temas.iter().enumerate() {
        if tema.definiciones.is_empty() {
            continue;
        }
        let name = format!("definicion-{}.png", i + 1);
        let p = media.join(&name);
        if !p.exists() || crate::content::is_placeholder(&p) {
            missing_or_stub_def.push(name);
            continue;
        }
        if let Err(e) = crate::build::adult::assert_study_definicion_card(
            &p,
            tema.definiciones
                .iter()
                .map(|d| (d.termino.as_str(), d.texto.as_str())),
        ) {
            crate::progress::warn(format!(
                "adult estudio {study}: {name} failed locked-layout QC ({e:#}) — \
                 build will omit that Definición slide; fix with --gen-definicion \
                 (OpenAI if key set, else local gold bake)"
            ));
            missing_or_stub_def.push(name);
        }
    }
    if !missing_or_stub_def.is_empty() {
        crate::progress::warn(format!(
            "adult estudio {study}: definición card(s) not ready ({}) — \
             deck omits those slides until `lamad prepare -n {study} \
             --audience adult --gen-definicion` (Cursor gold-edit by default; \
             OpenAI if openai_api_key is set)",
            missing_or_stub_def.join(", ")
        ));
    }
    Ok(paths)
}

/// Strip / coerce agent-authored quirks **before** schema parse.
///
/// Cloud agents often invent shapes that are content-complete but schema-wrong:
/// - `estudio` instead of `numero`; `lectura_slides` instead of `lectura_antifonal`
/// - legacy `A`/`B`/`C` blocks missing `texto_biblico` (lives on tema as slides)
/// - `definiciones` as bare strings; image maps instead of arrays
/// - youth leftovers (`section_images`, `packed_slides`, …)
///
/// Returns `true` when the file was rewritten.
pub fn normalize_adult_agent_json(json_path: &Path) -> Result<bool> {
    let text = std::fs::read_to_string(json_path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    let Some(obj) = v.as_object_mut() else {
        bail!("{} root must be an object", json_path.display());
    };
    let mut changed = false;

    // estudio → numero
    if obj.get("numero").is_none() {
        if let Some(est) = obj.remove("estudio") {
            obj.insert("numero".into(), est);
            changed = true;
        }
    } else if obj.remove("estudio").is_some() {
        changed = true;
    }

    // lectura_slides → lectura_antifonal [{cita, versiculos}]
    if obj.get("lectura_antifonal").is_none() {
        if let Some(slides) = obj.remove("lectura_slides") {
            if let Some(block) = coerce_slide_strings_to_lectura(&slides) {
                obj.insert(
                    "lectura_antifonal".into(),
                    serde_json::Value::Array(vec![block]),
                );
                changed = true;
            }
        }
    } else if obj.remove("lectura_slides").is_some() {
        changed = true;
    }

    // Clean datos_generales of agent extras (enseñanza duplicated under ensenanza).
    if let Some(serde_json::Value::Object(datos)) = obj.get_mut("datos_generales") {
        for junk in ["enseñanza", "ensenanza", "enseñaza"] {
            if datos.remove(junk).is_some() {
                changed = true;
            }
        }
    }

    if let Some(serde_json::Value::Array(temas)) = obj.get_mut("temas") {
        for tema in temas.iter_mut() {
            let Some(tema_obj) = tema.as_object_mut() else {
                continue;
            };
            if coerce_tema_agent_shape(tema_obj) {
                changed = true;
            }
        }
    }

    if let Some(def) = obj.get("definicion_images") {
        match def {
            serde_json::Value::Array(_) => {}
            serde_json::Value::Object(map) => {
                let mut pairs: Vec<(i64, String)> = map
                    .iter()
                    .filter_map(|(k, val)| {
                        let path = val.as_str()?.to_string();
                        let idx = k.parse::<i64>().ok().unwrap_or(i64::MAX);
                        Some((idx, path))
                    })
                    .collect();
                pairs.sort_by_key(|(i, _)| *i);
                let arr: Vec<serde_json::Value> = pairs
                    .into_iter()
                    .map(|(_, p)| serde_json::Value::String(p))
                    .collect();
                obj.insert("definicion_images".into(), serde_json::Value::Array(arr));
                changed = true;
            }
            _ => {
                obj.remove("definicion_images");
                changed = true;
            }
        }
    }

    if let Some(sc) = obj.get("scenic_images") {
        let ok = sc
            .as_object()
            .map(|m| {
                m.contains_key("intro_header") && m.contains_key("tema") && m.contains_key("ab")
            })
            .unwrap_or(false);
        if !ok {
            obj.remove("scenic_images");
            changed = true;
        }
    }

    for junk in [
        "section_images",
        "section_style",
        "packed_slides",
        "tema_trimestral",
    ] {
        if obj.remove(junk).is_some() {
            changed = true;
        }
    }

    if changed {
        std::fs::write(json_path, serde_json::to_string_pretty(&v)?)?;
    }
    Ok(changed)
}

/// Convert agent slide strings / verse arrays into `{cita, versiculos}`.
fn coerce_slide_strings_to_lectura(v: &serde_json::Value) -> Option<serde_json::Value> {
    match v {
        serde_json::Value::Object(o)
            if o.contains_key("cita") && o.contains_key("versiculos") =>
        {
            Some(v.clone())
        }
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            let mut cita = String::new();
            let mut versiculos: Vec<String> = Vec::new();
            for item in arr {
                match item {
                    serde_json::Value::String(s) => {
                        for line in s.lines().map(str::trim).filter(|l| !l.is_empty()) {
                            let starts_verse = line
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_digit())
                                .unwrap_or(false);
                            if cita.is_empty() && !starts_verse {
                                cita = line.to_string();
                            } else {
                                versiculos.push(line.to_string());
                            }
                        }
                    }
                    serde_json::Value::Object(o) => {
                        if cita.is_empty() {
                            if let Some(c) = o.get("cita").and_then(|x| x.as_str()) {
                                cita = c.to_string();
                            }
                        }
                        if let Some(vs) = o.get("versiculos").and_then(|x| x.as_array()) {
                            for v in vs {
                                if let Some(s) = v.as_str() {
                                    versiculos.push(s.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if cita.is_empty() && versiculos.is_empty() {
                return None;
            }
            if cita.is_empty() {
                cita = versiculos
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Texto".into());
            }
            Some(serde_json::json!({ "cita": cita, "versiculos": versiculos }))
        }
        _ => None,
    }
}

fn coerce_definicion_entry(v: &serde_json::Value) -> Option<serde_json::Value> {
    match v {
        serde_json::Value::Object(o)
            if o.get("termino").and_then(|x| x.as_str()).is_some()
                && o.get("texto").and_then(|x| x.as_str()).is_some() =>
        {
            Some(v.clone())
        }
        serde_json::Value::String(s) => {
            let s = s.trim();
            // `"Term". Body…` or `Term. Body…`
            let (termino, texto) = if let Some(rest) = s.strip_prefix('"') {
                if let Some(end) = rest.find('"') {
                    let term = rest[..end].trim().to_string();
                    let body = rest[end + 1..]
                        .trim()
                        .trim_start_matches('.')
                        .trim()
                        .to_string();
                    (term, body)
                } else {
                    (s.to_string(), String::new())
                }
            } else if let Some((term, body)) = s.split_once('.') {
                (term.trim().to_string(), body.trim().to_string())
            } else {
                (s.to_string(), String::new())
            };
            Some(serde_json::json!({ "termino": termino, "texto": texto }))
        }
        _ => None,
    }
}

/// Rewrite one tema object: string defs; coerce A–F / puntos texto_biblico.
///
/// **HARD (gold estudio 8):** never copy tema-wide Lectura into every point.
/// TEMA image uses `rango`; each A/B image + Texto Bíblico uses that point’s
/// own verses only.
fn coerce_tema_agent_shape(tema: &mut serde_json::Map<String, serde_json::Value>) -> bool {
    let mut changed = false;

    // definiciones: string[] → {termino,texto}[]
    if let Some(serde_json::Value::Array(defs)) = tema.get("definiciones").cloned() {
        let mut out = Vec::new();
        let mut any = false;
        for d in &defs {
            if let Some(c) = coerce_definicion_entry(d) {
                if !d.is_object()
                    || d.get("termino").is_none()
                    || d.get("texto").is_none()
                {
                    any = true;
                }
                out.push(c);
            }
        }
        if any || out.len() != defs.len() {
            tema.insert("definiciones".into(), serde_json::Value::Array(out));
            changed = true;
        }
    }

    // Tema-level texto_biblico / slides are not part of TemaAdult — drop them.
    // Do **not** inject into A/B (that made every Texto Bíblico show the full
    // TEMA range, e.g. `2 Samuel 6:1-11` on 1.A instead of `6:1,2`).
    if tema.remove("texto_biblico_slides").is_some() {
        changed = true;
    }
    match tema.get("texto_biblico") {
        Some(serde_json::Value::Object(_)) | Some(serde_json::Value::Array(_)) => {
            tema.remove("texto_biblico");
            changed = true;
        }
        _ => {}
    }

    let empty_tb = serde_json::json!({ "cita": "", "versiculos": [] });

    // Legacy A–F blocks: coerce shape only — never fill from tema rango.
    for letter in ["A", "B", "C", "D", "E", "F"] {
        let Some(block) = tema.get_mut(letter) else {
            continue;
        };
        let Some(bobj) = block.as_object_mut() else {
            continue;
        };
        match bobj.get("texto_biblico") {
            Some(serde_json::Value::Object(_)) => {}
            Some(other) => {
                if let Some(c) = coerce_slide_strings_to_lectura(other) {
                    bobj.insert("texto_biblico".into(), c);
                } else {
                    bobj.insert("texto_biblico".into(), empty_tb.clone());
                }
                changed = true;
            }
            None => {
                bobj.insert("texto_biblico".into(), empty_tb.clone());
                changed = true;
            }
        }
    }

    // puntos[]: same — coerce only, no tema-wide inject.
    if let Some(serde_json::Value::Array(puntos)) = tema.get_mut("puntos") {
        for p in puntos.iter_mut() {
            let Some(pobj) = p.as_object_mut() else {
                continue;
            };
            match pobj.get("texto_biblico") {
                Some(serde_json::Value::Object(_)) => {}
                Some(other) => {
                    if let Some(c) = coerce_slide_strings_to_lectura(other) {
                        pobj.insert("texto_biblico".into(), c);
                    } else {
                        pobj.insert("texto_biblico".into(), empty_tb.clone());
                    }
                    changed = true;
                }
                None => {
                    pobj.insert("texto_biblico".into(), empty_tb.clone());
                    changed = true;
                }
            }
        }
    }

    // Drop tema.numero (not in schema)
    if tema.remove("numero").is_some() {
        changed = true;
    }

    changed
}

/// Write `scenic_images` + `definicion_images` paths into adult JSON after PNGs land.
pub fn stamp_adult_image_paths(json_path: &Path, study: u32, root: &Path) -> Result<()> {
    let _ = normalize_adult_agent_json(json_path)?;
    let text = std::fs::read_to_string(json_path)?;
    let study_model: crate::model::adult_study::AdultStudy = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("{} is not valid adult study JSON ({e})", json_path.display()))?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("adult JSON root must be an object"))?;

    let _ = root;
    // Prefer section-layout images/ when that folder exists (or will exist).
    let images_subdir = root
        .join("studies")
        .join("adult")
        .join(study.to_string())
        .join("images")
        .is_dir();
    let rel = |name: &str| -> String {
        if images_subdir {
            format!("studies/adult/{study}/images/{name}")
        } else {
            format!("studies/adult/{study}/{name}")
        }
    };

    let tema_paths: Vec<serde_json::Value> = (1..=study_model.temas.len())
        .map(|i| serde_json::Value::String(rel(&format!("tema-{i}.png"))))
        .collect();
    let mut ab_paths = Vec::new();
    for (ti, tema) in study_model.temas.iter().enumerate() {
        for p in &tema.puntos {
            ab_paths.push(serde_json::Value::String(rel(&format!(
                "ab-{}{}.png",
                ti + 1,
                p.letra
            ))));
        }
    }
    obj.insert(
        "scenic_images".into(),
        serde_json::json!({
            "intro_header": rel("intro-header.png"),
            "tema": tema_paths,
            "ab": ab_paths
        }),
    );
    let def_paths: Vec<serde_json::Value> = (1..=study_model.temas.len())
        .map(|i| serde_json::Value::String(rel(&format!("definicion-{i}.png"))))
        .collect();
    obj.insert(
        "definicion_images".into(),
        serde_json::Value::Array(def_paths),
    );
    std::fs::write(json_path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// Map one artifact path to a local destination. Prefers exact names:
/// `{study}.json`, `{study}-section1.png`, …
///
/// Rules (checked in order):
/// 1. basename equals `{study}.json`, or the path ends with `/{study}.json`.
/// 2. basename equals `{study}-section{1,2,3}.{png,jpg,jpeg}` (also accept the
///    bare `section{1,2,3}.{png,jpg,jpeg}`, without the study-number prefix).
///
/// Returns `None` when neither rule matches. Callers should then fall back
/// to positional matching (see [`resolve_artifacts`]) — never guess a JSON
/// artifact positionally, only images.
fn map_artifact(study: u32, art_path: &str, dest: &Deliverables) -> Option<PathBuf> {
    let basename = art_path.rsplit('/').next().unwrap_or(art_path);

    let json_name = format!("{study}.json");
    if basename.eq_ignore_ascii_case(&json_name) || art_path.ends_with(&format!("/{json_name}")) {
        return Some(dest.json.clone());
    }

    for (n, slot) in [(1, &dest.img1), (2, &dest.img2), (3, &dest.img3)] {
        for ext in ["png", "jpg", "jpeg", "webp"] {
            let named = format!("{study}-section{n}.{ext}");
            let bare = format!("section{n}.{ext}");
            if basename.eq_ignore_ascii_case(&named) || basename.eq_ignore_ascii_case(&bare) {
                return Some(slot.clone());
            }
        }
    }

    None
}

fn is_section_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
}

/// Assign every downloaded artifact to its local destination, per
/// AGENTS.md's "never double-count" rule.
///
/// - **Pass 1 (exact names):** each artifact is matched via
///   [`map_artifact`]; first match wins a slot, so one artifact can't fill
///   two slots and one slot can't be filled twice.
/// - **Pass 2 (positional fallback):** any of the 3 section-image slots
///   still empty after pass 1 are filled from the *remaining* (unmatched)
///   image artifacts (png/jpg/jpeg/webp), taken in sorted-path order,
///   lowest-numbered missing slot first. The JSON slot is never guessed
///   positionally — an unresolved JSON is a hard error.
///
/// Returns `(artifact_path, destination)` pairs for every filled slot, or a
/// clear error listing what was found vs. what's still missing.
fn resolve_artifacts(
    study: u32,
    artifacts: &[Artifact],
    dest: &Deliverables,
) -> Result<Vec<(String, PathBuf)>> {
    let slot_names = [
        format!("{study}.json"),
        "section1.png".to_string(),
        "section2.png".to_string(),
        "section3.png".to_string(),
    ];
    let mut slots: [Option<(String, PathBuf)>; 4] = [None, None, None, None];
    let mut used = vec![false; artifacts.len()];

    // Pass 1 — exact-name matches.
    for (idx, art) in artifacts.iter().enumerate() {
        let Some(dest_path) = map_artifact(study, &art.path, dest) else {
            continue;
        };
        let slot = if dest_path == dest.json {
            0
        } else if dest_path == dest.img1 {
            1
        } else if dest_path == dest.img2 {
            2
        } else {
            3
        };
        if slots[slot].is_none() {
            slots[slot] = Some((art.path.clone(), dest_path));
            used[idx] = true;
        }
    }

    // Pass 2 — fill remaining section-image slots from leftover images.
    let mut leftover_imgs: Vec<&Artifact> = artifacts
        .iter()
        .enumerate()
        .filter(|(idx, art)| !used[*idx] && is_section_image_path(&art.path))
        .map(|(_, art)| art)
        .collect();
    leftover_imgs.sort_by(|a, b| a.path.cmp(&b.path));
    let mut leftover = leftover_imgs.into_iter();

    for (slot, dest_slot) in [(1, &dest.img1), (2, &dest.img2), (3, &dest.img3)] {
        if slots[slot].is_some() {
            continue;
        }
        if let Some(art) = leftover.next() {
            slots[slot] = Some((art.path.clone(), dest_slot.clone()));
        }
    }

    let missing: Vec<&str> = slots
        .iter()
        .zip(slot_names.iter())
        .filter(|(a, _)| a.is_none())
        .map(|(_, name)| name.as_str())
        .collect();
    if !missing.is_empty() {
        let found: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        bail!(
            "could not map artifacts to deliverables for estudio {study} — missing: {}. \
             Artifacts found ({}): {:?}. Expected {study}.json + 3 section images \
             (png/jpg) under artifacts/.",
            missing.join(", "),
            found.len(),
            found
        );
    }

    Ok(slots.into_iter().map(|s| s.unwrap()).collect())
}

/// If `path` is not already a PNG (by magic bytes), decode and rewrite as RGB PNG.
/// Cloud agents sometimes emit `.jpg` while destinations are always `*-sectionN.png`.
fn ensure_png_file(path: &Path) -> Result<()> {
    let raw = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    if raw.len() >= 8 && raw.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
        return Ok(());
    }
    let img = image::load_from_memory(&raw)
        .map_err(|e| anyhow::anyhow!("decode image {}: {e}", path.display()))?;
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let mut out = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Png,
    )
    .map_err(|e| anyhow::anyhow!("encode PNG {}: {e}", path.display()))?;
    std::fs::write(path, out).map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    Ok(())
}

pub async fn run_prepare_agent(req: PrepareRequest) -> Result<Deliverables> {
    match req.provider {
        AgentProvider::ChatGpt => {
            crate::agent::prepare_openai::run_prepare_openai(&req).await
        }
        AgentProvider::Cursor => match run_prepare_cursor(req.clone()).await {
            Ok(d) => Ok(d),
            Err(cursor_err) => {
                let Some(openai_key) = req
                    .image_api_key
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                else {
                    return Err(cursor_err).context(
                        "Cursor prepare failed (transient API errors were already retried) — \
                         re-run the same command to resume",
                    );
                };
                crate::progress::warn(format!(
                    "Cursor prepare failed — falling back to ChatGPT ({})",
                    short_err(&cursor_err)
                ));
                let mut fb = req;
                fb.provider = AgentProvider::ChatGpt;
                fb.api_key = openai_key.clone();
                fb.image_api_key = Some(openai_key);
                fb.model = fb.openai_model.clone();
                crate::agent::prepare_openai::run_prepare_openai(&fb)
                    .await
                    .with_context(|| {
                        format!(
                            "ChatGPT fallback also failed (after Cursor: {})",
                            short_err(&cursor_err)
                        )
                    })
            }
        },
    }
}

fn short_err(e: &anyhow::Error) -> String {
    let s = format!("{e:#}");
    if s.len() <= 160 {
        s
    } else {
        format!("{}…", &s[..157])
    }
}

async fn run_prepare_cursor(req: PrepareRequest) -> Result<Deliverables> {
    paths::studies_dir(req.audience)?;
    let pages_dir = paths::generated_dir()?.join("pages").join(format!("study{}", req.study));
    std::fs::create_dir_all(&pages_dir)?;

    // Rasterize **every** content page (+ próximo title page when present).
    // Never drop pages for size — Cursor’s HTTP API allows ≤5 attachments, so
    // we pack extras into vertical composites (see `page_pack`).
    let mut page_nums: Vec<u32> = (req.pages.0..=req.pages.1).collect();
    if let Some((na, _)) = req.next_pages {
        if !page_nums.contains(&na) {
            page_nums.push(na);
        }
    }

    crate::progress::now_step(
        2,
        4,
        "Upload + start agent",
        "Rasterizing pages, uploading scans, creating the Cursor cloud agent (~1 min)…",
    );
    let rasters = rasterize_pages(
        &req.pdf,
        &page_nums,
        &pages_dir,
        req.pdftoppm_path.as_deref(),
    )?;
    let pack_dir = pages_dir.join("cursor-packs");
    let packed = super::page_pack::pack_pages_for_api_limit(
        &rasters,
        &pack_dir,
        super::page_pack::CURSOR_MAX_IMAGES,
    )?;
    if packed.len() < rasters.len() {
        crate::progress::phase(format!(
            "Cursor API max {} images — packed {} pages into {} composites (no pages dropped)",
            super::page_pack::CURSOR_MAX_IMAGES,
            rasters.len(),
            packed.len()
        ));
    }
    crate::progress::phase("encoding page images for the agent…");
    let mut images = Vec::new();
    for r in &packed {
        images.push(client::image_from_path(r)?);
    }

    // Fast path: openai_api_key set → Cursor = JSON only, host OpenAI = images.
    // Otherwise Cursor generates images with the Cursor cloud agent (same as before).
    let host_images = req.host_images();
    if host_images {
        crate::progress::phase(
            "fast path ON — Cursor writes JSON only; host OpenAI generates images in parallel",
        );
    } else if req.audience == Audience::Adult {
        crate::progress::phase(
            "Cursor will generate adult scenic + definición images (via Cursor agent / your Cursor key)",
        );
    } else {
        crate::progress::phase(
            "Cursor will generate section images (set openai_api_key for host-parallel fast path)",
        );
    }

    let prompt = build_cloud_prepare_prompt(&req, host_images)?;
    let client = CursorClient::new(&req.api_key)?;
    let create_spin = crate::progress::Spinner::start(
        "Step 2/4 — Starting Cursor cloud agent (upload + create)… usually ~1 min",
    );
    let created = match client
        .create_agent(
            &prompt,
            &images,
            &req.model,
            &format!("lamad-prepare-{}", req.study),
        )
        .await
    {
        Ok(c) => {
            create_spin.succeed(format!(
                "agent {} · run {}",
                c.agent.id, c.run.id
            ));
            c
        }
        Err(e) => {
            create_spin.fail("failed to start Cursor agent");
            return Err(e);
        }
    };
    crate::progress::ok("Step 2/4 done — agent created");
    crate::progress::now_step(
        3,
        4,
        if host_images {
            "Agent (JSON only)"
        } else {
            "Agent work (longest)"
        },
        if host_images {
            "Cloud agent is writing study JSON only (~1–3 min)…"
        } else {
            match req.audience {
                Audience::Youth => {
                    "Cloud agent is writing JSON + 3 section images (~3–8 min)…"
                }
                Audience::Adult => {
                    "Cloud agent is writing section folders (then images) — progress per section…"
                }
            }
        },
    );
    let mut agent_progress = crate::agent::wait_progress::CloudAgentProgress::new(
        req.study,
        req.audience,
        host_images,
    );
    client
        .wait_run_with_progress(
            &created.agent.id,
            &created.run.id,
            req.stream,
            Some(&mut agent_progress),
        )
        .await?;

    crate::progress::ok(if host_images {
        "Step 3/4 — agent finished (JSON)"
    } else {
        "Step 3/4 done — agent finished"
    });

    let mut artifacts = client.list_artifacts(&created.agent.id).await?;
    if artifacts.is_empty() {
        crate::progress::warn(if host_images {
            "no artifacts yet — sending follow-up to write JSON…"
        } else {
            "no artifacts yet — sending follow-up to write JSON + images…"
        });
        let nudge = json_followup_nudge(req.study, req.audience, host_images);
        let follow = client
            .create_followup(&created.agent.id, &nudge)
            .await?;
        println!("  follow-up run {}", follow.run.id);
        client
            .wait_run(&created.agent.id, &follow.run.id, req.stream)
            .await?;
        artifacts = client.list_artifacts(&created.agent.id).await?;
    }
    if artifacts.is_empty() {
        bail!(
            "cloud agent finished but listed no artifacts (after follow-up). \
             Agent id: {}",
            created.agent.id
        );
    }

    // Adult: NEVER nudge for monolith JSON. Section folders are the deliverable.
    if req.audience == Audience::Adult {
        if !artifact_list_has_adult_section_tree(req.study, &artifacts) {
            artifacts = ensure_adult_section_tree_artifact(
                &client,
                &created.agent.id,
                req.study,
                &artifacts,
                req.stream,
            )
            .await?;
        } else {
            crate::progress::info(
                "adult section tree present in artifacts — skipping JSON follow-up",
            );
        }
    } else if !artifact_list_has_study_json(req.study, &artifacts) {
        artifacts = ensure_study_json_artifact(
            &client,
            &created.agent.id,
            req.study,
            &artifacts,
            req.stream,
        )
        .await?;
    }

    let dest = expected_paths(req.study, req.audience, &req.root);
    std::fs::create_dir_all(dest.json.parent().unwrap())?;
    if let Some(p) = dest.img1.parent() {
        std::fs::create_dir_all(p)?;
    }

    let can_host = req
        .image_api_key
        .as_ref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);

    let resolved = match req.audience {
        Audience::Adult => resolve_adult_artifacts(req.study, &artifacts, &req.root)?,
        Audience::Youth if host_images => {
            vec![resolve_json_artifact(req.study, &artifacts, &dest)?]
        }
        Audience::Youth => match resolve_artifacts(req.study, &artifacts, &dest) {
            Ok(r) => r,
            Err(e) => {
                if can_host && artifact_list_has_study_json(req.study, &artifacts) {
                    crate::progress::warn(format!(
                        "agent section images incomplete ({e:#}); \
                         downloading JSON and generating images via OpenAI…"
                    ));
                    vec![resolve_json_artifact(req.study, &artifacts, &dest)?]
                } else if artifact_list_has_study_json(req.study, &artifacts) {
                    crate::progress::warn(format!(
                        "agent section images incomplete ({e:#}); \
                         downloading JSON — placeholders will fill gaps"
                    ));
                    vec![resolve_json_artifact(req.study, &artifacts, &dest)?]
                } else {
                    return Err(e);
                }
            }
        },
    };

    crate::progress::now_step(
        4,
        4,
        "Finish",
        if host_images {
            "Downloading JSON, then host parallel images, then PowerPoint…"
        } else {
            "Downloading artifacts, then building PowerPoint (+ PDF if enabled)…"
        },
    );
    crate::progress::phase(format!(
        "downloading {} artifact{} in parallel…",
        resolved.len(),
        if resolved.len() == 1 { "" } else { "s" }
    ));
    download_resolved_artifacts(
        &client,
        &created.agent.id,
        resolved,
        req.audience == Audience::Adult,
    )
    .await?;

    if req.audience == Audience::Adult {
        if crate::content::has_adult_sections(req.study) {
            let model = crate::content::load_adult_sections(req.study).with_context(|| {
                format!(
                    "adult estudio {}: section folders on disk but failed to load — \
                     check studies/adult/{}/",
                    req.study, req.study
                )
            })?;
            crate::content::ensure_adult_placeholders(req.study, &model, &req.root)?;
            // Sidecar JSON from sections (overwrite broken agent JSON).
            if let Ok(text) = serde_json::to_string_pretty(&model) {
                let _ = std::fs::write(&dest.json, text);
            }
        } else if dest.json.is_file() {
            // Legacy JSON path only when no section tree landed.
            let _ = normalize_adult_agent_json(&dest.json)?;
            stamp_audience(&dest.json, req.audience)?;
            stamp_adult_image_paths(&dest.json, req.study, &req.root)?;
            let text = std::fs::read_to_string(&dest.json)?;
            let model: crate::model::adult_study::AdultStudy = serde_json::from_str(&text)
                .with_context(|| {
                    format!(
                        "parse {} for placeholders — prefer section folders under \
                         studies/adult/{}/ (delete this JSON and re-run to resume)",
                        dest.json.display(),
                        req.study
                    )
                })?;
            crate::content::ensure_adult_placeholders(req.study, &model, &req.root)?;
        } else {
            bail!(
                "adult estudio {}: no section tree under studies/adult/{}/ and no JSON. \
                 Re-run prepare to resume.",
                req.study,
                req.study
            );
        }
    } else {
        stamp_audience(&dest.json, req.audience)?;
    }

    // Youth: host images here when configured.
    // Adult: Cursor art (when not host_images) is already downloaded; placeholders
    // fill gaps and `lib` refreshes them via host OpenAI when that key is set.
    if req.audience == Audience::Youth
        && (host_images
            || (can_host && !required_images_present(req.study, req.audience, &req.root)))
    {
        if can_host {
            verify_json_only(req.study, req.audience, &req.root)?;
            match crate::agent::prepare_openai::generate_host_images(&req).await {
                Ok(()) => {}
                Err(e) => {
                    crate::progress::warn(format!(
                        "host image generation failed ({e:#}) — continuing"
                    ));
                }
            }
        }
    }

    let _ = pdf_page_count;
    verify_deliverables(req.study, req.audience, &req.root)
}

fn json_followup_nudge(study: u32, audience: Audience, host_images: bool) -> String {
    if host_images {
        return match audience {
            Audience::Adult => format!(
                "STOP. Zero artifacts listed. Write the adult **section folder tree** under \
                 `artifacts/{study}/` (`manifest.toml`, then `01-title/`, `02-lectura/`, … \
                 one folder at a time). Do **not** generate images — the host CLI does that. \
                 Use Write/Shell tools; chat text does not count."
            ),
            Audience::Youth => format!(
                "STOP. Zero artifacts listed. Write **only** `artifacts/{study}.json` \
                 (valid {aud} schema). Do **not** generate images — the host CLI does that. \
                 Use Write/Shell tools; chat text does not count.",
                aud = audience.as_str()
            ),
        };
    }
    match audience {
        Audience::Youth => format!(
            "STOP. You finished without writing any files under `artifacts/`.\n\n\
             Create these exact paths:\n\
             1. artifacts/{study}.json\n\
             2. artifacts/{study}-section1.png\n\
             3. artifacts/{study}-section2.png\n\
             4. artifacts/{study}-section3.png\n\n\
             Use Write/Shell/image tools — chat text does not count."
        ),
        Audience::Adult => format!(
            "STOP. Zero artifacts listed. Write the adult **section folder tree** under \
             `artifacts/{study}/` starting with `manifest.toml` then `01-title/`, \
             `02-lectura/`, … (one folder at a time). Also scenic PNGs under \
             `artifacts/{study}/images/` or flat `artifacts/`: {names}. \
             Use Write/Shell/image tools — chat text does not count.",
            names = adult_image_basenames().join(", ")
        ),
    }
}

fn resolve_json_artifact(
    study: u32,
    artifacts: &[Artifact],
    dest: &Deliverables,
) -> Result<(String, PathBuf)> {
    let json_name = format!("{study}.json");
    for art in artifacts {
        let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
        if basename.eq_ignore_ascii_case(&json_name) || art.path.ends_with(&format!("/{json_name}"))
        {
            return Ok((art.path.clone(), dest.json.clone()));
        }
    }
    bail!(
        "missing artifacts/{study}.json (found {} artifacts)",
        artifacts.len()
    )
}

async fn download_resolved_artifacts(
    client: &CursorClient,
    agent_id: &str,
    resolved: Vec<(String, PathBuf)>,
    placeholders_fill_gaps: bool,
) -> Result<()> {
    let dl_bar = crate::progress::bar(resolved.len() as u64, "Downloading artifacts");
    {
        use futures_util::stream::{self, StreamExt};
        let results: Vec<Result<()>> = stream::iter(resolved.into_iter())
            .map(|(art_path, dest_path)| {
                let client = client;
                let agent_id = agent_id.to_string();
                let bar = dl_bar.clone();
                async move {
                    if let Some(parent) = dest_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    // HARD: never "keep" an existing DEFINICIÓN PNG across downloads.
                    // Layout QC alone is not enough — leftover gold from another study
                    // also passes QC (wrong terms baked in). Always take the new artifact;
                    // assert_locked_definicion_card rejects invents after download.
                    let is_def = dest_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with("definicion-") && n.ends_with(".png"))
                        .unwrap_or(false);
                    let is_image = dest_path.extension().and_then(|e| e.to_str()) == Some("png");
                    let fetched: Result<()> = async {
                        client
                            .download_artifact_quiet(&agent_id, &art_path, &dest_path)
                            .await?;
                        if is_image {
                            ensure_png_file(&dest_path)?;
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(e) = fetched {
                        if !is_image || !placeholders_fill_gaps {
                            return Err(e);
                        }
                        let _ = std::fs::remove_file(&dest_path);
                        crate::progress::warn(format!(
                            "could not download {art_path} ({}) — placeholder will fill it",
                            short_err(&e)
                        ));
                        bar.inc(1);
                        return Ok(());
                    }
                    if is_def {
                        // Stale .terms from a prior estudio must not launder new art.
                        if let Some(stem) = dest_path.file_stem().and_then(|s| s.to_str()) {
                            if let Some(parent) = dest_path.parent() {
                                let _ = std::fs::remove_file(parent.join(format!("{stem}.terms")));
                            }
                        }
                        if crate::build::adult::definicion_card::is_byte_identical_to_any_gold(
                            &dest_path,
                        ) {
                            let _ = std::fs::remove_file(&dest_path);
                            if !placeholders_fill_gaps {
                                return Err(anyhow::anyhow!(
                                    "agent returned a verbatim gold-reference DEFINICIÓN PNG \
                                     (another study's baked terms) — refused"
                                ));
                            }
                            crate::progress::warn(
                                "Cursor DEFINICIÓN was a gold-reference copy (wrong-study \
                                 terms) — deleted; local gold bake will fill from this scan",
                            );
                            bar.inc(1);
                            return Ok(());
                        }
                        if let Err(e) =
                            crate::build::adult::assert_locked_definicion_card(&dest_path)
                        {
                            let _ = std::fs::remove_file(&dest_path);
                            if !placeholders_fill_gaps {
                                return Err(e).context(
                                    "agent DEFINICIÓN art rejected — layout is HARD-locked",
                                );
                            }
                            crate::progress::warn(format!(
                                "Cursor DEFINICIÓN art rejected by locked-layout QC ({}) — \
                                 deleted; local gold bake (or OpenAI --gen-definicion if key set) \
                                 will fill from THIS estudio's scan terms",
                                short_err(&e)
                            ));
                            bar.inc(1);
                            return Ok(());
                        }
                        // Layout OK from Cursor still has no proven term match —
                        // remove so local bake / --gen-definicion writes correct
                        // scan terms + .terms sidecar (never trust agent text alone).
                        let _ = std::fs::remove_file(&dest_path);
                        crate::progress::warn(format!(
                            "Cursor {} passed layout QC but has no terms proof — \
                             removed; local bake will redraw this estudio's Definiciones",
                            dest_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("definicion.png")
                        ));
                    }
                    if is_image {
                        crate::content::note_real_image(&dest_path);
                    }
                    bar.inc(1);
                    Ok(())
                }
            })
            .buffer_unordered(8)
            .collect()
            .await;
        for r in results {
            r?;
        }
    }
    dl_bar.finish_with_message("Downloads complete");
    Ok(())
}

/// Map adult cloud artifacts: `{N}.json` + named scenic/definición images.
/// Accepts `.png`/`.jpg`/`.jpeg`/`.webp` under `artifacts/` or `artifacts/assets/`
/// (host converts to PNG on download). Matching is case-insensitive on the stem
/// (`ab-1A.jpg` → `ab-1A.png`).
///
/// Required scenic count is **variable** (temas × points). We accept any
/// `intro-header` / `tema-N` / `ab-N{Letter}` / `definicion-N` stems present;
/// hard-fail only when `{N}.json` is missing. Missing image slots get placeholders
/// after download ([`crate::content::ensure_adult_placeholders`]).
fn resolve_adult_artifacts(
    study: u32,
    artifacts: &[Artifact],
    root: &Path,
) -> Result<Vec<(String, PathBuf)>> {
    let dest_json = expected_paths(study, Audience::Adult, root).json;
    let media = adult_media_dir(study, root);
    let study_dir = crate::content::adult_study_folder(study)?;
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut used = vec![false; artifacts.len()];

    // Prefer incremental section tree: artifacts/{N}/manifest.toml + NN-kind/…
    let tree_prefix = format!("artifacts/{study}/");
    let tree_prefix_alt = format!("{study}/");
    let mut section_files = 0usize;
    for (idx, art) in artifacts.iter().enumerate() {
        let path = art.path.replace('\\', "/");
        let rel = path
            .strip_prefix(&tree_prefix)
            .or_else(|| path.strip_prefix(&tree_prefix_alt));
        let Some(rel) = rel else {
            continue;
        };
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        used[idx] = true;
        section_files += 1;
        out.push((art.path.clone(), study_dir.join(rel)));
    }
    if section_files > 0 {
        crate::progress::info(format!(
            "adult prepare: downloading {section_files} section-tree file(s) → studies/adult/{study}/"
        ));
        // Also map any flat scenic/definición images still under artifacts/.
        for (idx, art) in artifacts.iter().enumerate() {
            if used[idx] {
                continue;
            }
            let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
            let lower = basename.to_lowercase();
            let (file_stem, ext) = match lower.rsplit_once('.') {
                Some(pair) => pair,
                None => continue,
            };
            if !matches!(ext, "png" | "jpg" | "jpeg" | "webp") {
                continue;
            }
            let Some(dest_name) = adult_media_dest_from_stem(file_stem, study) else {
                continue;
            };
            used[idx] = true;
            out.push((art.path.clone(), media.join(dest_name)));
        }
        return Ok(out);
    }

    let json_name = format!("{study}.json");
    for (idx, art) in artifacts.iter().enumerate() {
        let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
        if basename.eq_ignore_ascii_case(&json_name) || art.path.ends_with(&format!("/{json_name}"))
        {
            out.push((art.path.clone(), dest_json.clone()));
            used[idx] = true;
            break;
        }
    }
    if out.is_empty() {
        bail!(
            "adult prepare missing section tree artifacts/{study}/… \
             (or legacy artifacts/{study}.json). Found {} artifacts.",
            artifacts.len()
        );
    }

    let mut got_intro = false;
    let mut tema_n = 0usize;
    let mut ab_n = 0usize;
    let mut def_n = 0usize;

    for (idx, art) in artifacts.iter().enumerate() {
        if used[idx] {
            continue;
        }
        let basename = art.path.rsplit('/').next().unwrap_or(&art.path);
        let lower = basename.to_lowercase();
        let (file_stem, ext) = match lower.rsplit_once('.') {
            Some(pair) => pair,
            None => continue,
        };
        if !matches!(ext, "png" | "jpg" | "jpeg" | "webp") {
            continue;
        }
        let Some(dest_name) = adult_media_dest_from_stem(file_stem, study) else {
            continue;
        };
        used[idx] = true;
        if dest_name == "intro-header.png" {
            got_intro = true;
        } else if dest_name.starts_with("tema-") {
            tema_n += 1;
        } else if dest_name.starts_with("ab-") {
            ab_n += 1;
        } else if dest_name.starts_with("definicion-") {
            def_n += 1;
        }
        out.push((art.path.clone(), media.join(dest_name)));
    }

    if !got_intro || tema_n == 0 || ab_n == 0 {
        let found: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        crate::progress::warn(format!(
            "adult prepare: Cursor returned incomplete scenic art for estudio {study} \
             (intro={got_intro} tema={tema_n} ab={ab_n}; artifacts={found:?}). \
             Downloading what we have — placeholders fill the rest."
        ));
    }
    if def_n == 0 {
        crate::progress::warn(format!(
            "adult prepare: no definición cards in artifacts — placeholders / host will fill"
        ));
    }
    Ok(out)
}

/// Normalize an artifact stem to a local media basename (`ab-1c` → `ab-1C.png`).
fn adult_media_dest_from_stem(file_stem: &str, study: u32) -> Option<String> {
    let mut s = file_stem.to_lowercase();
    let prefix = format!("{study}-");
    if let Some(rest) = s.strip_prefix(&prefix) {
        s = rest.to_string();
    }

    if s == "intro-header" {
        return Some("intro-header.png".into());
    }
    if let Some(rest) = s.strip_prefix("tema-") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("tema-{rest}.png"));
        }
    }
    if let Some(rest) = s.strip_prefix("definicion-") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("definicion-{rest}.png"));
        }
    }
    if let Some(rest) = s.strip_prefix("ab-") {
        let mut num = String::new();
        let mut letter: Option<char> = None;
        for c in rest.chars() {
            if c.is_ascii_digit() && letter.is_none() {
                num.push(c);
            } else if c.is_ascii_alphabetic() && !num.is_empty() && letter.is_none() {
                letter = Some(c.to_ascii_uppercase());
            } else {
                return None;
            }
        }
        if let Some(l) = letter {
            return Some(format!("ab-{num}{l}.png"));
        }
    }
    None
}

pub(crate) fn stamp_audience(json_path: &Path, audience: Audience) -> Result<()> {
    let text = std::fs::read_to_string(json_path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "audience".into(),
            serde_json::Value::String(audience.as_str().into()),
        );
    }
    std::fs::write(json_path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

fn artifact_list_has_study_json(study: u32, artifacts: &[Artifact]) -> bool {
    let json_name = format!("{study}.json");
    artifacts.iter().any(|a| {
        let base = a.path.rsplit('/').next().unwrap_or(a.path.as_str());
        base.eq_ignore_ascii_case(&json_name)
    })
}

/// Complete enough to skip section follow-ups: manifest + tema header + point.
/// Front-matter-only stubs (title→texto-aureo) must NOT skip — that aborted
/// estudio 11 after a title-page-only extract.
fn artifact_list_has_adult_section_tree(study: u32, artifacts: &[Artifact]) -> bool {
    let prefix = format!("artifacts/{study}/");
    let prefix_alt = format!("{study}/");
    let mut has_manifest = false;
    let mut has_tema_header = false;
    let mut has_tema_point = false;
    for a in artifacts {
        let path = a.path.replace('\\', "/");
        let rel = path
            .strip_prefix(&prefix)
            .or_else(|| path.strip_prefix(&prefix_alt));
        let Some(rel) = rel else {
            continue;
        };
        // images/ alone is NOT a section tree (was falsely skipping text follow-ups).
        let top = rel.split('/').next().unwrap_or(rel);
        if top == "images" {
            continue;
        }
        if top == "manifest.toml" || rel.starts_with("manifest.toml") {
            has_manifest = true;
            continue;
        }
        // Section folders look like `01-title/…` or `10-tema-1-A/body.txt`.
        let looks_section = top.len() >= 4
            && top.as_bytes().get(0).is_some_and(|c| c.is_ascii_digit())
            && top.as_bytes().get(1).is_some_and(|c| c.is_ascii_digit())
            && top.as_bytes().get(2) == Some(&b'-');
        if !looks_section {
            continue;
        }
        let Some((_, kind)) = top.split_once('-') else {
            continue;
        };
        if let Some(rest) = kind.strip_prefix("tema-") {
            if rest.contains('-') {
                has_tema_point = true;
            } else if !rest.is_empty() {
                has_tema_header = true;
            }
        }
    }
    has_manifest && has_tema_header && has_tema_point
}

fn json_artifact_candidate_paths(study: u32) -> Vec<String> {
    vec![
        format!("artifacts/{study}.json"),
        format!("artifacts/assets/{study}.json"),
    ]
}

/// List artifacts, then probe download for JSON paths the API sometimes omits from List.
async fn refresh_artifacts_with_json_probe(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
) -> Result<Vec<Artifact>> {
    let mut artifacts = client.list_artifacts(agent_id).await?;
    if artifact_list_has_study_json(study, &artifacts) {
        return Ok(artifacts);
    }
    for path in json_artifact_candidate_paths(study) {
        if artifacts.iter().any(|a| a.path == path) {
            continue;
        }
        if client.artifact_downloadable(agent_id, &path).await {
            eprintln!("  found {path} via download probe (omitted from list)");
            artifacts.push(Artifact {
                path,
                size_bytes: None,
            });
            break;
        }
    }
    Ok(artifacts)
}

async fn poll_artifacts_for_json(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
    attempts: usize,
    delay_secs: u64,
) -> Result<Vec<Artifact>> {
    for i in 0..attempts {
        let artifacts = refresh_artifacts_with_json_probe(client, agent_id, study).await?;
        if artifact_list_has_study_json(study, &artifacts) {
            return Ok(artifacts);
        }
        if i + 1 < attempts {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
    }
    refresh_artifacts_with_json_probe(client, agent_id, study).await
}

fn json_followup_prompt(study: u32, artifacts: &[Artifact], attempt: u32) -> String {
    let listed: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    match attempt {
        1 => format!(
            "The Cloud Agents artifact list currently has: {listed:?}\n\n\
             There is **no** `artifacts/{study}.json`. Write that file NOW with the complete \
             study JSON (audience, section_style, section_images, packed slides). \
             Path must be exactly `artifacts/{study}.json` (not under assets/). \
             Do not regenerate PNGs unless missing.",
        ),
        2 => format!(
            "List Artifacts still has no `artifacts/{study}.json` (only: {listed:?}).\n\n\
             The host CLI downloads via the Cloud Agents API — files not in the list cannot be \
             retrieved even if they exist elsewhere in the VM.\n\n\
             Run shell commands NOW to register the JSON:\n\
             ```\n\
             mkdir -p artifacts\n\
             test -f artifacts/{study}.json || cp /agent/artifacts/{study}.json artifacts/{study}.json 2>/dev/null || true\n\
             test -f artifacts/{study}.json || cp /opt/cursor/artifacts/{study}.json artifacts/{study}.json 2>/dev/null || true\n\
             ls -la artifacts/{study}.json\n\
             ```\n\
             If copy fails, use Write to create `artifacts/{study}.json` with the full study JSON. \
             Do not regenerate PNGs.",
        ),
        _ => format!(
            "CRITICAL: `artifacts/{study}.json` is STILL missing from List Artifacts ({listed:?}).\n\n\
             Use the Write tool to overwrite `artifacts/{study}.json` with the complete study JSON. \
             After writing, run `ls -la artifacts/{study}.json` in shell. \
             The file must appear in the artifact API. Do not end until it does.",
        ),
    }
}

/// Follow-up until `{study}.json` is listable or downloadable (up to 3 nudges + polling).
/// Youth only — adult uses [`ensure_adult_section_tree_artifact`].
async fn ensure_study_json_artifact(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
    artifacts: &[Artifact],
    stream: bool,
) -> Result<Vec<Artifact>> {
    let mut artifacts = artifacts.to_vec();
    for attempt in 1..=3u32 {
        if artifact_list_has_study_json(study, &artifacts) {
            return Ok(artifacts);
        }
        eprintln!(
            "  Artifacts present but missing {study}.json — JSON follow-up {attempt}/3…"
        );
        let nudge = json_followup_prompt(study, &artifacts, attempt);
        let follow = client.create_followup(agent_id, &nudge).await?;
        println!("  JSON follow-up run {}", follow.run.id);
        client.wait_run(agent_id, &follow.run.id, stream).await?;
        artifacts = poll_artifacts_for_json(client, agent_id, study, 8, 3).await?;
    }
    Ok(artifacts)
}

/// Adult follow-up: require section folders under `artifacts/{study}/`, never monolith JSON.
async fn ensure_adult_section_tree_artifact(
    client: &CursorClient,
    agent_id: &str,
    study: u32,
    artifacts: &[Artifact],
    stream: bool,
) -> Result<Vec<Artifact>> {
    let mut artifacts = artifacts.to_vec();
    for attempt in 1..=3u32 {
        if artifact_list_has_adult_section_tree(study, &artifacts) {
            return Ok(artifacts);
        }
        let listed: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        eprintln!(
            "  Artifacts present but missing section tree artifacts/{study}/ — \
             section follow-up {attempt}/3…"
        );
        let nudge = format!(
            "STOP. Do **not** write artifacts/{study}.json.\n\n\
             Adult deliverable is a **complete** section folder tree — front matter alone \
             is NOT enough. Create these paths NOW (Write/Shell):\n\
             1. artifacts/{study}/manifest.toml\n\
             2. artifacts/{study}/01-title/ … through 08-introduccion/\n\
             3. artifacts/{study}/NN-tema-1/header.txt (+ definicion-*.txt)\n\
             4. artifacts/{study}/NN-tema-1-A/ (title.txt, body.txt, texto-biblico.txt) \
                and remaining A/B points + temas through RESUMEN GENERAL.\n\
             5. Lectura blocks MUST start with `CITA: …` then verse lines.\n\n\
             Incomplete (title/lectura only) will be rejected. Currently listed: {listed:?}\n\
             Chat text does not count. List Artifacts must show tema folders.",
        );
        let follow = client.create_followup(agent_id, &nudge).await?;
        println!("  section follow-up run {}", follow.run.id);
        client.wait_run(agent_id, &follow.run.id, stream).await?;
        artifacts = client.list_artifacts(agent_id).await?;
        for _ in 0..6 {
            if artifact_list_has_adult_section_tree(study, &artifacts) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            artifacts = client.list_artifacts(agent_id).await?;
        }
    }
    if !artifact_list_has_adult_section_tree(study, &artifacts) {
        bail!(
            "adult prepare: cloud agent never wrote section folders under \
             artifacts/{study}/ (after 3 follow-ups). Re-run the same command to resume. \
             Listed: {:?}",
            artifacts.iter().map(|a| a.path.as_str()).collect::<Vec<_>>()
        );
    }
    Ok(artifacts)
}

fn build_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    match req.audience {
        Audience::Youth => build_youth_cloud_prepare_prompt(req, host_images),
        Audience::Adult => build_adult_cloud_prepare_prompt(req, host_images),
    }
}

fn proximo_prompt_block(req: &PrepareRequest) -> Result<String> {
    if req.omit_proximo {
        Ok("Próximo: **omit** (last study — no próximo object, or null).".to_string())
    } else if let Some(np) = req.next_pages {
        Ok(format!(
            "Próximo: read from the **next** study’s title page \
             (attached image of the first page of pages {}–{}). \
             Put número, título, base bíblica in JSON `proximo`.",
            np.0, np.1
        ))
    } else {
        bail!("próximo metadata missing and no next_pages")
    }
}

fn build_youth_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    let aud = req.audience.as_str();
    let study = req.study;
    let proximo_block = proximo_prompt_block(req)?;
    let style_block = style_prompt_block(study);
    let base = format!("studies/{aud}");

    if host_images {
        return Ok(format!(
            r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church Bible study.

Follow PREPARE_STUDY.md and AGENTS.md (full text below). Do **NOT** build a .pptx.
Do **NOT** generate images — the host CLI generates section PNGs after you finish.

## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- Some attachments may be **vertical multi-page composites** (top→bottom = reading order; yellow tick marks the page band). Read **every** band — do not skip stacked pages.
- {proximo_block}

## Deliverable — ONE file only
Use Write/Shell to create:
1. `artifacts/{study}.json`  ← **required**; not under `artifacts/assets/`

JSON must include `"audience": "{aud}"`, three `puntos`, and the assigned `section_style`.
You may omit `section_images` (host stamps paths). Include `section_style` from the recipe below.

## Section style (for JSON only — do not render images)
{style_block}

## Rules
- {verbatim}
- Merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- End as soon as `artifacts/{study}.json` exists. Speed matters — JSON only.

---
# PREPARE_STUDY.md

{prepare}

---
# AGENTS.md (rules)

{agents}
"#,
            p0 = req.pages.0,
            p1 = req.pages.1,
            prepare = PREPARE_STUDY_MD,
            agents = AGENTS_MD,
            verbatim = VERBATIM_HARD,
        ));
    }

    Ok(format!(
        r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church Bible study.

Follow PREPARE_STUDY.md and AGENTS.md (full text below). Do **NOT** build a .pptx.

## Inputs
- Estudio **{study}** (audience={aud})
- Content pages **{p0}–{p1}** are attached as images (in order).
- Some attachments may be **vertical multi-page composites** (top→bottom = reading order; yellow tick marks the page band). Read **every** band — do not skip stacked pages.
- {proximo_block}

## Deliverables — write under `artifacts/` (mandatory)
You are in **agent mode**. Do **not** only plan or narrate. Use Write / Shell / image tools
to create real files. The host CLI downloads via List Artifacts — if the directory is empty,
the job **fails**. Pasting JSON in chat is not enough.

Exact paths (mkdir -p `artifacts` first if needed):
1. `artifacts/{study}.json`  ← **required**; do **not** put this under `artifacts/assets/`
2. `artifacts/{study}-section1.png` (or `artifacts/assets/{study}-section1.png`)
3. `artifacts/{study}-section2.png` (or `artifacts/assets/{study}-section2.png`)
4. `artifacts/{study}-section3.png` (or `artifacts/assets/{study}-section3.png`)

JSON must include `"audience": "{aud}"`, `section_images` pointing at the three PNG paths
(as `{base}/media/{study}-section{{1,2,3}}.png`), and the assigned `section_style`.

## Section images — HARD
{style_block}

Also: 16:9 ≈1408×768; no text/logos/watermarks/yellow dashed arcs; calm left for title.
**UNIQUE SCENES (HARD):** `section1`, `section2`, and `section3` must be **three different
illustrations** (same design family, different subjects). Never copy one PNG to the other paths.
Generate the three PNGs with the image tool (or equivalent) and save them to the paths above.

## Rules
- {verbatim}
- Merge cross-page cuts; exclude Ideas para el maestro / Preguntas.
- Pack body ~360–400 chars; Lectura/Texto = whole verses only.
- End only after artifact list would show `{study}.json` plus three section PNGs.

When finished, confirm the four artifact paths and `section_style.id`.

---
# PREPARE_STUDY.md

{prepare}

---
# AGENTS.md (rules)

{agents}
"#,
        p0 = req.pages.0,
        p1 = req.pages.1,
        prepare = PREPARE_STUDY_MD,
        agents = AGENTS_MD,
        verbatim = VERBATIM_HARD,
    ))
}

fn build_adult_cloud_prepare_prompt(req: &PrepareRequest, host_images: bool) -> Result<String> {
    let study = req.study;
    let proximo_block = proximo_prompt_block(req)?;
    let resume = crate::content::adult_resume_hint(study).unwrap_or_default();
    let resume_block = if resume.is_empty() {
        String::new()
    } else {
        format!("\n{resume}\n")
    };

    let layout = format!(
        r#"## Deliverable — SECTION FOLDERS (not a monolith JSON first)
Write files **incrementally** under `artifacts/{study}/` so the host can resume
after a failure and show progress per section.

Exact layout (same as GENERATE_STUDY_AGENT / studies/adult/7):

```
artifacts/{study}/
  manifest.toml
  images/                    ← scenic + definición PNGs (flat basenames)
  01-title/                  ← title.txt + base_biblica.txt
  02-lectura/                ← block-01.txt …
  03-objetivos/content.txt
  04-pensamiento/content.txt
  05-texto-aureo/            ← texto.txt + cita.txt
  06-ensenanza/content.txt
  07-datos/content.txt
  08-introduccion/slides.txt
  09-tema-1/                 ← titulo.txt, rango.txt, definicion-*.txt when printed
  10-tema-1-A/               ← titulo.txt, body.txt (or slides.txt), texto_biblico…
  …
  NN-proximo/                ← omit if last study / no next title
```

**HARD order:** create `manifest.toml`, then `01-title`, then `02-lectura`, …
Finish each folder (Write the .txt files) **before** starting the next.
Do **not** wait until the end to dump everything — the host polls List Artifacts
and shows each section as it appears.

Optional: also write `artifacts/{study}.json` at the very end if easy — not required
when the section tree is complete.
"#
    );

    let images_block = if host_images {
        "Do **NOT** generate images — the host CLI generates scenic/definición PNGs after text sections exist.".to_string()
    } else {
        format!(
            r#"## Images — under `artifacts/{study}/images/` (after text sections)
16:9 ≈1408×768. Unique scene per file. Basenames:
intro-header.png, tema-N.png, ab-N{{Letter}}.png, definicion-N.png (when that tema has a printed box).
"#
        )
    };

    Ok(format!(
        r#"You are a no-repo Cursor Cloud Agent preparing a Mount Zion Church **adult** Bible study.

Follow AGENTS.md adult HARD rules. Do **NOT** build a .pptx.
Adult source of truth is the **section folder tree** (not youth JSON).

## Inputs
- Estudio **{study}** (audience=adult)
- Content pages **{p0}–{p1}** attached as images (order). Composites = top→bottom bands.
- {proximo_block}
{resume_block}
{layout}

{images_block}

## Rules
- {verbatim}
- Merge two-column / cross-page cuts; skip Ideas / Afianzamiento / Preguntas / RESUMEN body.
- Lectura chapter jumps keep printed `5:1` (not bare `1`).
- Definiciones: only from printed «Definiciones y etimología» boxes; else omit those files.
- Variable temas/points: one folder per printed TEMA and lettered point — do not invent or drop.
- **Texto Bíblico (HARD — gold estudio 8):** TEMA image chrome uses tema `RANGO` only. \
Each `tema-N-X/texto-biblico.txt` must be **that point’s verses only** (same cita as the \
A/B image under the gray line). Never copy the full TEMA range into every A/B Texto.
- End when the section tree under `artifacts/{study}/` is complete{img_end}.

---
# AGENTS.md (rules)

{agents}
"#,
        p0 = req.pages.0,
        p1 = req.pages.1,
        agents = AGENTS_MD,
        verbatim = VERBATIM_HARD,
        img_end = if host_images {
            ""
        } else {
            " and required scenic PNGs exist under images/"
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_dest() -> Deliverables {
        Deliverables {
            json: PathBuf::from("/root/studies/youth/17.json"),
            img1: PathBuf::from("/root/studies/youth/media/17-section1.png"),
            img2: PathBuf::from("/root/studies/youth/media/17-section2.png"),
            img3: PathBuf::from("/root/studies/youth/media/17-section3.png"),
        }
    }

    fn art(path: &str) -> Artifact {
        Artifact {
            path: path.to_string(),
            size_bytes: None,
        }
    }

    #[test]
    fn maps_adult_jpg_assets_case_insensitive() {
        let root = PathBuf::from("/repo");
        let arts = vec![
            art("artifacts/4.json"),
            art("artifacts/assets/intro-header.jpg"),
            art("artifacts/assets/tema-1.jpg"),
            art("artifacts/assets/tema-2.jpg"),
            art("artifacts/assets/tema-3.jpg"),
            art("artifacts/assets/ab-1A.jpg"),
            art("artifacts/assets/ab-1B.jpg"),
            art("artifacts/assets/ab-2A.jpg"),
            art("artifacts/assets/ab-2B.jpg"),
            art("artifacts/assets/ab-3A.jpg"),
            art("artifacts/assets/ab-3B.jpg"),
            // no definición — should warn but succeed
        ];
        let resolved = resolve_adult_artifacts(4, &arts, &root).expect("map");
        assert_eq!(resolved.len(), 1 + 10); // json + intro + 3 tema + 6 ab
        assert!(resolved.iter().any(|(_, d)| d.ends_with("ab-1A.png")));
        assert!(resolved.iter().any(|(src, _)| src.ends_with("ab-1A.jpg")));
    }

    #[test]
    fn adult_incomplete_scenic_art_still_resolves() {
        let root = PathBuf::from("/repo");
        let arts = vec![
            art("artifacts/7.json"),
            art("artifacts/assets/intro-header.jpg"),
        ];
        let resolved = resolve_adult_artifacts(7, &arts, &root).expect("lenient");
        assert_eq!(resolved.len(), 2);
        assert!(resolved.iter().any(|(_, d)| d.ends_with("intro-header.png")));
    }

    #[test]
    fn adult_without_json_or_sections_still_errors() {
        let root = PathBuf::from("/repo");
        let arts = vec![art("artifacts/assets/intro-header.jpg")];
        assert!(resolve_adult_artifacts(7, &arts, &root).is_err());
    }

    #[test]
    fn maps_adult_section_tree_without_json() {
        let root = PathBuf::from("/repo");
        let arts = vec![
            art("artifacts/8/manifest.toml"),
            art("artifacts/8/01-title/title.txt"),
            art("artifacts/8/01-title/base_biblica.txt"),
            art("artifacts/8/02-lectura/block-01.txt"),
            art("artifacts/assets/intro-header.png"),
        ];
        let resolved = resolve_adult_artifacts(8, &arts, &root).expect("section tree");
        assert!(resolved
            .iter()
            .any(|(_, d)| d.ends_with("studies/adult/8/manifest.toml")
                || d.ends_with("adult/8/manifest.toml")));
        assert!(resolved
            .iter()
            .any(|(_, d)| d.ends_with("01-title/title.txt")));
        assert!(resolved
            .iter()
            .any(|(_, d)| d.ends_with("intro-header.png")));
        assert!(!resolved.iter().any(|(_, d)| d.ends_with("8.json")));
    }

    #[test]
    fn adult_section_tree_complete_requires_tema_folders() {
        let front_only = vec![
            art("artifacts/11/manifest.toml"),
            art("artifacts/11/01-title/title.txt"),
            art("artifacts/11/02-lectura/block-01.txt"),
            art("artifacts/11/05-texto-aureo/texto.txt"),
        ];
        assert!(!artifact_list_has_adult_section_tree(11, &front_only));

        let with_temas = vec![
            art("artifacts/11/manifest.toml"),
            art("artifacts/11/09-tema-1/header.txt"),
            art("artifacts/11/10-tema-1-A/body.txt"),
        ];
        assert!(artifact_list_has_adult_section_tree(11, &with_temas));
    }

    #[test]
    fn maps_adult_variable_tema_point_images() {
        // Estudio 4 shape: 2 temas, A–D + A–B — must not require tema-3 / ab-3*.
        let root = PathBuf::from("/repo");
        let arts = vec![
            art("artifacts/4.json"),
            art("artifacts/assets/ab-1A.jpg"),
            art("artifacts/assets/ab-1B.jpg"),
            art("artifacts/assets/ab-1C.jpg"),
            art("artifacts/assets/ab-1D.jpg"),
            art("artifacts/assets/ab-2A.jpg"),
            art("artifacts/assets/ab-2B.jpg"),
            art("artifacts/assets/definicion-1.jpg"),
            art("artifacts/assets/intro-header.jpg"),
            art("artifacts/assets/tema-1.jpg"),
            art("artifacts/assets/tema-2.jpg"),
        ];
        let resolved = resolve_adult_artifacts(4, &arts, &root).expect("map variable");
        assert!(resolved.iter().any(|(_, d)| d.ends_with("ab-1C.png")));
        assert!(resolved.iter().any(|(_, d)| d.ends_with("ab-1D.png")));
        assert!(!resolved.iter().any(|(_, d)| d.ends_with("tema-3.png")));
        assert_eq!(resolved.len(), 11); // json + 10 images
    }

    #[test]
    fn normalize_adult_agent_json_coerces_image_field_quirks() {
        let dir = std::env::temp_dir().join(format!(
            "lamad-normalize-adult-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("6.json");
        let raw = serde_json::json!({
            "numero": 6,
            "titulo": "T",
            "definicion_images": {"2": "b.png", "1": "a.png"},
            "scenic_images": {"intro-header": "x.png", "tema-1": "t.png"},
            "section_images": ["y.png"],
            "section_style": {"id": "nope"}
        });
        std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
        assert!(normalize_adult_agent_json(&path).unwrap());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["definicion_images"],
            serde_json::json!(["a.png", "b.png"])
        );
        assert!(v.get("scenic_images").is_none());
        assert!(v.get("section_images").is_none());
        assert!(v.get("section_style").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_adult_agent_json_salvages_legacy_a_b_without_texto_biblico() {
        let dir = std::env::temp_dir().join(format!(
            "lamad-normalize-adult-legacy-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("8.json");
        let raw = serde_json::json!({
            "estudio": 8,
            "titulo": "T",
            "base_biblica": "2 S 6",
            "lectura_slides": ["2 Samuel 6\n\n1 David reunió…"],
            "objetivos": ["a", "b", "c"],
            "pensamiento_central": "p",
            "texto_aureo": {"texto": "q", "cita": "Jn 15:7"},
            "ensenanza": "e",
            "datos_generales": {
                "autor": "a",
                "personajes": "p",
                "fecha": "f",
                "lugar": "l",
                "enseñanza": "dup"
            },
            "introduccion_slides": ["intro"],
            "packed_slides": {"introduccion": ["x"]},
            "temas": [{
                "titulo": "I. TEMA",
                "rango": "2 S 6:1-11",
                "definiciones": ["\"Arca\". Caja sagrada."],
                "texto_biblico_slides": ["2 Samuel 6:1-11\n\n1 David reunió…"],
                "A": {
                    "titulo": "A. Point",
                    "texto_slides": ["body A"]
                },
                "B": {
                    "titulo": "B. Point",
                    "texto_slides": ["body B"]
                }
            }],
            "proximo": {"numero": 9, "titulo": "N", "base_biblica": "x"}
        });
        std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
        assert!(normalize_adult_agent_json(&path).unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let study: crate::model::adult_study::AdultStudy =
            serde_json::from_str(&text).expect("parse after normalize");
        assert_eq!(study.numero.to_string(), "8");
        assert_eq!(study.lectura_antifonal.len(), 1);
        assert_eq!(study.temas.len(), 1);
        assert_eq!(study.temas[0].puntos.len(), 2);
        // Must NOT copy tema-wide `2 S 6:1-11` into every point (gold 8).
        assert!(
            study.temas[0].puntos[0].texto_biblico.cita.is_empty(),
            "legacy salvage must not inject tema-wide texto_biblico into A/B"
        );
        assert_eq!(study.temas[0].definiciones[0].termino, "Arca");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_v1_artifact_list_items_and_size_bytes() {
        let raw = r#"{
          "items": [
            {"path": "artifacts/assets/23-section1.png", "sizeBytes": 12, "updatedAt": "2026-01-01T00:00:00.000Z"},
            {"path": "artifacts/23.json", "sizeBytes": 99}
          ]
        }"#;
        let list: crate::agent::client::ArtifactList =
            serde_json::from_str(raw).expect("parse");
        assert_eq!(list.artifacts.len(), 2);
        assert_eq!(list.artifacts[0].path, "artifacts/assets/23-section1.png");
        assert_eq!(list.artifacts[0].size_bytes, Some(12));
        assert_eq!(list.artifacts[1].path, "artifacts/23.json");
        assert_eq!(list.artifacts[1].size_bytes, Some(99));
    }

    #[test]
    fn map_artifact_matches_assets_subdir_pngs() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/assets/17-section2.png", &dest),
            Some(dest.img2.clone())
        );
    }

    #[test]
    fn artifact_list_detects_json_basename() {
        assert!(artifact_list_has_study_json(
            23,
            &[art("artifacts/23.json")]
        ));
        assert!(artifact_list_has_study_json(
            23,
            &[art("artifacts/assets/23.json")]
        ));
        assert!(!artifact_list_has_study_json(
            23,
            &[art("artifacts/assets/23-section1.png")]
        ));
    }

    #[test]
    fn map_artifact_matches_exact_json_name() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/17.json", &dest),
            Some(dest.json.clone())
        );
        assert_eq!(
            map_artifact(17, "17.json", &dest),
            Some(dest.json.clone())
        );
    }

    #[test]
    fn map_artifact_rejects_json_for_a_different_study() {
        let dest = fake_dest();
        assert_eq!(map_artifact(17, "artifacts/18.json", &dest), None);
    }

    #[test]
    fn map_artifact_matches_exact_and_bare_section_names() {
        let dest = fake_dest();
        assert_eq!(
            map_artifact(17, "artifacts/17-section1.png", &dest),
            Some(dest.img1.clone())
        );
        assert_eq!(
            map_artifact(17, "artifacts/section2.png", &dest),
            Some(dest.img2.clone())
        );
        assert_eq!(
            map_artifact(17, "artifacts/17-section3.png", &dest),
            Some(dest.img3.clone())
        );
        // JPG (common agent miss) maps to the same PNG destinations.
        assert_eq!(
            map_artifact(1, "artifacts/assets/1-section1.jpg", &dest),
            Some(dest.img1.clone())
        );
        assert_eq!(
            map_artifact(1, "artifacts/assets/1-section2.jpeg", &dest),
            Some(dest.img2.clone())
        );
    }

    #[test]
    fn map_artifact_rejects_unrelated_names() {
        let dest = fake_dest();
        assert_eq!(map_artifact(17, "artifacts/random.png", &dest), None);
        assert_eq!(map_artifact(17, "artifacts/notes.txt", &dest), None);
    }

    #[test]
    fn resolve_artifacts_prefers_exact_names() {
        let dest = fake_dest();
        let artifacts = vec![
            art("artifacts/17-section2.png"),
            art("artifacts/17.json"),
            art("artifacts/17-section1.png"),
            art("artifacts/17-section3.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        assert_eq!(resolved.len(), 4);
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/17.json"], dest.json);
        assert_eq!(by_dest["artifacts/17-section1.png"], dest.img1);
        assert_eq!(by_dest["artifacts/17-section2.png"], dest.img2);
        assert_eq!(by_dest["artifacts/17-section3.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_falls_back_positionally_for_unnamed_pngs() {
        let dest = fake_dest();
        // No study-number prefix anywhere — must fall back to sorted order.
        let artifacts = vec![
            art("artifacts/17.json"),
            art("artifacts/b.png"),
            art("artifacts/a.png"),
            art("artifacts/c.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/a.png"], dest.img1);
        assert_eq!(by_dest["artifacts/b.png"], dest.img2);
        assert_eq!(by_dest["artifacts/c.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_never_double_counts_an_exact_match() {
        let dest = fake_dest();
        // `17-section1.png` should fill img1 exactly once; the leftover
        // unnamed PNG should NOT also be assigned to img1.
        let artifacts = vec![
            art("artifacts/17.json"),
            art("artifacts/17-section1.png"),
            art("artifacts/leftover.png"),
            art("artifacts/17-section3.png"),
        ];
        let resolved = resolve_artifacts(17, &artifacts, &dest).expect("resolves");
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/17-section1.png"], dest.img1);
        assert_eq!(by_dest["artifacts/leftover.png"], dest.img2);
        assert_eq!(by_dest["artifacts/17-section3.png"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_accepts_jpg_section_images() {
        let dest = fake_dest();
        let artifacts = vec![
            art("artifacts/1.json"),
            art("artifacts/assets/1-section1.jpg"),
            art("artifacts/assets/1-section2.jpg"),
            art("artifacts/assets/1-section3.jpg"),
        ];
        // Remap fake_dest study number: map_artifact uses study arg, not dest paths.
        let resolved = resolve_artifacts(1, &artifacts, &dest).expect("resolves jpg");
        assert_eq!(resolved.len(), 4);
        let by_dest: std::collections::HashMap<_, _> = resolved.into_iter().collect();
        assert_eq!(by_dest["artifacts/assets/1-section1.jpg"], dest.img1);
        assert_eq!(by_dest["artifacts/assets/1-section2.jpg"], dest.img2);
        assert_eq!(by_dest["artifacts/assets/1-section3.jpg"], dest.img3);
    }

    #[test]
    fn resolve_artifacts_errors_with_clear_found_vs_expected() {
        let dest = fake_dest();
        let artifacts = vec![art("artifacts/17.json"), art("artifacts/17-section1.png")];
        let err = resolve_artifacts(17, &artifacts, &dest).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("section2.png"), "{msg}");
        assert!(msg.contains("section3.png"), "{msg}");
        assert!(msg.contains("17-section1.png"), "{msg}");
    }
}
