//! Type definitions for Hox agent interactions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Claude model variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Opus,
    #[default]
    Sonnet,
    Haiku,
}

impl Model {
    /// Get the API model name
    pub fn api_name(&self) -> &'static str {
        match self {
            Model::Opus => "claude-opus-4-20250514",
            Model::Sonnet => "claude-sonnet-4-5-20250929",
            Model::Haiku => "claude-haiku-3-5-20250929",
        }
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Model::Opus => write!(f, "opus"),
            Model::Sonnet => write!(f, "sonnet"),
            Model::Haiku => write!(f, "haiku"),
        }
    }
}

impl std::str::FromStr for Model {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "opus" => Ok(Model::Opus),
            "sonnet" => Ok(Model::Sonnet),
            "haiku" => Ok(Model::Haiku),
            _ => Err(format!("Invalid model: {}. Use opus, sonnet, or haiku.", s)),
        }
    }
}

/// Token usage information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

/// Result from a single agent invocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// The iteration number
    pub iteration: usize,
    /// The agent's output text
    pub output: String,
    /// When this result was generated
    pub timestamp: DateTime<Utc>,
    /// Token usage if available
    pub usage: Option<Usage>,
}

/// Anthropic API message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: String,
}

/// Anthropic API request format
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: usize,
    pub messages: Vec<AnthropicMessage>,
}

/// Anthropic API response format
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicResponse {
    #[allow(dead_code)]
    pub id: String,
    pub content: Vec<AnthropicContent>,
    pub usage: Option<Usage>,
}

/// Content block in Anthropic response
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicContent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub content_type: String,
    pub text: String,
}

/// Backpressure check results
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackpressureResult {
    pub tests_passed: bool,
    pub lints_passed: bool,
    pub builds_passed: bool,
    pub errors: Vec<String>,
}

impl BackpressureResult {
    /// Check if all validations passed
    pub fn all_passed(&self) -> bool {
        self.tests_passed && self.lints_passed && self.builds_passed
    }

    /// Create a result where everything passed
    pub fn all_pass() -> Self {
        Self {
            tests_passed: true,
            lints_passed: true,
            builds_passed: true,
            errors: Vec::new(),
        }
    }
}

/// Configuration for the loop engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Maximum number of iterations before stopping
    pub max_iterations: usize,
    /// Model to use for agent spawning
    pub model: Model,
    /// Whether to run backpressure checks
    pub backpressure_enabled: bool,
    /// Maximum tokens for agent responses
    pub max_tokens: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            model: Model::Sonnet,
            backpressure_enabled: true,
            max_tokens: 16000,
        }
    }
}

/// Result from running a complete loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopResult {
    /// Number of iterations completed
    pub iterations: usize,
    /// Whether the loop completed successfully (all checks passed)
    pub success: bool,
    /// Final backpressure status
    pub final_status: BackpressureResult,
    /// Total files created
    pub files_created: Vec<String>,
    /// Total files modified
    pub files_modified: Vec<String>,
    /// Total token usage
    pub total_usage: Usage,
    /// Reason for stopping
    pub stop_reason: StopReason,
}

/// Why the loop stopped
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    /// All backpressure checks passed
    AllChecksPassed,
    /// Reached maximum iterations
    MaxIterations,
    /// Agent requested stop
    AgentStop,
    /// Error occurred
    Error(String),
    /// User cancelled
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_api_names() {
        assert_eq!(Model::Opus.api_name(), "claude-opus-4-20250514");
        assert_eq!(Model::Sonnet.api_name(), "claude-sonnet-4-5-20250929");
        assert_eq!(Model::Haiku.api_name(), "claude-haiku-3-5-20250929");
    }

    #[test]
    fn test_model_default() {
        assert_eq!(Model::default(), Model::Sonnet);
    }

    #[test]
    fn test_model_from_str() {
        assert_eq!("opus".parse::<Model>().unwrap(), Model::Opus);
        assert_eq!("sonnet".parse::<Model>().unwrap(), Model::Sonnet);
        assert_eq!("haiku".parse::<Model>().unwrap(), Model::Haiku);
        assert_eq!("OPUS".parse::<Model>().unwrap(), Model::Opus);
        assert!("invalid".parse::<Model>().is_err());
    }

    #[test]
    fn test_backpressure_all_passed() {
        let result = BackpressureResult::all_pass();
        assert!(result.all_passed());

        let result = BackpressureResult {
            tests_passed: false,
            ..BackpressureResult::all_pass()
        };
        assert!(!result.all_passed());
    }

    #[test]
    fn test_loop_config_default() {
        let config = LoopConfig::default();
        assert_eq!(config.max_iterations, 20);
        assert_eq!(config.model, Model::Sonnet);
        assert!(config.backpressure_enabled);
    }
}
