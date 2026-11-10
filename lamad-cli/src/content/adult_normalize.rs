//! Post-agent repair for adult section trees.
//!
//! Cursor agents often invent file shapes that `load_adult_sections` rejects
//! (wrong `manifest.toml`, `titulo.txt` instead of `header.txt` / `title.txt`,
//! defs without `TERMINO:`/`TEXTO:`, duplicate Lectura citation lines, printed
//! próximo numbers). Call [`normalize_adult_section_tree`] after download and
//! before load so `lamad prepare` works without hand-editing folders.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

use super::adult_sections::{adult_study_folder, AdultManifest};

/// Rewrite agent quirks under `studies/adult/{study}/` into the gold folder
/// layout. Returns `true` when any file changed.
pub fn normalize_adult_section_tree(study: u32) -> Result<bool> {
    let root = adult_study_folder(study)?;
    if !root.is_dir() {
        return Ok(false);
    }
    let mut changed = false;
    changed |= normalize_manifest(&root, study)?;
    changed |= normalize_lectura_blocks(&root)?;
    changed |= normalize_objetivos(&root)?;
    changed |= normalize_intro(&root)?;
    changed |= normalize_datos(&root)?;
    changed |= normalize_proximo(&root, study)?;
    changed |= normalize_tema_folders(&root)?;
    Ok(changed)
}

