//! On-disk study content (section folders) — separate from the PPTX builder.
//!
//! Adult source of truth: `studies/adult/{N}/` with `manifest.toml` + section dirs.
//! The builder still consumes `AdultStudy` / JSON; this module loads and writes
//! the folder tree and can emit a build-cache `{N}.json`.

pub mod adult_sections;

pub use adult_sections::{
    adult_study_folder, expand_json_to_sections, has_adult_sections, load_adult_sections,
    write_adult_build_json, write_adult_sections,
};
