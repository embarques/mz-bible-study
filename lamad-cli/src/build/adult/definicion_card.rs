//! HARD lock for adult **DEFINICIÓN Y ETIMOLOGÍA** card art.
//!
//! Layout authority: `reference/definicion-etimologia-design.png` and the
//! estudio-4 / user gold PNGs beside it. Generators may change **only**
//! term text, definition body text, and left/right icons. Never invent a
//! new layout (centered title pill, cartoon hills, play triangles, etc.).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Gold reference PNGs shipped with the adult builder (layout authority).
pub fn gold_reference_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/build/adult/reference");
    [
        "definicion-etimologia-design.png",
        "definicion-card-gold-estudio4-tema1.png",
        "definicion-card-gold-estudio4-tema2.png",
        "definicion-card-gold-user-reference.png",
    ]
    .into_iter()
    .map(|n| dir.join(n))
    .filter(|p| p.is_file())
    .collect()
}

/// Prefer the highest-res estudio-4 gold; fall back to design.png / user ref.
pub fn primary_gold_reference() -> Result<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/build/adult/reference");
    for name in [
        "definicion-card-gold-estudio4-tema1.png",
        "definicion-card-gold-user-reference.png",
        "definicion-etimologia-design.png",
    ] {
        let p = dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!(
        "missing DEFINICIÓN gold reference under {}",
        dir.display()
    )
}

/// True when the PNG matches the locked house card layout (not a free-form invent).
///
/// Accepts the gold design: top-left navy header bar, white horizontal rows,
/// soft scenic background. Rejects known-bad invents (centered white title pill
/// + cartoon hills + play-triangle chrome).
pub fn looks_like_locked_definicion_card(path: &Path) -> bool {
    match looks_like_locked_definicion_card_reason(path) {
        Ok(None) => true,
        _ => false,
    }
}

/// `Ok(None)` = pass; `Ok(Some(reason))` = fail with reason; `Err` = unreadable.
pub fn looks_like_locked_definicion_card_reason(path: &Path) -> Result<Option<String>> {
    let img = image::open(path)
        .with_context(|| format!("open definicion card {}", path.display()))?
        .to_rgb8();
    let (w, h) = img.dimensions();
    if w < 200 || h < 100 {
        return Ok(Some("image too small for a 16:9 definición card".into()));
    }

    // Top-left strip should contain dark navy (header bar + book chrome).
    let navy = count_navy(&img, 0, 0, (w / 4).max(1), (h / 8).max(1));
    let tl_pixels = ((w / 4).max(1) * (h / 8).max(1)) as f32;
    let navy_frac = navy as f32 / tl_pixels;
    if navy_frac < 0.02 {
        return Ok(Some(
            "missing top-left navy header bar (locked design has open-book header)".into(),
        ));
    }

    // Reject cartoon hills invent: flat light-blue sky (top 40%) + flat green band (bottom 30%).
    let sky = mean_rgb(&img, 0, 0, w, (h * 2 / 5).max(1));
    let ground = mean_rgb(&img, 0, h * 2 / 3, w, h);
    let sky_flat_blue =
        sky.2 > 160.0 && sky.2 > sky.0 + 20.0 && sky.2 > sky.1 + 10.0 && sky.0 > 140.0;
    let ground_flat_green =
        ground.1 > 90.0 && ground.1 > ground.0 + 25.0 && ground.1 > ground.2 + 25.0;
    if sky_flat_blue && ground_flat_green && navy_frac < 0.08 {
        return Ok(Some(
            "cartoon hills / centered-pill invent — not the locked chevron card layout".into(),
        ));
    }

    // Mid-left column should have colorful icon content (not empty wash / tab-only chrome).
    let mid_y0 = h / 5;
    let mid_y1 = h * 4 / 5;
    let left_x1 = (w / 5).max(1);
    let colorfulness = mean_chroma(&img, 0, mid_y0, left_x1, mid_y1);
    if colorfulness < 12.0 {
        return Ok(Some(
            "left column lacks circular icon colour (locked rows have left circle icons)".into(),
        ));
    }

    // White card bands somewhere in the middle third horizontally.
    let white_frac = frac_near_white(&img, w / 5, h / 6, w * 4 / 5, h * 5 / 6);
    if white_frac < 0.12 {
        return Ok(Some(
            "missing white horizontal definition rows (locked design uses white cards)".into(),
        ));
    }

    Ok(None)
}

