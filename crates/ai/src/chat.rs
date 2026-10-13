//! A client for OpenAI-compatible chat servers: local ones (Ollama, LM
//! Studio, llama.cpp `llama-server`) and OpenRouter. Synchronous, one
//! request per call; the caller decides about concurrency and retries.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

/// Where requests go.
#[derive(Clone)]
pub struct Endpoint {
    /// Up to and including `/v1` (`https://openrouter.ai/api/v1`,
    /// `http://127.0.0.1:11434/v1`).
    pub base: String,
    pub key: Option<String>,
    pub timeout: Duration,
    /// Send OpenRouter's request fields (provider routing, reasoning). Set
    /// from the base URL; tests point it at a stand-in.
    pub openrouter: bool,
}

impl std::fmt::Debug for Endpoint {
    // Never print the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("base", &self.base).field("key", &self.key.as_ref().map(|_| "…")).finish()
    }
}

pub const OPENROUTER: &str = "https://openrouter.ai/api/v1";

impl Endpoint {
    pub fn new(base: &str, key: Option<String>) -> Self {
        let base = base.trim_end_matches('/').to_string();
        let openrouter = base.starts_with("https://openrouter.ai/");
        Self { base, key, timeout: Duration::from_secs(120), openrouter }
    }

    pub fn is_openrouter(&self) -> bool {
        self.openrouter
    }

    /// The host requests go to, for consent text and the network list.
    pub fn host(&self) -> &str {
        let rest = self.base.split_once("://").map_or(self.base.as_str(), |(_, r)| r);
        rest.split('/').next().unwrap_or(rest)
    }

    fn agent(&self) -> ureq::Agent {
        ureq::Agent::config_builder().timeout_global(Some(self.timeout)).http_status_as_error(false).build().into()
    }

    /// GET a JSON document below the base.
    pub fn get(&self, path: &str) -> Result<Value, ChatError> {
        let mut req = self.agent().get(&format!("{}{path}", self.base)).header("Accept", "application/json");
        if let Some(k) = &self.key {
            req = req.header("Authorization", &format!("Bearer {k}"));
        }
        let mut res = req.call().map_err(ChatError::transport)?;
        let status = res.status().as_u16();
        let body = res.body_mut().read_to_string().map_err(ChatError::transport)?;
        if status >= 400 {
            return Err(ChatError::Status { status, message: message_of(&body) });
        }
        serde_json::from_str(&body).map_err(|e| ChatError::Malformed(e.to_string()))
    }

    /// The model ids the server offers (`GET /models`).
    pub fn models(&self) -> Result<Vec<String>, ChatError> {
        let v = self.get("/models")?;
        Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
    }

    /// `POST /chat/completions` with a prepared body.
    pub fn complete(&self, body: &Value) -> Result<Reply, ChatError> {
        let mut req = self.agent().post(&format!("{}/chat/completions", self.base)).header("Content-Type", "application/json");
        if let Some(k) = &self.key {
            req = req.header("Authorization", &format!("Bearer {k}"));
        }
        if self.is_openrouter() {
            req = req.header("X-Title", "Roll");
        }
        let mut res = req.send(body.to_string()).map_err(ChatError::transport)?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().map_err(ChatError::transport)?;
        if status >= 400 {
            return Err(ChatError::Status { status, message: message_of(&text) });
        }
        let raw: RawReply = serde_json::from_str(&text).map_err(|e| ChatError::Malformed(e.to_string()))?;
        if let Some(err) = raw.error {
            // OpenRouter reports some upstream failures with status 200.
            let status = err.get("code").and_then(Value::as_u64).unwrap_or(502) as u16;
            return Err(ChatError::Status { status, message: err.get("message").and_then(Value::as_str).unwrap_or("").to_string() });
        }
        let content = raw.choices.first().and_then(|c| c.message.content.clone()).unwrap_or_default();
        let usage = raw.usage.unwrap_or_default();
        Ok(Reply {
            id: raw.id.unwrap_or_default(),
            model: raw.model.unwrap_or_default(),
            content,
            tokens_in: usage.prompt_tokens,
            tokens_out: usage.completion_tokens,
            cost: usage.cost,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub id: String,
    pub model: String,
    pub content: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// USD, as reported by OpenRouter; `None` for local servers.
    pub cost: Option<f64>,
}

#[derive(Deserialize)]
struct RawReply {
    id: Option<String>,
    model: Option<String>,
    #[serde(default)]
    choices: Vec<RawChoice>,
    usage: Option<RawUsage>,
    error: Option<serde_json::Map<String, Value>>,
}

#[derive(Deserialize)]
struct RawChoice {
    message: RawMessage,
}

#[derive(Deserialize)]
struct RawMessage {
    content: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    cost: Option<f64>,
}

fn message_of(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).map(String::from))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("could not reach the server: {0}")]
    Transport(String),
    #[error("the server answered {status}: {message}")]
    Status { status: u16, message: String },
    #[error("the server's answer could not be read: {0}")]
    Malformed(String),
}

impl ChatError {
    fn transport(e: impl std::fmt::Display) -> Self {
        Self::Transport(e.to_string())
    }

    /// Worth trying again after a pause: rate limits, server errors,
    /// timeouts and dropped connections.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Status { status, .. } => *status == 408 || *status == 429 || *status >= 500,
            Self::Malformed(_) => false,
        }
    }

    /// The server asks us to slow down.
    pub fn throttled(&self) -> bool {
        matches!(self, Self::Status { status, .. } if *status == 429 || *status >= 500)
    }

    /// Plain-language text for the owner.
    pub fn explain(&self) -> String {
        match self {
            Self::Status { status: 401, .. } => "The API key was not accepted. Check it in Settings › AI.".into(),
            Self::Status { status: 402, .. } => "The OpenRouter account has no credit left.".into(),
            Self::Status { status: 404, message } => format!("The model is not available: {message}"),
            other => other.to_string(),
        }
    }
}

/// Default ports of local OpenAI-compatible servers.
pub const LOCAL_SERVERS: [(&str, &str); 3] =
    [("Ollama", "http://127.0.0.1:11434/v1"), ("LM Studio", "http://127.0.0.1:1234/v1"), ("llama.cpp", "http://127.0.0.1:8080/v1")];

/// Local servers that answer on their default port, with their models.
pub fn detect_local() -> Vec<(String, String, Vec<String>)> {
    LOCAL_SERVERS
        .iter()
        .filter_map(|(name, base)| {
            let mut ep = Endpoint::new(base, None);
            ep.timeout = Duration::from_millis(800);
            ep.models().ok().map(|m| (name.to_string(), base.to_string(), m))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_errors() {
        assert_eq!(Endpoint::new(OPENROUTER, None).host(), "openrouter.ai");
        assert!(Endpoint::new(OPENROUTER, None).is_openrouter());
        assert_eq!(Endpoint::new("http://127.0.0.1:11434/v1/", None).base, "http://127.0.0.1:11434/v1");
        assert!(ChatError::Status { status: 429, message: String::new() }.retryable());
        assert!(!ChatError::Status { status: 401, message: String::new() }.retryable());
        assert_eq!(message_of(r#"{"error":{"message":"No credit"}}"#), "No credit");
        let ep = Endpoint::new(OPENROUTER, Some("sk-or-secret".into()));
        assert!(!format!("{ep:?}").contains("secret"));
    }
}
