//! Claude CLI subprocess backend for agent execution
//!
//! Spawns `claude --print --output-format json -p <prompt>` as a subprocess,
//! parsing the JSON output into an `AgentResponse`.

use std::path::PathBuf;

use hox_core::{HoxError, Result};
use tokio::process::Command;
use std::process::Stdio;

use crate::types::{AgentResponse, Usage};

/// Agent backend that spawns a `claude` CLI subprocess.
///
/// Uses `claude --print --output-format json -p <prompt>` which runs a
/// single-shot non-interactive session and emits JSON to stdout.
pub struct ClaudeCliBackend {
    working_dir: PathBuf,
}

impl ClaudeCliBackend {
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        Self {
            working_dir: working_dir.into(),
        }
    }

    /// Returns `true` if the `claude` binary is present and exits successfully.
    pub async fn is_available() -> bool {
        Command::new("claude")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Execute a task prompt via the `claude` CLI subprocess.
    pub async fn execute(&self, prompt: &str) -> Result<AgentResponse> {
        let output = Command::new("claude")
            .args(["--print", "--output-format", "json", "-p"])
            .arg(prompt)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| HoxError::Agent(format!("Failed to spawn claude CLI: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HoxError::Agent(format!("claude CLI failed: {}", stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_claude_output(&stdout)
    }
}

/// Parse JSON output produced by `claude --print --output-format json`.
///
/// The CLI emits a JSON object with at minimum a `result` string field.
/// Example shape:
/// ```json
/// {
///   "result": "Here is the answer...",
///   "usage": { "input_tokens": 100, "output_tokens": 50 }
/// }
/// ```
pub(crate) fn parse_claude_output(output: &str) -> Result<AgentResponse> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(HoxError::Agent("claude CLI produced empty output".into()));
    }

    let v: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| HoxError::Agent(format!("Failed to parse claude CLI JSON: {}", e)))?;

    let text = v
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    let usage = if let Some(u) = v.get("usage") {
        Usage {
            input_tokens: u
                .get("input_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as usize,
            output_tokens: u
                .get("output_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as usize,
        }
    } else {
        Usage::default()
    };

    Ok(AgentResponse {
        thinking: String::new(),
        tool_calls: Vec::new(),
        text,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn is_available_does_not_crash_when_binary_missing() {
        // This test just verifies the function returns a bool without panicking.
        // The actual result depends on the environment.
        let _result = ClaudeCliBackend::is_available().await;
    }

    #[test]
    fn parse_valid_json_with_result_and_usage() {
        let json = r#"{"result":"Hello world","usage":{"input_tokens":10,"output_tokens":5}}"#;
        let resp = parse_claude_output(json).unwrap();
        assert_eq!(resp.text, "Hello world");
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert!(resp.tool_calls.is_empty());
        assert!(resp.thinking.is_empty());
    }

    #[test]
    fn parse_valid_json_without_usage() {
        let json = r#"{"result":"Done"}"#;
        let resp = parse_claude_output(json).unwrap();
        assert_eq!(resp.text, "Done");
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }

    #[test]
    fn parse_json_missing_result_field_returns_empty_text() {
        let json = r#"{"other_field":"value"}"#;
        let resp = parse_claude_output(json).unwrap();
        assert_eq!(resp.text, "");
    }

    #[test]
    fn parse_malformed_json_returns_error() {
        let bad = "not json at all {{}}";
        assert!(parse_claude_output(bad).is_err());
    }

    #[test]
    fn parse_empty_output_returns_error() {
        assert!(parse_claude_output("").is_err());
        assert!(parse_claude_output("   ").is_err());
    }
}
