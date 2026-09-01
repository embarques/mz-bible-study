//! PPTX package validation (port of validate.py).

use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

const ST: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";

/// Return Ok(()) if valid. Prints OK / FAIL like the Python tool.
pub fn validate_pptx(path: &Path) -> Result<()> {
    match validate_pptx_inner(path) {
        Ok(stats) => {
            println!("OK: {}", path.display());
            println!("  slides in order: {}", stats.sld_count);
            println!("  slide files in zip: {}", stats.slide_files);
            println!(
                "  Content_Types slide overrides: {}",
                stats.slide_overrides
            );
            Ok(())
        }
        Err(errors) => {
            println!("INVALID: {}", path.display());
            for e in &errors {
                println!("FAIL: {e}");
            }
            println!(
                "{} error(s). Do NOT deliver this pptx.",
                errors.len()
            );
            anyhow::bail!("validation failed")
        }
    }
}

struct Stats {
    sld_count: usize,
    slide_files: usize,
    slide_overrides: usize,
}

fn validate_pptx_inner(path: &Path) -> Result<Stats, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    if !path.exists() {
        return Err(vec![format!("file not found: {}", path.display())]);
    }

    let file = File::open(path).map_err(|e| vec![format!("open: {e}")])?;
    let mut zip = ZipArchive::new(file).map_err(|e| vec![format!("not a valid zip/pptx: {e}")])?;

    // CRC check
    for i in 0..zip.len() {
        let mut f = match zip.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("zip CRC/read error: {e}"));
                continue;
            }
        };
        let mut buf = Vec::new();
        if let Err(e) = f.read_to_end(&mut buf) {
            errors.push(format!("zip CRC error in {}: {e}", f.name()));
        }
    }

    let names: Vec<String> = {
        let mut n = Vec::new();
        for i in 0..zip.len() {
            if let Ok(f) = zip.by_index(i) {
                n.push(f.name().to_string());
            }
        }
        n
    };
    let name_set: std::collections::HashSet<_> = names.iter().cloned().collect();

    if !name_set.contains("_rels/.rels") {
        errors.push(
            "missing `_rels/.rels` (zip filter must not skip `.rels` via startswith('.'))"
                .into(),
        );
    }

    let ct = read_zip_str(&mut zip, "[Content_Types].xml").unwrap_or_default();
    if ct.contains("ns0:") || ct.contains("xmlns:ns0=") {
        errors.push("[Content_Types].xml has ns0: prefix (ElementTree corruption)".into());
    }
    if !ct.contains("<Types xmlns=") && !ct.contains("<Types xmlns='") {
        errors.push("[Content_Types].xml missing default xmlns Types".into());
    }

    let part_re = Regex::new(r#"PartName="([^"]+)""#).unwrap();
    let slide_ov_re = Regex::new(r"^/ppt/slides/slide\d+\.xml$").unwrap();
    let overrides: Vec<String> = part_re
        .captures_iter(&ct)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();
    let slide_overrides: Vec<String> = overrides
        .iter()
        .filter(|o| slide_ov_re.is_match(o))
        .cloned()
        .collect();
    for o in &slide_overrides {
        let part = o.trim_start_matches('/');
        if !name_set.contains(part) {
            errors.push(format!("Content_Types Override missing file: {o}"));
        }
    }

    let slide_file_re = Regex::new(r"^ppt/slides/slide\d+\.xml$").unwrap();
    let mut slide_files: Vec<String> = names
        .iter()
        .filter(|n| slide_file_re.is_match(n))
        .cloned()
        .collect();
    slide_files.sort();
    for sf in &slide_files {
        let part = format!("/{sf}");
        if !slide_overrides.contains(&part) {
            errors.push(format!("slide file has no Content_Types Override: {sf}"));
        }
    }

    let rels_xml = read_zip_str(&mut zip, "ppt/_rels/presentation.xml.rels").unwrap_or_default();
    if rels_xml.contains("ns0:Types") {
        errors.push("presentation.xml.rels looks corrupted (ns0:Types)".into());
    }
    if rels_xml.contains(
        r#"xmlns:ns0="http://schemas.openxmlformats.org/package/2006/content-types""#,
    ) {
        errors.push("presentation.xml.rels has content-types namespace (wrong file?)".into());
    }

    // Parse slide relationships
    let rid_re = Regex::new(
        r#"<Relationship\b[^>]*Id="([^"]+)"[^>]*Type="([^"]+)"[^>]*Target="([^"]+)"[^>]*/>"#,
    )
    .unwrap();
    // Also Type before Id
    let rid_re2 = Regex::new(
        r#"<Relationship\b[^>]*Type="([^"]+)"[^>]*Id="([^"]+)"[^>]*Target="([^"]+)"[^>]*/>"#,
    )
    .unwrap();

    let mut rid_map: HashMap<String, (String, String)> = HashMap::new();
    let mut slide_rels: Vec<(String, String)> = Vec::new();

    for cap in rid_re.captures_iter(&rels_xml) {
        let id = cap[1].to_string();
        let ty = cap[2].to_string();
        let target = cap[3].to_string();
        rid_map.insert(id.clone(), (ty.clone(), target.clone()));
        if ty == ST {
            slide_rels.push((id, target));
        }
    }
    // Alternate attribute order
    for cap in rid_re2.captures_iter(&rels_xml) {
        let ty = cap[1].to_string();
        let id = cap[2].to_string();
        let target = cap[3].to_string();
        rid_map.entry(id.clone()).or_insert((ty.clone(), target.clone()));
        if ty == ST && !slide_rels.iter().any(|(i, _)| i == &id) {
            slide_rels.push((id, target));
        }
    }

    for (_id, t) in &slide_rels {
        let full = if t.starts_with("ppt/") {
            t.clone()
        } else {
            format!("ppt/{t}")
        };
        if !name_set.contains(&full) {
            errors.push(format!("slide Relationship target missing: {t}"));
        }
    }

    let pres = read_zip_str(&mut zip, "ppt/presentation.xml").unwrap_or_default();
    let sld_re = Regex::new(r#"<p:sldId\b[^>]*r:id="([^"]+)""#).unwrap();
    let sld_ids: Vec<String> = sld_re
        .captures_iter(&pres)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();

    if sld_ids.len() != slide_rels.len() {
        errors.push(format!(
            "sldIdLst count ({}) != slide rels ({})",
            sld_ids.len(),
            slide_rels.len()
        ));
    }

    for rid in &sld_ids {
        match rid_map.get(rid) {
            None => errors.push(format!("sldId r:id not in rels: {rid}")),
            Some((_ty, t)) => {
                let full = if t.starts_with("ppt/") {
                    t.clone()
                } else {
                    format!("ppt/{t}")
                };
                if !name_set.contains(&full) {
                    errors.push(format!("sldIdLst points to missing slide: {t}"));
                }
            }
        }
    }

    let notes_re = Regex::new(r"ppt/slides/_rels/slide(\d+)\.xml\.rels").unwrap();
    let notes_slide_re = Regex::new(r"notesSlide(\d+)").unwrap();
    for name in &names {
        if let Some(m) = notes_re.captures(name) {
            let sn = &m[1];
            let txt = read_zip_str(&mut zip, name).unwrap_or_default();
            for nm in notes_slide_re.captures_iter(&txt) {
                let n = &nm[1];
                if n != sn {
                    errors.push(format!(
                        "slide{sn}.rels points to notesSlide{n} (must match or remove notes rel)"
                    ));
                }
                let notes = format!("ppt/notesSlides/notesSlide{n}.xml");
                if !name_set.contains(&notes) {
                    errors.push(format!("slide{sn}.rels notes target missing: {notes}"));
                }
            }
        }
    }

    // XML parse check (quick-xml well-formedness)
    for name in &names {
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        if let Ok(data) = read_zip_bytes(&mut zip, name) {
            let mut reader = quick_xml::Reader::from_reader(data.as_slice());
            reader.config_mut().trim_text(false);
            let mut buf = Vec::new();
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(quick_xml::events::Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => {
                        errors.push(format!("XML parse error {name}: {e}"));
                        break;
                    }
                }
                buf.clear();
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Stats {
        sld_count: sld_ids.len(),
        slide_files: slide_files.len(),
        slide_overrides: slide_overrides.len(),
    })
}

fn read_zip_str(zip: &mut ZipArchive<File>, name: &str) -> Result<String, ()> {
    read_zip_bytes(zip, name).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn read_zip_bytes(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, ()> {
    let mut f = zip.by_name(name).map_err(|_| ())?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|_| ())?;
    Ok(buf)
}

/// Exit-code style helper for CLI (0 = OK, 1 = fail).
pub fn validate_pptx_exit(path: &Path) -> i32 {
    match validate_pptx(path) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
