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

/// Severity of a check failure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Compilation errors, syntax errors - must fix
    Breaking,
    /// Lint warnings, style issues, test failures - nice to fix
    Warning,
}

/// Outcome of a single backpressure check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckOutcome {
    /// Human-readable name (e.g., "build", "lint", "test")
    pub name: String,
    /// Whether the check passed
    pub passed: bool,
    /// How severe a failure is
    pub severity: Severity,
    /// Error output (empty if passed)
    pub output: String,
}

/// Backpressure check results - generalized for any project type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackpressureResult {
    /// Individual check outcomes
    pub checks: Vec<CheckOutcome>,
    /// Breaking errors only (filtered from failed checks)
    pub errors: Vec<String>,
}

impl BackpressureResult {
    /// Check if all checks passed
    pub fn all_passed(&self) -> bool {
        self.checks.is_empty() || self.checks.iter().all(|c| c.passed)
    }

    /// Create a result where everything passed (no checks run)
    pub fn all_pass() -> Self {
        Self {
            checks: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Get names of failed checks (for selective re-run)
    pub fn failed_check_names(&self) -> Vec<&str> {
        self.checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect()
    }
}

/// Configuration for the loop engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Maximum number of iterations before stopping. 0 = no limit.
    pub max_iterations: usize,
    /// Model to use for agent spawning
    pub model: Model,
    /// Whether to run backpressure checks
    pub backpressure_enabled: bool,
    /// Maximum tokens for agent responses
    pub max_tokens: usize,
    /// Budget cap per agent invocation in USD. None = no limit.
    pub max_budget_usd: Option<f64>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 0, // 0 = no limit
            model: Model::Sonnet,
            backpressure_enabled: true,
            max_tokens: 16000,
            max_budget_usd: None,
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
    /// Agent signaled completion via <promise>COMPLETE</promise>
    PromiseComplete,
    /// Agent signaled completion with validation checks requested
    PromiseCompleteWithChecks,
    /// Error occurred
    Error(String),
    /// User cancelled
    Cancelled,
}

/// External orchestration state (JSON-serializable)
/// Used for bash-orchestratable single-iteration mode with JSON interchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLoopState {
    /// JJ change ID being worked on
    pub change_id: String,
    /// Current iteration number
    pub iteration: usize,
    /// Handoff context from previous iteration
    #[serde(flatten)]
    pub context: serde_json::Value,
    /// Backpressure result from previous iteration
    pub backpressure: Option<BackpressureResult>,
    /// Files touched so far
    pub files_touched: Vec<String>,
}

/// Result from a single external iteration
/// This is output as JSON to stdout for external orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLoopResult {
    /// Iteration number
    pub iteration: usize,
    /// Whether this iteration succeeded
    pub success: bool,
    /// Agent's raw output text
    pub output: String,
    /// Updated context (JSON for forward compatibility)
    pub context: serde_json::Value,
    /// Files created in this iteration
    pub files_created: Vec<String>,
    /// Files modified in this iteration
    pub files_modified: Vec<String>,
    /// Token usage for this iteration
    pub usage: Option<Usage>,
    /// Stop signal if present ("[DONE]", "promise", etc.)
    pub stop_signal: Option<String>,
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
            checks: vec![CheckOutcome {
                name: "build".into(),
                passed: false,
                severity: Severity::Breaking,
                output: "error".into(),
            }],
            errors: vec!["error".into()],
        };
        assert!(!result.all_passed());
    }

    #[test]
    fn test_failed_check_names() {
        let result = BackpressureResult {
            checks: vec![
                CheckOutcome {
                    name: "build".into(),
                    passed: true,
                    severity: Severity::Breaking,
                    output: String::new(),
                },
                CheckOutcome {
                    name: "lint".into(),
                    passed: false,
                    severity: Severity::Warning,
                    output: "warning".into(),
                },
            ],
            errors: Vec::new(),
        };
        assert_eq!(result.failed_check_names(), vec!["lint"]);
    }

    #[test]
    fn test_loop_config_default() {
        let config = LoopConfig::default();
        assert_eq!(config.max_iterations, 0); // 0 = no limit
        assert_eq!(config.model, Model::Sonnet);
        assert!(config.backpressure_enabled);
        assert_eq!(config.max_budget_usd, None);
    }
}
