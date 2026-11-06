//! Placeholder PNGs so adult decks can build before (or without) host image gen.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::prepare::{adult_basenames_for_study, adult_media_dir};
use crate::model::adult_study::AdultStudy;
use crate::paths;

const SCENIC_W: u32 = 1408;
const SCENIC_H: u32 = 768;

/// Create missing scenic / definición files under `studies/adult/{N}/images/`.
///
/// - Scenic slots: soft slate 16:9 PNG labeled as placeholder (sidecar `.placeholder`).
/// - Definición slots: copy of the locked gold reference (passes layout QC).
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
            continue;
        }
        if name.starts_with("definicion-") {
            let gold = crate::build::adult::primary_gold_reference()?;
            fs::copy(&gold, &dest)
                .with_context(|| format!("copy gold → {}", dest.display()))?;
        } else {
            write_scenic_placeholder(&dest, &name)?;
        }
        mark_placeholder(&dest)?;
        created += 1;
    }
    if created > 0 {
        crate::progress::phase(format!(
            "placeholders: wrote {created} image(s) under {} \
             (deck can build now; real art can replace later)",
            media.display()
        ));
    }
    Ok(created)
}

pub fn is_placeholder(png: &Path) -> bool {
    placeholder_mark(png).is_file()
}

pub fn any_placeholders(study: u32, model: &AdultStudy, root: &Path) -> bool {
    let media = adult_media_dir(study, root);
    adult_basenames_for_study(model)
        .into_iter()
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

    let mut img = RgbImage::from_pixel(SCENIC_W, SCENIC_H, Rgb([45, 55, 72]));
    // Left text-safe band (lighter) so title chrome stays readable.
    for y in 0..SCENIC_H {
        for x in 0..(SCENIC_W * 40 / 100) {
            let t = x as f32 / (SCENIC_W as f32 * 0.4);
            let v = (210.0 - t * 40.0) as u8;
            img.put_pixel(x, y, Rgb([v, v.saturating_sub(4), v.saturating_sub(8)]));
        }
    }
    // Tiny marker strip at bottom so humans see it's a stub.
    let mark = format!("PLACEHOLDER · {label}");
    let _ = mark; // visual label would need a font crate — color bands are enough
    for y in (SCENIC_H - 24)..SCENIC_H {
        for x in 0..SCENIC_W {
            img.put_pixel(x, y, Rgb([230, 126, 34]));
        }
    }

    img.save(path)
        .with_context(|| format!("write placeholder {}", path.display()))?;
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