pub fn assert_locked_definicion_card(path: &Path) -> Result<()> {
    if let Some(reason) = looks_like_locked_definicion_card_reason(path)? {
        bail!(
            "{} is not the locked DEFINICIÓN Y ETIMOLOGÍA layout ({reason}). \
             Layout is HARD-frozen — only term text, definition text, and icons may change. \
             Delete and re-run `lamad prepare --gen-definicion` (OpenAI when key \
             set; otherwise local gold bake).",
            path.display()
        );
    }
    Ok(())
}

/// Stable fingerprint of the terms that belong on a DEFINICIÓN card.
///
/// Layout QC alone is not enough — leftover gold from another study also
/// passes layout QC. Keep/reuse a PNG only when this fingerprint matches.
pub fn terms_fingerprint<'a, I>(rows: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    // Stable across processes — DefaultHasher is randomly seeded and would
    // invalidate `.terms` sidecars on every CLI invoke.
    use std::hash::{Hash, Hasher};
    let mut h = Fnv64::new();
    let mut n = 0u32;
    for (termino, texto) in rows {
        n += 1;
        n.hash(&mut h);
        termino.trim().hash(&mut h);
        texto.trim().hash(&mut h);
    }
    n.hash(&mut h);
    format!("{:016x}", h.finish())
}

struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl std::hash::Hasher for Fnv64 {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x1000_0000_01b3);
        }
    }
}

fn terms_sidecar_path(png: &Path) -> PathBuf {
    let mut p = png.to_path_buf();
    // definicion-1.png → definicion-1.terms
    if let Some(stem) = png.file_stem().and_then(|s| s.to_str()) {
        if let Some(parent) = png.parent() {
            return parent.join(format!("{stem}.terms"));
        }
    }
    p.set_extension("terms");
    p
}

/// Write the terms fingerprint beside a freshly generated DEFINICIÓN PNG.
pub fn write_terms_sidecar(png: &Path, fingerprint: &str) -> Result<()> {
    let side = terms_sidecar_path(png);
    std::fs::write(&side, format!("{fingerprint}\n"))
        .with_context(|| format!("write {}", side.display()))
}

/// True when a `.terms` sidecar exists and equals `expected`.
/// Missing sidecar → false (legacy / wrong-study leftovers must not be kept).
pub fn terms_sidecar_matches(png: &Path, expected: &str) -> bool {
    let side = terms_sidecar_path(png);
    match std::fs::read_to_string(&side) {
        Ok(s) => s.trim() == expected.trim(),
        Err(_) => false,
    }
}

/// Layout QC **and** terms fingerprint both pass.
pub fn keepable_definicion_card(png: &Path, expected_fingerprint: &str) -> bool {
    png.is_file()
        && !is_byte_identical_to_any_gold(png)
        && looks_like_locked_definicion_card(png)
        && terms_sidecar_matches(png, expected_fingerprint)
}

/// True when `png` is a byte-for-byte copy of a shipped gold reference.
/// Those cards bake **another study's** terms (e.g. Amalec / Justicia) — never deliver.
pub fn is_byte_identical_to_any_gold(png: &Path) -> bool {
    let Ok(bytes) = std::fs::read(png) else {
        return false;
    };
    for gold in gold_reference_paths() {
        if let Ok(g) = std::fs::read(&gold) {
            if g == bytes {
                return true;
            }
        }
    }
    false
}

