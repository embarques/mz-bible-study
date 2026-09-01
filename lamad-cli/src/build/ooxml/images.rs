//! Swap section-image media (`image4/5/6.png`), resized to ≈1408×768 RGB.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

const SECTION_W: u32 = 1408;
const SECTION_H: u32 = 768;

pub fn replace_section_images(build: &Path, images: &[PathBuf]) -> Result<()> {
    if images.len() != 3 {
        bail!("exactly 3 section images required, got {}", images.len());
    }
    for (i, img) in images.iter().enumerate() {
        let dest = build
            .join("ppt")
            .join("media")
            .join(format!("image{}.png", i + 4));
        if !img.is_file() {
            bail!("image not found: {}", img.display());
        }
        let raw = fs::read(img).with_context(|| format!("read {}", img.display()))?;
        let png = resize_section_png(&raw)
            .with_context(|| format!("resize section image {}", img.display()))?;
        fs::write(&dest, png).with_context(|| format!("write {}", dest.display()))?;
    }
    Ok(())
}

/// Cover-crop resize to 1408×768 **RGB** PNG (no alpha).
///
/// PowerPoint often shows a repair dialog when section media is RGBA
/// (PNG color type 6). Working decks (and Cursor-prepared assets) use
/// RGB color type 2 — match that.
pub fn resize_section_png(raw: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(raw).context("decode section PNG")?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        bail!("section image has zero dimensions");
    }

    let target_aspect = SECTION_W as f32 / SECTION_H as f32;
    let src_aspect = w as f32 / h as f32;
    let cropped = if (src_aspect - target_aspect).abs() < 0.01 {
        img
    } else if src_aspect > target_aspect {
        let new_w = ((h as f32) * target_aspect).round() as u32;
        let x = (w.saturating_sub(new_w)) / 2;
        DynamicImage::ImageRgba8(img.crop_imm(x, 0, new_w, h).to_rgba8())
    } else {
        let new_h = ((w as f32) / target_aspect).round() as u32;
        let y = (h.saturating_sub(new_h)) / 2;
        DynamicImage::ImageRgba8(img.crop_imm(0, y, w, new_h).to_rgba8())
    };

    let resized = cropped.resize_exact(SECTION_W, SECTION_H, FilterType::Lanczos3);
    // Drop alpha — RGB-only PNG (color type 2) for PowerPoint compatibility.
    let rgb = DynamicImage::ImageRgb8(resized.to_rgb8());
    let mut out = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .context("encode resized section PNG")?;
    Ok(out)
}
