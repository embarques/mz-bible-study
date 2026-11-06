//! On-disk study content (section folders) — separate from the PPTX builder.
//!
//! Adult source of truth: `studies/adult/{N}/` with `manifest.toml` + section dirs.
//! The builder still consumes `AdultStudy` / JSON; this module loads and writes
//! the folder tree.

pub mod adult_sections;
pub mod placeholders;

pub use adult_sections::{
    adult_study_folder, expand_json_to_sections, has_adult_sections, load_adult_sections,
    print_section_inventory, verify_adult_section_images, write_adult_sections,
};
pub use placeholders::{
    any_placeholders, ensure_adult_placeholders, is_placeholder, note_real_image,
};
