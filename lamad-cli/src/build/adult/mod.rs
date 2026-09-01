//! Adult PPTX builder (fixed 62-slide template).

mod ooxml;
pub mod study;

pub use study::{apply_adult_study, apply_adult_study_value, build_study as build_adult_study};
