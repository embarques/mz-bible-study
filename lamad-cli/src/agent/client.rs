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
pub struct FollowUpResponse {
    pub run: RunInfo,
}

#[derive(Debug, Deserialize)]
pub struct RunStatus {
    pub id: String,
    pub status: String,
    /// Final assistant reply text (terminal runs).
    #[serde(default)]
    pub result: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArtifactList {
    /// Cloud Agents API returns `items` (v1). Accept legacy `artifacts` too.
    #[serde(default, alias = "items")]
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default, alias = "sizeBytes")]
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
            // Force implement mode — plan-only narration never writes artifacts.
            "mode": "agent",
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

    /// Send a follow-up prompt on an existing agent (same workspace / conversation).
    pub async fn create_followup(
        &self,
        agent_id: &str,
        prompt_text: &str,
    ) -> Result<FollowUpResponse> {
        let body = json!({
            "prompt": { "text": prompt_text },
            "mode": "agent",
        });
        let resp = self
            .http
            .post(format!("{API_BASE}/v1/agents/{agent_id}/runs"))
            .header(AUTHORIZATION, self.auth_header())
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("POST follow-up run failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("follow-up run HTTP {status}: {text}");
        }
        serde_json::from_str(&text).with_context(|| format!("parse follow-up response: {text}"))
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
    ///
    /// Always shows a spinner with status + elapsed. Optional `--stream` dumps
    /// raw agent narration (often duplicated SSE chunks — opt-in only).
    ///
    /// When `progress` is set, polls artifacts and shows which prepare step
    /// is in flight (JSON / images / closing).
    pub async fn wait_run(
        &self,
        agent_id: &str,
        run_id: &str,
        stream: bool,
    ) -> Result<RunStatus> {
        self.wait_run_with_progress(agent_id, run_id, stream, None)
            .await
    }

    pub async fn wait_run_with_progress(
        &self,
        agent_id: &str,
        run_id: &str,
        stream: bool,
        mut progress_tracker: Option<&mut crate::agent::wait_progress::CloudAgentProgress>,
    ) -> Result<RunStatus> {
        use crate::progress;
        use std::time::Instant;

        let started = Instant::now();
        if let Some(p) = progress_tracker.as_mut() {
            p.print_plan();
        }
        let spinner = progress::Spinner::start(
            progress_tracker
                .as_ref()
                .map(|p| p.spinner_label("RUNNING"))
                .unwrap_or_else(|| format!("Waiting for Cursor agent (run {run_id})…")),
        );

        // Stream in the background so the spinner keeps ticking; raw text is
        // noisy so we only enable it when the user asked for `--stream`.
        let stream_handle = if stream {
            progress::phase("agent log streaming enabled (--stream)");
            let this = self.clone();
            let aid = agent_id.to_string();
            let rid = run_id.to_string();
            Some(tokio::spawn(async move {
                let _ = this.stream_run(&aid, &rid).await;
            }))
        } else {
            None
        };

        let mut poll_n: u32 = 0;
        if progress_tracker.is_some() {
            spinner.note("Step 3/4 · reading uploaded PDF pages…");
        }
        loop {
            let st = self.get_run(agent_id, run_id).await?;
            let s = st.status.to_uppercase();

            // Poll artifacts every other tick (~6s) so we don't hammer the API.
            let label = if let Some(tracker) = progress_tracker.as_mut() {
                poll_n = poll_n.wrapping_add(1);
                if poll_n % 2 == 1 {
                    match self.list_artifacts(agent_id).await {
                        Ok(arts) => {
                            let (label, notes) = tracker.on_artifacts(&s, &arts);
                            for n in notes {
                                spinner.note(n);
                            }
                            label
                        }
                        Err(_) => tracker.spinner_label(&s),
                    }
                } else {
                    tracker.spinner_label(&s)
                }
            } else {
                format!("Waiting for Cursor agent… {s}")
            };
            spinner.set_message(format!(
                "{label} ({})",
                progress::fmt_elapsed(started.elapsed())
            ));

            if s == "FINISHED" || s == "COMPLETED" || s == "DONE" {
                if let Some(h) = stream_handle {
                    h.abort();
                }
                // Final artifact sweep so late files still get ✓ lines.
                if let Some(tracker) = progress_tracker.as_mut() {
                    if let Ok(arts) = self.list_artifacts(agent_id).await {
                        let (_, notes) = tracker.on_artifacts(&s, &arts);
                        for n in notes {
                            spinner.note(n);
                        }
                    }
                    spinner.note("Step 3/4 · agent run finished");
                }
                spinner.succeed(format!(
                    "agent finished ({})",
                    progress::fmt_elapsed(started.elapsed())
                ));
                return Ok(st);
            }
            if s == "ERROR" || s == "FAILED" || s == "CANCELLED" || s == "CANCELED" {
                if let Some(h) = stream_handle {
                    h.abort();
                }
                spinner.fail(format!("agent ended with status {}", st.status));
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

    /// True when the download endpoint accepts this path (even if List Artifacts omitted it).
    pub async fn artifact_downloadable(&self, agent_id: &str, path: &str) -> bool {
        self.http
            .get(format!("{API_BASE}/v1/agents/{agent_id}/artifacts/download"))
            .query(&[("path", path)])
            .header(AUTHORIZATION, self.auth_header())
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn download_artifact(&self, agent_id: &str, path: &str, dest: &Path) -> Result<()> {
        self.download_artifact_inner(agent_id, path, dest, true).await
    }

    /// Download without a per-file spinner (use with a shared progress bar).
    pub async fn download_artifact_quiet(
        &self,
        agent_id: &str,
        path: &str,
        dest: &Path,
    ) -> Result<()> {
        self.download_artifact_inner(agent_id, path, dest, false)
            .await
    }

    async fn download_artifact_inner(
        &self,
        agent_id: &str,
        path: &str,
        dest: &Path,
        show_spinner: bool,
    ) -> Result<()> {
        use crate::progress;

        let spin = show_spinner.then(|| progress::Spinner::start(format!("Downloading {path}…")));
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
            if let Some(spin) = spin {
                spin.fail(format!("download failed HTTP {status}"));
            }
            bail!("artifact download HTTP {status}: {text}");
        }
        let url: DownloadUrl = serde_json::from_str(&text)
            .or_else(|_| {
                // Some responses may be a redirect URL string
                Ok::<_, serde_json::Error>(DownloadUrl {
                    url: text.trim().trim_matches('"').to_string(),
                })
            })
            .context("parse download url")?;

        if let Some(spin) = &spin {
            spin.set_message(format!("Fetching bytes for {path}…"));
        }
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
        let kb = bytes.len() / 1024;
        if let Some(spin) = spin {
            spin.succeed(format!(
                "{} → {} ({kb} KB)",
                path,
                dest.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
            ));
        }
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
