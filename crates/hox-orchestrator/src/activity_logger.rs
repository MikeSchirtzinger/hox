//! Activity Logger - Human-readable iteration logging to `.hox/activity.md`
//!
//! Provides transparent insight into loop engine progress by logging:
//! - Iteration starts and completions
//! - File changes (created/modified)
//! - Backpressure check results
//! - Agent output summaries
//! - Final loop summaries with token usage

use hox_agent::{BackpressureResult, Usage};
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use chrono::Utc;

/// Activity logger for loop iterations
pub struct ActivityLogger {
    output_path: PathBuf,
}

impl ActivityLogger {
    /// Create a new activity logger
    pub fn new(hox_dir: PathBuf) -> Self {
        Self {
            output_path: hox_dir.join("activity.md"),
        }
    }

    /// Log the start of a loop
    pub async fn log_loop_start(
        &self,
        task_desc: &str,
        max_iterations: usize,
    ) -> Result<(), std::io::Error> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

        let content = format!(
            "# Hox Activity Log\n\n## Task: {}\n**Started**: {}\n**Max Iterations**: {}\n\n---\n\n",
            task_desc.lines().next().unwrap_or(task_desc),
            timestamp,
            max_iterations
        );

        // Create or overwrite the file
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.output_path)
            .await?;

        file.write_all(content.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    /// Log the start of an iteration
    pub async fn log_iteration_start(
        &self,
        iteration: usize,
        max: usize,
    ) -> Result<(), std::io::Error> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

        let content = format!(
            "### Iteration {}/{}\n**Time**: {}\n\n",
            iteration,
            max,
            timestamp
        );

        self.append(&content).await
    }

    /// Log the completion of an iteration
    pub async fn log_iteration_complete(
        &self,
        iteration: usize,
        agent_output: &str,
        files_created: &[String],
        files_modified: &[String],
        backpressure: &BackpressureResult,
    ) -> Result<(), std::io::Error> {
        let mut content = String::new();

        // Iteration completion marker
        content.push_str(&format!("**Iteration {} completed**\n\n", iteration));

        // Backpressure status
        content.push_str(&format!(
            "**Backpressure**: Tests={}, Lints={}, Builds={}\n\n",
            if backpressure.tests_passed { "PASS" } else { "FAIL" },
            if backpressure.lints_passed { "PASS" } else { "FAIL" },
            if backpressure.builds_passed { "PASS" } else { "FAIL" }
        ));

        // Files changed
        if !files_created.is_empty() || !files_modified.is_empty() {
            content.push_str("**Files Changed**:\n");

            for file in files_created {
                content.push_str(&format!("- Created: {}\n", file));
            }

            for file in files_modified {
                content.push_str(&format!("- Modified: {}\n", file));
            }

            content.push('\n');
        }

        // Agent output (truncated to first 500 chars)
        let output_preview = if agent_output.chars().count() > 500 {
            let truncated: String = agent_output.chars().take(500).collect();
            format!("{truncated}...")
        } else {
            agent_output.to_string()
        };

        content.push_str("**Agent Output** (truncated):\n");
        content.push_str("> ");
        content.push_str(&output_preview.replace('\n', "\n> "));
        content.push_str("\n\n");

        // Errors if any
        if !backpressure.errors.is_empty() {
            content.push_str("**Errors**:\n");
            for error in &backpressure.errors {
                content.push_str(&format!("- {}\n", error));
            }
            content.push('\n');
        }

        content.push_str("---\n\n");

        self.append(&content).await
    }

    /// Log loop completion summary
    pub async fn log_loop_complete(
        &self,
        total_iterations: usize,
        success: bool,
        total_usage: &Usage,
        stop_reason: &str,
    ) -> Result<(), std::io::Error> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

        let success_icon = if success { "✓" } else { "✗" };
        let success_text = if success {
            "All checks passed"
        } else {
            "Checks failed"
        };

        let content = format!(
            "## Loop Summary\n\n\
            **Completed**: {}\n\
            **Total Iterations**: {}\n\
            **Success**: {} {}\n\
            **Stop Reason**: {}\n\
            **Tokens**: {} input, {} output\n\n",
            timestamp,
            total_iterations,
            success_icon,
            success_text,
            stop_reason,
            total_usage.input_tokens,
            total_usage.output_tokens
        );

        self.append(&content).await
    }

    /// Append content to the activity log
    async fn append(&self, content: &str) -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path)
            .await?;

        file.write_all(content.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_log_loop_start() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let logger = ActivityLogger::new(hox_dir.clone());

        logger
            .log_loop_start("Implement feature X", 20)
            .await
            .unwrap();

        let content = fs::read_to_string(hox_dir.join("activity.md"))
            .await
            .unwrap();

        assert!(content.contains("# Hox Activity Log"));
        assert!(content.contains("Task: Implement feature X"));
        assert!(content.contains("**Max Iterations**: 20"));
    }

    #[tokio::test]
    async fn test_log_iteration() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let logger = ActivityLogger::new(hox_dir.clone());

        logger.log_loop_start("Test task", 5).await.unwrap();
        logger.log_iteration_start(1, 5).await.unwrap();

        let backpressure = BackpressureResult {
            tests_passed: true,
            lints_passed: true,
            builds_passed: false,
            errors: vec!["Build error: missing semicolon".to_string()],
        };

        logger
            .log_iteration_complete(
                1,
                "Agent output here...",
                &["src/main.rs".to_string()],
                &["Cargo.toml".to_string()],
                &backpressure,
            )
            .await
            .unwrap();

        let content = fs::read_to_string(hox_dir.join("activity.md"))
            .await
            .unwrap();

        assert!(content.contains("### Iteration 1/5"));
        assert!(content.contains("Tests=PASS"));
        assert!(content.contains("Builds=FAIL"));
        assert!(content.contains("Created: src/main.rs"));
        assert!(content.contains("Modified: Cargo.toml"));
        assert!(content.contains("Build error: missing semicolon"));
    }

    #[tokio::test]
    async fn test_log_loop_complete() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let logger = ActivityLogger::new(hox_dir.clone());

        let usage = Usage {
            input_tokens: 45234,
            output_tokens: 23456,
        };

        logger.log_loop_start("Test task", 5).await.unwrap();
        logger
            .log_loop_complete(3, true, &usage, "All checks passed")
            .await
            .unwrap();

        let content = fs::read_to_string(hox_dir.join("activity.md"))
            .await
            .unwrap();

        assert!(content.contains("## Loop Summary"));
        assert!(content.contains("**Total Iterations**: 3"));
        assert!(content.contains("✓ All checks passed"));
        assert!(content.contains("45234 input"));
        assert!(content.contains("23456 output"));
    }

    #[tokio::test]
    async fn test_truncate_long_output() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let logger = ActivityLogger::new(hox_dir.clone());

        let long_output = "x".repeat(1000);
        let backpressure = BackpressureResult::all_pass();

        logger.log_loop_start("Test", 1).await.unwrap();
        logger.log_iteration_start(1, 1).await.unwrap();
        logger
            .log_iteration_complete(1, &long_output, &[], &[], &backpressure)
            .await
            .unwrap();

        let content = fs::read_to_string(hox_dir.join("activity.md"))
            .await
            .unwrap();

        // Should be truncated with "..."
        assert!(content.contains("..."));
        assert!(!content.contains(&"x".repeat(600)));
    }
}
