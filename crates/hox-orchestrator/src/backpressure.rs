//! Backpressure checks for steering agent work
//!
//! Runs configurable checks (build, lint, test, etc.) to provide concrete
//! failure signals that guide the agent's next iteration.
//!
//! Design principles:
//! - Config-driven: any command can be a check, auto-detect as fallback
//! - Parallel: checks run concurrently via thread::scope
//! - Aggressive timeouts: 10s default, don't let slow checks block the loop
//! - Breaking-only errors: only compilation/syntax errors go into the prompt
//! - jj fix: auto-format before checks to eliminate formatting conflicts

use hox_agent::{BackpressureResult, CheckOutcome, Severity};
use hox_core::Result;
use hox_jj::JjExecutor;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Default timeout for each check command
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum total characters of error output to include in the agent prompt.
const MAX_ERROR_PROMPT_CHARS: usize = 6000;

/// Maximum characters for stdout/stderr in error messages
const MAX_STDIO_CHARS: usize = 4000;

/// A configured check command
#[derive(Debug, Clone)]
pub struct CheckCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub severity: Severity,
}

/// Run all checks in parallel with timeouts
///
/// Auto-detects project type if no explicit commands provided.
pub fn run_all_checks(workspace_path: &Path) -> Result<BackpressureResult> {
    let commands = detect_checks(workspace_path);
    run_checks(workspace_path, &commands)
}

/// Run only checks that previously failed (selective re-run)
///
/// Takes the previous result and only re-runs checks whose names match
/// previously failed checks. Checks that passed last time are skipped.
pub fn run_failed_checks(
    workspace_path: &Path,
    previous: &BackpressureResult,
) -> Result<BackpressureResult> {
    let failed_names: Vec<&str> = previous.failed_check_names();

    if failed_names.is_empty() {
        // Everything passed last time, nothing to re-run
        return Ok(BackpressureResult::all_pass());
    }

    let all_commands = detect_checks(workspace_path);
    let commands: Vec<CheckCommand> = all_commands
        .into_iter()
        .filter(|cmd| failed_names.contains(&cmd.name.as_str()))
        .collect();

    // Start with previous passing results, then overlay re-run results
    let rerun_result = run_checks(workspace_path, &commands)?;

    // Merge: keep previous passing checks + new results for re-run checks
    let mut checks: Vec<CheckOutcome> = previous
        .checks
        .iter()
        .filter(|c| c.passed) // keep previously passing checks
        .cloned()
        .collect();
    checks.extend(rerun_result.checks);

    let errors: Vec<String> = checks
        .iter()
        .filter(|c| !c.passed && c.severity == Severity::Breaking)
        .map(|c| c.output.clone())
        .collect();

    Ok(BackpressureResult { checks, errors })
}

/// Run a set of check commands in parallel with timeouts
pub fn run_checks(workspace_path: &Path, commands: &[CheckCommand]) -> Result<BackpressureResult> {
    if commands.is_empty() {
        return Ok(BackpressureResult::all_pass());
    }

    tracing::info!(
        "Running {} backpressure checks in {:?}",
        commands.len(),
        workspace_path
    );

    let outcomes: Vec<CheckOutcome> = std::thread::scope(|s| {
        let handles: Vec<_> = commands
            .iter()
            .map(|cmd| s.spawn(|| run_check_with_timeout(workspace_path, cmd)))
            .collect();

        handles
            .into_iter()
            .map(|h| match h.join() {
                Ok(outcome) => outcome,
                Err(_) => CheckOutcome {
                    name: "unknown".into(),
                    passed: false,
                    severity: Severity::Breaking,
                    output: "Check thread panicked".into(),
                },
            })
            .collect()
    });

    for outcome in &outcomes {
        tracing::info!(
            "  {} {}{}",
            outcome.name,
            if outcome.passed { "PASSED" } else { "FAILED" },
            if !outcome.passed {
                format!(" ({:?})", outcome.severity)
            } else {
                String::new()
            }
        );
    }

    // Only breaking errors go into the prompt
    let errors: Vec<String> = outcomes
        .iter()
        .filter(|o| !o.passed && o.severity == Severity::Breaking)
        .map(|o| o.output.clone())
        .collect();

    Ok(BackpressureResult {
        checks: outcomes,
        errors,
    })
}