/// HARD gate before embedding a DEFINICIÓN card in a deck.
///
/// Rejects: missing file, placeholder stub, byte-copy of gold reference (wrong-study
/// terms), failed layout QC, missing/mismatched `.terms` sidecar vs this estudio's
/// verbatim `definicion-*.txt` / JSON terms.
pub fn assert_study_definicion_card<'a, I>(png: &Path, rows: I) -> Result<()>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    if !png.is_file() {
        bail!(
            "missing DEFINICIÓN card {} — generate from this estudio's printed \
             «Definiciones y etimología» box (never reuse another study's card).",
            png.display()
        );
    }
    // Sidecar written by content::placeholders — check path directly (avoid
    // build ↔ content module cycle).
    if PathBuf::from(format!("{}.placeholder", png.display())).is_file() {
        bail!(
            "{} is still a PLACEHOLDER — not this estudio's scan terms. \
             Generate the card from the printed Definiciones box \
             (`lamad prepare --gen-definicion`).",
            png.display()
        );
    }
    if is_byte_identical_to_any_gold(png) {
        bail!(
            "{} is a verbatim copy of the locked gold reference (another study's \
             baked terms — e.g. Amalec / Justicia / Hipócritas). Delete it and \
             regenerate from THIS estudio's scan Definiciones text only.",
            png.display()
        );
    }
    assert_locked_definicion_card(png)?;
    let rows: Vec<(&str, &str)> = rows.into_iter().collect();
    if rows.is_empty() {
        bail!(
            "{}: no definición terms supplied — refuse to embed a card without \
             scan text.",
            png.display()
        );
    }
    let fp = terms_fingerprint(rows.iter().copied());
    if !terms_sidecar_matches(png, &fp) {
        let terms: Vec<String> = rows
            .iter()
            .map(|(t, _)| (*t).to_string())
            .collect();
        bail!(
            "{} does not prove it shows THIS estudio's Definiciones terms \
             (expected fingerprint for [{}]). Layout QC alone is NOT enough — \
             leftover cards from other studies also pass layout. Delete the PNG \
             (+ .terms if present) and regenerate from the scan / folder \
             definicion-*.txt with `lamad prepare --gen-definicion`.",
            png.display(),
            terms.join(" | ")
        );
    }
    Ok(())
}

fn count_navy(img: &image::RgbImage, x0: u32, y0: u32, x1: u32, y1: u32) -> u32 {
    let mut n = 0u32;
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            let p = img.get_pixel(x, y).0;
            // Dark navy / indigo header chrome.
            if p[2] as i16 > p[0] as i16 + 15
                && p[2] as i16 > p[1] as i16 + 10
                && p[0] < 90
                && p[1] < 100
                && p[2] < 160
            {
                n += 1;
            }
        }
    }
    n
}

fn mean_rgb(img: &image::RgbImage, x0: u32, y0: u32, x1: u32, y1: u32) -> (f32, f32, f32) {
    let mut s = [0u64; 3];
    let mut n = 0u64;
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            let p = img.get_pixel(x, y).0;
            s[0] += p[0] as u64;
            s[1] += p[1] as u64;
            s[2] += p[2] as u64;
            n += 1;
        }
    }
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    let n = n as f32;
    (s[0] as f32 / n, s[1] as f32 / n, s[2] as f32 / n)
}

fn mean_chroma(img: &image::RgbImage, x0: u32, y0: u32, x1: u32, y1: u32) -> f32 {
    let mut s = 0f32;
    let mut n = 0f32;
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            let p = img.get_pixel(x, y).0;
            let mx = p[0].max(p[1]).max(p[2]) as f32;
            let mn = p[0].min(p[1]).min(p[2]) as f32;
            s += mx - mn;
            n += 1.0;
        }
    }
    if n == 0.0 {
        0.0
    } else {
        s / n
    }
}

fn frac_near_white(img: &image::RgbImage, x0: u32, y0: u32, x1: u32, y1: u32) -> f32 {
    let mut white = 0u64;
    let mut n = 0u64;
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            let p = img.get_pixel(x, y).0;
            if p[0] > 220 && p[1] > 220 && p[2] > 220 {
                white += 1;
            }
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        white as f32 / n as f32
    }
}