fn normalize_manifest(root: &Path, study: u32) -> Result<bool> {
    let path = root.join("manifest.toml");
    if !path.is_file() {
        // Synthesize from 01-title/ when agent skipped a valid manifest.
        let title_dir = find_kind_dir(root, "title");
        let Some(title_dir) = title_dir else {
            return Ok(false);
        };
        let titulo = read_first_existing(&title_dir, &["title.txt", "titulo.txt"])?;
        let base = read_nonempty_lines(&title_dir.join("base_biblica.txt")).unwrap_or_default();
        if titulo.is_empty() {
            return Ok(false);
        }
        write_canonical_manifest(&path, study, &titulo, &base, false)?;
        return Ok(true);
    }

    let text = fs::read_to_string(&path)?;
    if toml::from_str::<AdultManifest>(&text).is_ok() {
        // Still force numero == study folder.
        let mut m: AdultManifest = toml::from_str(&text)?;
        if m.numero != study {
            m.numero = study;
            write_manifest_struct(&path, &m)?;
            return Ok(true);
        }
        return Ok(false);
    }

    // Agent invent: [study] estudio=… title=… base_biblica=…
    let v: toml::Value = toml::from_str(&text)
        .with_context(|| format!("parse loose manifest {}", path.display()))?;
    let table = v
        .get("study")
        .and_then(|x| x.as_table())
        .cloned()
        .or_else(|| v.as_table().cloned())
        .context("manifest has no [study] table")?;

    let titulo = table
        .get("title")
        .or_else(|| table.get("titulo"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if titulo.is_empty() {
        // Fall back to 01-title/title.txt
        if let Some(td) = find_kind_dir(root, "title") {
            let t = read_first_existing(&td, &["title.txt", "titulo.txt"])?;
            if t.is_empty() {
                bail!(
                    "adult estudio {study}: cannot salvage manifest.toml — no title"
                );
            }
            let base = read_nonempty_lines(&td.join("base_biblica.txt")).unwrap_or_default();
            write_canonical_manifest(&path, study, &t, &base, false)?;
            return Ok(true);
        }
        bail!("adult estudio {study}: cannot salvage manifest.toml — no title");
    }

    let base = match table.get("base_biblica") {
        Some(toml::Value::String(s)) => s
            .split(|c| c == '\n' || c == ';')
            .map(|p| p.trim().trim_end_matches(';').trim().to_string())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>(),
        Some(toml::Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => {
            if let Some(td) = find_kind_dir(root, "title") {
                read_nonempty_lines(&td.join("base_biblica.txt")).unwrap_or_default()
            } else {
                Vec::new()
            }
        }
    };
    let omit = table
        .get("omit_proximo")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    write_canonical_manifest(&path, study, &titulo, &base, omit)?;
    Ok(true)
}

fn write_canonical_manifest(
    path: &Path,
    study: u32,
    titulo: &str,
    base: &[String],
    omit_proximo: bool,
) -> Result<()> {
    let m = AdultManifest {
        numero: study,
        titulo: titulo.to_string(),
        base_biblica: base.to_vec(),
        omit_proximo,
        audience: Some("adult".into()),
    };
    write_manifest_struct(path, &m)
}

fn write_manifest_struct(path: &Path, m: &AdultManifest) -> Result<()> {
    let mut out = String::new();
    out.push_str(&format!("numero = {}\n", m.numero));
    out.push_str(&format!("titulo = \"{}\"\n", escape_toml_str(&m.titulo)));
    out.push_str("base_biblica = [\n");
    for (i, b) in m.base_biblica.iter().enumerate() {
        let line = b.trim().trim_end_matches(';').trim();
        let with_semi = if i + 1 < m.base_biblica.len() && !line.ends_with(';') {
            format!("{line};")
        } else {
            line.to_string()
        };
        out.push_str(&format!("    \"{}\",\n", escape_toml_str(&with_semi)));
    }
    out.push_str("]\n");
    out.push_str(&format!("omit_proximo = {}\n", m.omit_proximo));
    out.push_str("audience = \"adult\"\n");
    fs::write(path, out)?;
    Ok(())
}

fn escape_toml_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn normalize_lectura_blocks(root: &Path) -> Result<bool> {
    let Some(dir) = find_kind_dir(root, "lectura") else {
        return Ok(false);
    };
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with("block-") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();

    // Fallback citation when every block is verse-only (common agent bug).
    let mut last_cita = fallback_lectura_cita(root);
    let mut changed = false;
    for path in paths {
        let text = fs::read_to_string(&path)?;
        let with_cita = ensure_cita_prefix(&text);
        let mut cleaned = strip_duplicate_cita_line(&with_cita);
        if !cleaned.lines().any(|l| l.trim_start().starts_with("CITA:")) {
            if let Some(ref cita) = last_cita {
                let body = cleaned.trim();
                cleaned = if body.is_empty() {
                    format!("CITA: {cita}\n")
                } else {
                    format!("CITA: {cita}\n\n{body}\n")
                };
            }
        }
        if let Some(c) = extract_cita_value(&cleaned) {
            last_cita = Some(c);
        }
        if cleaned != text {
            fs::write(&path, cleaned)?;
            changed = true;
        }
    }
    Ok(changed)
}

fn fallback_lectura_cita(root: &Path) -> Option<String> {
    if let Ok(m) = fs::read_to_string(root.join("manifest.toml")) {
        if let Ok(manifest) = toml::from_str::<AdultManifest>(&m) {
            if let Some(b) = manifest.base_biblica.first() {
                let c = b.trim().trim_end_matches(';').trim();
                if !c.is_empty() {
                    return Some(c.to_string());
                }
            }
        }
    }
    if let Some(td) = find_kind_dir(root, "title") {
        if let Ok(lines) = read_nonempty_lines(&td.join("base_biblica.txt")) {
            if let Some(b) = lines.first() {
                let c = b.trim().trim_end_matches(';').trim();
                if !c.is_empty() {
                    return Some(c.to_string());
                }
            }
        }
    }
    None
}

fn extract_cita_value(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("CITA:") {
            let c = rest.trim();
            if !c.is_empty() {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// Agents often omit `CITA:` — citation alone on line 1, or glued onto the verse.
pub(super) fn ensure_cita_prefix(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ensure_trailing_newline(text);
    }
    if trimmed.lines().any(|l| l.trim_start().starts_with("CITA:")) {
        return ensure_trailing_newline(text);
    }
    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or("").trim();
    let rest = lines.collect::<Vec<_>>().join("\n");
    if first.is_empty() {
        return ensure_trailing_newline(text);
    }
    // Pure citation line: "2 Crónicas 6:14-15" / "2 Samuel 6"
    let cite_only = Regex::new(
        r"^(?:\d+\s+)?[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.\s]+\d+(?::[\d,\-\s;]+)?$",
    )
    .unwrap();
    if cite_only.is_match(first) {
        return if rest.trim().is_empty() {
            format!("CITA: {first}\n")
        } else {
            format!("CITA: {first}\n\n{}\n", rest.trim())
        };
    }
    // Glued cite + verse: "2 Crónicas 6:14 Jehová Dios…"
    let glued = Regex::new(
        r"^(?P<cita>(?:\d+\s+)?[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.\s]*?\d+(?::\d+(?:\s*[-–]\s*\d+)?)?)\s+(?P<body>.+)$",
    )
    .unwrap();
    if let Some(caps) = glued.captures(first) {
        let cita = caps.name("cita").unwrap().as_str().trim();
        let body_first = caps.name("body").unwrap().as_str().trim();
        // Prefer verse-shaped body ("14 Jehová…") when the cite already includes :verse.
        let verse_line = if Regex::new(r"^\d+")
            .unwrap()
            .is_match(body_first)
        {
            body_first.to_string()
        } else if let Some(v) = cita.rsplit_once(':').map(|(_, v)| v.trim()) {
            // "2 Crónicas 6:14" + "Jehová…" → "14 Jehová…"
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                format!("{v} {body_first}")
            } else {
                body_first.to_string()
            }
        } else {
            body_first.to_string()
        };
        let mut body = verse_line;
        if !rest.trim().is_empty() {
            body.push('\n');
            body.push_str(rest.trim());
        }
        return format!("CITA: {cita}\n\n{body}\n");
    }
    ensure_trailing_newline(text)
}

/// Drop a bare citation line that repeats `CITA:` (causes double titles on slides).
pub fn strip_duplicate_cita_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let Some(cita_line) = lines.iter().find(|l| l.trim_start().starts_with("CITA:")) else {
        return ensure_trailing_newline(text);
    };
    let cita = cita_line
        .trim()
        .strip_prefix("CITA:")
        .unwrap_or("")
        .trim()
        .to_string();
    if cita.is_empty() {
        return ensure_trailing_newline(text);
    }
    let mut out = Vec::new();
    let mut skipped_dup = false;
    for line in &lines {
        let t = line.trim();
        if !skipped_dup && !t.is_empty() && !t.starts_with("CITA:") && is_citation_only_line(t, &cita)
        {
            skipped_dup = true;
            continue;
        }
        out.push((*line).to_string());
    }
    let joined = out.join("\n");
    ensure_trailing_newline(&joined)
}

fn is_citation_only_line(line: &str, cita: &str) -> bool {
    if line == cita {
        return true;
    }
    // "2 Samuel 6" / "2 Samuel 6:1-11" — book + chapter, no verse prose.
    let citation_re =
        Regex::new(r"^(?:\d+\s+)?[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.\s]+\d+(?::[\d,\-\s;]+)?$")
            .unwrap();
    if !citation_re.is_match(line) {
        return false;
    }
    // Must not look like a verse body ("6 Cuando…", "11:2 Y sucedió…").
    if Regex::new(r"^\d+:?\d*\s+\S").unwrap().is_match(line)
        && line.chars().any(|c| c.is_lowercase())
    {
        return false;
    }
    let a = line.replace(' ', "").to_lowercase();
    let b = cita.replace(' ', "").to_lowercase();
    a == b || b.starts_with(&a) || a.starts_with(&b)
}

fn normalize_objetivos(root: &Path) -> Result<bool> {
    let Some(dir) = find_kind_dir(root, "objetivos") else {
        return Ok(false);
    };
    let path = dir.join("content.txt");
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    let mut lines = Vec::new();
    let mut changed = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let stripped = strip_leading_number(t);
        if stripped != t {
            changed = true;
        }
        lines.push(stripped);
    }
    if changed {
        fs::write(&path, lines.join("\n") + "\n")?;
    }
    Ok(changed)
}

fn strip_leading_number(raw: &str) -> String {
    let t = raw.trim();
    let bytes = t.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && bytes[i] == b'.' {
        return t[i + 1..].trim_start().to_string();
    }
    t.to_string()
}

fn normalize_intro(root: &Path) -> Result<bool> {
    let Some(dir) = find_kind_dir(root, "introduccion") else {
        return Ok(false);
    };
    let path = dir.join("slides.txt");
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    if !text.contains("---") {
        return Ok(false);
    }
    let parts: Vec<_> = text
        .split("---")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    fs::write(&path, parts.join("\n\n") + "\n")?;
    Ok(true)
}

fn normalize_datos(root: &Path) -> Result<bool> {
    let Some(dir) = find_kind_dir(root, "datos") else {
        return Ok(false);
    };
    let path = dir.join("content.txt");
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    let mut out = Vec::new();
    let mut changed = false;
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let nk = k.trim().to_ascii_lowercase();
            let line2 = format!("{nk}: {}", v.trim());
            if line2 != line.trim() {
                changed = true;
            }
            out.push(line2);
        } else if !line.trim().is_empty() {
            out.push(line.to_string());
        }
    }
    if changed {
        fs::write(&path, out.join("\n") + "\n")?;
    }
    Ok(changed)
}

