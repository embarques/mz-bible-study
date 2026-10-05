//! Cloud agent backends: Cursor (default) or ChatGPT / OpenAI.

pub mod client;
pub mod openai;
pub mod prepare;
pub mod prepare_openai;
pub mod provider;
pub mod review;
pub mod review_openai;

pub use provider::AgentProvider;

pub const PREPARE_STUDY_MD: &str = include_str!("../../prompts/PREPARE_STUDY.md");
pub const AGENTS_MD: &str = include_str!("../../prompts/AGENTS.md");
pub const REVIEW_MD: &str = include_str!("../../prompts/REVIEW.md");
