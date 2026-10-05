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

/// Fit DEFINICIÓN card art into 1408×768 **without cropping the top**.
///
/// Cover-crop (center) was slicing `DEFINICIÓN Y ETIMOLOGÍA` off. Contain
/// letterboxing left empty side bands. This path scales to **full width**,
/// top-aligns, and only trims/pads the **bottom** (empty sky / mountains).
pub fn resize_definicion_png(raw: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(raw).context("decode definicion PNG")?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        bail!("definicion image has zero dimensions");
    }

    // Scale to full slide width.
    let scale = SECTION_W as f32 / w as f32;
    let new_w = SECTION_W;
    let new_h = ((h as f32) * scale).round().max(1.0) as u32;
    let resized = img
        .resize_exact(new_w, new_h, FilterType::Lanczos3)
        .to_rgb8();

    let pad = image::Rgb([236u8, 242u8, 248u8]);
    let mut canvas = image::RgbImage::from_pixel(SECTION_W, SECTION_H, pad);
    if new_h >= SECTION_H {
        // Crop bottom only — title/header at y=0 stays intact.
        let cropped = image::imageops::crop_imm(&resized, 0, 0, SECTION_W, SECTION_H).to_image();
        image::imageops::overlay(&mut canvas, &cropped, 0, 0);
    } else {
        // Shorter than slide — top-align, pad bottom.
        image::imageops::overlay(&mut canvas, &resized, 0, 0);
    }

    let rgb = DynamicImage::ImageRgb8(canvas);
    let mut out = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .context("encode definicion PNG")?;
    Ok(out)
}

/// Fit the **entire** image into 1408×768 (letterbox). Never crops.
///
/// Used when the full frame must stay visible. Vertical padding is
/// top-aligned (extra space at bottom); horizontal padding is centered.
/// Pad colour matches a light sky/paper wash.
pub fn resize_contain_png(raw: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(raw).context("decode contain PNG")?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        bail!("contain image has zero dimensions");
    }

    let scale = (SECTION_W as f32 / w as f32).min(SECTION_H as f32 / h as f32);
    let new_w = ((w as f32) * scale).round().max(1.0) as u32;
    let new_h = ((h as f32) * scale).round().max(1.0) as u32;
    let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3).to_rgb8();

    // Light wash close to the definicion design sky/paper.
    let pad = image::Rgb([236u8, 242u8, 248u8]);
    let mut canvas = image::RgbImage::from_pixel(SECTION_W, SECTION_H, pad);
    let x = (SECTION_W.saturating_sub(new_w)) / 2;
    let y = 0u32; // top-align — keep title/header visible
    image::imageops::overlay(&mut canvas, &resized, x as i64, y as i64);

    let rgb = DynamicImage::ImageRgb8(canvas);
    let mut out = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .context("encode contain PNG")?;
    Ok(out)
}