fn normalize_proximo(root: &Path, study: u32) -> Result<bool> {
    let Some(dir) = find_kind_dir(root, "proximo") else {
        return Ok(false);
    };
    let mut changed = false;

    // titulo.txt → title.txt
    let titulo = dir.join("titulo.txt");
    let title = dir.join("title.txt");
    if titulo.is_file() && !title.is_file() {
        fs::rename(&titulo, &title)?;
        changed = true;
    } else if titulo.is_file() && title.is_file() {
        fs::remove_file(&titulo)?;
        changed = true;
    }

    // Folder numbering: próximo is always study+1 (not printed scan badge).
    let omit = root
        .join("manifest.toml")
        .is_file()
        .then(|| fs::read_to_string(root.join("manifest.toml")).ok())
        .flatten()
        .and_then(|t| toml::from_str::<AdultManifest>(&t).ok())
        .map(|m| m.omit_proximo)
        .unwrap_or(false);
    if !omit {
        let num_path = dir.join("numero.txt");
        let want = (study + 1).to_string();
        let cur = fs::read_to_string(&num_path)
            .unwrap_or_default()
            .trim()
            .to_string();
        if cur != want {
            fs::write(&num_path, format!("{want}\n"))?;
            changed = true;
        }
    }
    Ok(changed)
}