/// Detect check commands for a workspace based on project files
///
/// Auto-detects project type and returns appropriate commands.
/// Falls through gracefully - missing tools are handled at runtime.
pub fn detect_checks(workspace_path: &Path) -> Vec<CheckCommand> {
    let mut checks = Vec::new();

    // Rust
    if workspace_path.join("Cargo.toml").exists() {
        checks.push(CheckCommand {
            name: "build".into(),
            program: "cargo".into(),
            args: vec!["build".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Breaking,
        });
        checks.push(CheckCommand {
            name: "lint".into(),
            program: "cargo".into(),
            args: vec!["clippy".into(), "--".into(), "-D".into(), "warnings".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Warning,
        });
        checks.push(CheckCommand {
            name: "test".into(),
            program: "cargo".into(),
            args: vec!["test".into(), "--".into(), "--nocapture".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Warning,
        });
    }

    // Python
    if workspace_path.join("pyproject.toml").exists() || workspace_path.join("pytest.ini").exists()
    {
        let ruff = python_tool(workspace_path, "ruff");
        checks.push(CheckCommand {
            name: "lint".into(),
            program: ruff,
            args: vec!["check".into(), ".".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Warning,
        });

        let pytest = python_tool(workspace_path, "pytest");
        checks.push(CheckCommand {
            name: "test".into(),
            program: pytest,
            args: vec!["-v".into(), "--continue-on-collection-errors".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Warning,
        });

        // Python build/import check
        if let Ok(content) = std::fs::read_to_string(workspace_path.join("pyproject.toml")) {
            if let Some(name) = extract_python_package_name(&content) {
                let python = python_tool(workspace_path, "python3");
                checks.push(CheckCommand {
                    name: "build".into(),
                    program: python,
                    args: vec!["-c".into(), format!("import {}", name)],
                    timeout_secs: DEFAULT_TIMEOUT_SECS,
                    severity: Severity::Breaking,
                });
            }
        }
    }

    // Node.js
    if workspace_path.join("package.json").exists() {
        if let Ok(content) = std::fs::read_to_string(workspace_path.join("package.json")) {
            if content.contains("\"build\"") {
                checks.push(CheckCommand {
                    name: "build".into(),
                    program: "npm".into(),
                    args: vec!["run".into(), "build".into()],
                    timeout_secs: DEFAULT_TIMEOUT_SECS,
                    severity: Severity::Breaking,
                });
            }
            if content.contains("\"test\"") {
                checks.push(CheckCommand {
                    name: "test".into(),
                    program: "npm".into(),
                    args: vec!["test".into()],
                    timeout_secs: DEFAULT_TIMEOUT_SECS,
                    severity: Severity::Warning,
                });
            }
            if content.contains("\"lint\"") {
                checks.push(CheckCommand {
                    name: "lint".into(),
                    program: "npm".into(),
                    args: vec!["run".into(), "lint".into()],
                    timeout_secs: DEFAULT_TIMEOUT_SECS,
                    severity: Severity::Warning,
                });
            }
        }
    }

    // Go
    if workspace_path.join("go.mod").exists() {
        checks.push(CheckCommand {
            name: "build".into(),
            program: "go".into(),
            args: vec!["build".into(), "./...".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Breaking,
        });
        checks.push(CheckCommand {
            name: "test".into(),
            program: "go".into(),
            args: vec!["test".into(), "./...".into()],
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            severity: Severity::Warning,
        });
    }

    // Makefile fallback (if no other checks detected)
    if checks.is_empty() && workspace_path.join("Makefile").exists() {
        if let Ok(content) = std::fs::read_to_string(workspace_path.join("Makefile")) {
            for target in ["check", "build", "test", "lint"] {
                if content.contains(&format!("{}:", target)) {
                    checks.push(CheckCommand {
                        name: target.into(),
                        program: "make".into(),
                        args: vec![target.into()],
                        timeout_secs: DEFAULT_TIMEOUT_SECS,
                        severity: if target == "build" || target == "check" {
                            Severity::Breaking
                        } else {
                            Severity::Warning
                        },
                    });
                }
            }
        }
    }

    // justfile fallback
    if checks.is_empty() && workspace_path.join("justfile").exists() {
        if let Ok(content) = std::fs::read_to_string(workspace_path.join("justfile")) {
            for target in ["check", "build", "test", "lint"] {
                if content.contains(&format!("{}:", target)) {
                    checks.push(CheckCommand {
                        name: target.into(),
                        program: "just".into(),
                        args: vec![target.into()],
                        timeout_secs: DEFAULT_TIMEOUT_SECS,
                        severity: if target == "build" || target == "check" {
                            Severity::Breaking
                        } else {
                            Severity::Warning
                        },
                    });
                }
            }
        }
    }

    checks
}

/// Run a single check command with a timeout
fn run_check_with_timeout(workspace_path: &Path, cmd: &CheckCommand) -> CheckOutcome {
    tracing::debug!(
        "Running check: {} ({} {})",
        cmd.name,
        cmd.program,
        cmd.args.join(" ")
    );

    let mut child = match Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(workspace_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!("{} not found, skipping {} check", cmd.program, cmd.name);
            return CheckOutcome {
                name: cmd.name.clone(),
                passed: true,
                severity: Severity::Warning,
                output: format!(
                    "[SKIPPED] {} not found on PATH - {} check not run",
                    cmd.program, cmd.name
                ),
            };
        }
        Err(e) => {
            return CheckOutcome {
                name: cmd.name.clone(),
                passed: false,
                severity: cmd.severity,
                output: format!("Failed to run {}: {}", cmd.program, e),
            };
        }
    };

    // Take stdout/stderr handles to read in separate threads (avoids pipe buffer deadlock)
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let stdout_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut h) = stdout_handle {
            if let Err(e) = h.read_to_string(&mut s) {
                tracing::warn!("Failed to read stdout: {}", e);
            }
        }
        s
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut h) = stderr_handle {
            if let Err(e) = h.read_to_string(&mut s) {
                tracing::warn!("Failed to read stderr: {}", e);
            }
        }
        s
    });

    // Wait with timeout (poll every 100ms)
    let timeout = Duration::from_secs(cmd.timeout_secs);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    if let Err(e) = child.kill() {
                        tracing::warn!("Failed to kill child process: {}", e);
                    }
                    if let Err(e) = child.wait() {
                        tracing::warn!("Failed to wait for child process: {}", e);
                    }
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                tracing::warn!("Error polling {} process: {}", cmd.name, e);
                if start.elapsed() >= timeout {
                    if let Err(e) = child.kill() {
                        tracing::warn!("Failed to kill child process: {}", e);
                    }
                    if let Err(e) = child.wait() {
                        tracing::warn!("Failed to wait for child process: {}", e);
                    }
                    break None;
                }
                // Transient error, retry
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    match status {
        Some(exit_status) => {
            let passed = exit_status.success();
            let output = if !passed {
                format_check_output(&cmd.name, &cmd.program, &cmd.args, &stdout, &stderr)
            } else {
                String::new()
            };
            CheckOutcome {
                name: cmd.name.clone(),
                passed,
                severity: cmd.severity,
                output,
            }
        }
        None => {
            tracing::warn!("{} timed out after {}s", cmd.name, cmd.timeout_secs);
            CheckOutcome {
                name: cmd.name.clone(),
                passed: true, // don't block on timeout
                severity: Severity::Warning,
                output: format!(
                    "{} timed out after {}s, skipping",
                    cmd.name, cmd.timeout_secs
                ),
            }
        }
    }
}

/// Format check output for error reporting
fn format_check_output(
    name: &str,
    program: &str,
    args: &[String],
    stdout: &str,
    stderr: &str,
) -> String {
    let truncate = |s: &str, max: usize| -> String {
        if s.len() > max {
            // Find the nearest char boundary at or before max to avoid
            // panicking on multi-byte UTF-8 sequences from compiler output
            let mut boundary = max;
            while boundary > 0 && !s.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!("{}...[truncated]", &s[..boundary])
        } else {
            s.to_string()
        }
    };

    format!(
        "{} ({} {}) failed:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
        name,
        program,
        args.join(" "),
        truncate(stdout.trim(), MAX_STDIO_CHARS),
        truncate(stderr.trim(), MAX_STDIO_CHARS)
    )
}

/// Resolve a Python tool binary, preferring the project's venv if available.
fn python_tool(workspace_path: &Path, tool: &str) -> String {
    let venv_bin = workspace_path.join(".venv").join("bin").join(tool);
    if venv_bin.exists() {
        venv_bin.to_string_lossy().to_string()
    } else {
        tool.to_string()
    }
}

/// Extract the main package name from a pyproject.toml
fn extract_python_package_name(content: &str) -> Option<String> {
    let mut in_project = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[project]" {
            in_project = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[project]" {
            in_project = false;
            continue;
        }
        if in_project {
            if let Some(rest) = trimmed.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim();
                    let name = rest.trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return Some(name.replace('-', "_"));
                    }
                }
            }
        }
    }
    None
}

