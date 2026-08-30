//! Cursor Cloud Agents HTTP client + prepare/review prompts.

pub mod client;
pub mod prepare;
pub mod review;

pub const PREPARE_STUDY_MD: &str = include_str!("../../prompts/PREPARE_STUDY.md");
pub const AGENTS_MD: &str = include_str!("../../prompts/AGENTS.md");
pub const REVIEW_MD: &str = include_str!("../../prompts/REVIEW.md");