fn normalize_tema_folders(root: &Path) -> Result<bool> {
    let mut changed = false;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let Some((_, kind)) = name.split_once('-') else {
            continue;
        };
        if !kind.starts_with("tema-") {
            continue;
        }
        let rest = &kind["tema-".len()..];
        if rest.contains('-') {
            // tema-1-A point
            changed |= normalize_ab_point(&path)?;
        } else {
            // tema-1 header
            changed |= normalize_tema_header(&path)?;
        }
    }
    Ok(changed)
}

fn normalize_tema_header(dir: &Path) -> Result<bool> {
    let mut changed = false;
    let header = dir.join("header.txt");
    let titulo_p = dir.join("titulo.txt");
    let rango_p = dir.join("rango.txt");
    if !header.is_file() && (titulo_p.is_file() || rango_p.is_file()) {
        let titulo = fs::read_to_string(&titulo_p)
            .unwrap_or_default()
            .trim()
            .to_string();
        let rango = fs::read_to_string(&rango_p)
            .unwrap_or_default()
            .trim()
            .to_string();
        fs::write(
            &header,
            format!("TITULO: {titulo}\nRANGO: {rango}\n"),
        )?;
        let _ = fs::remove_file(&titulo_p);
        let _ = fs::remove_file(&rango_p);
        changed = true;
    }
    // Drop stray texto on header folder (points hold texto-biblico).
    for name in ["texto_biblico.txt", "texto-biblico.txt"] {
        let p = dir.join(name);
        if p.is_file() {
            fs::remove_file(&p)?;
            changed = true;
        }
    }
    // Definicion files → TERMINO:/TEXTO:
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !name.starts_with("definicion-") || !name.ends_with(".txt") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let fixed = ensure_definicion_format(&text);
        if fixed != text {
            fs::write(&path, fixed)?;
            changed = true;
        }
    }
    Ok(changed)
}

