//! Backpressure checks for steering agent work
//!
//! Runs tests, lints, and builds to provide concrete failure signals
//! that guide the agent's next iteration.

use hox_agent::BackpressureResult;
use hox_core::Result;
use std::path::Path;
use std::process::Command;

/// Run all backpressure checks (tests, lints, builds)
///
/// Auto-detects project type based on configuration files present.
pub fn run_all_checks(workspace_path: &Path) -> Result<BackpressureResult> {
    tracing::info!("Running backpressure checks in {:?}", workspace_path);

    let tests = run_tests(workspace_path);
    let lints = run_lints(workspace_path);
    let builds = run_builds(workspace_path);

    let mut errors = Vec::new();

    if !tests.passed {
        errors.extend(tests.errors);
    }
    if !lints.passed {
        errors.extend(lints.errors);
    }
    if !builds.passed {
        errors.extend(builds.errors);
    }

    Ok(BackpressureResult {
        tests_passed: tests.passed,
        lints_passed: lints.passed,
        builds_passed: builds.passed,
        errors,
    })
}

struct CheckResult {
    passed: bool,
    errors: Vec<String>,
}

/// Run tests (auto-detect project type)
fn run_tests(workspace_path: &Path) -> CheckResult {
    tracing::debug!("Running tests");

    // Try Rust first
    if workspace_path.join("Cargo.toml").exists() {
        return run_command(workspace_path, "cargo", &["test", "--", "--nocapture"]);
    }

    // Try Python
    if workspace_path.join("pyproject.toml").exists()
        || workspace_path.join("pytest.ini").exists()
    {
        return run_command(workspace_path, "pytest", &["-v"]);
    }

    // Try Node.js
    if workspace_path.join("package.json").exists() {
        return run_command(workspace_path, "npm", &["test"]);
    }

    // No test framework detected
    CheckResult {
        passed: true,
        errors: vec!["No test framework detected, skipping tests".to_string()],
    }
}

/// Run lints (auto-detect project type)
fn run_lints(workspace_path: &Path) -> CheckResult {
    tracing::debug!("Running lints");

    // Try Rust clippy
    if workspace_path.join("Cargo.toml").exists() {
        return run_command(workspace_path, "cargo", &["clippy", "--", "-D", "warnings"]);
    }

    // Try Python ruff
    if workspace_path.join("pyproject.toml").exists() {
        return run_command(workspace_path, "ruff", &["check", "."]);
    }

    // Try ESLint
    if workspace_path.join(".eslintrc.js").exists()
        || workspace_path.join(".eslintrc.json").exists()
        || workspace_path.join("eslint.config.js").exists()
    {
        return run_command(workspace_path, "npx", &["eslint", "."]);
    }

    // No linter detected
    CheckResult {
        passed: true,
        errors: vec!["No linter detected, skipping lints".to_string()],
    }
}

/// Run builds (auto-detect project type)
fn run_builds(workspace_path: &Path) -> CheckResult {
    tracing::debug!("Running builds");

    // Try Rust build
    if workspace_path.join("Cargo.toml").exists() {
        return run_command(workspace_path, "cargo", &["build"]);
    }

    // Try Python build
    if workspace_path.join("setup.py").exists() {
        return run_command(workspace_path, "python", &["setup.py", "build"]);
    }

    // Try Node.js build
    if workspace_path.join("package.json").exists() {
        // Check if build script exists
        if let Ok(content) = std::fs::read_to_string(workspace_path.join("package.json")) {
            if content.contains("\"build\"") {
                return run_command(workspace_path, "npm", &["run", "build"]);
            }
        }
    }

    // No build process detected
    CheckResult {
        passed: true,
        errors: vec!["No build process detected, skipping build".to_string()],
    }
}

/// Run a command and capture its result
fn run_command(workspace_path: &Path, program: &str, args: &[&str]) -> CheckResult {
    let output = Command::new(program)
        .args(args)
        .current_dir(workspace_path)
        .output();

    match output {
        Ok(output) => {
            let passed = output.status.success();
            let mut errors = Vec::new();

            if !passed {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);

                // Truncate very long output
                let truncate = |s: &str, max: usize| -> String {
                    if s.len() > max {
                        format!("{}...[truncated]", &s[..max])
                    } else {
                        s.to_string()
                    }
                };

                errors.push(format!(
                    "{} {} failed:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                    program,
                    args.join(" "),
                    truncate(stdout.trim(), 4000),
                    truncate(stderr.trim(), 4000)
                ));
            }

            CheckResult { passed, errors }
        }
        Err(e) => {
            // Command not found or execution failed
            CheckResult {
                passed: false,
                errors: vec![format!("Failed to run {}: {}", program, e)],
            }
        }
    }
}

/// Format backpressure errors for inclusion in agent prompt
pub fn format_errors_for_prompt(result: &BackpressureResult) -> String {
    if result.errors.is_empty() {
        return "All checks passed!\n".to_string();
    }

    let mut output = String::from("## Backpressure Failures\n\n");
    output.push_str("The following checks failed. Fix these issues:\n\n");

    for error in &result.errors {
        output.push_str("```\n");
        output.push_str(error);
        output.push_str("\n```\n\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_format_errors_empty() {
        let result = BackpressureResult {
            tests_passed: true,
            lints_passed: true,
            builds_passed: true,
            errors: Vec::new(),
        };
        assert!(format_errors_for_prompt(&result).contains("All checks passed"));
    }

    #[test]
    fn test_format_errors_with_failures() {
        let result = BackpressureResult {
            tests_passed: false,
            lints_passed: true,
            builds_passed: true,
            errors: vec!["test_foo failed: assertion failed".to_string()],
        };
        let formatted = format_errors_for_prompt(&result);
        assert!(formatted.contains("Backpressure Failures"));
        assert!(formatted.contains("test_foo"));
    }

    #[test]
    fn test_run_checks_no_project() {
        let temp_dir = TempDir::new().unwrap();
        let result = run_all_checks(temp_dir.path()).unwrap();

        // All should pass (no project detected = skip)
        assert!(result.tests_passed);
        assert!(result.lints_passed);
        assert!(result.builds_passed);
    }
}
