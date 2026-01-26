//! Prompt builder for agent iterations
//!
//! Constructs prompts that provide agents with:
//! - Task description and context
//! - Progress from previous iterations
//! - Backpressure errors to fix
//! - File operation instructions

use hox_agent::{file_operation_instructions, BackpressureResult};
use hox_core::{HandoffContext, Task};

/// Build a prompt for a loop iteration
///
/// This prompt provides the agent with all context needed to continue work:
/// - Task description
/// - Current progress and focus
/// - Validation status and errors to fix
/// - Instructions for file operations
pub fn build_iteration_prompt(
    task: &Task,
    context: &HandoffContext,
    backpressure: &BackpressureResult,
    iteration: usize,
    max_iterations: usize,
) -> String {
    let mut prompt = String::new();

    // Header
    prompt.push_str(&format!(
        "# HOX AGENT - Iteration {} of {}\n\n",
        iteration, max_iterations
    ));

    // Task section
    prompt.push_str("## TASK\n\n");
    prompt.push_str(&task.description);
    prompt.push_str("\n\n");

    // Current context
    prompt.push_str("## CURRENT CONTEXT\n\n");
    if !context.current_focus.is_empty() {
        prompt.push_str(&format!("**Focus:** {}\n\n", context.current_focus));
    }

    if !context.progress.is_empty() {
        prompt.push_str("**Progress:**\n");
        for item in &context.progress {
            prompt.push_str(&format!("- [x] {}\n", item));
        }
        prompt.push('\n');
    }

    if !context.next_steps.is_empty() {
        prompt.push_str("**Next Steps:**\n");
        for item in &context.next_steps {
            prompt.push_str(&format!("- [ ] {}\n", item));
        }
        prompt.push('\n');
    }

    if !context.blockers.is_empty() {
        prompt.push_str("**Blockers:**\n");
        for item in &context.blockers {
            prompt.push_str(&format!("- {}\n", item));
        }
        prompt.push('\n');
    }

    if !context.files_touched.is_empty() {
        prompt.push_str("**Files Touched:**\n");
        for file in &context.files_touched {
            prompt.push_str(&format!("- {}\n", file));
        }
        prompt.push('\n');
    }

    // Validation status
    prompt.push_str("## VALIDATION STATUS\n\n");
    prompt.push_str(&format!(
        "- Tests: {}\n",
        if backpressure.tests_passed {
            "PASSED"
        } else {
            "FAILED"
        }
    ));
    prompt.push_str(&format!(
        "- Lints: {}\n",
        if backpressure.lints_passed {
            "PASSED"
        } else {
            "FAILED"
        }
    ));
    prompt.push_str(&format!(
        "- Builds: {}\n",
        if backpressure.builds_passed {
            "PASSED"
        } else {
            "FAILED"
        }
    ));
    prompt.push('\n');

    // Errors to fix
    if !backpressure.errors.is_empty() {
        prompt.push_str("## ERRORS TO FIX\n\n");
        prompt.push_str("You MUST fix these errors before proceeding:\n\n");
        for error in &backpressure.errors {
            prompt.push_str("```\n");
            prompt.push_str(error);
            prompt.push_str("\n```\n\n");
        }
    }

    // File operation instructions
    prompt.push_str(file_operation_instructions());
    prompt.push('\n');

    // Objective
    prompt.push_str("## OBJECTIVE\n\n");
    if !backpressure.all_passed() {
        prompt.push_str("1. **FIX** all validation failures shown above\n");
        prompt.push_str("2. Make necessary code changes using <write_to_file> blocks\n");
        prompt.push_str("3. Explain what you changed and why\n");
    } else {
        prompt.push_str("1. Continue implementing the task\n");
        prompt.push_str("2. Complete the next step from the plan\n");
        prompt.push_str("3. Write code using <write_to_file> blocks\n");
    }
    prompt.push('\n');

    // Context update request
    prompt.push_str("## UPDATE CONTEXT\n\n");
    prompt.push_str("At the end of your response, provide an updated context block:\n\n");
    prompt.push_str("```context\n");
    prompt.push_str("FOCUS: <what you're working on now>\n");
    prompt.push_str("PROGRESS: <what was completed this iteration>\n");
    prompt.push_str("NEXT: <what needs to happen next>\n");
    prompt.push_str("BLOCKERS: <any blockers, or 'none'>\n");
    prompt.push_str("```\n\n");

    // Completion signal instructions
    prompt.push_str("## COMPLETION SIGNAL\n\n");
    prompt.push_str("When the task is fully complete and all validation checks pass, signal completion:\n\n");
    prompt.push_str("<promise>COMPLETE</promise>\n\n");
    prompt.push_str("Optionally include reasoning for completion:\n\n");
    prompt.push_str("<completion_reasoning>\n");
    prompt.push_str("Explanation of what was completed and why the task is done.\n");
    prompt.push_str("Confidence: XX%\n");
    prompt.push_str("</completion_reasoning>\n\n");
    prompt.push_str("Note: Only use this when the task is genuinely complete.\n");
    prompt.push_str("Legacy formats [STOP] and [DONE] are also supported.\n");

    prompt
}