fn normalize_ab_point(dir: &Path) -> Result<bool> {
    let mut changed = false;
    let letter = dir
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|n| n.rsplit('-').next())
        .unwrap_or("")
        .to_string();

    // title.txt from titulo.txt (+ optional rango) — plain title only
    let title_p = dir.join("title.txt");
    let titulo_p = dir.join("titulo.txt");
    let rango_p = dir.join("rango.txt");
    let _ = letter; // letter is on the folder name; builder prefixes N.Letter
    if titulo_p.is_file() {
        let titulo = fs::read_to_string(&titulo_p)?.trim().to_string();
        if !title_p.is_file() {
            fs::write(&title_p, format!("{titulo}\n"))?;
        }
        let _ = fs::remove_file(&titulo_p);
        let _ = fs::remove_file(&rango_p);
        changed = true;
        let cur = fs::read_to_string(&title_p).unwrap_or(titulo);
        let clean = sanitize_ab_point_titulo(&cur);
        if clean != cur.trim() {
            fs::write(&title_p, format!("{clean}\n"))?;
            changed = true;
        }
    } else if title_p.is_file() {
        let cur = fs::read_to_string(&title_p)?;
        let clean = sanitize_ab_point_titulo(&cur);
        if clean != cur.trim() {
            fs::write(&title_p, format!("{clean}\n"))?;
            changed = true;
        }
        let _ = fs::remove_file(&rango_p);
    }

    // slides.txt → body.txt (--- → blank paragraphs)
    let slides = dir.join("slides.txt");
    let body = dir.join("body.txt");
    if slides.is_file() && !body.is_file() {
        let text = fs::read_to_string(&slides)?;
        let parts: Vec<_> = if text.contains("---") {
            text.split("---")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect()
        } else {
            text.split("\n\n")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect()
        };
        fs::write(&body, parts.join("\n\n") + "\n")?;
        fs::remove_file(&slides)?;
        changed = true;
    }

    // texto_biblico.txt (+ optional cita) → texto-biblico.txt with CITA:
    let src = dir.join("texto_biblico.txt");
    let dst = dir.join("texto-biblico.txt");
    let cita_f = dir.join("texto_biblico_cita.txt");
    if src.is_file() {
        let mut t = fs::read_to_string(&src)?;
        let cita = if cita_f.is_file() {
            let c = fs::read_to_string(&cita_f)?.trim().to_string();
            let _ = fs::remove_file(&cita_f);
            c
        } else {
            String::new()
        };
        if !t.lines().next().unwrap_or("").starts_with("CITA:") {
            let cita = if !cita.is_empty() {
                cita
            } else {
                t.lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            // If first line looks like a citation-only line, don't duplicate it as verse.
            let body = if is_citation_only_line(t.lines().next().unwrap_or("").trim(), &cita)
                || t.lines().next().unwrap_or("").trim() == cita
            {
                let mut ls = t.lines();
                let _ = ls.next();
                ls.collect::<Vec<_>>().join("\n")
            } else {
                t.clone()
            };
            t = format!("CITA: {cita}\n\n{}\n", body.trim());
        }
        t = strip_duplicate_cita_line(&t);
        fs::write(&dst, ensure_trailing_newline(&t))?;
        fs::remove_file(&src)?;
        changed = true;
    } else if dst.is_file() {
        let t = fs::read_to_string(&dst)?;
        let cleaned = strip_duplicate_cita_line(&t);
        if cleaned != t {
            fs::write(&dst, cleaned)?;
            changed = true;
        }
    }
    Ok(changed)
}

fn ensure_definicion_format(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    if text.lines().any(|l| l.trim().starts_with("TERMINO:"))
        && text.lines().any(|l| l.trim().starts_with("TEXTO:") || l.trim() == "TEXTO:")
    {
        return ensure_trailing_newline(raw);
    }
    // Quoted term: "Rabá", capital…
    let re_q = Regex::new(r#"^[“"]([^”"]+)[”"]\s*(.*)$"#).unwrap();
    if let Some(c) = re_q.captures(text.replace('\n', " ").trim()) {
        let term = c.get(1).unwrap().as_str().trim();
        let body = c.get(2).unwrap().as_str().trim().trim_start_matches(['.', ',', ' ']);
        return format!("TERMINO: {term}\nTEXTO:\n{body}\n");
    }
    let re_guillemet = Regex::new(r"^«([^»]+)»\s*(.*)$").unwrap();
    if let Some(c) = re_guillemet.captures(text.replace('\n', " ").trim()) {
        let term = c.get(1).unwrap().as_str().trim();
        let body = c.get(2).unwrap().as_str().trim();
        return format!("TERMINO: {term}\nTEXTO:\n{body}\n");
    }
    // First line = term, rest = body
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim();
    let rest = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if !rest.is_empty() {
        return format!("TERMINO: {first}\nTEXTO:\n{rest}\n");
    }
    // Single paragraph — first sentence-ish as term
    let parts: Vec<_> = text.splitn(2, ". ").collect();
    if parts.len() == 2 && parts[0].chars().count() < 90 {
        return format!(
            "TERMINO: {}\nTEXTO:\n{}\n",
            parts[0].trim().trim_matches(|c| c == '"' || c == '“' || c == '”'),
            parts[1].trim()
        );
    }
    format!("TERMINO: {}\nTEXTO:\n{}\n", &text[..text.len().min(60)], text)
}

/// Same rules as adult builder — keep plain Spanish titles only.
pub fn sanitize_ab_point_titulo(raw: &str) -> String {
    let point = Regex::new(r"(?i)^\d+\.[A-D]\s*[-–—]?\s*").unwrap();
    let letter = Regex::new(r"(?i)^[A-D]\.\s*").unwrap();
    let verse_paren =
        Regex::new(r"\s*\(\s*\d+(?:\s*[,:\-–—]\s*\d+)*\s*\)\s*\.?\s*$").unwrap();
    let mut t = raw.trim().to_string();
    for _ in 0..6 {
        let before = t.clone();
        t = point.replace(&t, "").into_owned();
        t = letter.replace(t.trim_start(), "").into_owned();
        t = verse_paren.replace(t.trim_end(), "").into_owned();
        t = t.trim().trim_end_matches('.').trim().to_string();
        if t == before {
            break;
        }
    }
    t
}

fn find_kind_dir(root: &Path, kind: &str) -> Option<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if let Some((_, k)) = name.split_once('-') {
            if k == kind {
                return Some(e.path());
            }
        }
    }
    None
}