/// Local bake (no OpenAI): start from the locked gold PNG, erase prior term/body
/// text, draw **this estudio’s** Spanish terms, write `.terms` sidecar.
///
/// Used when `openai_api_key` is unset. Icons stay from gold (layout chrome);
/// chevron + body text match the scan. Prefer OpenAI reference-edit when the
/// key **is** set (`--gen-definicion` / host image path).
pub fn bake_definicion_card_local<'a, I>(dest: &Path, rows: I) -> Result<()>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let rows: Vec<(String, String)> = rows
        .into_iter()
        .map(|(t, b)| (t.trim().to_string(), b.trim().to_string()))
        .filter(|(t, b)| !t.is_empty() || !b.is_empty())
        .collect();
    if rows.is_empty() {
        bail!("bake_definicion_card_local: no terms to bake");
    }
    if rows.len() > DEFINICION_ROWS {
        bail!(
            "bake_definicion_card_local: {} terms — locked card holds at most 3 rows",
            rows.len()
        );
    }

    let gold_path = primary_gold_reference()?;
    let gold = image::open(&gold_path)
        .with_context(|| format!("open gold {}", gold_path.display()))?
        .to_rgb8();
    let (w, h) = gold.dimensions();
    let layouts = scaled_row_layouts(w as f32 / 850.0, h as f32 / 464.0);
    let mut img = gold;
    for (i, layout) in layouts.iter().enumerate() {
        clear_text_area(&mut img, layout.term);
        clear_text_area(&mut img, layout.body);
        if let Some((term, body)) = rows.get(i) {
            draw_term_on_chevron(&mut img, layout, term)?;
            draw_body_text(&mut img, layout, body)?;
        }
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    img.save(dest)
        .with_context(|| format!("write baked definición {}", dest.display()))?;

    // Clear placeholder sidecar if present.
    let _ = std::fs::remove_file(PathBuf::from(format!("{}.placeholder", dest.display())));
    if is_byte_identical_to_any_gold(dest) {
        bail!(
            "local bake produced a gold byte-identical PNG at {} — refuse to ship",
            dest.display()
        );
    }
    assert_locked_definicion_card(dest)?;
    let fp = terms_fingerprint(rows.iter().map(|(t, b)| (t.as_str(), b.as_str())));
    write_terms_sidecar(dest, &fp)?;
    Ok(())
}

pub(crate) const DEFINICION_ROWS: usize = 3;

pub(crate) fn definicion_card_page(first: &Path, page: usize) -> PathBuf {
    if page == 0 {
        return first.to_path_buf();
    }
    let mut stem = first.file_stem().unwrap_or_default().to_os_string();
    stem.push(format!("-{}.png", page + 1));
    first.with_file_name(stem)
}

/// Bake every missing/stub/wrong DEFINICIÓN card for an adult estudio (local, no API).
pub fn bake_adult_definicion_cards_local(
    study: u32,
    model: &crate::model::adult_study::AdultStudy,
    root: &Path,
    force: bool,
) -> Result<usize> {
    let media = crate::agent::prepare::adult_media_dir(study, root);
    std::fs::create_dir_all(&media)?;
    let mut n = 0usize;
    for (i, tema) in model.temas.iter().enumerate() {
        if tema.definiciones.is_empty() {
            continue;
        }
        let first = media.join(format!("definicion-{}.png", i + 1));
        for (page, definitions) in tema.definiciones.chunks(DEFINICION_ROWS).enumerate() {
            let dest = definicion_card_page(&first, page);
            let rows = || definitions.iter().map(|d| (d.termino.as_str(), d.texto.as_str()));
            let fp = terms_fingerprint(rows());
            if !force && keepable_definicion_card(&dest, &fp) {
                continue;
            }
            bake_definicion_card_local(&dest, rows())?;
            n += 1;
        }
    }
    if n > 0 {
        crate::progress::ok(format!(
            "DEFINICIÓN: baked {n} card(s) locally from gold layout (estudio {study})"
        ));
    }
    Ok(n)
}

struct RowLayout {
    term: TextArea,
    body: TextArea,
}

