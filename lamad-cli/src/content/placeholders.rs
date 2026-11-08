//! Placeholder PNGs so adult decks can soft-build scenic art before host image gen.
//!
//! **HARD — DEFINICIÓN:** never copy the gold reference card as a placeholder.
//! Gold bakes another study's terms (Amalec / Justicia / …). A definición stub
//! must be an obvious non-deliverable slate that fails study-terms QC at build.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::prepare::{adult_basenames_for_study, adult_media_dir};
use crate::model::adult_study::AdultStudy;
use crate::paths;

const SCENIC_W: u32 = 1408;
const SCENIC_H: u32 = 768;

/// Create missing scenic / definición stub files under `studies/adult/{N}/images/`.
///
/// - Scenic slots: soft slate 16:9 PNG labeled as placeholder (sidecar `.placeholder`).
/// - Definición slots: **slate stub only** (never gold — gold has wrong-study text).
///
/// Existing real files are left untouched.
pub fn ensure_adult_placeholders(study: u32, model: &AdultStudy, root: &Path) -> Result<usize> {
    let media = adult_media_dir(study, root);
    fs::create_dir_all(&media)?;
    let needed = adult_basenames_for_study(model);
    let mut created = 0usize;

    for name in needed {
        let dest = media.join(&name);
        if dest.is_file() && !is_placeholder(&dest) {
            // HARD: a "real" file that is still a byte-copy of gold is leftover
            // wrong-study art — delete and replace with a stub so build fails
            // cleanly until regen with this estudio's scan terms.
            if name.starts_with("definicion-")
                && crate::build::adult::definicion_card::is_byte_identical_to_any_gold(&dest)
            {
                crate::progress::warn(format!(
                    "removing {name} — it was a verbatim gold reference copy \
                     (wrong-study terms). Stub until regenerated from this scan."
                ));
                let _ = fs::remove_file(&dest);
                if let Some(stem) = dest.file_stem().and_then(|s| s.to_str()) {
                    if let Some(parent) = dest.parent() {
                        let _ = fs::remove_file(parent.join(format!("{stem}.terms")));
                    }
                }
            } else {
                continue;
            }
        }
        if name.starts_with("definicion-") {
            write_definicion_stub(&dest, &name)?;
        } else {
            write_scenic_placeholder(&dest, &name)?;
        }
        mark_placeholder(&dest)?;
        created += 1;
    }
    if created > 0 {
        crate::progress::phase(format!(
            "placeholders: wrote {created} image(s) under {} \
             (scenic OK for soft build; DEFINICIÓN stubs are NOT deliverable — \
             regenerate from this estudio's scan terms)",
            media.display()
        ));
    }
    Ok(created)
}

pub fn is_placeholder(png: &Path) -> bool {
    placeholder_mark(png).is_file()
}

pub fn any_placeholders(study: u32, model: &AdultStudy, root: &Path) -> bool {
    any_scenic_placeholders(study, model, root)
        || any_definicion_placeholders(study, model, root)
}

/// Scenic intro/tema/ab stubs — these block resume/build (must be unique real art).
pub fn any_scenic_placeholders(study: u32, model: &AdultStudy, root: &Path) -> bool {
    let media = adult_media_dir(study, root);
    crate::agent::prepare::adult_scenic_basenames_for(model)
        .into_iter()
        .any(|n| is_placeholder(&media.join(n)))
}

/// DEFINICIÓN stubs — need openai `--gen-definicion`; build skips those slides.
pub fn any_definicion_placeholders(study: u32, model: &AdultStudy, root: &Path) -> bool {
    let media = adult_media_dir(study, root);
    adult_basenames_for_study(model)
        .into_iter()
        .filter(|n| n.starts_with("definicion-"))
        .any(|n| is_placeholder(&media.join(n)))
}

pub fn clear_placeholder_mark(png: &Path) {
    let _ = fs::remove_file(placeholder_mark(png));
}

fn placeholder_mark(png: &Path) -> PathBuf {
    PathBuf::from(format!("{}.placeholder", png.display()))
}

fn mark_placeholder(png: &Path) -> Result<()> {
    fs::write(placeholder_mark(png), b"placeholder\n")?;
    Ok(())
}

fn write_scenic_placeholder(path: &Path, label: &str) -> Result<()> {
    use image::{Rgb, RgbImage};

    // Per-label tint so identical-byte uniqueness QC does not fire on stubs
    // (stubs still carry `.placeholder` and must be replaced before delivery).
    let mut h: u32 = 2166136261;
    for b in label.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16777619);
    }
    let tint = Rgb([
        40 + (h % 40) as u8,
        50 + ((h >> 8) % 40) as u8,
        60 + ((h >> 16) % 50) as u8,
    ]);

    let mut img = RgbImage::from_pixel(SCENIC_W, SCENIC_H, tint);
    // Left text-safe band (lighter) so title chrome stays readable.
    for y in 0..SCENIC_H {
        for x in 0..(SCENIC_W * 40 / 100) {
            let t = x as f32 / (SCENIC_W as f32 * 0.4);
            let v = (210.0 - t * 40.0) as u8;
            img.put_pixel(x, y, Rgb([v, v.saturating_sub(4), v.saturating_sub(8)]));
        }
    }
    // Encode label into a unique stripe so no two stubs share bytes.
    let stripe_y = 40u32 + (h % (SCENIC_H.saturating_sub(80).max(1)));
    for x in 0..SCENIC_W {
        let c = (x.wrapping_add(h) % 200) as u8;
        img.put_pixel(x, stripe_y, Rgb([c, 180, 90]));
    }
    for y in (SCENIC_H - 24)..SCENIC_H {
        for x in 0..SCENIC_W {
            img.put_pixel(x, y, Rgb([230, 126, 34]));
        }
    }

    img.save(path)
        .with_context(|| format!("write placeholder {}", path.display()))?;
    Ok(())
}

/// Slate stub for missing DEFINICIÓN — deliberately NOT the gold card layout / terms.
fn write_definicion_stub(path: &Path, label: &str) -> Result<()> {
    use image::{Rgb, RgbImage};

    let mut img = RgbImage::from_pixel(SCENIC_W, SCENIC_H, Rgb([248, 250, 252]));
    // Orange banner so humans see it's a stub, not a finished card.
    for y in 0..64 {
        for x in 0..SCENIC_W {
            img.put_pixel(x, y, Rgb([230, 126, 34]));
        }
    }
    let _ = label;
    // Dark band — no navy header / no white chevron rows → fails locked layout QC.
    for y in 200..280 {
        for x in 80..(SCENIC_W - 80) {
            img.put_pixel(x, y, Rgb([80, 80, 90]));
        }
    }
    img.save(path)
        .with_context(|| format!("write definición stub {}", path.display()))?;
    Ok(())
}

/// After a real image overwrites a placeholder PNG, drop the sidecar.
pub fn note_real_image(path: &Path) {
    clear_placeholder_mark(path);
}

/// Project-relative helper used when media dir resolution needs MZBS_ROOT.
#[allow(dead_code)]
pub fn adult_images_dir_for(study: u32) -> Result<PathBuf> {
    Ok(paths::adult_study_dir(study)?.join("images"))
}
