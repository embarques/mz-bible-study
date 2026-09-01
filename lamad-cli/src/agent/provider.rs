//! Which cloud backend runs prepare / review.

use serde::Deserialize;
use std::fmt;
use std::str::FromStr;

/// Cloud engine for prepare + review. Cursor is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProvider {
    #[default]
    Cursor,
    /// OpenAI ChatGPT API (vision + Images). Accepts aliases `openai` / `chatgpt`.
    #[serde(alias = "openai", alias = "chatgpt")]
    ChatGpt,
}

impl AgentProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentProvider::Cursor => "cursor",
            AgentProvider::ChatGpt => "chatgpt",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentProvider::Cursor => "Cursor",
            AgentProvider::ChatGpt => "ChatGPT (OpenAI)",
        }
    }
}

impl fmt::Display for AgentProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cursor" => Ok(AgentProvider::Cursor),
            "chatgpt" | "openai" | "gpt" => Ok(AgentProvider::ChatGpt),
            other => Err(format!(
                "unknown agent_provider {other:?}; use cursor or chatgpt"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases() {
        assert_eq!("cursor".parse::<AgentProvider>().unwrap(), AgentProvider::Cursor);
        assert_eq!("chatgpt".parse::<AgentProvider>().unwrap(), AgentProvider::ChatGpt);
        assert_eq!("openai".parse::<AgentProvider>().unwrap(), AgentProvider::ChatGpt);
        assert_eq!("GPT".parse::<AgentProvider>().unwrap(), AgentProvider::ChatGpt);
    }

    #[test]
    fn toml_aliases() {
        #[derive(Deserialize)]
        struct Wrap {
            p: AgentProvider,
        }
        let w: Wrap = toml::from_str("p = \"openai\"").unwrap();
        assert_eq!(w.p, AgentProvider::ChatGpt);
        let w: Wrap = toml::from_str("p = \"chatgpt\"").unwrap();
        assert_eq!(w.p, AgentProvider::ChatGpt);
    }
}
