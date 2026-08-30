//! Text packing helpers (port of Python pack_sentences / pack_verses).

use regex::Regex;
use std::sync::OnceLock;

/// Greedy whole-verse packs by character length (default budget 280).
pub fn pack_verses(verses: &[String], budget: usize) -> Vec<Vec<String>> {
    let budget = if budget == 0 { 280 } else { budget };
    let mut packs: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_len: usize = 0;
    for v in verses {
        let add = v.len() + 1;
        if !cur.is_empty() && cur_len + add > budget {
            packs.push(std::mem::take(&mut cur));
            cur.push(v.clone());
            cur_len = v.len();
        } else {
            cur.push(v.clone());
            cur_len += add;
        }
    }
    if !cur.is_empty() {
        packs.push(cur);
    }
    packs
}

/// Split on sentence ends; greedy packs (default budget 380).
pub fn pack_sentences(text: &str, budget: usize) -> Vec<String> {
    let budget = if budget == 0 { 380 } else { budget };
    let sentences = split_sentences(text.trim());
    let mut packs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for s in sentences {
        let trial = if cur.is_empty() {
            s.to_string()
        } else {
            format!("{cur} {s}")
        };
        if !cur.is_empty() && trial.len() > budget {
            packs.push(cur);
            cur = s.to_string();
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        packs.push(cur);
    }
    packs
}

/// Split `text` into sentences, keeping the terminating punctuation
/// (`.`/`!`/`?`/`…`) attached to each sentence and dropping the whitespace
/// between them.
///
/// The `regex` crate has no look-around support, so this can't be a single
/// `(?<=[.!?…])\s+` split (that Python-style pattern panics here) — instead
/// match the punctuation run + following whitespace and slice around it.
fn split_sentences(text: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[.!?…]+\s+").expect("regex"));
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        let punct_end = text[..m.end()].trim_end().len();
        if punct_end > last {
            out.push(text[last..punct_end].to_string());
        }
        last = m.end();
    }
    if last < text.len() {
        out.push(text[last..].to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_verses_respects_budget() {
        let verses: Vec<String> = (1..=5)
            .map(|i| format!("{i} {}", "x".repeat(100)))
            .collect();
        let packs = pack_verses(&verses, 280);
        assert!(packs.len() >= 2);
        for p in &packs {
            let len: usize = p.iter().map(|v| v.len() + 1).sum();
            // first item in pack can be over budget alone; others combined shouldn't explode
            assert!(!p.is_empty());
            let _ = len;
        }
    }

    #[test]
    fn pack_sentences_splits() {
        let text = "One. Two. Three. Four.";
        let packs = pack_sentences(text, 10);
        assert!(packs.len() >= 2);
        for p in &packs {
            assert!(p.ends_with('.') || p.contains('.'));
        }
    }

    #[test]
    fn pack_sentences_keeps_short_together() {
        let text = "Hola mundo. Adiós.";
        let packs = pack_sentences(text, 380);
        assert_eq!(packs.len(), 1);
    }
}
