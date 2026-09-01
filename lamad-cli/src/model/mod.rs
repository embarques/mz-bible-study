//! Typed reference model for study JSON (see `PREPARE_STUDY.md` / AGENTS.md
//! for the authoritative shape). `crate::build` accepts `serde_json::Value`
//! directly for flexibility — these structs are a convenient, documented
//! view of the same data for callers (agents, validators, TUI) that want
//! typed access instead.

pub mod study;

pub use study::Study;
