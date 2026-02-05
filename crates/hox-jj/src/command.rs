//! JJ command execution abstraction

use async_trait::async_trait;
use hox_core::{HoxError, Result};
use std::path::PathBuf;
use std::process::Output;
use tokio::process::Command;
use tracing::{debug, instrument};

/// Output from a JJ command
#[derive(Debug, Clone)]
pub struct JjOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl From<Output> for JjOutput {
    fn from(output: Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
        }
    }
}

/// Trait for executing JJ commands (allows mocking in tests)
#[async_trait]
pub trait JjExecutor: Send + Sync {
    /// Execute a JJ command with the given arguments
    async fn exec(&self, args: &[&str]) -> Result<JjOutput>;

    /// Get the repository root
    fn repo_root(&self) -> &PathBuf;
}

/// Real JJ command executor
#[derive(Clone)]
pub struct JjCommand {
    repo_root: PathBuf,
}

impl JjCommand {
    /// Create a new JJ command executor for the given repository
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Auto-detect repository root from current directory
    pub async fn detect() -> Result<Self> {
        let output = Command::new("jj")
            .args(["root"])
            .output()
            .await
            .map_err(|e| HoxError::JjCommand(format!("Failed to run jj root: {}", e)))?;

        if !output.status.success() {
            return Err(HoxError::JjCommand("Not in a jj repository".to_string()));
        }

        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::new(root))
    }
}

#[async_trait]
impl JjExecutor for JjCommand {
    #[instrument(skip(self), fields(repo = %self.repo_root.display()))]
    async fn exec(&self, args: &[&str]) -> Result<JjOutput> {
        debug!("Executing jj {:?}", args);

        let output = Command::new("jj")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| HoxError::JjCommand(format!("Failed to execute jj: {}", e)))?;

        let jj_output = JjOutput::from(output);

        if !jj_output.success {
            debug!("JJ command failed: {}", jj_output.stderr);
        }

        Ok(jj_output)
    }

    fn repo_root(&self) -> &PathBuf {
        &self.repo_root
    }
}

/// Mock JJ executor for testing
#[derive(Clone)]
pub struct MockJjExecutor {
    repo_root: PathBuf,
    responses: std::collections::HashMap<String, JjOutput>,
}

impl Default for MockJjExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl MockJjExecutor {
    pub fn new() -> Self {
        Self {
            repo_root: PathBuf::from("/mock/repo"),
            responses: std::collections::HashMap::new(),
        }
    }

    pub fn with_response(mut self, command: &str, output: JjOutput) -> Self {
        self.responses.insert(command.to_string(), output);
        self
    }
}

#[async_trait]
impl JjExecutor for MockJjExecutor {
    async fn exec(&self, args: &[&str]) -> Result<JjOutput> {
        let key = args.join(" ");
        self.responses
            .get(&key)
            .cloned()
            .ok_or_else(|| HoxError::JjCommand(format!("No mock response for: {}", key)))
    }

    fn repo_root(&self) -> &PathBuf {
        &self.repo_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_executor() {
        let executor = MockJjExecutor::new().with_response(
            "log -r @",
            JjOutput {
                stdout: "test output".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let output = executor.exec(&["log", "-r", "@"]).await.unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "test output");
    }
}
