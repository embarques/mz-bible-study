//! On-disk study content (section folders) — separate from the PPTX builder.
//!
//! Adult source of truth: `studies/adult/{N}/` with `manifest.toml` + section dirs.
//! The builder still consumes `AdultStudy` / JSON; this module loads and writes
//! the folder tree.

pub mod adult_sections;
pub mod placeholders;
pub mod scan_coverage;

pub use adult_sections::{
    adult_resume_hint, adult_study_folder, expand_json_to_sections, has_adult_sections,
    load_adult_sections, missing_front_matter_kinds, present_section_dir_names,
    print_section_inventory, verify_adult_section_images, write_adult_sections, ADULT_FRONT_MATTER,
};
pub use placeholders::{
    adult_definicion_cards_need_regen, any_definicion_placeholders, any_placeholders,
    any_scenic_placeholders,
    ensure_adult_placeholders, is_placeholder, note_real_image,
};
pub use scan_coverage::{assert_adult_scan_coverage, verify_adult_scan_coverage};
