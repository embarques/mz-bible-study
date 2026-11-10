//! Adult section-folder layout under `studies/adult/{N}/`.
//!
//! ```text
//! studies/adult/8/
//!   manifest.toml
//!   images/                  # scenic + definición PNGs (builder paths)
//!   01-title/
//!   02-lectura/
//!   03-objetivos/
//!   04-pensamiento/
//!   05-texto-aureo/
//!   06-ensenanza/
//!   07-datos/
//!   08-introduccion/
//!   09-tema-1/               # header + definiciones
//!   10-tema-1-A/             # point body
//!   …
//!   99-proximo/              # omit on last study
//! ```
//!
//! Folder names are sorted; the kind is taken from the suffix after `NN-`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::adult_study::{
    AdultScenicImages, AdultStudy, DatosGenerales, Definicion, PuntoAdult, TemaAdult, TextoAureo,
};
use crate::model::study::{Lectura, Numero, Proximo, StringOrList};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultManifest {
    pub numero: u32,
    pub titulo: String,
    pub base_biblica: Vec<String>,
    #[serde(default)]
    pub omit_proximo: bool,
    #[serde(default)]
    pub audience: Option<String>,
}

/// `studies/adult/{N}/`
pub fn adult_study_folder(study: u32) -> Result<PathBuf> {
    Ok(paths::studies_dir(crate::job::Audience::Adult)?.join(study.to_string()))
}

/// True when a **loadable** adult tree is on disk (manifest + at least one tema).
/// Manifest-only / front-matter stubs do **not** count — that caused prepare to
/// skip follow-ups and then crash on incomplete estudio 11 extracts.
pub fn has_adult_sections(study: u32) -> bool {
    let Ok(dir) = adult_study_folder(study) else {
        return false;
    };
    if !dir.join("manifest.toml").is_file() {
        return false;
    }
    adult_tree_has_tema(&dir)
}

/// True when `studies/adult/{N}/` has at least one `NN-tema-N` header folder
/// (e.g. `09-tema-1`). Point-only folders like `10-tema-1-A` do not count alone.
pub fn adult_tree_has_tema(study_dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(study_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        // After `NN-`: `tema-1` (header) vs `tema-1-A` (point).
        let Some((_, kind)) = s.split_once('-') else {
            continue;
        };
        if let Some(rest) = kind.strip_prefix("tema-") {
            if !rest.is_empty() && !rest.contains('-') {
                return true;
            }
        }
    }
    false
}

/// Fixed front-matter kinds (before variable `tema-*` folders), in order.
pub const ADULT_FRONT_MATTER: &[&str] = &[
    "title",
    "lectura",
    "objetivos",
    "pensamiento",
    "texto-aureo",
    "ensenanza",
    "datos",
    "introduccion",
];

