//! SmartArt / diagram text (Propósitos 36pt, Idea / Memorizar).

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

use super::xml::{self, Elem};

pub fn replace_diagram_texts(
    data_path: &Path,
    drawing_path: Option<&Path>,
    texts: &[String],
    force_drawing_sz: Option<u32>,
) -> Result<()> {
    let xml = fs::read_to_string(data_path).with_context(|| format!("read {}", data_path.display()))?;
    let new_xml = replace_longest_texts(&xml, texts)
        .with_context(|| format!("diagram text count mismatch in {}", data_path.display()))?;
    fs::write(data_path, new_xml).with_context(|| format!("write {}", data_path.display()))?;

    if let Some(dp) = drawing_path {
        if dp.is_file() {
            let dxml = fs::read_to_string(dp).with_context(|| format!("read {}", dp.display()))?;
            let dxml = replace_longest_texts(&dxml, texts).unwrap_or(dxml);
            let dxml = match force_drawing_sz {
                Some(sz) => force_min_sz(&dxml, 5000, sz),
                None => dxml,
            };
            fs::write(dp, dxml).with_context(|| format!("write {}", dp.display()))?;
        }
    }
    Ok(())
}

/// Force every `<a:rPr ... sz="NNNN" ...>` with `sz >= threshold` down to
/// `new_sz` (Propósitos template runs at 5600 → must become 3600, see
/// AGENTS.md hard rule).
pub fn force_propositos_36pt(drawing2: &Path) -> Result<()> {
    let xml = fs::read_to_string(drawing2).with_context(|| format!("read {}", drawing2.display()))?;
    let xml = force_min_sz(&xml, 5000, 3600);
    fs::write(drawing2, xml).with_context(|| format!("write {}", drawing2.display()))
}

fn force_min_sz(xml: &str, threshold: u32, new_sz: u32) -> String {
    let re = Regex::new(r#"(<a:rPr\b[^>]*\bsz=")(\d+)("[^>]*>)"#).unwrap();
    re.replace_all(xml, |caps: &regex::Captures| {
        let sz: u32 = caps[2].parse().unwrap_or(0);
        if sz >= threshold {
            format!("{}{}{}", &caps[1], new_sz, &caps[3])
        } else {
            caps[0].to_string()
        }
    })
    .into_owned()
}

fn replace_longest_texts(xml: &str, texts: &[String]) -> Result<String> {
    let nodes = xml::all_elements(xml, "a:t");
    let non_empty: Vec<Elem> = nodes
        .iter()
        .copied()
        .filter(|e| !e.inner(xml, "a:t").trim().is_empty())
        .collect();

    let mut candidates: Vec<Elem> = non_empty
        .iter()
        .copied()
        .filter(|e| e.inner(xml, "a:t").trim().chars().count() > 15)
        .take(texts.len())
        .collect();

    if candidates.len() != texts.len() {
        let n = texts.len();
        candidates = non_empty
            .iter()
            .rev()
            .take(n)
            .rev()
            .copied()
            .collect();
    }
    if candidates.len() != texts.len() {
        bail!(
            "need {} text node(s), found {}",
            texts.len(),
            candidates.len()
        );
    }

    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;
    for (el, text) in candidates.iter().zip(texts.iter()) {
        out.push_str(&xml[cursor..el.start]);
        out.push_str(el.open_tag(xml));
        out.push_str(&xml::escape_text(text));
        out.push_str("</a:t>");
        cursor = el.end;
    }
    out.push_str(&xml[cursor..]);
    Ok(out)
}

/// Overwrite the first `<a:t>` node whose trimmed text is short (`< 40`
/// chars) and contains `:` — the diagram-footer citation slot (e.g. Para
/// Memorizar's `2 Reyes 4:21`).
pub fn set_diagram_citation(data_path: &Path, cite: &str) -> Result<()> {
    let xml = fs::read_to_string(data_path).with_context(|| format!("read {}", data_path.display()))?;
    let nodes = xml::all_elements(&xml, "a:t");
    let target = nodes.into_iter().find(|e| {
        let t = e.inner(&xml, "a:t");
        let s = t.trim();
        !s.is_empty() && s.chars().count() < 40 && s.contains(':')
    });
    let Some(el) = target else { return Ok(()) };
    let new_xml = format!(
        "{}{}{}",
        &xml[..el.start],
        xml::build_t(cite),
        &xml[el.end..]
    );
    fs::write(data_path, new_xml).with_context(|| format!("write {}", data_path.display()))
}