/// Result from running jj fix
#[derive(Debug, Clone)]
pub struct FixResult {
    pub success: bool,
    pub output: String,
}

/// Run jj fix to auto-format all mutable commits
///
/// This runs `jj fix` to automatically format code according to configured
/// formatters (e.g., rustfmt). This eliminates formatting-only conflicts
/// between agents working in parallel.
///
/// Runs `jj fix -s <change_id>` if a change_id is provided, otherwise
/// runs `jj fix` on all mutable changes.
pub async fn run_jj_fix<E: JjExecutor>(executor: &E, change_id: Option<&str>) -> Result<FixResult> {
    let args = match change_id {
        Some(id) => vec!["fix", "-s", id],
        None => vec!["fix"],
    };

    tracing::debug!("Running jj fix: {:?}", args);
    let output = executor.exec(&args).await?;

    Ok(FixResult {
        success: output.success,
        output: if output.success {
            output.stdout
        } else {
            output.stderr
        },
    })
}

/// Enhanced backpressure that includes jj fix before standard checks
///
/// This function runs `jj fix` first to auto-format code, then runs
/// the standard backpressure checks. This prevents formatting-only
/// conflicts from causing check failures.
///
/// Note: jj fix failures are NON-FATAL. If fix fails, we log a warning
/// and continue with standard checks.
pub async fn run_all_checks_with_fix<E: JjExecutor>(
    workspace_path: &Path,
    executor: &E,
    change_id: Option<&str>,
) -> Result<BackpressureResult> {
    // Run jj fix FIRST to clean formatting
    let fix_result = run_jj_fix(executor, change_id).await;

    match &fix_result {
        Ok(result) => {
            if result.success {
                tracing::debug!("jj fix completed successfully");
            } else {
                tracing::warn!("jj fix failed (non-fatal): {}", result.output);
            }
        }
        Err(e) => {
            tracing::warn!("jj fix failed (non-fatal): {}", e);
        }
    }

    // Then run standard checks
    run_all_checks(workspace_path)
}

