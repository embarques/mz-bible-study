//! Youth PPTX builder — port of python/mz_bible_study/build/.

mod ooxml;
mod proto;
mod study;

pub use study::build_study;
pub use proto::PROTO;
