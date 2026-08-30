//! TOML config loading.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::job::Audience;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub cursor_api_key: Option<String>,
    #[serde(default = "default_model")]
    pub cursor_model: String,
    #[serde(default = "default_audience")]
    pub audience: String,
    #[serde(default = "default_true")]
    pub export_pdf: bool,
    pub scans_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
}

fn default_model() -> String {
    "composer-2.5".into()
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
            cursor_api_key: None,
            cursor_model: default_model(),
            audience: default_audience(),
            export_pdf: true,
            scans_dir: None,
            output_dir: None,
        }
    }
}

impl Config {
    pub fn audience_enum(&self) -> Audience {
        if self.audience.eq_ignore_ascii_case("adult") {
            Audience::Adult
        } else {
            Audience::Youth
        }
    }

    /// Load order: --config → ./config.toml → mzbs/config.toml beside binary → ~/.config/mzbs/config.toml
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
                    v.push(dir.join("mzbs").join("config.toml"));
                }
            }
            // Dev: mzbs/config.toml from cwd
            v.push(PathBuf::from("mzbs/config.toml"));
            if let Some(home) = dirs::config_dir() {
                v.push(home.join("mzbs").join("config.toml"));
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

        // Env overrides key
        if let Ok(key) = std::env::var("CURSOR_API_KEY") {
            if !key.is_empty() {
                cfg.cursor_api_key = Some(key);
            }
        }

        Ok(cfg)
    }

    pub fn apply_cli_overrides(
        &mut self,
        api_key: Option<String>,
        model: Option<String>,
        audience: Option<Audience>,
        export_pdf: Option<bool>,
    ) {
        if let Some(k) = api_key {
            if !k.is_empty() {
                self.cursor_api_key = Some(k);
            }
        }
        if let Some(m) = model {
            self.cursor_model = m;
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
        cursor_api_key: over.cursor_api_key.or(base.cursor_api_key),
        cursor_model: if over.cursor_model.is_empty() {
            base.cursor_model
        } else {
            over.cursor_model
        },
        audience: if over.audience.is_empty() {
            base.audience
        } else {
            over.audience
        },
        export_pdf: over.export_pdf,
        scans_dir: over.scans_dir.or(base.scans_dir),
        output_dir: over.output_dir.or(base.output_dir),
    }
}
