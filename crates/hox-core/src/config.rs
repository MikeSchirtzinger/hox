//! Configuration management for Hox
//!
//! This module provides configuration structures for repository-level Hox settings,
//! including protected files, loop defaults, backpressure checks, and model configuration.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::Result;

/// Repository-level Hox configuration
///
/// Loaded from `.hox/config.toml` in the repo root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoxConfig {
    /// Files/directories that agents cannot modify
    #[serde(default = "default_protected_files")]
    pub protected_files: Vec<String>,

    /// Loop execution defaults
    #[serde(default)]
    pub loop_defaults: LoopDefaults,

    /// Backpressure check configuration
    #[serde(default)]
    pub backpressure: BackpressureConfig,

    /// Model selection
    #[serde(default)]
    pub models: ModelConfig,

    /// Agent execution backend (legacy top-level field — kept for backward compat).
    ///
    /// If `agent.backend` is explicitly set, it takes precedence over this field.
    /// New configs should use the `[agent]` table instead.
    #[serde(default)]
    pub agent_backend: AgentBackend,

    /// Fine-grained agent configuration (`[agent]` table).
    ///
    /// When present, `agent.backend` overrides `agent_backend`.
    #[serde(default)]
    pub agent: AgentConfig,

    /// Directory for agent workspaces. Defaults to `.hox-workspaces/` relative to repo root.
    #[serde(default)]
    pub workspace_dir: Option<PathBuf>,
}

/// Default loop execution parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDefaults {
    /// Maximum iterations before stopping
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,

    /// Maximum total tokens (input + output)
    #[serde(default)]
    pub max_tokens: Option<usize>,

    /// Maximum budget in USD
    #[serde(default)]
    pub max_budget_usd: Option<f64>,
}

/// Backpressure check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// Run these checks on every iteration
    #[serde(default = "default_fast_checks")]
    pub fast_checks: Vec<String>,

    /// Run these checks every N iterations
    #[serde(default)]
    pub slow_checks: Vec<SlowCheck>,
}

/// Slow check that runs periodically
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowCheck {
    /// Command to execute
    pub command: String,

    /// Run every N iterations
    pub every_n_iterations: usize,
}

/// Model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Default model to use
    #[serde(default = "default_model")]
    pub default: String,

    /// Environment variable containing API key
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

/// Agent execution backend
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AgentBackend {
    /// Use the Anthropic HTTP API directly (default)
    #[default]
    AnthropicApi,
    /// Spawn a `claude` CLI subprocess
    ClaudeCli,
    /// Use any OpenAI-compatible API (OpenAI, OpenRouter, Ollama, …)
    ///
    /// Serialises as `"openai-compatible"` (explicit rename to avoid
    /// serde kebab-casing it as `"open-ai-compatible"`).
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

/// Fine-grained agent configuration.
///
/// Written to `.hox/config.toml` under the `[agent]` table.  When present,
/// `agent.backend` takes precedence over the legacy top-level `agent_backend`
/// field.
///
/// Example TOML:
/// ```toml
/// [agent]
/// backend = "openai-compatible"
/// model = "gpt-4o"
/// # api_base = "https://openrouter.ai/api/v1"   # optional override
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Which backend to use.  Overrides the legacy `agent_backend` field.
    #[serde(default)]
    pub backend: AgentBackend,
    /// Model name override.  When `None`, a sensible per-backend default is used.
    #[serde(default)]
    pub model: Option<String>,
    /// API base URL.  Relevant only for `OpenAiCompatible`.
    #[serde(default)]
    pub api_base: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            backend: AgentBackend::default(),
            model: None,
            api_base: None,
        }
    }
}

/// Supported programming languages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
}

// Default value providers
fn default_protected_files() -> Vec<String> {
    vec![
        ".git".to_string(),
        ".jj".to_string(),
        ".env".to_string(),
        "Cargo.lock".to_string(),
        ".secrets".to_string(),
        ".gitignore".to_string(),
    ]
}

fn default_fast_checks() -> Vec<String> {
    vec![
        "cargo check".to_string(),
        "cargo clippy".to_string(),
    ]
}

fn default_max_iterations() -> usize {
    20
}

fn default_model() -> String {
    "claude-sonnet-4".to_string()
}

fn default_api_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}

impl HoxConfig {
    /// Returns the configured workspace directory, defaulting to `.hox-workspaces/`.
    pub fn workspace_dir(&self) -> PathBuf {
        self.workspace_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(".hox-workspaces"))
    }

    /// Load configuration from `.hox/config.toml` or use defaults
    pub fn load_or_default(repo_root: &Path) -> Result<Self> {
        let config_path = repo_root.join(".hox/config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&content).map_err(|e| {
                crate::HoxError::Other(format!("Failed to parse config file: {}", e))
            })?)
        } else {
            Ok(Self::default())
        }
    }

    /// Write default configuration to `.hox/config.toml`
    pub fn write_default(repo_root: &Path) -> Result<()> {
        let config_dir = repo_root.join(".hox");
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("config.toml");
        let config = Self::default();
        let content = toml::to_string_pretty(&config).map_err(|e| {
            crate::HoxError::Other(format!("Failed to serialize config: {}", e))
        })?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// Detect the primary programming language of a repository
    pub fn detect_language(repo_root: &Path) -> Option<Language> {
        if repo_root.join("Cargo.toml").exists() {
            Some(Language::Rust)
        } else if repo_root.join("pyproject.toml").exists() {
            Some(Language::Python)
        } else if repo_root.join("package.json").exists() {
            Some(Language::JavaScript)
        } else {
            None
        }
    }

    /// Get default backpressure configuration for a language
    pub fn default_for_language(lang: Language) -> BackpressureConfig {
        match lang {
            Language::Rust => BackpressureConfig {
                fast_checks: vec![
                    "cargo check".to_string(),
                    "cargo clippy".to_string(),
                ],
                slow_checks: vec![SlowCheck {
                    command: "cargo test".to_string(),
                    every_n_iterations: 3,
                }],
            },
            Language::Python => BackpressureConfig {
                fast_checks: vec!["ruff check .".to_string(), "mypy .".to_string()],
                slow_checks: vec![SlowCheck {
                    command: "pytest".to_string(),
                    every_n_iterations: 2,
                }],
            },
            Language::JavaScript => BackpressureConfig {
                fast_checks: vec!["npm run lint".to_string()],
                slow_checks: vec![SlowCheck {
                    command: "npm test".to_string(),
                    every_n_iterations: 2,
                }],
            },
        }
    }
}

