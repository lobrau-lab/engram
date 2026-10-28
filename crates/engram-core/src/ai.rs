//! OpenAI-compatible LLM sidecar client and grounded prompt construction.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::Provider;

#[derive(Debug, Clone, Serialize)]
pub struct Msg {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReq {
    pub model: String,
    pub messages: Vec<Msg>,
    pub temperature: f32,
    pub max_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ChatOpts {
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for ChatOpts {
    fn default() -> Self {
        Self {
            temperature: 0.2,
            max_tokens: 512,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatResp {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: RespMsg,
}

#[derive(Debug, Deserialize)]
struct RespMsg {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResp {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

pub trait LlmClient {
    fn provider(&self) -> Provider;
    fn complete(&self, prompt: &str, opts: &ChatOpts) -> Result<String>;
}

/// Validate Engram's Rust inference boundary.
///
/// Local loopback HTTP is allowed because MLX-Serve/oMLX and similar local
/// sidecars expose HTTP APIs. Internet and arbitrary-network destinations are
/// rejected before ureq is constructed or invoked.
pub fn validate_local_endpoint(base: &str) -> Result<Url> {
    let url = Url::parse(base)
        .map_err(|_| anyhow!("inference endpoint must be a valid plain HTTP loopback URL"))?;
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("inference endpoint must use plain HTTP without userinfo")
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("inference endpoint must include a loopback host"))?;
    if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]") {
        anyhow::bail!("inference endpoint host must be localhost, 127.0.0.1, or ::1")
    }
    Ok(url)
}

pub struct OpenAiCompat {
    base: String,
    model: String,
    key: Option<String>,
    provider: Provider,
}

impl OpenAiCompat {
    pub fn new(
        base: impl Into<String>,
        model: impl Into<String>,
        key: Option<String>,
        provider: Provider,
    ) -> Self {
        Self {
            base: base.into(),
            model: model.into(),
            key,
            provider,
        }
    }
}

impl LlmClient for OpenAiCompat {
    fn provider(&self) -> Provider {
        self.provider
    }

    fn complete(&self, prompt: &str, opts: &ChatOpts) -> Result<String> {
        let endpoint = validate_local_endpoint(&self.base)?;
        let url = format!(
            "{}/chat/completions",
            endpoint.as_str().trim_end_matches('/')
        );
        let model = if self.model.trim().is_empty() {
            self.discover_model()?
        } else {
            self.model.clone()
        };
        let req = ChatReq {
            model,
            messages: vec![Msg {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
        };

        let mut call = ureq::post(&url).set("Content-Type", "application/json");
        if let Some(k) = &self.key {
            call = call.set("Authorization", &format!("Bearer {k}"));
        }
        let resp = call
            .send_json(serde_json::to_value(&req)?)
            .map_err(|e| anyhow!("LLM request to {url} failed: {e}"))?;
        let parsed: ChatResp = resp.into_json().context("parsing LLM response")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("LLM returned no choices"))
    }
}

impl OpenAiCompat {
    fn discover_model(&self) -> Result<String> {
        let endpoint = validate_local_endpoint(&self.base)?;
        let url = format!("{}/models", endpoint.as_str().trim_end_matches('/'));
        let mut call = ureq::get(&url);
        if let Some(k) = &self.key {
            call = call.set("Authorization", &format!("Bearer {k}"));
        }
        let resp: ModelsResp = call
            .call()
            .map_err(|e| anyhow!("model discovery request to {url} failed: {e}"))?
            .into_json()
            .context("parsing model discovery response")?;
        resp.data
            .into_iter()
            .find(|m| !m.id.trim().is_empty())
            .map(|m| m.id)
            .ok_or_else(|| anyhow!("LLM server advertised no models at {url}"))
    }
}

/// Build a grounded prompt from retrieved notes. Has an explicit empty-hits
/// guard and a divide-by-zero-safe per-note budget.
pub fn build_context_prompt(
    question: &str,
    notes: &[(String, String)],
    max_chars: usize,
) -> String {
    if notes.is_empty() {
        return format!(
            "You are Engram, a local-first notes assistant. No notes in the vault matched \
             the question.\nSay so plainly, then answer from general knowledge without citing \
             any note.\n\nQuestion: {question}\n\nAnswer:"
        );
    }
    let budget = (max_chars / notes.len()).max(1);
    let mut parts = Vec::with_capacity(notes.len());
    for (id, body) in notes {
        parts.push(format!("### {id}\n{}", truncate(body, budget)));
    }
    format!(
        "You are Engram, a local-first notes assistant. Answer ONLY from the notes below and \
         cite note ids like [note.md]. If the notes do not contain the answer, say so.\n\n\
         Notes:\n{}\n\nQuestion: {question}\n\nAnswer:",
        parts.join("\n\n")
    )
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn test_validate_local_endpoint_valid() {
        assert_eq!(
            validate_local_endpoint("http://localhost:8080/v1").unwrap(),
            Url::parse("http://localhost:8080/v1").unwrap()
        );
        assert_eq!(
            validate_local_endpoint("http://127.0.0.1:11434").unwrap(),
            Url::parse("http://127.0.0.1:11434").unwrap()
        );
        assert_eq!(
            validate_local_endpoint("http://[::1]:8080/api").unwrap(),
            Url::parse("http://[::1]:8080/api").unwrap()
        );
    }

    #[test]
    fn test_validate_local_endpoint_invalid_scheme() {
        let err = validate_local_endpoint("https://localhost:8080/v1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "inference endpoint must use plain HTTP without userinfo"
        );
    }

    #[test]
    fn test_validate_local_endpoint_with_userinfo() {
        let err = validate_local_endpoint("http://user:pass@localhost:8080/v1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "inference endpoint must use plain HTTP without userinfo"
        );
    }

    #[test]
    fn test_validate_local_endpoint_non_loopback() {
        let err = validate_local_endpoint("http://example.com:8080/v1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "inference endpoint host must be localhost, 127.0.0.1, or ::1"
        );
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello world", 100), "hello world");
        assert_eq!(truncate("hello world", 5), "hello…");
        assert_eq!(truncate("hello world", 0), "…");

        // "a" (1 byte), "👋" (4 bytes), "b" (1 byte)
        let s = "a👋b";
        assert_eq!(truncate(s, 6), "a👋b");
        // max 5 -> cuts before b, keeping "a👋"
        assert_eq!(truncate(s, 5), "a👋…");
        // max 4 -> cuts before b, keeping "a👋"
        assert_eq!(truncate(s, 4), "a…"); // Wait, char_boundary at 4 is actually after 👋 but max is 4, loop finds boundary at 1

        let s2 = "a👋b";
        // Let's test the boundary loop manually: max=4.
        // end=4: is_char_boundary(4)? "a"(1) + "👋"(4) = 5. So at 4 it's inside the emoji.
        // end=3: inside. end=2: inside. end=1: after 'a'.
        // So truncate(s2, 4) should be "a…".
        assert_eq!(truncate(s2, 4), "a…");
    }

    #[test]
    fn test_build_context_prompt_empty() {
        let prompt = build_context_prompt("What is life?", &[], 1000);
        assert!(prompt.contains("No notes in the vault matched"));
        assert!(prompt.contains("Question: What is life?"));
    }

    #[test]
    fn test_build_context_prompt_with_notes() {
        let notes = vec![
            ("note1.md".to_string(), "Life is 42.".to_string()),
            ("note2.md".to_string(), "Another note.".to_string()),
        ];
        let prompt = build_context_prompt("What is life?", &notes, 1000);
        assert!(prompt.contains("### note1.md\nLife is 42."));
        assert!(prompt.contains("### note2.md\nAnother note."));
        assert!(prompt.contains("Question: What is life?"));
    }

    #[test]
    fn test_build_context_prompt_truncation() {
        let notes = vec![
            ("note1.md".to_string(), "This is a very long note that needs to be truncated.".to_string()),
            ("note2.md".to_string(), "Another very long note here.".to_string()),
        ];
        // Budget per note = 20 / 2 = 10
        let prompt = build_context_prompt("What?", &notes, 20);
        assert!(prompt.contains("### note1.md\nThis is a …"));
        assert!(prompt.contains("### note2.md\nAnother ve…"));
    }
}
