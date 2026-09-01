//! OpenAI (ChatGPT) HTTP client — vision chat + image generation.
//! Separate from the Cursor Cloud Agents client in [`super::client`].

use anyhow::{bail, Context, Result};
use base64::Engine;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const API_BASE: &str = "https://api.openai.com/v1";

/// Landscape size closest to 16:9 among gpt-image-1 presets.
pub const IMAGE_SIZE: &str = "1536x1024";

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Clone)]
pub struct VisionImage {
    pub mime_type: String,
    pub data_b64: String,
}

impl OpenAiClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            api_key: api_key.into(),
        })
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// Chat Completions with optional vision images; expects a JSON object reply.
    pub async fn chat_json(
        &self,
        model: &str,
        system: &str,
        user_text: &str,
        images: &[VisionImage],
    ) -> Result<Value> {
        let mut user_content: Vec<Value> = vec![json!({
            "type": "text",
            "text": user_text,
        })];
        for img in images {
            user_content.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", img.mime_type, img.data_b64),
                }
            }));
        }

        let body = json!({
            "model": model,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user_content },
            ],
        });

        let resp = self
            .http
            .post(format!("{API_BASE}/chat/completions"))
            .header(AUTHORIZATION, self.auth())
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("POST /chat/completions failed")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("OpenAI chat HTTP {status}: {text}");
        }

        let parsed: ChatResponse =
            serde_json::from_str(&text).with_context(|| format!("parse chat response: {text}"))?;
        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("")
            .trim();
        if content.is_empty() {
            bail!("OpenAI chat returned empty content");
        }
        // Strip optional markdown fences if the model ignores json_object.
        let content = strip_json_fence(content);
        serde_json::from_str(content)
            .with_context(|| format!("parse model JSON: {}", truncate(content, 400)))
    }

    /// Plain-text chat (for review markdown).
    pub async fn chat_text(
        &self,
        model: &str,
        system: &str,
        user_text: &str,
    ) -> Result<String> {
        let body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user_text },
            ],
        });

        let resp = self
            .http
            .post(format!("{API_BASE}/chat/completions"))
            .header(AUTHORIZATION, self.auth())
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("POST /chat/completions (text) failed")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("OpenAI chat HTTP {status}: {text}");
        }

        let parsed: ChatResponse =
            serde_json::from_str(&text).with_context(|| format!("parse chat response: {text}"))?;
        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        if content.trim().is_empty() {
            bail!("OpenAI chat returned empty content");
        }
        Ok(content)
    }

    /// Generate one PNG via Images API; returns raw PNG bytes (resized to section size).
    pub async fn generate_section_png(
        &self,
        image_model: &str,
        prompt: &str,
    ) -> Result<Vec<u8>> {
        let body = json!({
            "model": image_model,
            "prompt": prompt,
            "n": 1,
            "size": IMAGE_SIZE,
            "quality": "high",
        });

        let resp = self
            .http
            .post(format!("{API_BASE}/images/generations"))
            .header(AUTHORIZATION, self.auth())
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("POST /images/generations failed")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("OpenAI images HTTP {status}: {text}");
        }

        let parsed: ImageResponse =
            serde_json::from_str(&text).with_context(|| format!("parse images response: {text}"))?;
        let b64 = parsed
            .data
            .first()
            .and_then(|d| d.b64_json.as_deref())
            .context("images response missing b64_json")?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("decode image base64")?;
        resize_section_png(&raw)
    }
}

pub fn vision_image_from_path(path: &Path) -> Result<VisionImage> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read image {}", path.display()))?;
    // OpenAI vision soft limit is generous; keep a hard cap for safety.
    if bytes.len() > 20 * 1024 * 1024 {
        bail!("image exceeds 20MB: {}", path.display());
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
    Ok(VisionImage {
        mime_type: mime.into(),
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// Cover-crop resize to 1408×768 RGB PNG (shared with build/ooxml/images).
fn resize_section_png(raw: &[u8]) -> Result<Vec<u8>> {
    crate::build::resize_section_png(raw)
}

fn strip_json_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest
            .strip_suffix("```")
            .unwrap_or(rest)
            .trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest
            .strip_suffix("```")
            .unwrap_or(rest)
            .trim();
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageResponse {
    data: Vec<ImageData>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    b64_json: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_fence() {
        assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(
            strip_json_fence("```json\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
    }
}
