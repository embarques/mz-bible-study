//! Auto light/dark chrome text from scenic-image luminance.
//!
//! The CLI must pick readable title/verse colours without AI: sample the
//! text-safe band of the slide's PNG, then set OOXML run fills. Does **not**
//! rewrite slide layout — only `srgbClr` on existing chrome shapes.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use image::GenericImageView;
use regex::Regex;

use crate::build::ooxml::shape::transform_shape;

/// Left text-safe band mean below this → light (white) chrome.
const DARK_BAND_THRESHOLD: f32 = 0.48;
/// Full-frame mean below this → treat as a dark cinematic scene.
const DARK_GLOBAL_THRESHOLD: f32 = 0.55;
/// Soft left mist on a dark scene still gets white chrome (band below this).
const MIST_BAND_CAP: f32 = 0.72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTone {
    /// Dark navy title / navy verse (true light parchment/wash).
    DarkOnLight,
    /// White title / white verse (dark scenic / soft mist over dark scene).
    LightOnDark,
}

impl TextTone {
    pub fn title_rgb(self) -> &'static str {
        match self {
            Self::DarkOnLight => "1A1A2E",
            Self::LightOnDark => "FFFFFF",
        }
    }

    pub fn verse_rgb(self) -> &'static str {
        match self {
            // Same navy as title — mid-gray `#444` washed out on lavender mist.
            Self::DarkOnLight => "1A1A2E",
            Self::LightOnDark => "FFFFFF",
        }
    }

    pub fn gray_line_rgb(self) -> &'static str {
        match self {
            Self::DarkOnLight => "B0B0B0",
            Self::LightOnDark => "E0E0E0",
        }
    }
}

/// Sample the left text-safe band (~40% width, full height) of a PNG/JPEG.
pub fn tone_for_image(image_path: &Path) -> Result<TextTone> {
    let raw = fs::read(image_path).with_context(|| format!("read {}", image_path.display()))?;
    tone_for_image_bytes(&raw)
}

pub fn tone_for_image_bytes(raw: &[u8]) -> Result<TextTone> {
    let img = image::load_from_memory(raw).context("decode image for contrast")?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        bail!("image has zero dimensions");
    }
    let band = mean_luma(&img, 0, 0, ((w as f32 * 0.40) as u32).max(1), h)?;
    let global = mean_luma(&img, 0, 0, w, h)?;
    // Dark left band → white. Also: dark overall scene with a soft left mist
    // (common adult cinematic art) → white — the mist alone used to force
    // navy/gray chrome that looked wrong on a dark battlefield/dusk photo.
    Ok(
        if band < DARK_BAND_THRESHOLD
            || (global < DARK_GLOBAL_THRESHOLD && band < MIST_BAND_CAP)
        {
            TextTone::LightOnDark
        } else {
            TextTone::DarkOnLight
        },
    )
}

fn mean_luma(
    img: &image::DynamicImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> Result<f32> {
    let w = (x1 - x0).max(1);
    let h = (y1 - y0).max(1);
    let step_x = (w / 32).max(1);
    let step_y = (h / 32).max(1);
    let mut sum = 0f32;
    let mut n = 0u32;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let p = img.get_pixel(x, y).0;
            let r = p[0] as f32 / 255.0;
            let g = p[1] as f32 / 255.0;
            let b = p[2] as f32 / 255.0;
            // Rec. 709 relative luminance.
            sum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
            n += 1;
            x += step_x;
        }
        y += step_y;
    }
    if n == 0 {
        bail!("contrast sample empty");
    }
    Ok(sum / n as f32)
}

/// Resolve the first image relationship target for a slide XML path.
pub fn slide_media_path(slide_xml: &Path) -> Result<std::path::PathBuf> {
    let slide_name = slide_xml
        .file_name()
        .and_then(|s| s.to_str())
        .context("slide path has no file name")?;
    let rels = slide_xml
        .parent()
        .unwrap()
        .join("_rels")
        .join(format!("{slide_name}.rels"));
    let rels_text = fs::read_to_string(&rels).with_context(|| format!("read {}", rels.display()))?;
    let re = Regex::new(
        r#"Type="[^"]*/relationships/image"[^>]*Target="([^"]+)""#,
    )
    .unwrap();
    let target = re
        .captures(&rels_text)
        .map(|c| c[1].to_string())
        .context("slide has no image relationship")?;
    // Target is like ../media/image8.png relative to ppt/slides/
    let media = slide_xml.parent().unwrap().join(Path::new(&target));
    let media = media.canonicalize().unwrap_or(media);
    if !media.is_file() {
        bail!("slide media missing: {}", media.display());
    }
    Ok(media)
}