#[derive(Clone, Copy)]
struct TextArea {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

fn scaled_row_layouts(sx: f32, sy: f32) -> [RowLayout; 3] {
    let area = |x0: u32, y0: u32, x1: u32, y1: u32| TextArea {
        x0: (x0 as f32 * sx).round() as u32,
        y0: (y0 as f32 * sy).round() as u32,
        x1: (x1 as f32 * sx).round() as u32,
        y1: (y1 as f32 * sy).round() as u32,
    };
    // Text rectangles on the locked reference, measured at 850 × 464.
    [
        RowLayout { term: area(162, 113, 282, 164), body: area(334, 108, 718, 170) },
        RowLayout { term: area(160, 240, 352, 274), body: area(380, 225, 726, 287) },
        RowLayout { term: area(160, 350, 336, 401), body: area(380, 342, 726, 405) },
    ]
}

fn clear_text_area(img: &mut image::RgbImage, area: TextArea) {
    // Copy each column's own background from just above the old lettering.
    for x in area.x0..area.x1 {
        let background = *img.get_pixel(x, area.y0);
        for y in area.y0 + 1..area.y1 {
            img.put_pixel(x, y, background);
        }
    }
}

fn font_path(bold: bool) -> Result<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    let bundled = if bold {
        base.join("DejaVuSans-Bold.ttf")
    } else {
        base.join("DejaVuSans.ttf")
    };
    if bundled.is_file() {
        return Ok(bundled);
    }
    let candidates: &[&str] = if bold {
        &[
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ]
    } else {
        &[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]
    };
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!(
        "no TTF for definición bake (expected {} or system Arial/DejaVu)",
        bundled.display()
    )
}

fn draw_term_on_chevron(img: &mut image::RgbImage, layout: &RowLayout, term: &str) -> Result<()> {
    draw_fitted_text(img, layout.term, term, true, 28.0, [255, 255, 255])
}

fn draw_body_text(img: &mut image::RgbImage, layout: &RowLayout, body: &str) -> Result<()> {
    draw_fitted_text(img, layout.body, body, false, 22.0, [20, 20, 20])
}

fn draw_fitted_text(
    img: &mut image::RgbImage,
    area: TextArea,
    text: &str,
    bold: bool,
    mut size: f32,
    color: [u8; 3],
) -> Result<()> {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

    let font_data = std::fs::read(font_path(bold)?)?;
    let font = FontRef::try_from_slice(&font_data).context("parse definition font")?;
    let max_w = (area.x1 - area.x0).saturating_sub(8) as f32;
    let max_h = (area.y1 - area.y0).saturating_sub(8) as f32;
    loop {
        let scale = PxScale::from(size);
        let line_h = font.as_scaled(scale).height() + 3.0;
        let lines = wrap_text(&font, scale, text, max_w);
        let height = lines.len() as f32 * line_h;
        if height <= max_h && lines.iter().all(|line| text_width(&font, scale, line) <= max_w) {
            let y = area.y0 as f32 + (max_h - height) / 2.0 + 4.0;
            for (i, line) in lines.iter().enumerate() {
                draw_text_line(img, &font, scale, area.x0 as f32 + 4.0, y + i as f32 * line_h, line, color);
            }
            return Ok(());
        }
        if size <= 12.0 {
            bail!("definition text does not fit the locked card: {text}");
        }
        size -= 1.0;
    }
}

fn text_width(font: &ab_glyph::FontRef<'_>, scale: ab_glyph::PxScale, text: &str) -> f32 {
    use ab_glyph::{Font, ScaleFont};
    let scaled = font.as_scaled(scale);
    let mut w = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        if let Some(p) = prev {
            w += scaled.kern(p, gid);
        }
        w += scaled.h_advance(gid);
        prev = Some(gid);
    }
    w
}

fn wrap_text(
    font: &ab_glyph::FontRef<'_>,
    scale: ab_glyph::PxScale,
    text: &str,
    max_w: f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let trial = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if text_width(font, scale, &trial) <= max_w || cur.is_empty() {
            cur = trial;
        } else {
            lines.push(cur);
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn draw_text_line(
    img: &mut image::RgbImage,
    font: &ab_glyph::FontRef<'_>,
    scale: ab_glyph::PxScale,
    x: f32,
    y: f32,
    text: &str,
    color: [u8; 3],
) {
    use ab_glyph::{Font, Glyph, Point, ScaleFont};
    let scaled = font.as_scaled(scale);
    let mut caret = x;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        if let Some(p) = prev {
            caret += scaled.kern(p, gid);
        }
        let glyph = Glyph {
            id: gid,
            scale,
            position: Point { x: caret, y: y + scaled.ascent() },
        };
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                if cov < 0.05 {
                    return;
                }
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 {
                    return;
                }
                let (px, py) = (px as u32, py as u32);
                if px >= img.width() || py >= img.height() {
                    return;
                }
                let base = img.get_pixel(px, py).0;
                let a = cov.clamp(0.0, 1.0);
                let blended = [
                    ((color[0] as f32) * a + (base[0] as f32) * (1.0 - a)).round() as u8,
                    ((color[1] as f32) * a + (base[1] as f32) * (1.0 - a)).round() as u8,
                    ((color[2] as f32) * a + (base[2] as f32) * (1.0 - a)).round() as u8,
                ];
                img.put_pixel(px, py, image::Rgb(blended));
            });
        }
        caret += scaled.h_advance(gid);
        prev = Some(gid);
    }
}

