//! Adult SmartArt fill — Objetivos + Pensamiento / Texto Áureo.
//!
//! Objetivos (`data1`/`drawing1`): fill by SmartArt **`srcOrd`** (visual cascade
//! order), not raw document order of `<a:t>` nodes — those disagree on the
//! adult vProcess template and scramble 1→2→3 on the slide.
//!
//! Pensamiento (`data2`/`drawing2`): three content slots in different orders.
//! Blind "longest N texts" replacement mis-fills them.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::build::ooxml::xml::{self, Elem};

/// Fill the three Objetivos cascade boxes in **print order** (1 top → 3 bottom).
///
/// Also shrinks drawing body runs so long Spanish lines stay inside the blue boxes.
pub fn set_objetivos(
    data_path: &Path,
    drawing_path: &Path,
    objetivos: &[String],
) -> Result<()> {
    if objetivos.len() != 3 {
        bail!("objetivos must have 3 entries, got {}", objetivos.len());
    }
    let data = fs::read_to_string(data_path)
        .with_context(|| format!("read {}", data_path.display()))?;
    let pairs = objetivos_srcord_replacements(&data, objetivos)?;
    let new_data = apply_model_id_text_replacements(&data, &pairs)?;
    fs::write(data_path, new_data).with_context(|| format!("write {}", data_path.display()))?;

    if drawing_path.is_file() {
        let drawing = fs::read_to_string(drawing_path)
            .with_context(|| format!("read {}", drawing_path.display()))?;
        // Cached drawing shape Y order ≠ data srcOrd on this template. Write
        // top→bottom by Y so PDF export stays correct even before PowerPoint
        // regenerates SmartArt from data.
        let new_drawing = apply_drawing_by_y_order(&drawing, objetivos)?;
        fs::write(drawing_path, &new_drawing)
            .with_context(|| format!("write {}", drawing_path.display()))?;
        shrink_objetivos_runs(drawing_path, objetivos)?;
    }
    Ok(())
}

