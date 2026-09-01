//! TOML config loading.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::agent::AgentProvider;
use crate::job::Audience;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// `cursor` (default) or `chatgpt` / `openai`.
    #[serde(default)]
    pub agent_provider: AgentProvider,

    pub cursor_api_key: Option<String>,
    #[serde(default = "default_cursor_model")]
    pub cursor_model: String,

    pub openai_api_key: Option<String>,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    #[serde(default = "default_openai_image_model")]
    pub openai_image_model: String,

    #[serde(default = "default_audience")]
    pub audience: String,
    #[serde(default = "default_true")]
    pub export_pdf: bool,
    /// Path to `pdftoppm` (absolute, or relative to app / project root).
    /// Default when unset: `tools/pdftoppm` then PATH.
    pub pdftoppm_path: Option<PathBuf>,
    pub scans_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
}

fn default_cursor_model() -> String {
    "composer-2.5".into()
}
fn default_openai_model() -> String {
    "gpt-4o".into()
}
fn default_openai_image_model() -> String {
    "gpt-image-1".into()
}
fn default_audience() -> String {
    "youth".into()
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agent_provider: AgentProvider::Cursor,
            cursor_api_key: None,
            cursor_model: default_cursor_model(),
            openai_api_key: None,
            openai_model: default_openai_model(),
            openai_image_model: default_openai_image_model(),
            audience: default_audience(),
            export_pdf: true,
            pdftoppm_path: None,
            scans_dir: None,
            output_dir: None,
        }
    }
}

/// Credentials + models for the active [`AgentProvider`].
#[derive(Debug, Clone)]
pub struct ProviderCreds {
    pub provider: AgentProvider,
    pub api_key: String,
    pub model: String,
    pub image_model: String,
}

impl Config {
    pub fn audience_enum(&self) -> Audience {
        if self.audience.eq_ignore_ascii_case("adult") {
            Audience::Adult
        } else {
            Audience::Youth
        }
    }

    /// Resolve API key + models for the configured provider.
    pub fn provider_creds(&self) -> Result<ProviderCreds> {
        match self.agent_provider {
            AgentProvider::Cursor => {
                let api_key = self
                    .cursor_api_key
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .context(
                        "No Cursor API key. Set cursor_api_key in config.toml or CURSOR_API_KEY \
                         (see lamad-cli/config.example.toml).",
                    )?
                    .to_string();
                Ok(ProviderCreds {
                    provider: AgentProvider::Cursor,
                    api_key,
                    model: self.cursor_model.clone(),
                    image_model: String::new(),
                })
            }
            AgentProvider::ChatGpt => {
                let api_key = self
                    .openai_api_key
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .context(
                        "No OpenAI API key. Set openai_api_key in config.toml or OPENAI_API_KEY \
                         (see lamad-cli/config.example.toml).",
                    )?
                    .to_string();
                Ok(ProviderCreds {
                    provider: AgentProvider::ChatGpt,
                    api_key,
                    model: self.openai_model.clone(),
                    image_model: self.openai_image_model.clone(),
                })
            }
        }
    }

    /// Load order: --config → ./config.toml → beside binary → ~/.config/lamad
    pub fn load(cli_config: Option<&Path>) -> Result<Self> {
        let mut cfg = Config::default();

        let candidates: Vec<PathBuf> = {
            let mut v = Vec::new();
            if let Some(p) = cli_config {
                v.push(p.to_path_buf());
            }
            v.push(PathBuf::from("config.toml"));
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    v.push(dir.join("config.toml"));
                    v.push(dir.join("lamad").join("config.toml"));
                    v.push(dir.join("lamad-cli").join("config.toml"));
                    v.push(dir.join("mzbs").join("config.toml")); // legacy folder name
                }
            }
            // Dev: crate dir from repo root cwd
            v.push(PathBuf::from("lamad-cli/config.toml"));
            v.push(PathBuf::from("mzbs/config.toml")); // legacy
            if let Some(home) = dirs::config_dir() {
                v.push(home.join("lamad").join("config.toml"));
                v.push(home.join("mzbs").join("config.toml")); // legacy
            }
            v
        };

        for path in &candidates {
            if path.is_file() {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("read config {}", path.display()))?;
                let file_cfg: Config = toml::from_str(&text)
                    .with_context(|| format!("parse config {}", path.display()))?;
                cfg = merge(cfg, file_cfg);
                break; // first hit wins (load order)
            }
        }

        // Env overrides keys (provider-specific)
        if let Ok(key) = std::env::var("CURSOR_API_KEY") {
            if !key.is_empty() {
                cfg.cursor_api_key = Some(key);
            }
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.is_empty() {
                cfg.openai_api_key = Some(key);
            }
        }
        if let Ok(p) = std::env::var("LAMAD_AGENT_PROVIDER") {
            if !p.is_empty() {
                cfg.agent_provider = p
                    .parse()
                    .map_err(anyhow::Error::msg)?;
            }
        }

        Ok(cfg)
    }

    pub fn apply_cli_overrides(
        &mut self,
        provider: Option<AgentProvider>,
        api_key: Option<String>,
        model: Option<String>,
        audience: Option<Audience>,
        export_pdf: Option<bool>,
    ) {
        if let Some(p) = provider {
            self.agent_provider = p;
        }
        if let Some(k) = api_key {
            if !k.is_empty() {
                match self.agent_provider {
                    AgentProvider::Cursor => self.cursor_api_key = Some(k),
                    AgentProvider::ChatGpt => self.openai_api_key = Some(k),
                }
            }
        }
        if let Some(m) = model {
            match self.agent_provider {
                AgentProvider::Cursor => self.cursor_model = m,
                AgentProvider::ChatGpt => self.openai_model = m,
            }
        }
        if let Some(a) = audience {
            self.audience = a.as_str().to_string();
        }
        if let Some(e) = export_pdf {
            self.export_pdf = e;
        }
    }
}

fn merge(base: Config, over: Config) -> Config {
    Config {
        agent_provider: over.agent_provider,
        cursor_api_key: over.cursor_api_key.or(base.cursor_api_key),
        cursor_model: if over.cursor_model.is_empty() {
            base.cursor_model
        } else {
            over.cursor_model
        },
        openai_api_key: over.openai_api_key.or(base.openai_api_key),
        openai_model: if over.openai_model.is_empty() {
            base.openai_model
        } else {
            over.openai_model
        },
        openai_image_model: if over.openai_image_model.is_empty() {
            base.openai_image_model
        } else {
            over.openai_image_model
        },
        audience: if over.audience.is_empty() {
            base.audience
        } else {
            over.audience
        },
        export_pdf: over.export_pdf,
        pdftoppm_path: over.pdftoppm_path.or(base.pdftoppm_path),
        scans_dir: over.scans_dir.or(base.scans_dir),
        output_dir: over.output_dir.or(base.output_dir),
    }
}
