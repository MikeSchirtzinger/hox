//! JJ command executor abstraction layer
//!
//! This module provides an abstraction for executing jj commands, allowing for
//! both real command execution and mocked execution for testing.

use async_trait::async_trait;
use bd_core::{HoxError, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, instrument};

#[cfg(test)]
use std::collections::HashMap;

/// Trait for executing jj commands
///
/// This trait allows for different implementations of JJ command execution,
/// including real execution via `Command` and mocked execution for testing.
#[async_trait]
pub trait JjExecutor: Send + Sync {
    /// Execute a jj command with the given arguments
    ///
    /// # Arguments
    /// * `args` - Command line arguments to pass to jj
    ///
    /// # Returns
    /// The stdout output from the command on success
    async fn exec(&self, args: &[&str]) -> Result<String>;

    /// Execute a jj command and return both stdout and stderr
    ///
    /// # Arguments
    /// * `args` - Command line arguments to pass to jj
    ///
    /// # Returns
    /// A tuple of (stdout, stderr) on success
    async fn exec_with_stderr(&self, args: &[&str]) -> Result<(String, String)> {
        // Default implementation just calls exec and returns empty stderr
        // Implementations can override for better error handling
        let stdout = self.exec(args).await?;
        Ok((stdout, String::new()))
    }
}

/// Default implementation of JjExecutor using tokio Command
///
/// This executor runs actual jj commands against a repository.
pub struct JjCommand {
    repo_path: PathBuf,
}

impl JjCommand {
    /// Create a new JjCommand executor for the given repository
    ///
    /// # Arguments
    /// * `repo_path` - Path to the jj repository
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
        }
    }

    /// Get the repository path
    pub fn repo_path(&self) -> &PathBuf {
        &self.repo_path
    }
}

#[async_trait]
impl JjExecutor for JjCommand {
    #[instrument(skip(self), fields(repo = %self.repo_path.display(), args = ?args))]
    async fn exec(&self, args: &[&str]) -> Result<String> {
        debug!("Executing jj command");
        let (stdout, _) = self.exec_with_stderr(args).await?;
        Ok(stdout)
    }

    #[instrument(skip(self), fields(repo = %self.repo_path.display(), args = ?args))]
    async fn exec_with_stderr(&self, args: &[&str]) -> Result<(String, String)> {
        // Validate repository path exists
        if !self.repo_path.exists() {
            return Err(HoxError::RepoNotFound(format!(
                "Repository path does not exist: {}",
                self.repo_path.display()
            )));
        }

        // Build the command
        let mut cmd = Command::new("jj");
        cmd.arg("--repository")
            .arg(&self.repo_path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Execute the command
        debug!("Running jj command");
        let output = cmd.output().await.map_err(|e| {
            HoxError::JjError(format!("Failed to execute jj command: {}", e))
        })?;

        // Convert stdout and stderr to strings
        let stdout = String::from_utf8(output.stdout)
            .map_err(|e| HoxError::Parse(format!("invalid UTF-8 in stdout: {}", e)))?;
        let stderr = String::from_utf8(output.stderr)
            .map_err(|e| HoxError::Parse(format!("invalid UTF-8 in stderr: {}", e)))?;

        // Check if the command was successful
        if !output.status.success() {
            return Err(HoxError::JjError(format!(
                "Command failed with exit code {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        Ok((stdout, stderr))
    }
}

/// Mock executor for testing
///
/// This executor returns pre-configured responses for specific command arguments,
/// allowing for deterministic testing without actually running jj commands.
#[cfg(test)]
pub struct MockJjExecutor {
    /// Map of command arguments to their expected responses
    responses: HashMap<Vec<String>, String>,
    /// Map of command arguments to their expected stderr
    stderr_responses: HashMap<Vec<String>, String>,
    /// Whether to fail on unknown commands (default: true)
    fail_on_unknown: bool,
}

#[cfg(test)]
impl MockJjExecutor {
    /// Create a new mock executor with no pre-configured responses
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            stderr_responses: HashMap::new(),
            fail_on_unknown: true,
        }
    }

    /// Add a response for a specific command
    ///
    /// # Arguments
    /// * `args` - The command arguments
    /// * `response` - The response to return when these args are executed
    pub fn add_response(&mut self, args: Vec<String>, response: String) -> &mut Self {
        self.responses.insert(args, response);
        self
    }

    /// Add a stderr response for a specific command
    ///
    /// # Arguments
    /// * `args` - The command arguments
    /// * `stderr` - The stderr to return when these args are executed
    pub fn add_stderr(&mut self, args: Vec<String>, stderr: String) -> &mut Self {
        self.stderr_responses.insert(args, stderr);
        self
    }

    /// Set whether to fail on unknown commands
    ///
    /// If true (default), executing a command without a configured response will error.
    /// If false, unknown commands return an empty string.
    pub fn fail_on_unknown(&mut self, fail: bool) -> &mut Self {
        self.fail_on_unknown = fail;
        self
    }

    /// Helper to add a response with string slice args
    pub fn with_response(mut self, args: &[&str], response: &str) -> Self {
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.add_response(args_vec, response.to_string());
        self
    }
}

#[cfg(test)]
impl Default for MockJjExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[async_trait]
impl JjExecutor for MockJjExecutor {
    async fn exec(&self, args: &[&str]) -> Result<String> {
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        match self.responses.get(&args_vec) {
            Some(response) => Ok(response.clone()),
            None => {
                if self.fail_on_unknown {
                    Err(HoxError::JjError(format!(
                        "Mock executor: no response configured for args: {:?}",
                        args
                    )))
                } else {
                    Ok(String::new())
                }
            }
        }
    }

    async fn exec_with_stderr(&self, args: &[&str]) -> Result<(String, String)> {
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let stdout = match self.responses.get(&args_vec) {
            Some(response) => response.clone(),
            None => {
                if self.fail_on_unknown {
                    return Err(HoxError::JjError(format!(
                        "Mock executor: no response configured for args: {:?}",
                        args
                    )));
                } else {
                    String::new()
                }
            }
        };

        let stderr = self.stderr_responses.get(&args_vec).cloned().unwrap_or_default();

        Ok((stdout, stderr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_executor_with_response() {
        let mut mock = MockJjExecutor::new();
        mock.add_response(
            vec!["log".to_string(), "-r".to_string(), "@".to_string()],
            "test output".to_string(),
        );

        let result = mock.exec(&["log", "-r", "@"]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test output");
    }

    #[tokio::test]
    async fn test_mock_executor_unknown_command() {
        let mock = MockJjExecutor::new();
        let result = mock.exec(&["unknown", "command"]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_executor_unknown_command_no_fail() {
        let mut mock = MockJjExecutor::new();
        mock.fail_on_unknown(false);

        let result = mock.exec(&["unknown", "command"]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[tokio::test]
    async fn test_mock_executor_with_stderr() {
        let mut mock = MockJjExecutor::new();
        mock.add_response(
            vec!["status".to_string()],
            "working copy clean".to_string(),
        );
        mock.add_stderr(
            vec!["status".to_string()],
            "warning: something".to_string(),
        );

        let result = mock.exec_with_stderr(&["status"]).await;
        assert!(result.is_ok());
        let (stdout, stderr) = result.unwrap();
        assert_eq!(stdout, "working copy clean");
        assert_eq!(stderr, "warning: something");
    }

    #[tokio::test]
    async fn test_mock_executor_builder_pattern() {
        let mock = MockJjExecutor::new()
            .with_response(&["log", "-r", "@"], "commit abc123");

        let result = mock.exec(&["log", "-r", "@"]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "commit abc123");
    }
}
