//! Pack many page rasters into ≤N composite images for APIs with hard
//! attachment limits (Cursor Cloud Agents: max 5 images).
//!
//! **Never drops pages.** Extra pages are stacked vertically into the same
//! attachment so the agent still sees the full study.

use anyhow::{bail, Context, Result};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

/// Cursor Cloud Agents API hard limit (HTTP 400 above this).
pub const CURSOR_MAX_IMAGES: usize = 5;

/// Target max width for each page strip inside a composite (keeps uploads lean).
const STRIP_MAX_WIDTH: u32 = 1400;

/// If `paths.len() ≤ max_images`, return them unchanged.
/// Otherwise write `pack-01.png`… under `out_dir` covering **every** page in order.
pub fn pack_pages_for_api_limit(
    paths: &[PathBuf],
    out_dir: &Path,
    max_images: usize,
) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        bail!("no page rasters to pack");
    }
    if max_images == 0 {
        bail!("max_images must be ≥ 1");
    }
    if paths.len() <= max_images {
        return Ok(paths.to_vec());
    }

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("mkdir {}", out_dir.display()))?;

    let groups = partition_even(paths.len(), max_images);
    let mut out = Vec::with_capacity(groups.len());
    let mut offset = 0usize;
    for (i, n) in groups.into_iter().enumerate() {
        let chunk = &paths[offset..offset + n];
        offset += n;
        let dest = out_dir.join(format!("pack-{:02}.png", i + 1));
        stack_pages_vertically(chunk, &dest)?;
        out.push(dest);
    }
    Ok(out)
}

/// Split `total` items into `buckets` non-empty groups as evenly as possible
/// (earlier buckets get the extras).
fn partition_even(total: usize, buckets: usize) -> Vec<usize> {
    let buckets = buckets.min(total).max(1);
    let base = total / buckets;
    let rem = total % buckets;
    (0..buckets)
        .map(|i| base + if i < rem { 1 } else { 0 })
        .collect()
}

fn stack_pages_vertically(paths: &[PathBuf], dest: &Path) -> Result<()> {
    let mut strips: Vec<RgbaImage> = Vec::with_capacity(paths.len());
    let mut width = 0u32;
    let mut height = 0u32;
    const GAP: u32 = 12;

    for (i, p) in paths.iter().enumerate() {
        let img = image::open(p)
            .with_context(|| format!("open {}", p.display()))?
            .to_rgba8();
        let scaled = scale_to_max_width(DynamicImage::ImageRgba8(img), STRIP_MAX_WIDTH).to_rgba8();
        // Thin label bar so the agent knows page order inside the composite.
        let labeled = with_order_bar(&scaled, i + 1, paths.len())?;
        width = width.max(labeled.width());
        height = height.saturating_add(labeled.height());
        if i + 1 < paths.len() {
            height = height.saturating_add(GAP);
        }
        strips.push(labeled);
    }

    let mut canvas: RgbaImage = ImageBuffer::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    let mut y = 0u32;
    for (i, strip) in strips.iter().enumerate() {
        image::imageops::replace(&mut canvas, strip, 0, y as i64);
        y += strip.height();
        if i + 1 < strips.len() {
            y += GAP;
        }
    }

    canvas
        .save(dest)
        .with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}

fn scale_to_max_width(img: DynamicImage, max_w: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w <= max_w {
        return img;
    }
    let nh = ((h as u64 * max_w as u64) / w as u64).max(1) as u32;
    img.resize(max_w, nh, image::imageops::FilterType::Triangle)
}

fn with_order_bar(page: &RgbaImage, index: usize, of: usize) -> Result<RgbaImage> {
    // Small solid bar — no font dependency; agent still gets geometric separation.
    let bar_h = 28u32;
    let w = page.width();
    let mut out: RgbaImage =
        ImageBuffer::from_pixel(w, page.height() + bar_h, Rgba([30, 41, 59, 255]));
    // Lighter ticks proportional to index (visual cue without text).
    let tick_w = (w / of.max(1) as u32).max(8);
    let x0 = ((index - 1) as u32).saturating_mul(tick_w);
    for yy in 4..bar_h.saturating_sub(4) {
        for xx in x0..(x0 + tick_w).min(w) {
            out.put_pixel(xx, yy, Rgba([250, 204, 21, 255]));
        }
    }
    image::imageops::replace(&mut out, page, 0, bar_h as i64);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn partition_spreads_evenly() {
        assert_eq!(partition_even(9, 5), vec![2, 2, 2, 2, 1]);
        assert_eq!(partition_even(5, 5), vec![1, 1, 1, 1, 1]);
        assert_eq!(partition_even(3, 5), vec![1, 1, 1]);
        assert_eq!(partition_even(8, 5), vec![2, 2, 2, 1, 1]);
    }

    #[test]
    fn pack_keeps_all_pages_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mut pages = Vec::new();
        for i in 0..9 {
            let p = dir.path().join(format!("p{i}.png"));
            let img: RgbaImage = ImageBuffer::from_pixel(40, 60, Rgba([i as u8 * 20, 0, 0, 255]));
            img.save(&p).unwrap();
            pages.push(p);
        }
        let out_dir = dir.path().join("packs");
        let packs = pack_pages_for_api_limit(&pages, &out_dir, CURSOR_MAX_IMAGES).unwrap();
        assert_eq!(packs.len(), CURSOR_MAX_IMAGES);
        for p in &packs {
            assert!(p.is_file(), "{}", p.display());
        }
        // Under limit: identity
        let same = pack_pages_for_api_limit(&pages[..3], &out_dir, CURSOR_MAX_IMAGES).unwrap();
        assert_eq!(same.len(), 3);
        assert_eq!(same, pages[..3]);
    }
}