fn read_first_existing(dir: &Path, names: &[&str]) -> Result<String> {
    for n in names {
        let p = dir.join(n);
        if p.is_file() {
            return Ok(fs::read_to_string(p)?.trim().to_string());
        }
    }
    Ok(String::new())
}

fn read_nonempty_lines(path: &Path) -> Result<Vec<String>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn ensure_trailing_newline(s: &str) -> String {
    let t = s.trim_end_matches('\n');
    format!("{t}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_duplicate_cita_line() {
        let raw = "CITA: 2 Samuel 6\n\n2 Samuel 6\n6 Cuando llegaron…\n";
        let out = strip_duplicate_cita_line(raw);
        assert!(out.contains("CITA: 2 Samuel 6"));
        assert_eq!(out.matches("2 Samuel 6").count(), 1);
        assert!(out.contains("6 Cuando"));
    }

    #[test]
    fn ensure_cita_prefix_promotes_first_line() {
        let raw = "2 Crónicas 6:14-15\n14 Jehová Dios de Israel…\n";
        let out = ensure_cita_prefix(raw);
        assert!(out.starts_with("CITA: 2 Crónicas 6:14-15"));
        assert!(out.contains("14 Jehová"));
    }

    #[test]
    fn ensure_cita_prefix_splits_glued_cite_and_verse() {
        let raw = "2 Crónicas 6:14 Jehová Dios de Israel, no hay Dios semejante a ti\n";
        let out = ensure_cita_prefix(raw);
        assert!(out.starts_with("CITA: 2 Crónicas 6:14\n"), "{out}");
        assert!(out.contains("14 Jehová Dios de Israel"), "{out}");
    }

    #[test]
    fn normalize_lectura_carries_cita_across_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("manifest.toml"),
            "numero = 11\ntitulo = \"T\"\nbase_biblica = [\"2 Crónicas 6 y 7\"]\nomit_proximo = true\naudience = \"adult\"\n",
        )
        .unwrap();
        let lec = root.join("02-lectura");
        fs::create_dir_all(&lec).unwrap();
        fs::write(
            lec.join("block-01.txt"),
            "2 Crónicas 6:14 Jehová Dios de Israel…\n",
        )
        .unwrap();
        fs::write(
            lec.join("block-02.txt"),
            "15 que has guardado a tu siervo David…\n",
        )
        .unwrap();
        assert!(normalize_lectura_blocks(root).unwrap());
        let b1 = fs::read_to_string(lec.join("block-01.txt")).unwrap();
        let b2 = fs::read_to_string(lec.join("block-02.txt")).unwrap();
        assert!(b1.starts_with("CITA: 2 Crónicas 6:14"), "{b1}");
        assert!(b2.starts_with("CITA: 2 Crónicas 6:14"), "{b2}");
        assert!(b2.contains("15 que has guardado"), "{b2}");
    }

    #[test]
    fn sanitize_ab_titulo() {
        assert_eq!(
            sanitize_ab_point_titulo("A. Hacer todo con gozo (6:12)."),
            "Hacer todo con gozo"
        );
    }

    #[test]
    fn definicion_from_quoted() {
        let out = ensure_definicion_format(
            "“Rabá”, capital de Amón, a 35 kilómetros al oriente del Jordán.",
        );
        assert!(out.contains("TERMINO: Rabá"));
        assert!(out.contains("TEXTO:"));
        assert!(out.contains("capital de Amón"));
    }
}