/// Section folder names present on disk (e.g. `01-title`, `09-tema-1`), sorted.
pub fn present_section_dir_names(study: u32) -> Result<Vec<String>> {
    let dir = adult_study_folder(study)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = list_section_dirs(&dir)?
        .into_iter()
        .map(|(_, _, path)| {
            path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

/// Front-matter kinds still missing (by suffix after `NN-`).
pub fn missing_front_matter_kinds(study: u32) -> Result<Vec<&'static str>> {
    let present = present_section_dir_names(study)?;
    let kinds: Vec<String> = present
        .iter()
        .filter_map(|n| n.split_once('-').map(|(_, k)| k.to_string()))
        .collect();
    Ok(ADULT_FRONT_MATTER
        .iter()
        .copied()
        .filter(|k| !kinds.iter().any(|have| have == k))
        .collect())
}

/// Human-readable resume hint for the cloud agent (empty if nothing on disk yet).
pub fn adult_resume_hint(study: u32) -> Result<String> {
    let dir = adult_study_folder(study)?;
    let mut lines = Vec::new();
    if dir.join("manifest.toml").is_file() {
        lines.push("manifest.toml".into());
    }
    for name in present_section_dir_names(study)? {
        lines.push(format!("{name}/"));
    }
    if lines.is_empty() {
        return Ok(String::new());
    }
    let missing = missing_front_matter_kinds(study)?;
    let mut out = format!(
        "## RESUME — already on host disk under studies/adult/{study}/ (do NOT rewrite these)\n"
    );
    for l in &lines {
        out.push_str(&format!("- {l}\n"));
    }
    out.push_str(
        "\nWrite **only** missing section folders under `artifacts/{study}/` \
         (same layout). Keep numbers sequential after what already exists.\n",
    );
    if !missing.is_empty() {
        out.push_str(&format!("Missing front-matter kinds: {:?}\n", missing));
    }
    out.push_str(
        "If temas/points are incomplete, continue from the next tema/point after \
         the last existing `NN-tema-*` folder. Prefer section folders over a monolith JSON.\n",
    );
    out.push_str(
        "\n## Images — REPLACE any PLACEHOLDER scenic under `artifacts/{study}/images/`\n\
         Write unique 16:9 PNGs: intro-header.png, tema-N.png, ab-N{{Letter}}.png \
         (and definicion-N.png when that tema has printed Definiciones). \
         Never reuse the same scene bytes across slots.\n",
    );
    Ok(out)
}

/// Images live under `{study_dir}/images/` (flat basenames the builder already expects).
pub fn adult_images_dir(study_dir: &Path) -> PathBuf {
    study_dir.join("images")
}

/// Write AdultStudy → section tree + images dir stubs. Does not move binary images;
/// call after images exist (or copy them into `images/`).
pub fn write_adult_sections(study: &AdultStudy, study_dir: &Path) -> Result<()> {
    fs::create_dir_all(study_dir)?;
    fs::create_dir_all(adult_images_dir(study_dir))?;

    let numero: u32 = match &study.numero {
        Numero::Int(n) if *n >= 0 && *n <= u32::MAX as i64 => *n as u32,
        Numero::Text(s) => s
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .with_context(|| format!("numero {:?} is not a study number", study.numero))?,
        other => bail!("unsupported adult numero: {other}"),
    };

    let manifest = AdultManifest {
        numero,
        titulo: study.titulo.clone(),
        base_biblica: study.base_biblica_lines(),
        omit_proximo: false,
        audience: Some("adult".into()),
    };
    fs::write(
        study_dir.join("manifest.toml"),
        toml::to_string_pretty(&manifest).context("serialize manifest")?,
    )?;

    let mut idx = 1u32;

    // title
    {
        let dir = section_dir(study_dir, idx, "title");
        idx += 1;
        write_text(&dir.join("title.txt"), &study.titulo)?;
        write_text(
            &dir.join("base_biblica.txt"),
            &study.base_biblica_lines().join("\n"),
        )?;
    }

    // lectura (one folder; multiple blocks as block-01.txt …)
    {
        let dir = section_dir(study_dir, idx, "lectura");
        idx += 1;
        for (i, block) in study.lectura_antifonal.iter().enumerate() {
            write_text(
                &dir.join(format!("block-{:02}.txt", i + 1)),
                &format_lectura(block),
            )?;
        }
    }

    // objetivos
    {
        let dir = section_dir(study_dir, idx, "objetivos");
        idx += 1;
        write_text(&dir.join("content.txt"), &study.objetivos.join("\n"))?;
    }

    // pensamiento
    {
        let dir = section_dir(study_dir, idx, "pensamiento");
        idx += 1;
        write_text(&dir.join("content.txt"), &study.pensamiento_central)?;
    }

    // texto-aureo
    {
        let dir = section_dir(study_dir, idx, "texto-aureo");
        idx += 1;
        write_text(&dir.join("texto.txt"), &study.texto_aureo.texto)?;
        write_text(&dir.join("cita.txt"), &study.texto_aureo.cita)?;
    }

    // ensenanza
    {
        let dir = section_dir(study_dir, idx, "ensenanza");
        idx += 1;
        write_text(&dir.join("content.txt"), &study.ensenanza)?;
    }

    // datos
    {
        let dir = section_dir(study_dir, idx, "datos");
        idx += 1;
        write_text(
            &dir.join("content.txt"),
            &format!(
                "autor: {}\npersonajes: {}\nfecha: {}\nlugar: {}",
                study.datos_generales.autor,
                study.datos_generales.personajes,
                study.datos_generales.fecha,
                study.datos_generales.lugar
            ),
        )?;
    }

    // introduccion
    {
        let dir = section_dir(study_dir, idx, "introduccion");
        idx += 1;
        write_text(
            &dir.join("slides.txt"),
            &join_paragraphs(&study.introduccion_slides),
        )?;
    }

    for (ti, tema) in study.temas.iter().enumerate() {
        let tema_n = ti + 1;
        let dir = section_dir(study_dir, idx, &format!("tema-{tema_n}"));
        idx += 1;
        write_text(
            &dir.join("header.txt"),
            &format!("TITULO: {}\nRANGO: {}", tema.titulo, tema.rango),
        )?;
        for (di, def) in tema.definiciones.iter().enumerate() {
            let mut body = format!("TERMINO: {}\n", def.termino);
            if let Some(r) = &def.referencia {
                body.push_str(&format!("REFERENCIA: {r}\n"));
            }
            body.push_str("TEXTO:\n");
            body.push_str(&def.texto);
            write_text(&dir.join(format!("definicion-{:02}.txt", di + 1)), &body)?;
        }

        for punto in &tema.puntos {
            let dir = section_dir(
                study_dir,
                idx,
                &format!("tema-{tema_n}-{}", punto.letra),
            );
            idx += 1;
            write_text(&dir.join("title.txt"), &punto.titulo)?;
            write_text(
                &dir.join("body.txt"),
                &join_paragraphs(&punto.texto_slides),
            )?;
            write_text(
                &dir.join("texto-biblico.txt"),
                &format_lectura(&punto.texto_biblico),
            )?;
        }
    }

    // proximo
    {
        let dir = section_dir(study_dir, idx, "proximo");
        write_text(
            &dir.join("numero.txt"),
            &study.proximo.numero.to_string(),
        )?;
        write_text(&dir.join("title.txt"), &study.proximo.titulo)?;
        if let Some(base) = &study.proximo.base_biblica {
            write_text(&dir.join("base_biblica.txt"), &base.as_lines().join("\n"))?;
        }
    }

    let _ = idx;
    Ok(())
}

/// Load section tree → AdultStudy. Image paths stamped to `studies/adult/{N}/images/…`.
pub fn load_adult_sections(study: u32) -> Result<AdultStudy> {
    // Repair agent quirks (manifest shape, file names, dup CITA, próximo #) first.
    let _ = crate::content::adult_normalize::normalize_adult_section_tree(study)?;
    read_adult_sections(study)
}

fn read_adult_sections(study: u32) -> Result<AdultStudy> {
    let study_dir = adult_study_folder(study)?;
    let manifest_path = study_dir.join("manifest.toml");
    if !manifest_path.is_file() {
        bail!(
            "missing {} — create section content (see GENERATE_STUDY_AGENT.md) \
             or run with --generate / -g",
            manifest_path.display()
        );
    }
    let manifest: AdultManifest = toml::from_str(&fs::read_to_string(&manifest_path)?)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    if study == 0 || manifest.numero != study {
        bail!("{}: numero must match positive study number {study}", manifest_path.display());
    }

    let mut sections = list_section_dirs(&study_dir)?;
    sections.sort_by(|a, b| a.0.cmp(&b.0));

    let mut lectura_antifonal = Vec::new();
    let mut objetivos = Vec::new();
    let mut pensamiento_central = String::new();
    let mut texto_aureo = TextoAureo {
        texto: String::new(),
        cita: String::new(),
    };
    let mut ensenanza = String::new();
    let mut datos_generales = DatosGenerales {
        autor: String::new(),
        personajes: String::new(),
        fecha: String::new(),
        lugar: String::new(),
    };
    let mut introduccion_slides = Vec::new();
    let mut temas: Vec<(usize, TemaAdult)> = Vec::new();
    let mut puntos: Vec<(usize, String, PuntoAdult)> = Vec::new();
    let mut proximo = Proximo {
        numero: Numero::Int(0),
        titulo: String::new(),
        base_biblica: None,
    };
    let mut saw_proximo = false;
    let mut seen_kinds = std::collections::HashSet::new();

    for (_ord, kind, dir) in &sections {
        if !seen_kinds.insert(kind) {
            bail!("duplicate section kind {kind} under {}", study_dir.display());
        }
        match kind.as_str() {
            "title" => { /* title/base come from manifest */ }
            "lectura" => {
                let mut blocks = list_files_with_prefix(dir, "block-")?;
                blocks.sort();
                for b in blocks {
                    lectura_antifonal.push(parse_lectura(&fs::read_to_string(&b)?)
                        .with_context(|| format!("parse {}", b.display()))?);
                }
            }
            "objetivos" => {
                objetivos = read_nonempty_lines(&dir.join("content.txt"))?
                    .into_iter()
                    .map(|l| {
                        let t = l.trim();
                        let b = t.as_bytes();
                        let mut i = 0usize;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                        if i > 0 && i < b.len() && b[i] == b'.' {
                            t[i + 1..].trim_start().to_string()
                        } else {
                            t.to_string()
                        }
                    })
                    .collect();
            }
            "pensamiento" => {
                pensamiento_central = read_trimmed(&dir.join("content.txt"))?;
            }
            "texto-aureo" => {
                texto_aureo.texto = read_trimmed(&dir.join("texto.txt"))?;
                texto_aureo.cita = read_trimmed(&dir.join("cita.txt"))?;
            }
            "ensenanza" => {
                ensenanza = read_trimmed(&dir.join("content.txt"))?;
            }
            "datos" => {
                datos_generales = parse_datos(&read_trimmed(&dir.join("content.txt"))?)?;
            }
            "introduccion" => {
                introduccion_slides = split_paragraphs(&read_trimmed(&dir.join("slides.txt"))?);
            }
            "proximo" => {
                saw_proximo = true;
                // Folder numbering wins over printed scan badges.
                let expected = (study + 1) as i64;
                let n_txt = read_trimmed(&dir.join("numero.txt")).unwrap_or_default();
                let parsed = n_txt.parse::<i64>().ok();
                if parsed != Some(expected) || manifest.omit_proximo {
                    bail!("{}: Próximo requires numero.txt containing {expected} and omit_proximo=false", dir.display());
                }
                proximo.numero = Numero::Int(expected);
                let title_path = if dir.join("title.txt").is_file() {
                    dir.join("title.txt")
                } else {
                    dir.join("titulo.txt")
                };
                proximo.titulo = read_trimmed(&title_path)?;
                let base_path = dir.join("base_biblica.txt");
                if base_path.is_file() {
                    let lines = read_nonempty_lines(&base_path)?;
                    proximo.base_biblica = Some(StringOrList::Many(lines));
                }
                if proximo.titulo.is_empty() || proximo.base_biblica.as_ref()
                    .is_none_or(|base| base.as_lines().is_empty())
                {
                    bail!("{}: Próximo requires title.txt and base_biblica.txt", dir.display());
                }
            }
            other if other.starts_with("tema-") => {
                let rest = &other["tema-".len()..];
                if let Some((n_str, letra)) = rest.split_once('-') {
                    // tema-1-A
                    let tema_n: usize = n_str.parse().with_context(|| {
                        format!("bad tema point folder name: {other}")
                    })?;
                    let title = crate::content::adult_normalize::sanitize_ab_point_titulo(
                        &read_trimmed(&dir.join("title.txt"))?,
                    );
                    let body = split_paragraphs(&read_trimmed(&dir.join("body.txt"))?);
                    let texto_path = dir.join("texto-biblico.txt");
                    let texto_raw = fs::read_to_string(&texto_path)?;
                    let texto = parse_lectura(&super::adult_normalize::ensure_cita_prefix(&texto_raw))
                        .with_context(|| format!("parse {}", texto_path.display()))?;
                    puntos.push((
                        tema_n,
                        letra.to_string(),
                        PuntoAdult {
                            letra: letra.to_ascii_uppercase(),
                            titulo: title,
                            texto_slides: body,
                            texto_biblico: texto,
                        },
                    ));
                } else {
                    // tema-1
                    let tema_n: usize = rest.parse().with_context(|| {
                        format!("bad tema folder name: {other}")
                    })?;
                    let header = read_trimmed(&dir.join("header.txt"))?;
                    let (titulo, rango) = parse_tema_header(&header, dir)
                        .with_context(|| format!("parse {}", dir.join("header.txt").display()))?;
                    let mut defs = Vec::new();
                    let mut def_files = list_files_with_prefix(dir, "definicion-")?;
                    def_files.sort();
                    for f in def_files {
                        defs.push(parse_definicion(&fs::read_to_string(&f)?)?);
                    }
                    temas.push((
                        tema_n,
                        TemaAdult {
                            titulo,
                            rango,
                            definiciones: defs,
                            puntos: Vec::new(),
                        },
                    ));
                }
            }
            other => bail!("unknown section kind `{other}` in {}", dir.display()),
        }
    }

    temas.sort_by_key(|(n, _)| *n);
    for (index, (number, _)) in temas.iter().enumerate() {
        if *number != index + 1 {
            bail!("missing tema-{} header under {}", index + 1, study_dir.display());
        }
    }
    puntos.sort_by(|(a_n, a_l, _), (b_n, b_l, _)| a_n.cmp(b_n).then(a_l.cmp(b_l)));

    let mut tema_models: Vec<TemaAdult> = temas.into_iter().map(|(_, t)| t).collect();
    for (tema_n, _letra, punto) in puntos {
        let idx = tema_n
            .checked_sub(1)
            .ok_or_else(|| anyhow::anyhow!("tema index 0"))?;
        if idx >= tema_models.len() {
            bail!("point for tema-{tema_n} but that tema header folder is missing");
        }
        tema_models[idx].puntos.push(punto);
    }
    if tema_models.is_empty() {
        bail!("no tema sections found under {}", study_dir.display());
    }
    for (i, t) in tema_models.iter().enumerate() {
        if t.puntos.is_empty() {
            bail!("tema {} has no points (need tema-N-A …)", i + 1);
        }
        for (index, point) in t.puntos.iter().enumerate() {
            let expected = (b'A' + index.min(25) as u8) as char;
            if index >= 26 || !point.letra.eq_ignore_ascii_case(&expected.to_string()) {
                bail!("tema {}: expected point {expected}, found {}", i + 1, point.letra);
            }
        }
    }

    if manifest.omit_proximo && !saw_proximo {
        // placeholder — builder may still need a Proximo object; leave empty title
        proximo.titulo = String::new();
    } else if !saw_proximo && !manifest.omit_proximo {
        bail!(
            "missing 99-proximo/ (or set omit_proximo=true in manifest.toml for the last study)"
        );
    }

    let images = adult_images_dir(&study_dir);
    let rel_images = images.to_string_lossy();
    let mut ab_paths = Vec::new();
    for (ti, t) in tema_models.iter().enumerate() {
        for p in &t.puntos {
            ab_paths.push(format!("{rel_images}/ab-{}{}.png", ti + 1, p.letra));
        }
    }
    let scenic = AdultScenicImages {
        intro_header: format!("{rel_images}/intro-header.png"),
        tema: (1..=tema_models.len())
            .map(|i| format!("{rel_images}/tema-{i}.png"))
            .collect(),
        ab: ab_paths,
    };
    let definicion_images: Vec<String> = (1..=tema_models.len())
        .map(|i| format!("{rel_images}/definicion-{i}.png"))
        .collect();

    let model = AdultStudy {
        numero: Numero::Int(manifest.numero as i64),
        titulo: manifest.titulo,
        base_biblica: StringOrList::Many(manifest.base_biblica),
        lectura_antifonal,
        objetivos,
        pensamiento_central,
        texto_aureo,
        ensenanza,
        datos_generales,
        introduccion_slides,
        temas: tema_models,
        proximo,
        template: None,
        audience: Some("adult".into()),
        definicion_images: Some(definicion_images),
        scenic_images: Some(scenic),
    };
    validate_section_content(&model)?;
    Ok(model)
}

fn validate_section_content(model: &AdultStudy) -> Result<()> {
    let fields = [
        ("manifest titulo", model.titulo.as_str()),
        ("pensamiento/content.txt", model.pensamiento_central.as_str()),
        ("texto-aureo/texto.txt", model.texto_aureo.texto.as_str()),
        ("texto-aureo/cita.txt", model.texto_aureo.cita.as_str()),
        ("ensenanza/content.txt", model.ensenanza.as_str()),
        ("datos autor", model.datos_generales.autor.as_str()),
        ("datos personajes", model.datos_generales.personajes.as_str()),
        ("datos fecha", model.datos_generales.fecha.as_str()),
        ("datos lugar", model.datos_generales.lugar.as_str()),
    ];
    let mut errors: Vec<String> = fields.iter()
        .filter(|(_, value)| value.trim().is_empty())
        .map(|(name, _)| format!("{name} is required"))
        .collect();
    if model.titulo.contains(['/', '\\']) {
        errors.push("manifest titulo cannot contain a path separator".into());
    }
    let base = model.base_biblica.as_lines();
    if base.is_empty() || base.iter().any(|line| line.trim().is_empty()) {
        errors.push("manifest base_biblica needs nonempty citations".into());
    }
    if model.objetivos.len() != 3 || model.objetivos.iter().any(|s| s.trim().is_empty()) {
        errors.push("objetivos/content.txt needs exactly three nonempty objectives".into());
    }
    if model.lectura_antifonal.is_empty() {
        errors.push("lectura needs at least one block-*.txt".into());
    }
    if model.introduccion_slides.is_empty() {
        errors.push("introduccion/slides.txt needs text".into());
    }
    for (index, tema) in model.temas.iter().enumerate() {
        for point in &tema.puntos {
            if point.titulo.trim().is_empty() || point.texto_slides.is_empty() {
                errors.push(format!("tema-{}-{} needs a title and body", index + 1, point.letra));
            }
        }
    }
    if !errors.is_empty() {
        bail!("invalid adult study content:\n  {}", errors.join("\n  "));
    }
    Ok(())
}

/// Validate a manually supplied section tree without rewriting its source files.
pub fn validate_local_adult_study(study: u32) -> Result<AdultStudy> {
    let model = read_adult_sections(study)
        .with_context(|| format!("validate adult study {study}"))?;
    let scenic = model.scenic_images.as_ref().expect("section loader sets scenic paths");
    let paths: Vec<PathBuf> = std::iter::once(&scenic.intro_header)
        .chain(&scenic.tema).chain(&scenic.ab).map(PathBuf::from).collect();
    for path in &paths {
        if crate::content::is_placeholder(path) {
            bail!("{} is placeholder artwork", path.display());
        }
        let image = image::ImageReader::open(path)
            .with_context(|| format!("open scenic PNG {}", path.display()))?;
        if image.format() != Some(image::ImageFormat::Png) {
            bail!("{} must be a PNG", path.display());
        }
        image.decode().with_context(|| format!("decode scenic PNG {}", path.display()))?;
    }
    crate::build::assert_unique_image_files(&paths, "adult scenic images")?;
    Ok(model)
}

/// Print a short inventory of what lives under a study section folder.
pub fn print_section_inventory(study_dir: &Path) -> Result<()> {
    println!("  Content written:");
    let mut entries: Vec<String> = Vec::new();
    if study_dir.join("manifest.toml").is_file() {
        entries.push("manifest.toml".into());
    }
    let images = adult_images_dir(study_dir);
    if images.is_dir() {
        let mut pngs: Vec<_> = fs::read_dir(&images)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("png"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        pngs.sort();
        entries.push(format!("images/ ({} png)", pngs.len()));
        for p in pngs.iter().take(12) {
            entries.push(format!("  {p}"));
        }
        if pngs.len() > 12 {
            entries.push(format!("  … +{} more", pngs.len() - 12));
        }
    }
    let mut sections = list_section_dirs(study_dir)?;
    sections.sort_by_key(|(ord, _, _)| *ord);
    for (_ord, kind, path) in sections {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(kind);
        entries.push(format!("{name}/"));
    }
    for line in entries {
        println!("    {line}");
    }
    Ok(())
}

/// Ensure scenic (+ definición when needed) images exist under `{N}/images/`.
pub fn verify_adult_section_images(study: u32, model: &AdultStudy, root: &Path) -> Result<()> {
    let media = crate::agent::prepare::adult_media_dir(study, root);
    let scenic = crate::agent::prepare::adult_scenic_basenames_for(model);
    let mut missing = Vec::new();
    for name in &scenic {
        if !media.join(name).is_file() {
            missing.push(format!("{}/{}", media.display(), name));
        }
    }
    if !missing.is_empty() {
        bail!(
            "adult estudio {study} missing scenic images under {}:\n  {}",
            media.display(),
            missing.join("\n  ")
        );
    }
    for (i, tema) in model.temas.iter().enumerate() {
        if tema.definiciones.is_empty() {
            continue;
        }
        let name = format!("definicion-{}.png", i + 1);
        let p = media.join(&name);
        use crate::build::adult::{assert_study_definicion_card, definicion_card_page, DEFINICION_ROWS};
        for (page, definitions) in tema.definiciones.chunks(DEFINICION_ROWS).enumerate() {
            let card = definicion_card_page(&p, page);
            assert_study_definicion_card(
                &card,
                definitions.iter().map(|d| (d.termino.as_str(), d.texto.as_str())),
            ).with_context(|| format!("adult estudio {study}: verify {}", card.display()))?;
        }
    }
    Ok(())
}

/// Expand an existing adult JSON (+ optional flat media dir) into the section tree.
pub fn expand_json_to_sections(json_path: &Path, study: u32) -> Result<PathBuf> {
    // Coerce agent quirks (estudio/lectura_slides/A-B without texto_biblico, …).
    let _ = crate::agent::prepare::normalize_adult_agent_json(json_path)?;
    let text = fs::read_to_string(json_path)?;
    let model: AdultStudy = serde_json::from_str(&text)
        .with_context(|| format!("parse adult JSON {}", json_path.display()))?;
    let study_dir = adult_study_folder(study)?;
    write_adult_sections(&model, &study_dir)?;

    // Move/copy images from legacy flat study dir into images/
    let images = adult_images_dir(&study_dir);
    fs::create_dir_all(&images)?;
    for name in crate::agent::prepare::adult_basenames_for_study(&model) {
        let src_flat = study_dir.join(&name);
        let src_legacy_parent = json_path
            .parent()
            .map(|p| p.join(study.to_string()).join(&name));
        let dest = images.join(&name);
        if dest.is_file() {
            continue;
        }
        if src_flat.is_file() {
            fs::rename(&src_flat, &dest).or_else(|_| fs::copy(&src_flat, &dest).map(|_| ()))?;
        } else if let Some(p) = src_legacy_parent {
            if p.is_file() {
                fs::copy(&p, &dest)?;
            }
        }
    }
    Ok(study_dir)
}

fn section_dir(study_dir: &Path, idx: u32, kind: &str) -> PathBuf {
    let dir = study_dir.join(format!("{idx:02}-{kind}"));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = text.trim_end().to_string() + "\n";
    fs::write(path, body).with_context(|| format!("write {}", path.display()))
}

fn read_trimmed(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?
        .trim()
        .to_string())
}

fn read_nonempty_lines(path: &Path) -> Result<Vec<String>> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn join_paragraphs(paras: &[String]) -> String {
    paras
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn split_paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|p| {
            p.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|p| !p.is_empty())
        .collect()
}

fn format_lectura(block: &Lectura) -> String {
    let mut out = format!("CITA: {}\n\n", block.cita.trim());
    for v in &block.versiculos {
        out.push_str(v.trim());
        out.push('\n');
    }
    out
}

fn parse_lectura(text: &str) -> Result<Lectura> {
    let text = crate::content::adult_normalize::strip_duplicate_cita_line(text);
    let mut cita = String::new();
    let mut versiculos = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("CITA:") {
            cita = rest.trim().to_string();
        } else {
            versiculos.push(line.to_string());
        }
    }
    if cita.is_empty() {
        bail!("lectura block missing CITA: line");
    }
    // Drop a leftover citation-only first verse (defense in depth).
    if let Some(first) = versiculos.first() {
        let a = first.replace(' ', "").to_lowercase();
        let b = cita.replace(' ', "").to_lowercase();
        if a == b || b.starts_with(&a) || a.starts_with(&b) {
            versiculos.remove(0);
        }
    }
    if versiculos.is_empty() {
        bail!("lectura block has no verses");
    }
    for line in &versiculos {
        let Some((marker, words)) = line.split_once(char::is_whitespace) else {
            bail!("expected a verse number followed by text: {line}");
        };
        if words.trim().is_empty() || !marker.split(':').all(|part| {
            part.parse::<u32>().is_ok_and(|number| number > 0)
        }) || marker.matches(':').count() > 1 {
            bail!("expected a verse number or chapter:verse followed by text: {line}");
        }
    }
    Ok(Lectura { cita, versiculos })
}

fn parse_tema_header(text: &str, dir: &Path) -> Result<(String, String)> {
    let mut titulo = String::new();
    let mut rango = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("TITULO:") {
            titulo = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("RANGO:") {
            rango = rest.trim().to_string();
        }
    }
    if titulo.is_empty() || rango.is_empty() {
        let lines: Vec<_> = text.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
        if let [title] = lines.as_slice() {
            let range_path = dir.join("rango.txt");
            if !title.starts_with("RANGO:") && range_path.is_file() {
                let range = read_trimmed(&range_path)?;
                if !range.is_empty() {
                    let title = title.strip_prefix("TITULO:").unwrap_or(title).trim();
                    if !title.is_empty() {
                        return Ok((title.to_string(), range));
                    }
                }
            }
        }
        if let [title, range] = lines.as_slice() {
            if !title.contains(':')
                && range.rsplit_once(' ').is_some_and(|(_, verses)| {
                    verses.contains(':')
                        && verses.chars().all(|c| c.is_ascii_digit() || matches!(c, ':' | '-' | '–'))
                })
            {
                return Ok((title.to_string(), range.to_string()));
            }
        }
        bail!("tema header.txt needs TITULO: and RANGO: lines");
    }
    Ok((titulo, rango))
}

fn parse_definicion(text: &str) -> Result<Definicion> {
    let mut termino = String::new();
    let mut referencia = None;
    let mut in_texto = false;
    let mut texto_lines = Vec::new();
    for line in text.lines() {
        if in_texto {
            texto_lines.push(line.to_string());
            continue;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("TERMINO:") {
            termino = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("REFERENCIA:") {
            let r = rest.trim();
            if !r.is_empty() {
                referencia = Some(r.to_string());
            }
        } else if t == "TEXTO:" || t.starts_with("TEXTO:") {
            in_texto = true;
            if let Some(rest) = t.strip_prefix("TEXTO:") {
                let r = rest.trim();
                if !r.is_empty() {
                    texto_lines.push(r.to_string());
                }
            }
        }
    }
    let texto = texto_lines.join("\n").trim().to_string();
    if termino.is_empty() || texto.is_empty() {
        bail!("definicion needs TERMINO: and TEXTO:");
    }
    Ok(Definicion {
        termino,
        texto,
        referencia,
        icono_izq: None,
        icono_der: None,
    })
}

fn parse_datos(text: &str) -> Result<DatosGenerales> {
    let mut autor = String::new();
    let mut personajes = String::new();
    let mut fecha = String::new();
    let mut lugar = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "autor" => autor = v,
                "personajes" => personajes = v,
                "fecha" => fecha = v,
                "lugar" => lugar = v,
                _ => {}
            }
        }
    }
    Ok(DatosGenerales {
        autor,
        personajes,
        fecha,
        lugar,
    })
}

