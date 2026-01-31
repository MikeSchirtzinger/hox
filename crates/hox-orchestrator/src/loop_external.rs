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
use hox_core::{BackpressureStatus, HandoffContext, HoxError, Result, Task};
use hox_jj::{JjExecutor, MetadataManager};
use std::path::PathBuf;
use tracing::{debug, info};

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
/// * `task` - The task being worked on
/// * `context` - Handoff context from previous iteration (or initial)
/// * `backpressure` - Backpressure result from previous iteration (or initial)
/// * `iteration` - Current iteration number (1-indexed)
/// * `max_iterations` - Maximum iterations for the loop
/// * `model` - Model to use for agent spawning
/// * `max_tokens` - Maximum tokens for agent response
/// * `workspace_path` - Path to workspace for backpressure checks
/// * `executor` - JJ command executor for running jj fix
/// * `run_backpressure` - Whether to run backpressure checks
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
    task: &Task,
    context: &HandoffContext,
    backpressure: &BackpressureResult,
    iteration: usize,
    max_iterations: usize,
    model: Model,
    max_tokens: usize,
    workspace_path: &PathBuf,
    executor: &E,
    run_backpressure: bool,
) -> Result<ExternalLoopResult> {
    info!(
        "Running external iteration {} of {} for task {}",
        iteration, max_iterations, task.change_id
    );

    // Build prompt with current context and backpressure
    let prompt = build_iteration_prompt(task, context, backpressure, iteration, max_iterations);
    debug!("Prompt length: {} chars", prompt.len());

    // Spawn fresh agent
    let result = spawn_agent(&prompt, iteration, model, max_tokens).await?;

    info!(
        "Agent iteration {} complete ({} chars output)",
        iteration,
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
        context.clone()
    };

    // Update iteration tracking
    updated_context.loop_iteration = Some(iteration);
    updated_context
        .files_touched
        .extend(exec_result.files_created.clone());
    updated_context
        .files_touched
        .extend(exec_result.files_modified.clone());

    // Run backpressure checks if enabled (with jj fix)
    let new_backpressure = if run_backpressure {
        let bp = run_all_checks_with_fix(workspace_path, executor, Some(&task.change_id)).await?;
        info!(
            "Backpressure: tests={}, lints={}, builds={}",
            bp.tests_passed, bp.lints_passed, bp.builds_passed
        );

        // Update context with backpressure status
        updated_context.backpressure_status = Some(BackpressureStatus {
            tests_passed: bp.tests_passed,
            lints_passed: bp.lints_passed,
            builds_passed: bp.builds_passed,
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
        iteration,
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
pub async fn load_state(path: &PathBuf) -> Result<ExternalLoopState> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| HoxError::Io(format!("Failed to read state file: {}", e)))?;

    serde_json::from_str(&content)
        .map_err(|e| HoxError::Io(format!("Failed to parse state JSON: {}", e)))
}

/// Save external loop state to JSON file
pub async fn save_state(state: &ExternalLoopState, path: &PathBuf) -> Result<()> {
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
        assert_eq!(
            detect_stop_signal(output),
            Some("legacy_stop".to_string())
        );
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
