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
             Delete the file and re-run with openai image gen (reference edit), \
             or restore a gold card from lamad-cli/src/build/adult/reference/.",
            path.display()
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