fn list_section_dirs(study_dir: &Path) -> Result<Vec<(u32, String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(study_dir).with_context(|| format!("read {}", study_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "images" {
            continue;
        }
        // NN-kind
        let Some((num, kind)) = name.split_once('-') else {
            continue;
        };
        let Ok(ord) = num.parse::<u32>() else {
            continue;
        };
        if kind.is_empty() {
            continue;
        }
        out.push((ord, kind.to_string(), path));
    }
    Ok(out)
}

fn list_files_with_prefix(dir: &Path, prefix: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(prefix) && name.ends_with(".txt") {
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::adult_study::BloqueAdult;

    #[allow(dead_code)]
    fn sample_study() -> AdultStudy {
        AdultStudy {
            numero: Numero::Int(7),
            titulo: "EL TIEMPO".into(),
            base_biblica: StringOrList::Many(vec!["2 Samuel 2:1-7;".into()]),
            lectura_antifonal: vec![Lectura {
                cita: "2 Samuel 2:1".into(),
                versiculos: vec!["1 Después de esto.".into()],
            }],
            objetivos: vec!["Uno.".into(), "Dos.".into(), "Tres.".into()],
            pensamiento_central: "Pensamiento.".into(),
            texto_aureo: TextoAureo {
                texto: "Cita áurea.".into(),
                cita: "2 Samuel 5:1".into(),
            },
            ensenanza: "Enseñanza.".into(),
            datos_generales: DatosGenerales {
                autor: "Natán".into(),
                personajes: "David".into(),
                fecha: "900".into(),
                lugar: "Hebrón".into(),
            },
            introduccion_slides: vec!["Intro uno.".into(), "Intro dos.".into()],
            temas: vec![TemaAdult {
                titulo: "TEMA UNO".into(),
                rango: "2 Samuel 2:1-7".into(),
                definiciones: vec![Definicion {
                    termino: "Hebrón".into(),
                    texto: "Ciudad antigua.".into(),
                    referencia: None,
                    icono_izq: None,
                    icono_der: None,
                }],
                puntos: vec![PuntoAdult::from_bloque(
                    "A",
                    BloqueAdult {
                        titulo: "Punto A".into(),
                        texto_slides: vec!["Cuerpo A.".into()],
                        texto_biblico: Lectura {
                            cita: "2 Samuel 2:1-3".into(),
                            versiculos: vec!["1 Después.".into(), "2 Y subió.".into()],
                        },
                    },
                )],
            }],
            proximo: Proximo {
                numero: Numero::Int(8),
                titulo: "SIGUIENTE".into(),
                base_biblica: Some(StringOrList::Many(vec!["2 Samuel 6".into()])),
            },
            template: None,
            audience: Some("adult".into()),
            definicion_images: None,
            scenic_images: None,
        }
    }

    #[test]
    fn roundtrip_sections() {
        let dir = std::env::temp_dir().join(format!(
            "lamad-adult-sections-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let study = sample_study();
        write_adult_sections(&study, &dir).unwrap();
        assert!(dir.join("manifest.toml").is_file());
        assert!(dir.join("09-tema-1").join("header.txt").is_file()
            || dir
                .read_dir()
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().contains("tema-1")));

        // load via temp: monkey by writing into a fake studies path is heavy —
        // parse manifest + folders with a local helper path override via load internals.
        let manifest: AdultManifest =
            toml::from_str(&fs::read_to_string(dir.join("manifest.toml")).unwrap()).unwrap();
        assert_eq!(manifest.numero, 7);
        assert_eq!(manifest.titulo, "EL TIEMPO");

        let lectura = parse_lectura(
            &fs::read_to_string(
                dir.read_dir()
                    .unwrap()
                    .find(|e| {
                        e.as_ref()
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .contains("lectura")
                    })
                    .unwrap()
                    .unwrap()
                    .path()
                    .join("block-01.txt"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(lectura.cita, "2 Samuel 2:1");
        let _ = fs::remove_dir_all(&dir);
    }
}
