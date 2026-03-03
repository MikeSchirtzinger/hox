//! OpenAI-compatible API backend for agent execution.
//!
//! Supports:
//! - OpenAI          — `https://api.openai.com/v1`
//! - OpenRouter      — `https://openrouter.ai/api/v1`
//! - Ollama (local)  — `http://localhost:11434/v1`
//! - Any other OpenAI-compatible endpoint via `api_base` override.
//!
//! Auth priority:
//!   1. `OPENAI_API_KEY`
//!   2. `OPENROUTER_API_KEY`
//!   Ollama typically needs no key; if neither env var is set the key is
//!   sent as an empty string (Ollama accepts that).

use async_trait::async_trait;
use chrono::Utc;
use hox_core::{HoxError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::auth::get_openai_auth_token;
use crate::executor::AgentExecutor;
use crate::types::{AgentResult, Usage};

// Retry configuration — mirrors the Anthropic client's strategy.
const MAX_RETRIES: u32 = 5;
const INITIAL_BACKOFF_SECS: u64 = 10;
const MAX_BACKOFF_SECS: u64 = 120;

/// Agent executor that calls OpenAI-compatible `/chat/completions` endpoints.
pub struct OpenAiExecutor {
    /// Model name, e.g. `"gpt-4o"`, `"deepseek-r1"`, `"llama3"`.
    pub model: String,
    /// Base URL for the API, e.g. `"https://api.openai.com/v1"`.
    pub api_base: String,
    /// Maximum tokens for the response.
    pub max_tokens: usize,
}

impl OpenAiExecutor {
    pub fn new(model: impl Into<String>, api_base: impl Into<String>, max_tokens: usize) -> Self {
        Self {
            model: model.into(),
            api_base: api_base.into(),
            max_tokens,
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAI wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: usize,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Executor implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AgentExecutor for OpenAiExecutor {
    async fn execute(&self, prompt: &str, iteration: usize) -> Result<AgentResult> {
        info!(
            "OpenAI executor: iteration {} model={} api_base={}",
            iteration, self.model, self.api_base
        );

        // Auth — empty string is fine for Ollama / unauthenticated proxies.
        let auth_token = get_openai_auth_token().unwrap_or_default();

        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));

        let request_body = ChatRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let client = reqwest::Client::new();

        let mut retries = 0u32;
        let mut backoff_secs = INITIAL_BACKOFF_SECS;

        loop {
            debug!(
                "OpenAI request to {} (attempt {})",
                url,
                retries + 1
            );

            let mut req = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&request_body);

            if !auth_token.is_empty() {
                req = req.header("authorization", format!("Bearer {}", auth_token));
            }

            let response = req
                .send()
                .await
                .map_err(|e| HoxError::Api(format!("OpenAI request failed: {}", e)))?;

            let status = response.status();

            // Rate limit — retry with backoff.
            if status.as_u16() == 429 {
                retries += 1;
                if retries > MAX_RETRIES {
                    let body = response.text().await.unwrap_or_default();
                    return Err(HoxError::ApiLimit(format!(
                        "OpenAI rate limit exceeded after {} retries: {}",
                        MAX_RETRIES, body
                    )));
                }
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(backoff_secs);

                warn!(
                    "OpenAI rate limited (429). Waiting {}s before retry {}/{}",
                    wait, retries, MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(wait)).await;
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                continue;
            }

            // Server errors — retry.
            if status.is_server_error() && retries < MAX_RETRIES {
                retries += 1;
                warn!(
                    "OpenAI server error ({}). Waiting {}s before retry {}/{}",
                    status, backoff_secs, retries, MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                continue;
            }

            // Other non-success statuses are hard errors.
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(HoxError::Api(format!(
                    "OpenAI API error {}: {}",
                    status, body
                )));
            }

            // Parse success.
            let chat: ChatResponse = response
                .json()
                .await
                .map_err(|e| HoxError::Api(format!("OpenAI parse error: {}", e)))?;

            let output = chat
                .choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .ok_or_else(|| HoxError::Api("OpenAI response had no choices".to_string()))?;

            let usage = chat.usage.map(|u| Usage {
                input_tokens: u.prompt_tokens.unwrap_or(0) as usize,
                output_tokens: u.completion_tokens.unwrap_or(0) as usize,
            });

            info!(
                "OpenAI executor iteration {} complete ({} chars)",
                iteration,
                output.len()
            );

            return Ok(AgentResult {
                iteration,
                output,
                timestamp: Utc::now(),
                usage,
            });
        }
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_name_is_openai_compatible() {
        let e = OpenAiExecutor::new("gpt-4o", "https://api.openai.com/v1", 4096);
        assert_eq!(e.name(), "openai-compatible");
    }

    #[test]
    fn executor_stores_fields_correctly() {
        let e = OpenAiExecutor::new("deepseek-r1", "https://openrouter.ai/api/v1", 8192);
        assert_eq!(e.model, "deepseek-r1");
        assert_eq!(e.api_base, "https://openrouter.ai/api/v1");
        assert_eq!(e.max_tokens, 8192);
    }

    #[test]
    fn url_construction_trims_trailing_slash() {
        // Verify that a trailing slash on api_base doesn't produce a double slash.
        let base = "https://api.openai.com/v1/";
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }
}
