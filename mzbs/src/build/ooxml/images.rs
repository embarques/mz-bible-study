//! Swap section-image media (`image4/5/6.png`).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

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
        fs::copy(img, &dest)
            .with_context(|| format!("copy {} -> {}", img.display(), dest.display()))?;
    }
    Ok(())
}
