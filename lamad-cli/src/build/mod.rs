//! PPTX builders — youth (dynamic packing) and adult (fixed 62-slide template).

mod ooxml;
mod proto;
mod study;
pub mod adult;

pub use proto::PROTO;
pub use study::{apply_study, apply_study_value};
pub use ooxml::resize_section_png;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::job::Audience;

/// Build a study deck for the requested audience.
pub fn build_study(
    study_path: &Path,
    output: &Path,
    template: Option<&Path>,
    audience: Audience,
    export_pdf: bool,
) -> Result<PathBuf> {
    match audience {
        Audience::Youth => study::build_study(study_path, output, template, export_pdf),
        Audience::Adult => adult::study::build_study(study_path, output, template, export_pdf),
    }
}