/// Parse context updates from agent output
pub fn parse_context_update(output: &str) -> Option<HandoffContext> {
    let start = output.find("```context")?;
    let end = output[start..].find("```\n").or_else(|| output[start..].rfind("```"))?;

    let block = &output[start + "```context".len()..start + end];

    let mut context = HandoffContext::default();

    for line in block.lines() {
        let line = line.trim();
        if let Some(focus) = line.strip_prefix("FOCUS:") {
            context.current_focus = focus.trim().to_string();
        } else if let Some(progress) = line.strip_prefix("PROGRESS:") {
            context.progress.push(progress.trim().to_string());
        } else if let Some(next) = line.strip_prefix("NEXT:") {
            context.next_steps.push(next.trim().to_string());
        } else if let Some(blockers) = line.strip_prefix("BLOCKERS:") {
            let blockers = blockers.trim();
            if blockers.to_lowercase() != "none" && !blockers.is_empty() {
                context.blockers.push(blockers.to_string());
            }
        }
    }

    Some(context)
}

/// Build a simple one-shot prompt for tasks that don't need iteration
pub fn build_simple_prompt(task: &Task) -> String {
    let mut prompt = String::new();

    prompt.push_str("# TASK\n\n");
    prompt.push_str(&task.description);
    prompt.push_str("\n\n");

    prompt.push_str(file_operation_instructions());

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_test_task() -> Task {
        Task::new("test-change-id", "Implement a hello world function")
    }

    #[test]
    fn test_build_iteration_prompt() {
        let task = make_test_task();
        let context = HandoffContext {
            current_focus: "Adding function".to_string(),
            progress: vec!["Created file".to_string()],
            next_steps: vec!["Add tests".to_string()],
            blockers: Vec::new(),
            files_touched: vec!["src/lib.rs".to_string()],
            decisions: Vec::new(),
            loop_iteration: Some(1),
            backpressure_status: None,
        };
        let backpressure = BackpressureResult {
            tests_passed: true,
            lints_passed: false,
            builds_passed: true,
            errors: vec!["clippy warning: unused variable".to_string()],
        };

        let prompt = build_iteration_prompt(&task, &context, &backpressure, 1, 20);

        assert!(prompt.contains("HOX AGENT - Iteration 1 of 20"));
        assert!(prompt.contains("hello world"));
        assert!(prompt.contains("Adding function"));
        assert!(prompt.contains("Lints: FAILED"));
        assert!(prompt.contains("clippy warning"));
        assert!(prompt.contains("<write_to_file>"));
    }

    #[test]
    fn test_parse_context_update() {
        let output = r#"
I've implemented the changes.

```context
FOCUS: Adding error handling
PROGRESS: Implemented main function
NEXT: Add unit tests
BLOCKERS: none
```
"#;

        let context = parse_context_update(output).unwrap();
        assert_eq!(context.current_focus, "Adding error handling");
        assert_eq!(context.progress, vec!["Implemented main function"]);
        assert_eq!(context.next_steps, vec!["Add unit tests"]);
        assert!(context.blockers.is_empty());
    }

    #[test]
    fn test_parse_context_with_blockers() {
        let output = r#"
```context
FOCUS: Working on feature
PROGRESS: Started implementation
NEXT: Continue
BLOCKERS: Need API documentation
```
"#;

        let context = parse_context_update(output).unwrap();
        assert_eq!(context.blockers, vec!["Need API documentation"]);
    }
}