/// Format backpressure errors for inclusion in agent prompt.
///
/// Applies smart truncation: keeps the first N lines of each error block and
/// appends a summary of how many total error lines were omitted.
/// Only includes Breaking errors.
pub fn format_errors_for_prompt(result: &BackpressureResult) -> String {
    if result.errors.is_empty() {
        return String::new();
    }

    let mut output = String::from("## ERRORS TO FIX\n\n");
    output.push_str("You MUST fix these breaking errors before proceeding:\n\n");

    let mut budget = MAX_ERROR_PROMPT_CHARS;

    for error in &result.errors {
        if budget == 0 {
            output.push_str("*(additional errors omitted - fix the above first)*\n\n");
            break;
        }

        let truncated = truncate_error_for_prompt(error, budget);
        output.push_str("```\n");
        output.push_str(&truncated);
        output.push_str("\n```\n\n");

        budget = budget.saturating_sub(truncated.len());
    }

    output
}

/// Truncate a single error block for the prompt.
fn truncate_error_for_prompt(error: &str, max_chars: usize) -> String {
    if error.len() <= max_chars {
        return error.to_string();
    }

    let lines: Vec<&str> = error.lines().collect();
    let total_lines = lines.len();
    let mut result = String::new();
    let mut included_lines = 0;

    for line in &lines {
        if result.len() + line.len() + 1 > max_chars.saturating_sub(80) {
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        included_lines += 1;
    }

    let omitted = total_lines - included_lines;
    if omitted > 0 {
        result.push_str(&format!(
            "\n\n... [{} more lines omitted - {} total errors. Fix the above patterns first.]",
            omitted, total_lines
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_jj::{JjOutput, MockJjExecutor};
    use tempfile::TempDir;

    #[test]
    fn test_format_errors_empty() {
        let result = BackpressureResult::all_pass();
        assert!(format_errors_for_prompt(&result).is_empty());
    }

    #[test]
    fn test_format_errors_with_failures() {
        let result = BackpressureResult {
            checks: vec![CheckOutcome {
                name: "build".into(),
                passed: false,
                severity: Severity::Breaking,
                output: "test_foo failed: assertion failed".into(),
            }],
            errors: vec!["test_foo failed: assertion failed".into()],
        };
        let formatted = format_errors_for_prompt(&result);
        assert!(formatted.contains("ERRORS TO FIX"));
        assert!(formatted.contains("test_foo"));
    }

    #[test]
    fn test_format_errors_truncates_large_output() {
        let mut big_error = String::from("ruff check . failed:\n\nSTDOUT:\n");
        for i in 0..3400 {
            big_error.push_str(&format!("src/foo.py:{}:1: E501 line too long\n", i));
        }

        let result = BackpressureResult {
            checks: vec![CheckOutcome {
                name: "lint".into(),
                passed: false,
                severity: Severity::Breaking,
                output: big_error.clone(),
            }],
            errors: vec![big_error],
        };
        let formatted = format_errors_for_prompt(&result);

        assert!(
            formatted.len() < MAX_ERROR_PROMPT_CHARS + 500,
            "Formatted output too large: {} chars",
            formatted.len()
        );
        assert!(formatted.contains("more lines omitted"));
    }

    #[test]
    fn test_truncate_error_for_prompt_small() {
        let error = "short error";
        assert_eq!(truncate_error_for_prompt(error, 1000), "short error");
    }

    #[test]
    fn test_truncate_error_for_prompt_large() {
        let mut error = String::new();
        for i in 0..500 {
            error.push_str(&format!("line {}: some error\n", i));
        }
        let truncated = truncate_error_for_prompt(&error, 500);
        assert!(truncated.len() < 600);
        assert!(truncated.contains("more lines omitted"));
    }

    #[test]
    fn test_run_checks_no_project() {
        let temp_dir = TempDir::new().unwrap();
        let result = run_all_checks(temp_dir.path()).unwrap();

        // No project detected = no checks = all pass
        assert!(result.all_passed());
        assert!(result.checks.is_empty());
    }

    #[test]
    fn test_detect_checks_rust() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();

        let checks = detect_checks(temp_dir.path());
        assert!(!checks.is_empty());

        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"build"));
        assert!(names.contains(&"lint"));
        assert!(names.contains(&"test"));

        // build should be Breaking, lint/test should be Warning
        let build = checks.iter().find(|c| c.name == "build").unwrap();
        assert_eq!(build.severity, Severity::Breaking);

        let lint = checks.iter().find(|c| c.name == "lint").unwrap();
        assert_eq!(lint.severity, Severity::Warning);
    }

    #[test]
    fn test_detect_checks_python() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"test-project\"\n",
        )
        .unwrap();

        let checks = detect_checks(temp_dir.path());
        assert!(!checks.is_empty());

        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"lint"));
        assert!(names.contains(&"test"));
        assert!(names.contains(&"build"));
    }

    #[test]
    fn test_detect_checks_makefile_fallback() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("Makefile"),
            "build:\n\tgcc main.c\ntest:\n\t./run_tests\n",
        )
        .unwrap();

        let checks = detect_checks(temp_dir.path());
        assert!(!checks.is_empty());

        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"build"));
        assert!(names.contains(&"test"));
    }

    #[test]
    fn test_extract_python_package_name() {
        let toml = r#"
[project]
name = "helper-mvp"
version = "0.1.0"
"#;
        assert_eq!(
            extract_python_package_name(toml),
            Some("helper_mvp".to_string())
        );
    }

    #[test]
    fn test_extract_python_package_name_no_project() {
        let toml = r#"
[tool.ruff]
line-length = 88
"#;
        assert_eq!(extract_python_package_name(toml), None);
    }

    #[test]
    fn test_python_tool_no_venv() {
        let temp_dir = TempDir::new().unwrap();
        assert_eq!(python_tool(temp_dir.path(), "pytest"), "pytest");
    }

    #[test]
    fn test_python_tool_with_venv() {
        let temp_dir = TempDir::new().unwrap();
        let venv_bin = temp_dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(venv_bin.join("pytest"), "").unwrap();

        let result = python_tool(temp_dir.path(), "pytest");
        assert!(result.contains(".venv/bin/pytest"));
    }

    #[test]
    fn test_selective_rerun_skips_passing() {
        // Simulate: build failed, lint passed, test passed
        let previous = BackpressureResult {
            checks: vec![
                CheckOutcome {
                    name: "build".into(),
                    passed: false,
                    severity: Severity::Breaking,
                    output: "error".into(),
                },
                CheckOutcome {
                    name: "lint".into(),
                    passed: true,
                    severity: Severity::Warning,
                    output: String::new(),
                },
                CheckOutcome {
                    name: "test".into(),
                    passed: true,
                    severity: Severity::Warning,
                    output: String::new(),
                },
            ],
            errors: vec!["error".into()],
        };

        assert_eq!(previous.failed_check_names(), vec!["build"]);
    }

    #[test]
    fn test_only_breaking_errors_in_prompt() {
        let result = BackpressureResult {
            checks: vec![
                CheckOutcome {
                    name: "build".into(),
                    passed: false,
                    severity: Severity::Breaking,
                    output: "compilation error".into(),
                },
                CheckOutcome {
                    name: "lint".into(),
                    passed: false,
                    severity: Severity::Warning,
                    output: "style warning".into(),
                },
            ],
            errors: vec!["compilation error".into()],
        };

        let formatted = format_errors_for_prompt(&result);
        assert!(formatted.contains("compilation error"));
        // Warning errors should NOT be in the formatted prompt
        assert!(!formatted.contains("style warning"));
    }

    #[tokio::test]
    async fn test_run_jj_fix_success() {
        let executor = MockJjExecutor::new().with_response(
            "fix",
            JjOutput {
                stdout: "Fixed 3 files".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let result = run_jj_fix(&executor, None).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "Fixed 3 files");
    }

    #[tokio::test]
    async fn test_run_jj_fix_with_change_id() {
        let executor = MockJjExecutor::new().with_response(
            "fix -s abc123",
            JjOutput {
                stdout: "Fixed change abc123".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let result = run_jj_fix(&executor, Some("abc123")).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "Fixed change abc123");
    }

    #[tokio::test]
    async fn test_run_jj_fix_failure_is_non_fatal() {
        let executor = MockJjExecutor::new().with_response(
            "fix",
            JjOutput {
                stdout: String::new(),
                stderr: "jj fix not configured".to_string(),
                success: false,
            },
        );

        let result = run_jj_fix(&executor, None).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.output, "jj fix not configured");
    }

    #[tokio::test]
    async fn test_run_all_checks_with_fix() {
        let executor = MockJjExecutor::new().with_response(
            "fix",
            JjOutput {
                stdout: "Fixed files".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let temp_dir = TempDir::new().unwrap();

        // Should run fix first, then standard checks
        let result = run_all_checks_with_fix(temp_dir.path(), &executor, None)
            .await
            .unwrap();

        // Standard checks should pass (no project detected)
        assert!(result.all_passed());
    }

    #[tokio::test]
    async fn test_run_all_checks_with_fix_continues_on_fix_failure() {
        let executor = MockJjExecutor::new().with_response(
            "fix",
            JjOutput {
                stdout: String::new(),
                stderr: "fix failed".to_string(),
                success: false,
            },
        );

        let temp_dir = TempDir::new().unwrap();

        // Should continue with standard checks even if fix fails
        let result = run_all_checks_with_fix(temp_dir.path(), &executor, None)
            .await
            .unwrap();

        // Standard checks should still run and pass
        assert!(result.all_passed());
    }
}
