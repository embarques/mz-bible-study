//! Cursor Cloud Agents API client (no-repo).
//! Docs: https://cursor.com/docs/cloud-agent/api/endpoints

use anyhow::{bail, Context, Result};
use base64::Engine;
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

const API_BASE: &str = "https://api.cursor.com";

#[derive(Debug, Clone)]
pub struct CursorClient {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptImage {
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAgentResponse {
    pub agent: AgentInfo,
    pub run: RunInfo,
}

#[derive(Debug, Deserialize)]
pub struct AgentInfo {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct RunInfo {
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunStatus {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ArtifactList {
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DownloadUrl {
    url: String,
}

impl CursorClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            http,
            api_key: api_key.into(),
        })
    }

    fn auth_header(&self) -> String {
        // Basic (API_KEY:)
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:", self.api_key));
        format!("Basic {token}")
    }

    /// Create a no-repo cloud agent with initial run.
    pub async fn create_agent(
        &self,
        prompt_text: &str,
        images: &[PromptImage],
        model: &str,
        name: &str,
    ) -> Result<CreateAgentResponse> {
        let mut prompt = json!({ "text": prompt_text });
        if !images.is_empty() {
            prompt["images"] = json!(images);
        }
        let body = json!({
            "prompt": prompt,
            "model": { "id": model },
            "name": name,
            // Omit repos → no-repo agent
        });

        let resp = self
            .http
            .post(format!("{API_BASE}/v1/agents"))
            .header(AUTHORIZATION, self.auth_header())
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("POST /v1/agents failed")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("create agent HTTP {status}: {text}");
        }
        serde_json::from_str(&text).with_context(|| format!("parse create response: {text}"))
    }

    pub async fn get_run(&self, agent_id: &str, run_id: &str) -> Result<RunStatus> {
        let resp = self
            .http
            .get(format!("{API_BASE}/v1/agents/{agent_id}/runs/{run_id}"))
            .header(AUTHORIZATION, self.auth_header())
            .send()
            .await
            .context("GET run failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("get run HTTP {status}: {text}");
        }
        serde_json::from_str(&text).context("parse run status")
    }

    /// Poll until FINISHED / ERROR / CANCELLED.
    pub async fn wait_run(
        &self,
        agent_id: &str,
        run_id: &str,
        stream: bool,
    ) -> Result<RunStatus> {
        if stream {
            let _ = self.stream_run(agent_id, run_id).await;
        }
        loop {
            let st = self.get_run(agent_id, run_id).await?;
            let s = st.status.to_uppercase();
            if s == "FINISHED" || s == "COMPLETED" || s == "DONE" {
                return Ok(st);
            }
            if s == "ERROR" || s == "FAILED" || s == "CANCELLED" || s == "CANCELED" {
                bail!("agent run ended with status {}", st.status);
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    /// Best-effort SSE stream of run events (prints text deltas).
    pub async fn stream_run(&self, agent_id: &str, run_id: &str) -> Result<()> {
        let resp = self
            .http
            .get(format!(
                "{API_BASE}/v1/agents/{agent_id}/runs/{run_id}/stream"
            ))
            .header(AUTHORIZATION, self.auth_header())
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await;

        let Ok(resp) = resp else {
            return Ok(());
        };
        if !resp.status().is_success() {
            return Ok(());
        }

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let Ok(bytes) = chunk else { break };
            buf.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim_end_matches('\r').to_string();
                buf = buf[pos + 1..].to_string();
                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(t) = v
                            .pointer("/delta/text")
                            .or_else(|| v.pointer("/text"))
                            .or_else(|| v.get("message").and_then(|m| m.get("text")))
                            .and_then(|x| x.as_str())
                        {
                            print!("{t}");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                        }
                    }
                }
            }
        }
        println!();
        Ok(())
    }

    pub async fn list_artifacts(&self, agent_id: &str) -> Result<Vec<Artifact>> {
        let resp = self
            .http
            .get(format!("{API_BASE}/v1/agents/{agent_id}/artifacts"))
            .header(AUTHORIZATION, self.auth_header())
            .send()
            .await
            .context("list artifacts")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("list artifacts HTTP {status}: {text}");
        }
        let list: ArtifactList =
            serde_json::from_str(&text).with_context(|| format!("parse artifacts: {text}"))?;
        Ok(list.artifacts)
    }

    pub async fn download_artifact(&self, agent_id: &str, path: &str, dest: &Path) -> Result<()> {
        let resp = self
            .http
            .get(format!("{API_BASE}/v1/agents/{agent_id}/artifacts/download"))
            .query(&[("path", path)])
            .header(AUTHORIZATION, self.auth_header())
            .send()
            .await
            .context("artifact download URL")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("artifact download HTTP {status}: {text}");
        }
        let url: DownloadUrl = serde_json::from_str(&text)
            .or_else(|_| {
                // Some responses may be a redirect URL string
                Ok::<_, serde_json::Error>(DownloadUrl { url: text.trim().trim_matches('"').to_string() })
            })
            .context("parse download url")?;

        let bytes = self
            .http
            .get(&url.url)
            .send()
            .await
            .context("fetch artifact bytes")?
            .bytes()
            .await?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, &bytes)
            .with_context(|| format!("write artifact {}", dest.display()))?;
        Ok(())
    }
}

/// Load a local image as a PromptImage (base64).
pub fn image_from_path(path: &Path) -> Result<PromptImage> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read image {}", path.display()))?;
    if bytes.len() > 15 * 1024 * 1024 {
        bail!("image exceeds 15MB: {}", path.display());
    }
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Ok(PromptImage {
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type: mime.into(),
    })
}
