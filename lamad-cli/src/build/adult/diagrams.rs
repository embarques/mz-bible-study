//! Adult Pensamiento Central / Texto Áureo SmartArt fill.
//!
//! The adult `data2` / `drawing2` diagrams have **three** content slots in
//! different orders. Blind "longest N texts" replacement mis-fills them
//! (pensamiento lands in the quote slot, citation leftover overflows).

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::build::ooxml::xml::{self, Elem};

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
    if drawing_path.is_file() {
        fill_file(drawing_path, pensamiento, quote, cita)?;
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
}
