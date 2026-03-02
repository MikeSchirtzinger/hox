//! `AgentExecutor` trait — abstraction over agent execution backends.
//!
//! Every backend (Anthropic HTTP API, Claude CLI, OpenAI-compatible) implements
//! this trait. Callers obtain a `Box<dyn AgentExecutor>` from `build_executor`
//! and interact only with this interface.

use async_trait::async_trait;
use chrono::Utc;
use hox_core::Result;
use std::path::PathBuf;

use crate::types::{AgentResult, Model};

/// Single-shot prompt → result execution contract.
///
/// Implementations are responsible for auth, retries, and producing a
/// fully-populated `AgentResult`. The trait is object-safe so callers can
/// store `Box<dyn AgentExecutor>`.
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    /// Execute a prompt and return the agent's result.
    async fn execute(&self, prompt: &str, iteration: usize) -> Result<AgentResult>;

    /// Human-readable name for this backend (used in tracing/logs).
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// AnthropicExecutor — wraps the existing Anthropic HTTP API path
// ---------------------------------------------------------------------------

/// Executes prompts via the Anthropic HTTP API.
///
/// This is a thin wrapper around the `spawn_agent` function so that the
/// Anthropic path participates in the `AgentExecutor` trait hierarchy without
/// duplicating any logic. `spawn_agent` is kept as a free function for
/// backward compatibility.
pub struct AnthropicExecutor {
    pub(crate) model: Model,
    pub(crate) max_tokens: usize,
}

impl AnthropicExecutor {
    pub fn new(model: Model, max_tokens: usize) -> Self {
        Self { model, max_tokens }
    }
}

#[async_trait]
impl AgentExecutor for AnthropicExecutor {
    async fn execute(&self, prompt: &str, iteration: usize) -> Result<AgentResult> {
        crate::client::spawn_agent(prompt, iteration, self.model, self.max_tokens).await
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

// ---------------------------------------------------------------------------
// ClaudeCliExecutor — wraps the existing ClaudeCliBackend
// ---------------------------------------------------------------------------

/// Executes prompts by spawning a `claude` CLI subprocess.
pub struct ClaudeCliExecutor {
    pub(crate) working_dir: PathBuf,
}

impl ClaudeCliExecutor {
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        Self {
            working_dir: working_dir.into(),
        }
    }
}

#[async_trait]
impl AgentExecutor for ClaudeCliExecutor {
    async fn execute(&self, prompt: &str, iteration: usize) -> Result<AgentResult> {
        let backend = crate::claude_cli::ClaudeCliBackend::new(&self.working_dir);
        let response = backend.execute(prompt).await?;
        Ok(AgentResult {
            iteration,
            output: response.text,
            timestamp: Utc::now(),
            usage: Some(response.usage),
        })
    }

    fn name(&self) -> &str {
        "claude-cli"
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/// Construct the appropriate executor from `AgentBackend` config.
///
/// `agent_config` provides optional model/api_base overrides and is consulted
/// first; the `backend` + `model` + `max_tokens` + `working_dir` parameters
/// supply defaults when the override is absent.
pub fn build_executor(
    backend: &hox_core::AgentBackend,
    model: Model,
    max_tokens: usize,
    working_dir: &std::path::Path,
    agent_config: Option<&hox_core::AgentConfig>,
) -> Box<dyn AgentExecutor> {
    // If an explicit agent config is present, let its backend field take
    // precedence over the legacy `backend` parameter.
    let effective_backend = agent_config
        .map(|c| &c.backend)
        .unwrap_or(backend);

    match effective_backend {
        hox_core::AgentBackend::AnthropicApi => {
            Box::new(AnthropicExecutor::new(model, max_tokens))
        }
        hox_core::AgentBackend::ClaudeCli => {
            Box::new(ClaudeCliExecutor::new(working_dir))
        }
        hox_core::AgentBackend::OpenAiCompatible => {
            // Resolve model name and api_base from agent_config overrides.
            let model_name = agent_config
                .and_then(|c| c.model.as_deref())
                .unwrap_or("gpt-4o")
                .to_string();
            let api_base = agent_config
                .and_then(|c| c.api_base.as_deref())
                .unwrap_or("https://api.openai.com/v1")
                .to_string();
            Box::new(crate::openai_client::OpenAiExecutor::new(
                model_name, api_base, max_tokens,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_core::{AgentBackend, AgentConfig};

    #[test]
    fn build_executor_anthropic_by_default() {
        let exec = build_executor(
            &AgentBackend::AnthropicApi,
            Model::Sonnet,
            16000,
            std::path::Path::new("."),
            None,
        );
        assert_eq!(exec.name(), "anthropic");
    }

    #[test]
    fn build_executor_claude_cli() {
        let exec = build_executor(
            &AgentBackend::ClaudeCli,
            Model::Sonnet,
            16000,
            std::path::Path::new("."),
            None,
        );
        assert_eq!(exec.name(), "claude-cli");
    }

    #[test]
    fn build_executor_openai_compatible() {
        let exec = build_executor(
            &AgentBackend::OpenAiCompatible,
            Model::Sonnet,
            16000,
            std::path::Path::new("."),
            None,
        );
        assert_eq!(exec.name(), "openai-compatible");
    }

    #[test]
    fn build_executor_respects_agent_config_backend_override() {
        // Legacy backend says AnthropicApi, but AgentConfig says OpenAiCompatible
        let agent_config = AgentConfig {
            backend: AgentBackend::OpenAiCompatible,
            model: Some("gpt-4o".to_string()),
            api_base: None,
        };
        let exec = build_executor(
            &AgentBackend::AnthropicApi,
            Model::Sonnet,
            16000,
            std::path::Path::new("."),
            Some(&agent_config),
        );
        assert_eq!(exec.name(), "openai-compatible");
    }

    #[test]
    fn build_executor_openai_with_custom_api_base() {
        let agent_config = AgentConfig {
            backend: AgentBackend::OpenAiCompatible,
            model: Some("deepseek-r1".to_string()),
            api_base: Some("https://openrouter.ai/api/v1".to_string()),
        };
        let exec = build_executor(
            &AgentBackend::OpenAiCompatible,
            Model::Sonnet,
            4096,
            std::path::Path::new("."),
            Some(&agent_config),
        );
        assert_eq!(exec.name(), "openai-compatible");
    }
}
