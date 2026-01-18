//! Loop Engine - Ralph-style autonomous iteration manager
//!
//! Key design: Each iteration spawns a fresh agent with NO conversation history.
//! State comes from:
//! - JJ change descriptions (HandoffContext, metadata)
//! - Backpressure errors (test/lint/build failures)
//! - Previous iteration results
//!
//! This prevents context compaction/drift that plagues long-running agents.

use crate::backpressure::run_all_checks;
use crate::prompt::{build_iteration_prompt, parse_context_update};
use crate::workspace::WorkspaceManager;
use hox_agent::{
    execute_file_operations, spawn_agent, BackpressureResult, LoopConfig, LoopResult, StopReason,
    Usage,
};
use hox_core::{BackpressureStatus, HandoffContext, HoxError, Result, Task};
use hox_jj::{JjExecutor, MetadataManager};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Loop engine for running Ralph-style autonomous iterations
pub struct LoopEngine<E: JjExecutor> {
    executor: E,
    workspace_manager: WorkspaceManager<E>,
    config: LoopConfig,
    workspace_path: PathBuf,
}

impl<E: JjExecutor + Clone + 'static> LoopEngine<E> {
    /// Create a new loop engine
    pub fn new(
        executor: E,
        workspace_manager: WorkspaceManager<E>,
        config: LoopConfig,
        workspace_path: PathBuf,
    ) -> Self {
        Self {
            executor,
            workspace_manager,
            config,
            workspace_path,
        }
    }

    /// Run the loop on a task
    ///
    /// This is the main entry point for Ralph-style autonomous iteration.
    /// Each iteration:
    /// 1. Reads context from JJ metadata
    /// 2. Runs backpressure checks
    /// 3. Builds a prompt with context + errors
    /// 4. Spawns a fresh agent
    /// 5. Parses file operations from output
    /// 6. Executes writes in workspace
    /// 7. Updates JJ change with new metadata
    /// 8. Repeats until all checks pass or max iterations
    pub async fn run(&mut self, task: &Task) -> Result<LoopResult> {
        info!("Starting loop for task: {}", task.change_id);

        let mut total_usage = Usage::default();
        let mut files_created: Vec<String> = Vec::new();
        let mut files_modified: Vec<String> = Vec::new();

        // Initial context from task
        let mut context = self.read_context(task).await?;

        // Initial backpressure - run before first iteration
        let mut backpressure = if self.config.backpressure_enabled {
            run_all_checks(&self.workspace_path)?
        } else {
            BackpressureResult::all_pass()
        };

        for iteration in 1..=self.config.max_iterations {
            info!("=== Iteration {} of {} ===", iteration, self.config.max_iterations);

            // Check if we're already done
            if backpressure.all_passed() && iteration > 1 {
                info!("All checks passed, loop complete");
                return Ok(LoopResult {
                    iterations: iteration - 1,
                    success: true,
                    final_status: backpressure,
                    files_created,
                    files_modified,
                    total_usage,
                    stop_reason: StopReason::AllChecksPassed,
                });
            }

            // Build prompt
            let prompt = build_iteration_prompt(
                task,
                &context,
                &backpressure,
                iteration,
                self.config.max_iterations,
            );

            debug!("Prompt length: {} chars", prompt.len());

            // Spawn fresh agent
            let result = spawn_agent(
                &prompt,
                iteration,
                self.config.model,
                self.config.max_tokens,
            )
            .await?;

            // Update usage
            if let Some(usage) = &result.usage {
                total_usage.input_tokens += usage.input_tokens;
                total_usage.output_tokens += usage.output_tokens;
            }

            info!(
                "Agent iteration {} complete ({} chars output)",
                iteration,
                result.output.len()
            );

            // Parse and execute file operations
            let exec_result = execute_file_operations(&result.output);
            info!("File operations: {}", exec_result.summary());

            files_created.extend(exec_result.files_created.clone());
            files_modified.extend(exec_result.files_modified.clone());

            // Update context from agent output
            if let Some(new_context) = parse_context_update(&result.output) {
                context = new_context;
                context.loop_iteration = Some(iteration);
                context.files_touched.extend(exec_result.files_created);
                context.files_touched.extend(exec_result.files_modified);
            }

            // Run backpressure checks
            if self.config.backpressure_enabled {
                backpressure = run_all_checks(&self.workspace_path)?;
                info!(
                    "Backpressure: tests={}, lints={}, builds={}",
                    backpressure.tests_passed,
                    backpressure.lints_passed,
                    backpressure.builds_passed
                );

                // Update context with backpressure status
                context.backpressure_status = Some(BackpressureStatus {
                    tests_passed: backpressure.tests_passed,
                    lints_passed: backpressure.lints_passed,
                    builds_passed: backpressure.builds_passed,
                    last_errors: backpressure.errors.clone(),
                });
            }

            // Update JJ metadata with current state
            self.update_metadata(task, &context, iteration).await?;

            // Check for agent-requested stop
            if result.output.contains("[STOP]") || result.output.contains("[DONE]") {
                info!("Agent requested stop");
                return Ok(LoopResult {
                    iterations: iteration,
                    success: backpressure.all_passed(),
                    final_status: backpressure,
                    files_created,
                    files_modified,
                    total_usage,
                    stop_reason: StopReason::AgentStop,
                });
            }
        }

        // Max iterations reached
        warn!("Max iterations ({}) reached", self.config.max_iterations);
        Ok(LoopResult {
            iterations: self.config.max_iterations,
            success: backpressure.all_passed(),
            final_status: backpressure,
            files_created,
            files_modified,
            total_usage,
            stop_reason: StopReason::MaxIterations,
        })
    }

    /// Read context from JJ change metadata
    async fn read_context(&self, task: &Task) -> Result<HandoffContext> {
        let manager = MetadataManager::new(self.executor.clone());
        let metadata = manager.read(&task.change_id).await?;

        // Build context from task description and metadata
        let mut context = HandoffContext {
            current_focus: task.description.clone(),
            loop_iteration: metadata.loop_iteration,
            ..Default::default()
        };

        // Parse progress from task description if present
        if let Some(progress_section) = extract_section(&task.description, "## Progress") {
            context.progress = parse_checklist(&progress_section);
        }

        if let Some(next_section) = extract_section(&task.description, "## Next Steps") {
            context.next_steps = parse_checklist(&next_section);
        }

        if let Some(files_section) = extract_section(&task.description, "## Files Touched") {
            context.files_touched = parse_list(&files_section);
        }

        Ok(context)
    }

    /// Update JJ metadata with loop state
    async fn update_metadata(
        &self,
        task: &Task,
        context: &HandoffContext,
        iteration: usize,
    ) -> Result<()> {
        // Update the change description with context (metadata embedded in description)
        let description = format_description(task, context, iteration);

        // Update via jj describe
        let output = self
            .executor
            .exec(&[
                "describe",
                "-r",
                &task.change_id,
                "-m",
                &description,
            ])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to update description: {}",
                output.stderr
            )));
        }

        Ok(())
    }
}

