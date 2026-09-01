//! Lectura / Texto Bíblico verse slides.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

use super::shape::{
    all_runs, clone_run, find_txbody, first_para_ppr, replace_range, transform_shape,
};
use super::xml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerseKind {
    Lectura,
    Texto,
}

/// Split `"18 Y el niño creció..."` into `("18", " Y el niño creció...")`.
pub(crate) fn split_verse(v: &str) -> Result<(String, String)> {
    let re = Regex::new(r"(?s)^(\d+)\s+(.*)$").unwrap();
    let caps = re
        .captures(v.trim_start())
        .ok_or_else(|| anyhow!("verse must start with number: {:.60}", v))?;
    Ok((caps[1].to_string(), format!(" {}", &caps[2])))
}

struct VerseSamples {
    cite: Option<String>,
    num: Option<String>,
    body: Option<String>,
}

/// Pick cite/number/body run samples by content, not fixed index —
/// continuation Lectura/Texto slides start with a number run, so
/// index-based picking would swap colors (port of `pick_verse_samples`).
fn pick_verse_samples(runs: &[String]) -> VerseSamples {
    let mut sample_num: Option<String> = None;
    let mut sample_body: Option<String> = None;
    let mut sample_cite: Option<String> = None;

    for r in runs {
        let text = xml::run_text(r);
        let stripped = text.trim();
        if stripped.is_empty() {
            continue;
        }
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            if sample_num.is_none() {
                sample_num = Some(r.clone());
            }
            continue;
        }
        if sample_cite.is_none() && stripped.contains(':') && stripped.chars().count() < 40 {
            sample_cite = Some(r.clone());
            continue;
        }
        if sample_body.is_none() && (text.starts_with(' ') || stripped.chars().count() > 15) {
            sample_body = Some(r.clone());
        }
    }

    if sample_num.is_none() {
        sample_num = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
            })
            .cloned()
            .or_else(|| runs.first().cloned());
    }
    if sample_body.is_none() {
        sample_body = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            })
            .cloned()
            .or_else(|| runs.last().cloned());
    }
    if sample_cite.is_none() {
        sample_cite = sample_num.clone();
    }

    VerseSamples {
        cite: sample_cite,
        num: sample_num,
        body: sample_body,
    }
}

/// Fill a Lectura/Texto Bíblico slide with whole verses (+ optional
/// citation on the first slide of a range). One paragraph per citation /
/// per verse — no soft `a:br` breaks (matches known-good decks).
pub fn set_verses(path: &Path, verses: &[String], citation: Option<&str>, kind: VerseKind) -> Result<()> {
    let shape_name = match kind {
        VerseKind::Lectura => "CuadroTexto 5",
        VerseKind::Texto => "TextBox 4",
    };
    let accent = match kind {
        VerseKind::Lectura => "FF0000",
        VerseKind::Texto => "FFFF00",
    };
    let body_rgb: Option<&str> = match kind {
        VerseKind::Lectura => None, // default black — leave inherited
        VerseKind::Texto => Some("FFFFFF"),
    };

    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape_name, |shape_xml| {
        let (tb, tag) = find_txbody(shape_xml)?;
        let content = tb.inner(shape_xml, tag);
        let runs = all_runs(content);
        if runs.is_empty() {
            bail!("no sample runs in shape {shape_name}");
        }
        let samples = pick_verse_samples(&runs);

        let p_start = xml::next_element(content, "a:p", 0)
            .map(|e| e.start)
            .unwrap_or(content.len());
        let head = &content[..p_start];
        let ppr = first_para_ppr(content).unwrap_or_default();

        let mut body = String::new();
        if let Some(cite) = citation {
            let r = clone_run(samples.cite.as_deref(), cite, Some(true), Some(Some(accent)), None)?;
            body.push_str(&format!("<a:p>{ppr}{r}</a:p>"));
        }
        for v in verses {
            let (num, verse_body) = split_verse(v)?;
            let r_num = clone_run(samples.num.as_deref(), &num, Some(true), Some(Some(accent)), None)?;
            // body_rgb=None means "leave inherited" (lectura black body) —
            // color=None (outer Option) skips touching the fill entirely.
            let color_opt = body_rgb.map(Some);
            let r_body = clone_run(samples.body.as_deref(), &verse_body, Some(false), color_opt, None)?;
            body.push_str(&format!("<a:p>{ppr}{r_num}{r_body}</a:p>"));
        }

        let new_content = format!("{head}{body}");
        let open_tag = tb.open_tag(shape_xml);
        let new_txbody = format!("{open_tag}{new_content}</{tag}>");
        Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}
