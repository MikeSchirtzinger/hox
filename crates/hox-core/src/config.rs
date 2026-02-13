//! Configuration management for Hox
//!
//! This module provides configuration structures for repository-level Hox settings,
//! including protected files, loop defaults, backpressure checks, and model configuration.

use serde::{Deserialize, Serialize};
use std::path::Path;

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
