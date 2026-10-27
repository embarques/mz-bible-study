//! Text packing helpers (port of Python pack_sentences / pack_verses).

use regex::Regex;
use std::sync::OnceLock;

/// Greedy whole-verse packs by **Unicode character** length (default 280).
/// Never splits a verse; if the next whole verse would exceed the budget,
/// start a new pack (new slide). A single verse longer than the budget
/// still gets its own slide (cannot mid-verse split).
pub fn pack_verses(verses: &[String], budget: usize) -> Vec<Vec<String>> {
    let budget = if budget == 0 { 280 } else { budget };
    let mut packs: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_len: usize = 0;
    for v in verses {
        let v_len = v.chars().count();
        let add = v_len + 1;
        if !cur.is_empty() && cur_len + add > budget {
            packs.push(std::mem::take(&mut cur));
            cur.push(v.clone());
            cur_len = v_len;
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

/// True when every pack's joined char length is ≤ budget (single oversize
/// verse alone is allowed — same rule as [`pack_verses`]).
pub fn verse_packs_within_budget(packs: &[Vec<String>], budget: usize) -> bool {
    let budget = if budget == 0 { 280 } else { budget };
    packs.iter().all(|pack| {
        if pack.len() <= 1 {
            return true;
        }
        let len: usize = pack
            .iter()
            .map(|v| v.chars().count() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        len <= budget
    })
}

/// Split on sentence ends; greedy packs by **Unicode character** length
/// (default budget 380). Never splits mid-sentence.
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
        if !cur.is_empty() && trial.chars().count() > budget {
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

/// Evenly distribute sentences across the minimum number of slides that fit
/// under `budget` (logo / footer floor).
///
/// 1. Split into sentences; count total chars.  
/// 2. Start from `n = max(ceil(total/budget), greedy_min_slides)`.  
/// 3. Cut at cumulative targets (`total*i/n`) on sentence boundaries so packs
///    are balanced and each stays ≤ `budget`. Bump `n` if any cut overflows.
///
/// Use for adult Introducción / A-B bodies so dense JSON slides no longer
/// clip under the Mount Zion logo top.
pub fn pack_sentences_even(text: &str, budget: usize) -> Vec<String> {
    let budget = if budget == 0 { 380 } else { budget };
    let sentences = split_sentences(text.trim());
    if sentences.is_empty() {
        return Vec::new();
    }
    // Per-sentence sizes + join spaces (space before sentence i>0).
    let mut sizes = Vec::with_capacity(sentences.len());
    for (i, s) in sentences.iter().enumerate() {
        let c = s.chars().count();
        sizes.push(if i == 0 { c } else { c + 1 });
    }
    let total: usize = sizes.iter().sum();
    let greedy_n = pack_sentences(text, budget).len().max(1);
    let mut n = ((total + budget - 1) / budget).max(greedy_n);
    let max_n = sentences.len().max(1);
    while n <= max_n {
        if let Some(packs) = try_even_packs(&sentences, &sizes, n, budget, total) {
            return packs;
        }
        n += 1;
    }
    pack_sentences(text, budget)
}

fn try_even_packs(
    sentences: &[String],
    sizes: &[usize],
    n: usize,
    budget: usize,
    total: usize,
) -> Option<Vec<String>> {
    if n == 0 || sentences.is_empty() || sizes.len() != sentences.len() {
        return None;
    }
    // Cumulative char cost including leading spaces.
    let mut cum = Vec::with_capacity(sentences.len());
    let mut run = 0usize;
    for &sz in sizes {
        run += sz;
        cum.push(run);
    }

    // Cut after sentence index `cuts[i]` (exclusive end) for pack i.
    let mut ends: Vec<usize> = Vec::with_capacity(n);
    let mut start = 0usize;
    for pack_i in 0..n {
        let packs_left_including = n - pack_i;
        let sents_left = sentences.len() - start;
        if sents_left < packs_left_including {
            return None;
        }
        if pack_i == n - 1 {
            ends.push(sentences.len());
            break;
        }
        // Absolute cumulative target for the end of this pack.
        let target = total * (pack_i + 1) / n;
        let min_end = start + 1;
        let max_end = sentences.len() - (n - pack_i - 1);
        let mut best = None::<(usize, usize)>; // (abs_diff, end)
        for end in min_end..=max_end {
            let prev_cum = if start == 0 { 0 } else { cum[start - 1] };
            let pack_len = cum[end - 1] - prev_cum;
            if pack_len == 0 || pack_len > budget {
                continue;
            }
            let abs_diff = cum[end - 1].abs_diff(target);
            match best {
                None => best = Some((abs_diff, end)),
                Some((d, _)) if abs_diff < d => best = Some((abs_diff, end)),
                // Prefer later cut on a tie so packs stay fuller / more even.
                Some((d, e)) if abs_diff == d && end > e => best = Some((abs_diff, end)),
                _ => {}
            }
        }
        let end = best?.1;
        ends.push(end);
        start = end;
    }

    let mut packs = Vec::with_capacity(n);
    let mut prev = 0usize;
    for end in ends {
        let joined = sentences[prev..end].join(" ");
        if joined.chars().count() > budget {
            return None;
        }
        packs.push(joined);
        prev = end;
    }
    Some(packs)
}

/// Join pre-packed slides and re-pack evenly under `budget` (adult body).
pub fn rebalance_body_slides(slides: &[String], budget: usize) -> Vec<String> {
    let joined = slides
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return Vec::new();
    }
    pack_sentences_even(&joined, budget)
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
        assert!(verse_packs_within_budget(&packs, 280));
    }

    #[test]
    fn verse_packs_within_budget_rejects_overstuffed_pack() {
        let fat = vec![vec![
            format!("1 {}", "a".repeat(200)),
            format!("2 {}", "b".repeat(200)),
        ]];
        assert!(!verse_packs_within_budget(&fat, 280));
        let ok = vec![vec![format!("1 {}", "a".repeat(100))]];
        assert!(verse_packs_within_budget(&ok, 280));
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

    #[test]
    fn pack_sentences_even_balances_and_respects_budget() {
        // Four long sentences (~200 chars each) → need ≥3 slides at budget 360.
        let s = "A".repeat(180);
        let text = format!("{s} uno. {s} dos. {s} tres. {s} cuatro.");
        let packs = pack_sentences_even(&text, 360);
        assert!(packs.len() >= 3, "expected ≥3 packs, got {}", packs.len());
        for p in &packs {
            assert!(
                p.chars().count() <= 360,
                "pack over budget: {} chars",
                p.chars().count()
            );
        }
        // Rough evenness: no pack more than ~1.5× the smallest (when multi-slide).
        if packs.len() > 1 {
            let lens: Vec<_> = packs.iter().map(|p| p.chars().count()).collect();
            let min = *lens.iter().min().unwrap();
            let max = *lens.iter().max().unwrap();
            assert!(
                max <= min * 2 + 40,
                "packs too uneven: min={min} max={max} lenses={lens:?}"
            );
        }
    }

    #[test]
    fn rebalance_body_slides_fixes_overstuffed_intro() {
        let slides = vec![
            "Primera oración larga sobre el tema del estudio bíblico para la introducción. Segunda oración continúa el pensamiento con más detalle histórico.".into(),
            "Tercera oración añade contexto sobre Saúl y David en la monarquía unida. Cuarta oración cierra este bloque con una aplicación espiritual clara.".into(),
            "Quinta oración destaca la victoria sobre el gigante filisteo en el valle. Sexta oración anima al creyente a depender del Señor en cada batalla.".into(),
            "Séptima oración resume las dos victorias y la popularidad de David. Octava oración invita a prepararse delante de Dios con virtudes y destrezas.".into(),
        ];
        let out = rebalance_body_slides(&slides, 360);
        assert!(!out.is_empty());
        for p in &out {
            assert!(p.chars().count() <= 360, "{} chars", p.chars().count());
        }
    }
}