impl Default for HoxConfig {
    fn default() -> Self {
        Self {
            protected_files: default_protected_files(),
            loop_defaults: LoopDefaults::default(),
            backpressure: BackpressureConfig::default(),
            models: ModelConfig::default(),
            workspace_dir: None,
            agent_backend: AgentBackend::default(),
            agent: AgentConfig::default(),
        }
    }
}

impl Default for LoopDefaults {
    fn default() -> Self {
        Self {
            max_iterations: default_max_iterations(),
            max_tokens: None,
            max_budget_usd: None,
        }
    }
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            fast_checks: default_fast_checks(),
            slow_checks: vec![],
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: default_model(),
            api_key_env: default_api_key_env(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_returns_default_when_none() {
        let config = HoxConfig::default();
        assert_eq!(config.workspace_dir(), PathBuf::from(".hox-workspaces"));
    }

    #[test]
    fn workspace_dir_returns_configured_path() {
        let config = HoxConfig {
            workspace_dir: Some(PathBuf::from("/custom/workspaces")),
            ..Default::default()
        };
        assert_eq!(config.workspace_dir(), PathBuf::from("/custom/workspaces"));
    }

    #[test]
    fn workspace_dir_roundtrips_through_toml() {
        let config = HoxConfig {
            workspace_dir: Some(PathBuf::from("custom-ws")),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: HoxConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.workspace_dir(), PathBuf::from("custom-ws"));
    }

    #[test]
    fn workspace_dir_toml_default_omits_field() {
        let config = HoxConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        // None serialises as absent; deserialized config should use default path
        let loaded: HoxConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.workspace_dir(), PathBuf::from(".hox-workspaces"));
    }

    #[test]
    fn agent_backend_default_is_anthropic_api() {
        assert_eq!(AgentBackend::default(), AgentBackend::AnthropicApi);
        let config = HoxConfig::default();
        assert_eq!(config.agent_backend, AgentBackend::AnthropicApi);
    }

    #[test]
    fn agent_backend_serializes_to_kebab_case() {
        let api = AgentBackend::AnthropicApi;
        let cli = AgentBackend::ClaudeCli;
        let oai = AgentBackend::OpenAiCompatible;
        assert_eq!(
            serde_json::to_string(&api).unwrap(),
            r#""anthropic-api""#
        );
        assert_eq!(
            serde_json::to_string(&cli).unwrap(),
            r#""claude-cli""#
        );
        assert_eq!(
            serde_json::to_string(&oai).unwrap(),
            r#""openai-compatible""#
        );
    }

    #[test]
    fn agent_backend_deserializes_from_toml() {
        let toml_str = r#"agent_backend = "claude-cli""#;
        // Wrap in a struct that mirrors the field
        #[derive(serde::Deserialize)]
        struct Wrapper {
            agent_backend: AgentBackend,
        }
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.agent_backend, AgentBackend::ClaudeCli);
    }

    #[test]
    fn hox_config_roundtrips_with_claude_cli_backend() {
        let mut config = HoxConfig::default();
        config.agent_backend = AgentBackend::ClaudeCli;
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: HoxConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.agent_backend, AgentBackend::ClaudeCli);
    }

    #[test]
    fn agent_backend_openai_compatible_deserializes_from_toml() {
        let toml_str = r#"agent_backend = "openai-compatible""#;
        #[derive(serde::Deserialize)]
        struct Wrapper {
            agent_backend: AgentBackend,
        }
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.agent_backend, AgentBackend::OpenAiCompatible);
    }

    #[test]
    fn agent_config_default_is_anthropic_api() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.backend, AgentBackend::AnthropicApi);
        assert!(cfg.model.is_none());
        assert!(cfg.api_base.is_none());
    }

    #[test]
    fn agent_config_roundtrips_through_toml() {
        let cfg = AgentConfig {
            backend: AgentBackend::OpenAiCompatible,
            model: Some("gpt-4o".to_string()),
            api_base: Some("https://openrouter.ai/api/v1".to_string()),
        };
        // Wrap so we can serialise the [agent] section.
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            agent: AgentConfig,
        }
        let w = Wrapper { agent: cfg };
        let toml_str = toml::to_string_pretty(&w).unwrap();
        let loaded: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.agent.backend, AgentBackend::OpenAiCompatible);
        assert_eq!(loaded.agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            loaded.agent.api_base.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
    }

    #[test]
    fn hox_config_agent_section_roundtrips() {
        let mut config = HoxConfig::default();
        config.agent = AgentConfig {
            backend: AgentBackend::OpenAiCompatible,
            model: Some("gpt-4o".to_string()),
            api_base: None,
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: HoxConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.agent.backend, AgentBackend::OpenAiCompatible);
        assert_eq!(loaded.agent.model.as_deref(), Some("gpt-4o"));
    }
}