/// Extract a section from markdown text
fn extract_section(text: &str, header: &str) -> Option<String> {
    let start = text.find(header)?;
    let content_start = start + header.len();

    // Find next section header or end
    let end = text[content_start..]
        .find("\n## ")
        .map(|i| content_start + i)
        .unwrap_or(text.len());

    Some(text[content_start..end].trim().to_string())
}

/// Parse a markdown checklist
fn parse_checklist(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("- [x]") || line.starts_with("- [X]") {
                Some(line[5..].trim().to_string())
            } else if line.starts_with("- [ ]") {
                // Skip unchecked items
                None
            } else {
                None
            }
        })
        .collect()
}

/// Parse a markdown list
fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("- ")
                .map(|s| s.trim().to_string())
        })
        .collect()
}

/// Format a task description with context
fn format_description(task: &Task, context: &HandoffContext, iteration: usize) -> String {
    let mut desc = String::new();

    // Original task title
    let title = task.description.lines().next().unwrap_or(&task.description);
    desc.push_str(title);
    desc.push_str("\n\n");

    // Loop state
    desc.push_str(&format!("Loop-Iteration: {}\n", iteration));

    // Context section
    desc.push_str("\n## Context\n\n");
    desc.push_str(&format!("Focus: {}\n", context.current_focus));

    // Progress
    if !context.progress.is_empty() {
        desc.push_str("\n## Progress\n\n");
        for item in &context.progress {
            desc.push_str(&format!("- [x] {}\n", item));
        }
    }

    // Next steps
    if !context.next_steps.is_empty() {
        desc.push_str("\n## Next Steps\n\n");
        for item in &context.next_steps {
            desc.push_str(&format!("- [ ] {}\n", item));
        }
    }

    // Files touched
    if !context.files_touched.is_empty() {
        desc.push_str("\n## Files Touched\n\n");
        for file in &context.files_touched {
            desc.push_str(&format!("- {}\n", file));
        }
    }

    // Backpressure status
    if let Some(bp) = &context.backpressure_status {
        desc.push_str("\n## Backpressure\n\n");
        desc.push_str(&format!(
            "Tests: {}\n",
            if bp.tests_passed { "PASSED" } else { "FAILED" }
        ));
        desc.push_str(&format!(
            "Lints: {}\n",
            if bp.lints_passed { "PASSED" } else { "FAILED" }
        ));
        desc.push_str(&format!(
            "Builds: {}\n",
            if bp.builds_passed { "PASSED" } else { "FAILED" }
        ));
    }

    desc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section() {
        let text = r#"
# Task

Some description

## Progress

- [x] Done item 1
- [ ] Pending item

## Next Steps

- Do thing
"#;

        let progress = extract_section(text, "## Progress").unwrap();
        assert!(progress.contains("Done item 1"));

        let next = extract_section(text, "## Next Steps").unwrap();
        assert!(next.contains("Do thing"));
    }

    #[test]
    fn test_parse_checklist() {
        let text = r#"
- [x] Completed task
- [X] Also completed
- [ ] Not done yet
- Regular item
"#;

        let items = parse_checklist(text);
        assert_eq!(items.len(), 2);
        assert!(items.contains(&"Completed task".to_string()));
        assert!(items.contains(&"Also completed".to_string()));
    }

    #[test]
    fn test_format_description() {
        let task = Task::new("test-id", "Implement feature X");
        let context = HandoffContext {
            current_focus: "Adding tests".to_string(),
            progress: vec!["Created module".to_string()],
            next_steps: vec!["Add error handling".to_string()],
            blockers: Vec::new(),
            files_touched: vec!["src/lib.rs".to_string()],
            decisions: Vec::new(),
            loop_iteration: Some(3),
            backpressure_status: None,
        };

        let desc = format_description(&task, &context, 3);

        assert!(desc.contains("Implement feature X"));
        assert!(desc.contains("Loop-Iteration: 3"));
        assert!(desc.contains("Adding tests"));
        assert!(desc.contains("Created module"));
        assert!(desc.contains("src/lib.rs"));
    }
}