/// `(model_id, old_text, new_text)` in srcOrd order 0..2.
fn objetivos_srcord_replacements(
    data_xml: &str,
    objetivos: &[String],
) -> Result<Vec<(String, String, String)>> {
    // modelId → current objective text
    let pt_re = Regex::new(r#"(?s)<dgm:pt\b([^>]*)>(.*?)</dgm:pt>"#).unwrap();
    let mut id_to_text: Vec<(String, String)> = Vec::new();
    for cap in pt_re.captures_iter(data_xml) {
        let attrs = &cap[1];
        let body = &cap[2];
        let Some(mid) = Regex::new(r#"modelId="([^"]+)""#)
            .unwrap()
            .captures(attrs)
            .map(|c| c[1].to_string())
        else {
            continue;
        };
        let texts: Vec<String> = Regex::new(r#"(?s)<a:t[^>]*>(.*?)</a:t>"#)
            .unwrap()
            .captures_iter(body)
            .map(|c| {
                c[1].replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
            })
            .filter(|t| !t.trim().is_empty())
            .collect();
        if let Some(t) = texts.into_iter().next() {
            id_to_text.push((mid, t));
        }
    }
    if id_to_text.len() < 3 {
        bail!(
            "objetivos data diagram: expected ≥3 text points, found {}",
            id_to_text.len()
        );
    }

    let cxn = Regex::new(r#"(?s)<dgm:cxnLst>(.*?)</dgm:cxnLst>"#)
        .unwrap()
        .captures(data_xml)
        .map(|c| c[1].to_string())
        .context("objetivos data diagram missing dgm:cxnLst")?;
    // Only parent→step edges (no type=pres*). Presentation links also carry
    // srcOrd and will scramble 1→2→3 if mixed in. PowerPoint paints by these.
    let cxn_re = Regex::new(
        r#"<dgm:cxn\b([^>]*)/>"#,
    )
    .unwrap();
    let mut ord_to_id: Vec<(u32, String)> = Vec::new();
    for cap in cxn_re.captures_iter(&cxn) {
        let attrs = &cap[1];
        if attrs.contains("type=\"pres") {
            continue;
        }
        let Some(ord) = Regex::new(r#"\bsrcOrd="(\d+)""#)
            .unwrap()
            .captures(attrs)
            .and_then(|c| c[1].parse::<u32>().ok())
        else {
            continue;
        };
        let Some(id) = Regex::new(r#"\bdestId="([^"]+)""#)
            .unwrap()
            .captures(attrs)
            .map(|c| c[1].to_string())
        else {
            continue;
        };
        if id_to_text.iter().any(|(m, _)| m == &id) {
            ord_to_id.push((ord, id));
        }
    }
    ord_to_id.sort_by_key(|(o, _)| *o);
    ord_to_id.dedup_by_key(|(_, id)| id.clone());
    if ord_to_id.len() != 3 {
        bail!(
            "objetivos data diagram: need 3 srcOrd→text links, found {}",
            ord_to_id.len()
        );
    }
    for (i, (ord, _)) in ord_to_id.iter().enumerate() {
        if *ord != i as u32 {
            bail!(
                "objetivos data diagram: expected srcOrd {i}, got {ord}"
            );
        }
    }

    let mut out = Vec::with_capacity(3);
    for (i, (_, id)) in ord_to_id.iter().enumerate() {
        let old = id_to_text
            .iter()
            .find(|(m, _)| m == id)
            .map(|(_, t)| t.clone())
            .with_context(|| format!("missing text for modelId {id}"))?;
        out.push((id.clone(), old, objetivos[i].clone()));
    }
    Ok(out)
}

fn apply_model_id_text_replacements(
    data_xml: &str,
    pairs: &[(String, String, String)],
) -> Result<String> {
    let pt_re = Regex::new(r#"(?s)<dgm:pt\b[^>]*>.*?</dgm:pt>"#).unwrap();
    let mut out = String::with_capacity(data_xml.len());
    let mut last = 0usize;
    for m in pt_re.find_iter(data_xml) {
        out.push_str(&data_xml[last..m.start()]);
        let mut block = m.as_str().to_string();
        for (id, _old, new) in pairs {
            if block.contains(&format!("modelId=\"{id}\"")) {
                block = replace_first_a_t(&block, new)?;
                break;
            }
        }
        out.push_str(&block);
        last = m.end();
    }
    out.push_str(&data_xml[last..]);
    Ok(out)
}

fn apply_drawing_by_y_order(drawing_xml: &str, objetivos: &[String]) -> Result<String> {
    let sp_re = Regex::new(r"(?s)<dsp:sp\b.*?</dsp:sp>").unwrap();
    let mut shapes: Vec<(usize, usize, i64, Elem)> = Vec::new();
    for m in sp_re.find_iter(drawing_xml) {
        let block = m.as_str();
        let y = Regex::new(r#"<a:off[^>]*\by="(\d+)""#)
            .unwrap()
            .captures(block)
            .and_then(|c| c[1].parse::<i64>().ok())
            .unwrap_or(i64::MAX);
        let nodes = xml::all_elements(block, "a:t");
        let Some(el_local) = nodes
            .into_iter()
            .find(|e| !e.inner(block, "a:t").trim().is_empty())
        else {
            continue;
        };
        // Remap local offsets to absolute in drawing_xml.
        let el = Elem {
            start: m.start() + el_local.start,
            open_end: m.start() + el_local.open_end,
            end: m.start() + el_local.end,
            self_closing: el_local.self_closing,
        };
        shapes.push((m.start(), m.end(), y, el));
    }
    if shapes.len() != 3 {
        bail!(
            "objetivos drawing: expected 3 text shapes, found {}",
            shapes.len()
        );
    }
    shapes.sort_by_key(|(_, _, y, _)| *y);

    // Replace bottom-up so earlier offsets stay valid.
    let mut xml = drawing_xml.to_string();
    for (i, (_, _, _, el)) in shapes.iter().enumerate().rev() {
        let new = &objetivos[i];
        let mut next = String::with_capacity(xml.len());
        next.push_str(&xml[..el.start]);
        next.push_str(&xml::build_t(new));
        next.push_str(&xml[el.end..]);
        xml = next;
    }
    Ok(xml)
}

fn replace_first_a_t(block: &str, new_text: &str) -> Result<String> {
    let nodes = xml::all_elements(block, "a:t");
    let el = nodes
        .into_iter()
        .find(|e| !e.inner(block, "a:t").trim().is_empty())
        .context("dgm:pt has no non-empty a:t")?;
    Ok(format!(
        "{}{}{}",
        &block[..el.start],
        xml::build_t(new_text),
        &block[el.end..]
    ))
}

fn shrink_objetivos_runs(drawing_path: &Path, objetivos: &[String]) -> Result<()> {
    let xml = fs::read_to_string(drawing_path)
        .with_context(|| format!("read {}", drawing_path.display()))?;
    let longest = objetivos
        .iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0);
    // Cascade boxes are short — template ~36–40pt overflows mid-length Spanish.
    let target: u32 = if longest > 110 {
        2200
    } else if longest > 90 {
        2400
    } else if longest > 70 {
        2800
    } else {
        3200
    };
    let re = Regex::new(r#"(<(?:a:rPr|a:endParaRPr)\b[^>]*\bsz=")(\d+)(")"#).unwrap();
    let out = re.replace_all(&xml, |caps: &regex::Captures| {
        let sz: u32 = caps[2].parse().unwrap_or(0);
        if sz >= 2400 {
            format!("{}{}{}", &caps[1], target, &caps[3])
        } else {
            caps[0].to_string()
        }
    });
    fs::write(drawing_path, out.as_ref())
        .with_context(|| format!("write {}", drawing_path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Pensamiento,
    Quote,
    Cita,
}

/// Fill Pensamiento Central + Texto Áureo quote + citation (no parentheses).
pub fn set_pensamiento_aureo(
    data_path: &Path,
    drawing_path: &Path,
    pensamiento: &str,
    quote: &str,
    cita: &str,
) -> Result<()> {
    fill_file(data_path, pensamiento, quote, cita)?;
    split_cita_onto_next_line(data_path)?;
    strip_file_highlights(data_path)?;
    if drawing_path.is_file() {
        fill_file(drawing_path, pensamiento, quote, cita)?;
        // Gold keeps quote + cita as two runs in one paragraph → same line.
        // Put the biblical citation on its own line under the quote.
        split_cita_onto_next_line(drawing_path)?;
        strip_file_highlights(drawing_path)?;
        // Long pensamiento/quote at template ~44pt overflows the boxes.
        shrink_body_runs(drawing_path, pensamiento, quote)?;
    }
    Ok(())
}

fn fill_file(path: &Path, pensamiento: &str, quote: &str, cita: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let nodes = xml::all_elements(&xml, "a:t");
    let non_empty: Vec<Elem> = nodes
        .iter()
        .copied()
        .filter(|e| !e.inner(&xml, "a:t").trim().is_empty())
        .collect();

    let mut slots: Vec<(Elem, Slot)> = Vec::new();
    for el in &non_empty {
        let t = el.inner(&xml, "a:t");
        if let Some(slot) = classify_slot(t) {
            slots.push((*el, slot));
        }
    }

    let mut seen = [false; 3];
    for (_, s) in &slots {
        match s {
            Slot::Pensamiento => seen[0] = true,
            Slot::Quote => seen[1] = true,
            Slot::Cita => seen[2] = true,
        }
    }
    if !seen[0] || !seen[1] || !seen[2] {
        bail!(
            "pensamiento/aureo diagram {}: missing slots (pensamiento={} quote={} cita={})",
            path.display(),
            seen[0],
            seen[1],
            seen[2]
        );
    }

    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;
    // Replace in document order.
    let mut ordered = slots;
    ordered.sort_by_key(|(e, _)| e.start);
    for (el, slot) in ordered {
        let text = match slot {
            Slot::Pensamiento => pensamiento,
            Slot::Quote => quote,
            Slot::Cita => cita,
        };
        out.push_str(&xml[cursor..el.start]);
        out.push_str(el.open_tag(&xml));
        out.push_str(&xml::escape_text(text));
        out.push_str("</a:t>");
        cursor = el.end;
    }
    out.push_str(&xml[cursor..]);
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn classify_slot(raw: &str) -> Option<Slot> {
    let t = raw.trim();
    if t.is_empty() || t == " " {
        return None;
    }
    let chars = t.chars().count();
    // Footer citation: short, has chapter:verse.
    if chars < 48 && t.contains(':') && !t.contains('“') && !t.contains('"') {
        return Some(Slot::Cita);
    }
    // Quote: starts with quotation marks or leading ellipsis inside quotes.
    let first = t.chars().next().unwrap_or(' ');
    if first == '“' || first == '"' || first == '…' || t.starts_with("...") || t.starts_with("…")
    {
        return Some(Slot::Quote);
    }
    if t.contains('“') || (t.contains('"') && chars > 40) {
        return Some(Slot::Quote);
    }
    // Remaining long prose = pensamiento.
    if chars > 20 {
        return Some(Slot::Pensamiento);
    }
    None
}

/// If a paragraph has quote-run then citation-run, split into two `<a:p>`s
/// so the biblical reference sits on the next line.
fn split_cita_onto_next_line(path: &Path) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let para_re = Regex::new(r"(?s)<a:p\b[^>]*>.*?</a:p>").unwrap();
    let run_re = Regex::new(r"(?s)<a:r\b[^>]*>.*?</a:r>").unwrap();
    let t_re = Regex::new(r"(?s)<a:t[^>]*>(.*?)</a:t>").unwrap();
    let ppr_re = Regex::new(r"(?s)<a:pPr\b[^>]*/>|<a:pPr\b[^>]*>.*?</a:pPr>").unwrap();
    let end_re = Regex::new(r"(?s)<a:endParaRPr\b[^>]*/>|<a:endParaRPr\b[^>]*>.*?</a:endParaRPr>").unwrap();

    let mut out = String::with_capacity(xml.len());
    let mut last = 0usize;
    let mut changed = false;
    for cap in para_re.find_iter(&xml) {
        out.push_str(&xml[last..cap.start()]);
        let para = cap.as_str();
        let runs: Vec<&str> = run_re.find_iter(para).map(|m| m.as_str()).collect();
        if runs.len() == 2 {
            let t0 = t_re
                .captures(runs[0])
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            let t1 = t_re
                .captures(runs[1])
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            let quote_like = classify_slot(&t0) == Some(Slot::Quote)
                || t0.contains('“')
                || t0.starts_with('"');
            let cita_like = classify_slot(&t1) == Some(Slot::Cita);
            if quote_like && cita_like {
                let ppr = ppr_re
                    .find(para)
                    .map(|m| m.as_str())
                    .unwrap_or(r#"<a:pPr/>"#);
                let end = end_re
                    .find(para)
                    .map(|m| m.as_str())
                    .unwrap_or(r#"<a:endParaRPr lang="es-DO"/>"#);
                // Keep quote paragraph; citation starts a new paragraph.
                let split = format!(
                    "<a:p>{ppr}{run0}{end}</a:p><a:p>{ppr}{run1}{end}</a:p>",
                    run0 = runs[0],
                    run1 = runs[1],
                );
                out.push_str(&split);
                changed = true;
                last = cap.end();
                continue;
            }
        }
        out.push_str(para);
        last = cap.end();
    }
    out.push_str(&xml[last..]);
    if changed {
        fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn strip_file_highlights(path: &Path) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let re = Regex::new(r"<a:highlight\b[^>]*>[\s\S]*?</a:highlight>|<a:highlight\b[^/]*/>")
        .unwrap();
    let out = re.replace_all(&xml, "");
    if out.as_ref() != xml {
        fs::write(path, out.as_ref()).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn shrink_body_runs(drawing_path: &Path, pensamiento: &str, quote: &str) -> Result<()> {
    let xml = fs::read_to_string(drawing_path)
        .with_context(|| format!("read {}", drawing_path.display()))?;
    let longest = pensamiento.chars().count().max(quote.chars().count());
    // Template drawing runs sit around 44pt and overflow long Spanish.
    let target: u32 = if longest > 160 {
        2200
    } else if longest > 110 {
        2600
    } else if longest > 80 {
        3000
    } else {
        3400
    };
    // Match both `<a:rPr …>` and self-closing `<a:rPr …/>`, plus endParaRPr.
    let re = Regex::new(r#"(<(?:a:rPr|a:endParaRPr)\b[^>]*\bsz=")(\d+)(")"#).unwrap();
    let out = re.replace_all(&xml, |caps: &regex::Captures| {
        let sz: u32 = caps[2].parse().unwrap_or(0);
        // Only shrink body-sized runs; leave tiny labels alone.
        if sz >= 2800 {
            format!("{}{}{}", &caps[1], target, &caps[3])
        } else {
            caps[0].to_string()
        }
    });
    fs::write(drawing_path, out.as_ref())
        .with_context(|| format!("write {}", drawing_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objetivos_follow_srcord_not_document_order() {
        let tmpl = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("template/adult/master-template.pptx");
        if !tmpl.is_file() {
            // Repo-root template fallback used by some checkouts.
            return;
        }
        let dir = std::env::temp_dir().join(format!("lamad-obj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ppt/diagrams")).unwrap();
        // Extract via unzip CLI for the test.
        let status = std::process::Command::new("unzip")
            .args([
                "-q",
                "-o",
                tmpl.to_str().unwrap(),
                "ppt/diagrams/data1.xml",
                "ppt/diagrams/drawing1.xml",
                "-d",
                dir.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        let data = dir.join("ppt/diagrams/data1.xml");
        let drawing = dir.join("ppt/diagrams/drawing1.xml");
        let objs = vec![
            "UNO-analizar objetivo primero del estudio.".into(),
            "DOS-reconocer objetivo segundo del estudio.".into(),
            "TRES-confiar objetivo tercero del estudio.".into(),
        ];
        set_objetivos(&data, &drawing, &objs).unwrap();
        let data_xml = std::fs::read_to_string(&data).unwrap();
        let pairs = objetivos_srcord_replacements(&data_xml, &objs).unwrap();
        // After fill, srcOrd 0/1/2 texts must be UNO/DOS/TRES.
        assert!(pairs[0].1.starts_with("UNO"), "srcOrd0 old unexpected: {}", pairs[0].1);
        // Re-parse by reading current texts via srcOrd map on filled file:
        let filled = objetivos_srcord_replacements(
            &data_xml,
            &["a".into(), "b".into(), "c".into()],
        )
        .unwrap();
        assert!(
            filled[0].1.starts_with("UNO"),
            "srcOrd0 should be first objetivo, got {}",
            filled[0].1
        );
        assert!(filled[1].1.starts_with("DOS"), "{}", filled[1].1);
        assert!(filled[2].1.starts_with("TRES"), "{}", filled[2].1);
        let drawing_xml = std::fs::read_to_string(&drawing).unwrap();
        assert!(drawing_xml.contains("UNO-analizar"));
        assert!(drawing_xml.contains("DOS-reconocer"));
        assert!(drawing_xml.contains("TRES-confiar"));
        // Top→bottom by Y must match print order.
        let sp_re = Regex::new(r"(?s)<dsp:sp\b.*?</dsp:sp>").unwrap();
        let mut rows = Vec::new();
        for m in sp_re.find_iter(&drawing_xml) {
            let b = m.as_str();
            let y = Regex::new(r#"y="(\d+)""#)
                .unwrap()
                .captures(b)
                .and_then(|c| c[1].parse::<i64>().ok())
                .unwrap_or(0);
            if let Some(t) = Regex::new(r"<a:t[^>]*>([^<]+)</a:t>")
                .unwrap()
                .captures(b)
                .map(|c| c[1].to_string())
            {
                if !t.trim().is_empty() {
                    rows.push((y, t));
                }
            }
        }
        rows.sort_by_key(|(y, _)| *y);
        assert!(rows[0].1.starts_with("UNO"), "{:?}", rows[0]);
        assert!(rows[1].1.starts_with("DOS"), "{:?}", rows[1]);
        assert!(rows[2].1.starts_with("TRES"), "{:?}", rows[2]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classifies_gold_slots() {
        assert_eq!(
            classify_slot("El enfoque de la vida cristiana no debe estar vanidosamente"),
            Some(Slot::Pensamiento)
        );
        assert_eq!(
            classify_slot(
                "“… como también yo en todas las cosas agrado a todos”"
            ),
            Some(Slot::Quote)
        );
        assert_eq!(
            classify_slot("(1 Corintios 10:33)."),
            Some(Slot::Cita)
        );
        assert_eq!(classify_slot("1 Corintios 10:33"), Some(Slot::Cita));
    }

    #[test]
    fn splits_quote_and_cita_into_two_paragraphs() {
        let path = std::env::temp_dir().join("lamad-aureo-split-test.xml");
        let xml = r#"<?xml version="1.0"?>
<root><a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:pPr algn="l"/><a:r><a:rPr sz="2200"/><a:t>“... para que sean salvos”</a:t></a:r><a:r><a:rPr sz="2200" b="1"/><a:t>1 Corintios 10:33</a:t></a:r><a:endParaRPr lang="es-DO"/></a:p></root>"#;
        std::fs::write(&path, xml).unwrap();
        split_cita_onto_next_line(&path).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(out.matches("<a:p>").count() + out.matches("<a:p ").count(), 2);
        assert!(out.contains("sean salvos”</a:t></a:r><a:endParaRPr"));
        assert!(out.contains("<a:t>1 Corintios 10:33</a:t>"));
        assert!(!out.contains("salvos”</a:t></a:r><a:r>"));
    }
}