/// Prompt block for image models — layout frozen; text + icons only.
pub fn locked_layout_edit_prompt(tema_titulo: &str, rows_block: &str) -> String {
    format!(
        "EDIT this Mount Zion Church DEFINICIÓN Y ETIMOLOGÍA card. \
         HARD LOCK — keep this EXACT layout and visual language (do not redesign): \
         soft blurred lake/mountain scenic background; \
         top-left dark-blue header bar with open-book icon + bold navy title \
         'DEFINICIÓN Y ETIMOLOGÍA' + thin navy rule ending in a dot; \
         stacked horizontal WHITE rounded rows; each row = left CIRCULAR illustrated icon \
         | coloured chevron/arrow panel pointing right with the term in white bold \
         | thin grey vertical rule | black sans definition body | small matching icon on the far right; \
         chevron colours rotate teal / navy / purple (adjacent rows different). \
         ONLY change: row count to match the terms below, chevron texts, definition bodies, \
         and left/right icons so they match each definition. \
         Treat the reference's row artwork as sample content. Replace BOTH icons in every row. \
         Before editing, choose a concrete subject for each term and a complementary detail \
         for its definition. Use the left circle for a small illustrated scene or object and \
         the right slot for a simpler related symbol. For example, a definition about an \
         unearned gift could show a gift being offered on the left and an open hand on the right. \
         Keep the reference illustration scale, palette, and placement. Give each row its own \
         motif and keep the artwork clear of text. Remove all original row illustrations, \
         including tents, soldiers, crossed swords, and crowns. Preserve the header's book icon. \
         Before exporting, inspect the finished PNG row by row against the supplied list. \
         Verify exact Spanish text and order, two relevant icons per row, no reference row \
         artwork, no blank sample rows, and no overlap or clipped text. Fix any mismatch. \
         FORBIDDEN redesigns: centered white title pill, cartoon flat green hills + flat blue sky, \
         play-button triangles, vertical book/tab chrome, parchment essay, plain text dump, \
         cream title-only, new card chrome. \
         Spanish verbatim — no paraphrasing, no extra verses, no Hebrew/Greek roots, no watermarks. \
         Tema context: {tema_titulo}. Terms:\n{rows_block}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_estudio4_passes_layout_qc() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/build/adult/reference/definicion-card-gold-estudio4-tema1.png");
        assert!(
            looks_like_locked_definicion_card(&p),
            "gold tema1 must pass: {:?}",
            looks_like_locked_definicion_card_reason(&p).unwrap()
        );
    }

    #[test]
    fn user_gold_reference_passes_layout_qc() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/build/adult/reference/definicion-card-gold-user-reference.png");
        assert!(
            looks_like_locked_definicion_card(&p),
            "user gold must pass: {:?}",
            looks_like_locked_definicion_card_reason(&p).unwrap()
        );
    }

    #[test]
    fn gold_byte_copy_is_detected() {
        let gold = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/build/adult/reference/definicion-card-gold-estudio4-tema1.png");
        assert!(
            is_byte_identical_to_any_gold(&gold),
            "gold reference must match itself"
        );
    }

    #[test]
    fn assert_study_rejects_gold_without_matching_terms() {
        let gold = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/build/adult/reference/definicion-card-gold-estudio4-tema1.png");
        let err = assert_study_definicion_card(
            &gold,
            [("Hebrón", "ciudad antigua")],
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("gold") || msg.contains("verbatim copy"),
            "expected gold rejection, got: {msg}"
        );
    }

    #[test]
    fn assert_study_rejects_byte_copy_of_gold_elsewhere() {
        let gold = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/build/adult/reference/definicion-card-gold-estudio4-tema1.png");
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("definicion-1.png");
        std::fs::copy(&gold, &dest).expect("copy gold");
        assert!(is_byte_identical_to_any_gold(&dest));
        let err = assert_study_definicion_card(&dest, [("Hebrón", "ciudad antigua")])
            .expect_err("copy of gold must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("gold") || msg.contains("verbatim copy"),
            "expected gold rejection, got: {msg}"
        );
    }

    #[test]
    fn local_bake_passes_study_terms_qc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("definicion-1.png");
        bake_definicion_card_local(
            &dest,
            [
                (
                    "Que mora entre los querubines",
                    "Sobre el arca del pacto se colocaron dos querubines de oro.",
                ),
                (
                    "Pérez-Uza",
                    "Literalmente, significa el quebrantamiento de Uza.",
                ),
            ],
        )
        .expect("local bake");
        assert_study_definicion_card(
            &dest,
            [
                (
                    "Que mora entre los querubines",
                    "Sobre el arca del pacto se colocaron dos querubines de oro.",
                ),
                (
                    "Pérez-Uza",
                    "Literalmente, significa el quebrantamiento de Uza.",
                ),
            ],
        )
        .expect("study terms QC after bake");
        assert!(!is_byte_identical_to_any_gold(&dest));
    }

    /// `LAMAD_BAKE_ESTUDIO=8 cargo test bake_estudio_definicion_env -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bake_estudio_definicion_env() {
        let n: u32 = std::env::var("LAMAD_BAKE_ESTUDIO")
            .ok()
            .and_then(|s| s.parse().ok())
            .expect("set LAMAD_BAKE_ESTUDIO");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let model = crate::content::load_adult_sections(n).expect("load sections");
        bake_adult_definicion_cards_local(n, &model, &root, true).expect("bake");
    }

    /// Stamp `.terms` + assert QC for existing PNGs (no redraw).
    /// `LAMAD_BAKE_ESTUDIO=8 cargo test stamp_estudio_definicion_terms -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stamp_estudio_definicion_terms() {
        let n: u32 = std::env::var("LAMAD_BAKE_ESTUDIO")
            .ok()
            .and_then(|s| s.parse().ok())
            .expect("set LAMAD_BAKE_ESTUDIO");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let model = crate::content::load_adult_sections(n).expect("load sections");
        let media = crate::agent::prepare::adult_media_dir(n, &root);
        for (i, tema) in model.temas.iter().enumerate() {
            if tema.definiciones.is_empty() {
                continue;
            }
            let dest = media.join(format!("definicion-{}.png", i + 1));
            let rows: Vec<(&str, &str)> = tema
                .definiciones
                .iter()
                .map(|d| (d.termino.as_str(), d.texto.as_str()))
                .collect();
            let fp = terms_fingerprint(rows.iter().copied());
            write_terms_sidecar(&dest, &fp).expect("terms");
            crate::content::note_real_image(&dest);
            assert_study_definicion_card(&dest, rows.iter().copied())
                .unwrap_or_else(|e| panic!("{}: {e:#}", dest.display()));
        }
    }

    #[test]
    fn cartoon_hills_invent_fails_layout_qc() {
        // Synthetic invent: flat blue sky + green hills, no navy header.
        let mut img = image::RgbImage::new(400, 225);
        for y in 0..140 {
            for x in 0..400 {
                img.put_pixel(x, y, image::Rgb([170, 200, 230]));
            }
        }
        for y in 140..225 {
            for x in 0..400 {
                img.put_pixel(x, y, image::Rgb([60, 120, 50]));
            }
        }
        let dir = std::env::temp_dir().join(format!("lamad-bad-def-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("bad.png");
        img.save(&p).unwrap();
        assert!(
            !looks_like_locked_definicion_card(&p),
            "cartoon invent must FAIL: {:?}",
            looks_like_locked_definicion_card_reason(&p).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
