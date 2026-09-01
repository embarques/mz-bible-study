//! Youth PPTX builder — port of python/mz_bible_study/build/.

mod ooxml;
mod proto;
mod study;

pub use proto::PROTO;
pub use study::{apply_study, apply_study_value, build_study};
pub use ooxml::resize_section_png;
