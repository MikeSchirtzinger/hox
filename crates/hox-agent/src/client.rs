//! Anthropic API client for spawning fresh agents
//!
//! Key design: Each agent invocation is completely stateless.
//! No conversation history is maintained - state comes from
//! JJ metadata and backpressure signals.

use crate::auth;
use crate::circuit_breaker::CircuitBreaker;
use crate::types::{AgentResult, AnthropicMessage, AnthropicRequest, AnthropicResponse, Model};
use chrono::Utc;
use hox_core::{HoxError, Result};
use std::sync::OnceLock;
use std::time::Duration;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: usize = 16000;

// Rate limit retry configuration
const MAX_RETRIES: u32 = 5;
const INITIAL_BACKOFF_SECS: u64 = 30;
const MAX_BACKOFF_SECS: u64 = 300; // 5 minutes max

// Global circuit breaker - shared across all agent spawns
static CIRCUIT_BREAKER: OnceLock<CircuitBreaker> = OnceLock::new();

fn get_circuit_breaker() -> &'static CircuitBreaker {
    CIRCUIT_BREAKER.get_or_init(CircuitBreaker::default)
}

/// Agent client for Anthropic API interactions
#[derive(Debug, Clone)]
pub struct AgentClient {
    model: Model,
    max_tokens: usize,
}

impl AgentClient {
    /// Create a new agent client
    pub fn new(model: Model) -> Self {
        Self {
            model,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    /// Set max tokens for responses
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Spawn a fresh agent with the given prompt
    pub async fn spawn(&self, prompt: &str, iteration: usize) -> Result<AgentResult> {
        spawn_agent(prompt, iteration, self.model, self.max_tokens).await
    }
}

impl Default for AgentClient {
    fn default() -> Self {
        Self::new(Model::default())
    }
}

/// Spawn a fresh agent with the given prompt
///
/// This is the key to avoiding context drift - each agent is completely fresh.
/// No conversation history is maintained across iterations.
pub async fn spawn_agent(
    prompt: &str,
    iteration: usize,
    model: Model,
    max_tokens: usize,
) -> Result<AgentResult> {
    tracing::info!(
        "Spawning fresh agent for iteration {} with model {:?}",
        iteration,
        model
    );

    let circuit_breaker = get_circuit_breaker();

    // Check circuit breaker before attempting API call
    if !circuit_breaker.can_execute() {
        let time_until_retry = circuit_breaker.time_until_retry();
        return Err(HoxError::ApiLimit(format!(
            "Circuit breaker is OPEN - too many API failures. Wait {} seconds before retry.",
            time_until_retry / 1000
        )));
    }

    let auth_token = auth::get_auth_token()?;

    let request = AnthropicRequest {
        model: model.api_name().to_string(),
        max_tokens,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    // Retry loop with exponential backoff for rate limits
    let mut retries = 0;
    let mut backoff_secs = INITIAL_BACKOFF_SECS;

    loop {
        tracing::debug!("Sending request to Anthropic API (attempt {})", retries + 1);

        let client = reqwest::Client::new();
        let response = client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &auth_token)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| HoxError::Api(format!("Failed to send request: {}", e)))?;

        let status = response.status();

        // Handle rate limit (429) with retry
        if status.as_u16() == 429 {
            retries += 1;

            if retries > MAX_RETRIES {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown".to_string());
                return Err(HoxError::ApiLimit(format!(
                    "Rate limit exceeded after {} retries. Last error: {}",
                    MAX_RETRIES, error_text
                )));
            }

            // Parse retry-after header if present, otherwise use exponential backoff
            let wait_secs = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(backoff_secs);

            tracing::warn!(
                "Rate limited (429). Waiting {} seconds before retry {}/{}",
                wait_secs,
                retries,
                MAX_RETRIES
            );

            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
            backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
            continue;
        }

        // Handle other errors
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown".to_string());

            // Retry on 5xx errors
            if status.is_server_error() && retries < MAX_RETRIES {
                retries += 1;
                tracing::warn!(
                    "Server error ({}). Waiting {} seconds before retry {}/{}",
                    status,
                    backoff_secs,
                    retries,
                    MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                continue;
            }

            // Non-retryable error - record circuit breaker failure
            circuit_breaker.record_failure();
            tracing::error!(
                "Circuit breaker: recorded failure (count: {})",
                circuit_breaker.failure_count()
            );

            return Err(HoxError::Api(format!(
                "Anthropic API error {}: {}",
                status, error_text
            )));
        }

        // Success - parse response
        let anthropic_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| HoxError::Api(format!("Failed to parse response: {}", e)))?;

        let output = anthropic_response
            .content
            .first()
            .ok_or_else(|| HoxError::Api("No content in response".to_string()))?
            .text
            .clone();

        let usage = anthropic_response.usage;

        // Successful API call - reset circuit breaker
        circuit_breaker.record_success();

        if let Some(ref usage_info) = usage {
            tracing::info!(
                "Agent iteration {} complete ({} chars, {} input tokens, {} output tokens)",
                iteration,
                output.len(),
                usage_info.input_tokens,
                usage_info.output_tokens
            );
        } else {
            tracing::info!(
                "Agent iteration {} complete ({} chars)",
                iteration,
                output.len()
            );
        }

        return Ok(AgentResult {
            iteration,
            output,
            timestamp: Utc::now(),
            usage,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_agent_no_auth() {
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        std::env::remove_var("ANTHROPIC_API_KEY");

        let result = spawn_agent("test prompt", 1, Model::Sonnet, DEFAULT_MAX_TOKENS).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_client_builder() {
        let client = AgentClient::new(Model::Opus).with_max_tokens(8000);
        assert_eq!(client.model, Model::Opus);
        assert_eq!(client.max_tokens, 8000);
    }
}
