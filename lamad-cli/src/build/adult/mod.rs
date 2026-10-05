//! Adult PPTX builder (fixed 62-slide template).
//!
//! Visual/layout authority: see [`LAYOUT_GUIDE.md`](LAYOUT_GUIDE.md)
//! (Estudio 02 — *EL CRUCE MILAGROSO DEL JORDÁN* PDF). Study 24
//! `master-template.pptx` is the OOXML prototype; guard rails and
//! full-bleed image rules follow the layout guide.

mod diagrams;
mod ooxml;
pub mod study;

pub use study::{apply_adult_study, apply_adult_study_value, build_study as build_adult_study};
