//! External Loop Mode - Bash-orchestratable single-iteration execution
//!
//! This module implements external orchestration where each iteration is controlled
//! by an external bash script rather than internal looping. Key features:
//!
//! - Single iteration per invocation
//! - JSON state interchange (input/output)
//! - No internal loop - bash controls the iteration loop
//! - Compatible with external monitoring and control systems

use crate::backpressure::run_all_checks_with_fix;
use crate::prompt::{build_iteration_prompt, parse_context_update};
use hox_agent::{
    execute_file_operations, spawn_agent, BackpressureResult, CompletionPromise,
    ExternalLoopResult, ExternalLoopState, Model,
};
use hox_core::{BackpressureStatus, CheckStatusEntry, HandoffContext, HoxError, Result, Task};
use hox_jj::{JjExecutor, MetadataManager};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Configuration for a single external iteration
pub struct ExternalIterationConfig<'a> {
    /// The task being worked on
    pub task: &'a Task,
    /// Handoff context from previous iteration
    pub context: &'a HandoffContext,
    /// Backpressure result from previous iteration
    pub backpressure: &'a BackpressureResult,
    /// Current iteration number (1-indexed)
    pub iteration: usize,
    /// Maximum iterations for the loop
    pub max_iterations: usize,
    /// Model to use for agent spawning
    pub model: Model,
    /// Maximum tokens for agent response
    pub max_tokens: usize,
    /// Path to workspace for backpressure checks
    pub workspace_path: PathBuf,
    /// Whether to run backpressure checks
    pub run_backpressure: bool,
}

/// Run a single external iteration
///
/// This function executes one iteration of the loop and returns a JSON-serializable
/// result. The external orchestrator (bash script) is responsible for:
/// - Loading state from previous iteration
/// - Calling this function for each iteration
/// - Checking stop conditions
/// - Managing the overall loop
///
/// # Arguments
///
/// * `config` - Iteration configuration (see `ExternalIterationConfig`)
/// * `executor` - JJ command executor for running jj fix
///
/// # Returns
///
/// JSON-serializable result containing:
/// - Iteration number
/// - Success status
/// - Agent output
/// - Updated context
/// - Files created/modified
/// - Token usage
/// - Stop signal (if any)
pub async fn run_external_iteration<E: JjExecutor>(
    config: &ExternalIterationConfig<'_>,
    executor: &E,
) -> Result<ExternalLoopResult> {
    info!(
        "Running external iteration {} of {} for task {}",
        config.iteration, config.max_iterations, config.task.change_id
    );

    // Build prompt with current context and backpressure
    let prompt = build_iteration_prompt(
        config.task,
        config.context,
        config.backpressure,
        config.iteration,
        config.max_iterations,
    );
    debug!("Prompt length: {} chars", prompt.len());

    // Spawn fresh agent
    let result = spawn_agent(&prompt, config.iteration, config.model, config.max_tokens).await?;

    info!(
        "Agent iteration {} complete ({} chars output)",
        config.iteration,
        result.output.len()
    );

    // Parse and execute file operations
    let exec_result = execute_file_operations(&result.output);
    info!("File operations: {}", exec_result.summary());

    // Update context from agent output
    let mut updated_context = if let Some(new_context) = parse_context_update(&result.output) {
        new_context
    } else {
        // If no context update, keep existing
        config.context.clone()
    };

    // Update iteration tracking
    updated_context.loop_iteration = Some(config.iteration);
    updated_context
        .files_touched
        .extend(exec_result.files_created.clone());
    updated_context
        .files_touched
        .extend(exec_result.files_modified.clone());

    // Run backpressure checks if enabled (with jj fix)
    let new_backpressure = if config.run_backpressure {
        let bp = run_all_checks_with_fix(
            &config.workspace_path,
            executor,
            Some(&config.task.change_id),
        )
        .await?;
        for check in &bp.checks {
            info!(
                "  {} {}",
                check.name,
                if check.passed { "PASSED" } else { "FAILED" }
            );
        }

        // Update context with backpressure status
        updated_context.backpressure_status = Some(BackpressureStatus {
            checks: bp
                .checks
                .iter()
                .map(|c| CheckStatusEntry {
                    name: c.name.clone(),
                    passed: c.passed,
                })
                .collect(),
            last_errors: bp.errors.clone(),
        });

        bp
    } else {
        BackpressureResult::all_pass()
    };

    // Detect stop signals
    let stop_signal = detect_stop_signal(&result.output);
    if let Some(signal) = &stop_signal {
        info!("Stop signal detected: {}", signal);
    }

    // Serialize context to JSON for forward compatibility
    let context_json = serde_json::to_value(&updated_context)
        .map_err(|e| HoxError::Io(format!("Failed to serialize context: {}", e)))?;

    Ok(ExternalLoopResult {
        iteration: config.iteration,
        success: new_backpressure.all_passed(),
        output: result.output,
        context: context_json,
        files_created: exec_result.files_created,
        files_modified: exec_result.files_modified,
        usage: result.usage,
        stop_signal,
    })
}

/// Load external loop state from JSON file
pub async fn load_state(path: &Path) -> Result<ExternalLoopState> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| HoxError::Io(format!("Failed to read state file: {}", e)))?;

    serde_json::from_str(&content)
        .map_err(|e| HoxError::Io(format!("Failed to parse state JSON: {}", e)))
}

/// Save external loop state to JSON file
pub async fn save_state(state: &ExternalLoopState, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| HoxError::Io(format!("Failed to serialize state: {}", e)))?;

    tokio::fs::write(path, json)
        .await
        .map_err(|e| HoxError::Io(format!("Failed to write state file: {}", e)))?;

    Ok(())
}

/// Create initial state for a new external loop
pub async fn create_initial_state<E: JjExecutor>(
    executor: E,
    task: &Task,
) -> Result<ExternalLoopState> {
    // Read context from JJ metadata
    let manager = MetadataManager::new(executor);
    let metadata = manager.read(&task.change_id).await?;

    // Build initial context
    let context = HandoffContext {
        current_focus: task.description.clone(),
        loop_iteration: metadata.loop_iteration,
        ..Default::default()
    };

    let context_json = serde_json::to_value(&context)
        .map_err(|e| HoxError::Io(format!("Failed to serialize context: {}", e)))?;

    Ok(ExternalLoopState {
        change_id: task.change_id.clone(),
        iteration: 0,
        context: context_json,
        backpressure: None,
        files_touched: Vec::new(),
    })
}

/// Detect stop signals in agent output
///
/// Returns Some(signal_type) if a stop signal is detected:
/// - "legacy_stop" for [STOP] or [DONE]
/// - "promise_complete" for <promise>COMPLETE</promise>
/// - "promise_with_checks" for promise completion with validation checks needed
fn detect_stop_signal(output: &str) -> Option<String> {
    // Check for legacy signals
    if output.contains("[STOP]") || output.contains("[DONE]") {
        return Some("legacy_stop".to_string());
    }

    // Check for promise completion
    let promise = CompletionPromise::parse(output);
    if promise.is_complete() {
        return Some("promise_complete".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_stop_signal_legacy() {
        let output = "Some work done.\n[DONE]\n";
        assert_eq!(detect_stop_signal(output), Some("legacy_stop".to_string()));
    }

    #[test]
    fn test_detect_stop_signal_promise() {
        let output = "Work complete.\n<promise>COMPLETE</promise>\n";
        assert_eq!(
            detect_stop_signal(output),
            Some("promise_complete".to_string())
        );
    }

    #[test]
    fn test_detect_stop_signal_none() {
        let output = "Still working...";
        assert_eq!(detect_stop_signal(output), None);
    }
}