/// After chrome text is filled, set title/verse/gray-line colours from the
/// slide's scenic PNG. Safe on youth section + adult youth-clone image slides.
pub fn apply_image_chrome_contrast(slide_xml: &Path) -> Result<TextTone> {
    let media = slide_media_path(slide_xml)?;
    let tone = tone_for_image(&media)?;
    set_chrome_colors(slide_xml, tone)?;
    Ok(tone)
}

fn set_chrome_colors(slide_xml: &Path, tone: TextTone) -> Result<()> {
    let xml = fs::read_to_string(slide_xml)
        .with_context(|| format!("read {}", slide_xml.display()))?;
    let title = tone.title_rgb();
    let verse = tone.verse_rgb();
    let gray = tone.gray_line_rgb();

    let xml = transform_shape(&xml, "CuadroTexto 8", |b| Ok(recolor_runs(b, title)))?;
    let xml = transform_shape(&xml, "CuadroTexto 3", |b| Ok(recolor_runs(b, verse)))?;
    let xml = if xml.contains(r#"name="GrayLine""#) {
        transform_shape(&xml, "GrayLine", |b| Ok(recolor_solid_fills(b, gray)))?
    } else {
        xml
    };
    fs::write(slide_xml, xml).with_context(|| format!("write {}", slide_xml.display()))?;
    Ok(())
}

/// Force every `a:srgbClr` in a shape block to `rgb`. If a run has no fill,
/// insert one right after the opening `<a:rPr …>` tag.
fn recolor_runs(shape_xml: &str, rgb: &str) -> String {
    // Youth OOXML often has a space before `/>` (`val="1A1A2E" />`).
    let re = Regex::new(r#"<a:srgbClr val="[0-9A-Fa-f]{6}"\s*/>"#).unwrap();
    let fill = format!(r#"<a:srgbClr val="{rgb}"/>"#);
    let mut out = re.replace_all(shape_xml, fill.as_str()).into_owned();
    if !re.is_match(&out) {
        let inject = format!(r#"$0<a:solidFill><a:srgbClr val="{rgb}"/></a:solidFill>"#);
        let rpr_re = Regex::new(r#"<a:rPr\b[^>]*>"#).unwrap();
        out = rpr_re.replace_all(&out, inject.as_str()).into_owned();
    }
    out
}

fn recolor_solid_fills(shape_xml: &str, rgb: &str) -> String {
    let re = Regex::new(r#"<a:srgbClr val="[0-9A-Fa-f]{6}"\s*/>"#).unwrap();
    let fill = format!(r#"<a:srgbClr val="{rgb}"/>"#);
    re.replace_all(shape_xml, fill.as_str()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn png_bytes(rgb: [u8; 3]) -> Vec<u8> {
        let img = RgbImage::from_pixel(64, 64, Rgb(rgb));
        let mut out = Vec::new();
        let dynimg = image::DynamicImage::ImageRgb8(img);
        dynimg
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode");
        out
    }

    #[test]
    fn dark_image_gets_light_text() {
        let tone = tone_for_image_bytes(&png_bytes([20, 20, 30])).expect("tone");
        assert_eq!(tone, TextTone::LightOnDark);
        assert_eq!(tone.title_rgb(), "FFFFFF");
        assert_eq!(tone.verse_rgb(), "FFFFFF");
    }

    #[test]
    fn light_image_gets_dark_text() {
        let tone = tone_for_image_bytes(&png_bytes([240, 235, 220])).expect("tone");
        assert_eq!(tone, TextTone::DarkOnLight);
        assert_eq!(tone.title_rgb(), "1A1A2E");
        assert_eq!(tone.verse_rgb(), "1A1A2E");
    }

    /// Soft left mist (light band) over a dark cinematic frame → still white.
    #[test]
    fn dark_scene_with_soft_mist_gets_white_chrome() {
        let mut img = RgbImage::from_pixel(100, 80, Rgb([30, 28, 35])); // dark scene
        for y in 0..80 {
            for x in 0..40 {
                // lavender mist on the left text-safe band
                img.put_pixel(x, y, Rgb([175, 168, 180]));
            }
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode");
        let tone = tone_for_image_bytes(&out).expect("tone");
        assert_eq!(tone, TextTone::LightOnDark);
        assert_eq!(tone.verse_rgb(), "FFFFFF");
    }
}
