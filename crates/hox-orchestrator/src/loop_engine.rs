//! Loop Engine - Ralph-style autonomous iteration manager
//!
//! Key design: Each iteration spawns a fresh agent with NO conversation history.
//! State comes from:
//! - JJ change descriptions (HandoffContext, metadata)
//! - Backpressure errors (test/lint/build failures)
//! - Previous iteration results
//!
//! This prevents context compaction/drift that plagues long-running agents.

use crate::activity_logger::ActivityLogger;
use crate::backpressure::{run_all_checks_with_fix, run_failed_checks};
use crate::prompt::{build_iteration_prompt, parse_context_update};
use crate::recovery::RecoveryManager;
use crate::workspace::WorkspaceManager;
use hox_agent::{
    execute_file_operations, spawn_agent, BackpressureResult, CompletionPromise, LoopConfig,
    LoopResult, StopReason, Usage,
};
use hox_core::{BackpressureStatus, CheckStatusEntry, HandoffContext, HoxError, Result, Task};
use hox_jj::{JjExecutor, MetadataManager};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Loop engine for running Ralph-style autonomous iterations
pub struct LoopEngine<E: JjExecutor> {
    executor: E,
    workspace_manager: WorkspaceManager<E>,
    config: LoopConfig,
    workspace_path: PathBuf,
    activity_logger: Option<ActivityLogger>,
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
            activity_logger: None,
        }
    }

    /// Enable activity logging to `.hox/activity.md`
    pub fn with_activity_logging(mut self, hox_dir: PathBuf) -> Self {
        self.activity_logger = Some(ActivityLogger::new(hox_dir));
        self
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

        // Log loop start if activity logging is enabled
        if let Some(logger) = &self.activity_logger {
            logger
                .log_loop_start(&task.description, self.config.max_iterations)
                .await
                .map_err(|e| HoxError::Io(format!("Failed to log loop start: {}", e)))?;
        }

        let mut total_usage = Usage::default();
        let mut files_created: Vec<String> = Vec::new();
        let mut files_modified: Vec<String> = Vec::new();

        // Initial context from task
        let mut context = self.read_context(task).await?;

        // Skip initial backpressure - first iteration always runs.
        // Backpressure feedback only matters starting from iteration 2.
        let mut backpressure = BackpressureResult::all_pass();

        // Create recovery manager for rollback capability
        let recovery_manager =
            RecoveryManager::new(self.executor.clone(), self.workspace_path.clone());

        let mut iteration: usize = 0;
        loop {
            iteration += 1;
            if self.config.max_iterations > 0 && iteration > self.config.max_iterations {
                break;
            }
            let max_display = if self.config.max_iterations == 0 {
                "unlimited".to_string()
            } else {
                self.config.max_iterations.to_string()
            };
            info!("=== Iteration {} of {} ===", iteration, max_display);

            // Log iteration start
            if let Some(logger) = &self.activity_logger {
                logger
                    .log_iteration_start(iteration, self.config.max_iterations)
                    .await
                    .map_err(|e| HoxError::Io(format!("Failed to log iteration start: {}", e)))?;
            }

            // Check if we're already done
            if backpressure.all_passed() && iteration > 1 {
                info!("All checks passed, loop complete");

                // Log completion
                if let Some(logger) = &self.activity_logger {
                    logger
                        .log_loop_complete(iteration - 1, true, &total_usage, "All checks passed")
                        .await
                        .map_err(|e| HoxError::Io(format!("Failed to log completion: {}", e)))?;
                }

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

            // Create recovery point before spawning agent
            let recovery_point = recovery_manager
                .create_recovery_point(format!("Before iteration {}", iteration))
                .await?;
            debug!(
                "Recovery point created: {} ({})",
                recovery_point.operation_id, recovery_point.description
            );

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

            // Check if agent output is empty or broken
            if result.output.trim().is_empty() {
                warn!(
                    "Agent iteration {} produced empty output, rolling back",
                    iteration
                );

                // Rollback to recovery point
                let rollback_result = recovery_manager.restore_from(&recovery_point).await?;
                info!(
                    "Rolled back {} operations due to empty agent output",
                    rollback_result.operations_undone
                );

                // Continue to next iteration
                continue;
            }

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

            // Store iteration's files before moving exec_result
            let iteration_files_created = exec_result.files_created;
            let iteration_files_modified = exec_result.files_modified;

            // Update context from agent output
            if let Some(new_context) = parse_context_update(&result.output) {
                context = new_context;
                context.loop_iteration = Some(iteration);
                context
                    .files_touched
                    .extend(iteration_files_created.clone());
                context
                    .files_touched
                    .extend(iteration_files_modified.clone());
            }

            // Run backpressure checks (selective: only re-run previously failed)
            if self.config.backpressure_enabled {
                backpressure = if iteration == 1 {
                    // First iteration: run all checks with jj fix to establish baseline
                    run_all_checks_with_fix(
                        &self.workspace_path,
                        &self.executor,
                        Some(&task.change_id),
                    )
                    .await?
                } else {
                    // Subsequent: only re-run checks that failed last time
                    run_failed_checks(&self.workspace_path, &backpressure)?
                };

                for check in &backpressure.checks {
                    info!(
                        "  {} {}",
                        check.name,
                        if check.passed { "PASSED" } else { "FAILED" }
                    );
                }

                // Update context with backpressure status
                context.backpressure_status = Some(BackpressureStatus {
                    checks: backpressure
                        .checks
                        .iter()
                        .map(|c| CheckStatusEntry {
                            name: c.name.clone(),
                            passed: c.passed,
                        })
                        .collect(),
                    last_errors: backpressure.errors.clone(),
                });
            }

            // Update JJ metadata with current state
            self.update_metadata(task, &context, iteration).await?;

            // Log iteration completion
            if let Some(logger) = &self.activity_logger {
                logger
                    .log_iteration_complete(
                        iteration,
                        &result.output,
                        &iteration_files_created,
                        &iteration_files_modified,
                        &backpressure,
                    )
                    .await
                    .map_err(|e| {
                        HoxError::Io(format!("Failed to log iteration completion: {}", e))
                    })?;
            }

            // Check for agent-requested stop (legacy format)
            if result.output.contains("[STOP]") || result.output.contains("[DONE]") {
                info!("Agent requested stop");

                // Log completion
                if let Some(logger) = &self.activity_logger {
                    logger
                        .log_loop_complete(
                            iteration,
                            backpressure.all_passed(),
                            &total_usage,
                            "Agent requested stop",
                        )
                        .await
                        .map_err(|e| HoxError::Io(format!("Failed to log completion: {}", e)))?;
                }

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

            // Check for completion promise (Ralph-style)
            let promise = CompletionPromise::parse(&result.output);
            if promise.is_complete() {
                info!("Agent signaled completion via promise");
                if let Some(reasoning) = &promise.reasoning {
                    debug!("Completion reasoning: {}", reasoning);
                }
                if let Some(confidence) = promise.confidence() {
                    debug!("Completion confidence: {:.0}%", confidence * 100.0);
                }

                // If all checks passed, it's a clean completion
                // Otherwise, agent is requesting completion with checks
                let stop_reason = if backpressure.all_passed() {
                    StopReason::PromiseComplete
                } else {
                    info!("Promise signaled but validation checks not all passed");
                    StopReason::PromiseCompleteWithChecks
                };

                // Log completion
                if let Some(logger) = &self.activity_logger {
                    let reason_str = match &stop_reason {
                        StopReason::PromiseComplete => "Promise complete (all checks passed)",
                        StopReason::PromiseCompleteWithChecks => {
                            "Promise complete (with validation checks)"
                        }
                        // Other stop reasons use default message
                        _ => "Promise complete",
                    };
                    logger
                        .log_loop_complete(
                            iteration,
                            backpressure.all_passed(),
                            &total_usage,
                            reason_str,
                        )
                        .await
                        .map_err(|e| HoxError::Io(format!("Failed to log completion: {}", e)))?;
                }

                return Ok(LoopResult {
                    iterations: iteration,
                    success: backpressure.all_passed(),
                    final_status: backpressure,
                    files_created,
                    files_modified,
                    total_usage,
                    stop_reason,
                });
            }
        }

        // Max iterations reached (only when max_iterations > 0)
        warn!("Max iterations ({}) reached", self.config.max_iterations);

        // Log completion
        if let Some(logger) = &self.activity_logger {
            logger
                .log_loop_complete(
                    self.config.max_iterations,
                    backpressure.all_passed(),
                    &total_usage,
                    "Max iterations reached",
                )
                .await
                .map_err(|e| HoxError::Io(format!("Failed to log completion: {}", e)))?;
        }

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
            .exec(&["describe", "-r", &task.change_id, "-m", &description])
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
        .filter_map(|line| line.trim().strip_prefix("- ").map(|s| s.trim().to_string()))
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

    // Backpressure status (dynamic checks)
    if let Some(bp) = &context.backpressure_status {
        desc.push_str("\n## Backpressure\n\n");
        for check in &bp.checks {
            desc.push_str(&format!(
                "{}: {}\n",
                check.name,
                if check.passed { "PASSED" } else { "FAILED" }
            ));
        }
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

    #[test]
    fn test_format_description_with_dynamic_checks() {
        let task = Task::new("test-id", "Fix build");
        let context = HandoffContext {
            current_focus: "Fixing".to_string(),
            backpressure_status: Some(BackpressureStatus {
                checks: vec![
                    CheckStatusEntry {
                        name: "build".into(),
                        passed: true,
                    },
                    CheckStatusEntry {
                        name: "lint".into(),
                        passed: false,
                    },
                    CheckStatusEntry {
                        name: "test".into(),
                        passed: true,
                    },
                ],
                last_errors: Vec::new(),
            }),
            ..Default::default()
        };

        let desc = format_description(&task, &context, 2);
        assert!(desc.contains("build: PASSED"));
        assert!(desc.contains("lint: FAILED"));
        assert!(desc.contains("test: PASSED"));
    }
}
