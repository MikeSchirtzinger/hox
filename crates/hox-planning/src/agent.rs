//! Top-level planning orchestrator with three execution paths.
//!
//! [`PlanningAgent`] ties together discovery, importing, validation, and writing
//! into a single coherent planning session. Three modes are supported:
//!
//! | Mode | When to use |
//! |------|-------------|
//! | Auto | Single LLM call generates PRD from a description string |
//! | From-file | Load existing markdown, import it, validate |
//! | Interactive | 7-step guided discovery with collected responses |
//!
//! After the [`Prd`] is produced by any path:
//! 1. [`validate_prd`] runs — warnings are logged, errors block writing.
//! 2. Optionally write to a JJ change via [`write_to_jj`].
//! 3. Optionally write to a file via [`write_to_file`].
//!
//! All three modes produce the same [`PlanningResult`] type.

use std::path::{Path, PathBuf};

use hox_core::Result;

use crate::discovery::run_discovery;
use crate::hox_prd::Prd;
use crate::importer::{import_markdown, LlmClient};
use crate::validator::validate_prd;
use crate::writer::{write_to_file, write_to_jj, JjExecutorLike};

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// A record of what happened during a planning session, for debugging.
#[derive(Debug, Default)]
pub struct PlanningTrace {
    pub steps: Vec<String>,
}

impl PlanningTrace {
    fn push(&mut self, step: impl Into<String>) {
        self.steps.push(step.into());
    }
}

/// The outcome of a complete planning session.
#[derive(Debug)]
pub struct PlanningResult {
    /// The PRD produced during this session.
    pub prd: Prd,
    /// JJ change ID, set when a JJ executor was provided.
    pub change_id: Option<String>,
    /// Path the PRD was written to, set when `output_path` was provided.
    pub file_path: Option<PathBuf>,
    /// Trace of the planning session.
    pub trace: PlanningTrace,
}

// ---------------------------------------------------------------------------
// PlanningAgent
// ---------------------------------------------------------------------------

/// Top-level planning orchestrator.
///
/// All methods are `async` and return a [`PlanningResult`]. Callers can then
/// call [`PlanningAgent::write_result`] to persist the result to JJ and/or a
/// file.
pub struct PlanningAgent;

impl PlanningAgent {
    /// Auto-mode: generate a PRD from a plain-text description in one LLM call.
    ///
    /// The LLM is prompted to emit a complete `hox:prd:v1` document. The
    /// result is parsed and validated. Validation warnings are logged; errors
    /// cause the method to return an `Err`.
    pub async fn auto_plan(description: &str, llm: &dyn LlmClient) -> Result<PlanningResult> {
        let mut trace = PlanningTrace::default();
        trace.push(format!("auto_plan: description={:?}", description));

        // 1. Generate PRD via LLM.
        let prompt = build_auto_prompt(description);
        trace.push("auto_plan: calling LLM");
        let raw = llm.complete(&prompt).await?;

        // 2. Parse the response.
        trace.push("auto_plan: parsing LLM response");
        let prd = import_markdown(&raw, Some(llm)).await?;
        trace.push(format!("auto_plan: parsed PRD title={:?}", prd.title));

        // 3. Validate.
        Self::validate_or_err(&prd, &mut trace)?;

        Ok(PlanningResult {
            prd,
            change_id: None,
            file_path: None,
            trace,
        })
    }

    /// From-file: read a markdown file, import it, validate.
    ///
    /// If the file already contains a `hox:prd:v1` sentinel it is parsed
    /// directly. Otherwise the optional `llm` is used for LLM-based
    /// extraction. When `llm` is `None` and the file is not native PRD
    /// markdown, a backfilled [`Prd`] is returned.
    pub async fn from_file(path: &Path, llm: Option<&dyn LlmClient>) -> Result<PlanningResult> {
        let mut trace = PlanningTrace::default();
        trace.push(format!("from_file: path={:?}", path));

        // 1. Read file.
        let content = std::fs::read_to_string(path)
            .map_err(|e| hox_core::HoxError::Io(e.to_string()))?;
        trace.push("from_file: file read successfully");

        // 2. Import via importer.
        trace.push("from_file: importing markdown");
        let prd = import_markdown(&content, llm).await?;
        trace.push(format!("from_file: imported PRD title={:?}", prd.title));

        // 3. Validate.
        Self::validate_or_err(&prd, &mut trace)?;

        Ok(PlanningResult {
            prd,
            change_id: None,
            file_path: None,
            trace,
        })
    }

    /// Interactive: 7-step guided discovery.
    ///
    /// `responses` must be a slice of `(step_name, user_response)` pairs — one
    /// entry per discovery step, in any order. The LLM synthesizes them into a
    /// complete PRD which is then validated.
    ///
    /// The title used for the PRD is taken from the `vision` step response (or
    /// falls back to `"Untitled PRD"` when absent).
    pub async fn interactive(
        responses: &[(String, String)],
        llm: &dyn LlmClient,
    ) -> Result<PlanningResult> {
        let mut trace = PlanningTrace::default();
        trace.push(format!("interactive: {} responses collected", responses.len()));

        // Derive a title from the vision response or use a placeholder.
        let title = responses
            .iter()
            .find(|(k, _)| k == "vision")
            .map(|(_, v)| first_line(v))
            .unwrap_or_else(|| "Untitled PRD".to_owned());
        trace.push(format!("interactive: derived title={:?}", title));

        // 1. Run discovery synthesis.
        trace.push("interactive: running discovery synthesis");
        let prd = run_discovery(&title, responses, llm).await?;
        trace.push(format!("interactive: PRD title={:?}", prd.title));

        // 2. Validate.
        Self::validate_or_err(&prd, &mut trace)?;

        Ok(PlanningResult {
            prd,
            change_id: None,
            file_path: None,
            trace,
        })
    }

    /// Write a completed [`PlanningResult`] to JJ and/or a file.
    ///
    /// - When `executor` is `Some`, creates a new JJ change and sets
    ///   `result.change_id`.
    /// - When `output_path` is `Some`, writes the PRD markdown to that path
    ///   and sets `result.file_path`.
    pub async fn write_result<E: JjExecutorLike>(
        result: &mut PlanningResult,
        executor: Option<&E>,
        output_path: Option<&Path>,
    ) -> Result<()> {
        if let Some(exec) = executor {
            let change_id = write_to_jj(&result.prd, exec).await?;
            result.trace.push(format!("write_result: JJ change_id={}", change_id));
            result.change_id = Some(change_id);
        }

        if let Some(path) = output_path {
            write_to_file(&result.prd, path)?;
            result.trace.push(format!("write_result: file written to {:?}", path));
            result.file_path = Some(path.to_owned());
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Validate `prd`. Log warnings; convert errors into an `Err`.
    fn validate_or_err(prd: &Prd, trace: &mut PlanningTrace) -> Result<()> {
        let validation = validate_prd(prd);

        for warn in &validation.warnings {
            tracing::warn!("{}", warn);
        }
        trace.push(format!(
            "validate: {} errors, {} warnings",
            validation.errors.len(),
            validation.warnings.len()
        ));

        if !validation.is_valid() {
            let messages: Vec<String> = validation.errors.iter().map(|e| e.to_string()).collect();
            return Err(hox_core::HoxError::ValidationFailed(messages.join("; ")));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn build_auto_prompt(description: &str) -> String {
    format!(
        r#"Generate a complete PRD for the following feature or project.

Description: {description}

Output ONLY a valid `hox:prd:v1` document using the exact format below:

```
hox:prd:v1 — <title>

## Vision

<problem statement and why it matters>

## Actors

- <stakeholder or user>

## Success Criteria

- [ ] <measurable outcome>

## Scope

### In Scope
- <item>

### Out of Scope
- <item>

## Requirements

### Functional
- **[REQ-001]**: <description>

### Non-Functional
- **[NFR-001]**: <description>

## Technical Constraints

- <constraint>

## Decomposition Hints

1. **<Name>**: <description>. Files: `<path>`. Covers: REQ-001.
```

Rules:
- Use the description as the PRD title.
- Include at least one measurable success criterion.
- Include at least one functional requirement (REQ-NNN format).
- Include at least one in-scope item.
- Include at least one decomposition hint with file patterns.
- Output ONLY the hox:prd:v1 block, nothing else.
"#
    )
}

/// Extract the first non-empty line of a string, trimmed.
fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Untitled PRD")
        .to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hox_prd::{DecompositionHint, Requirement, RequirementKind};
    use crate::importer::MockLlmClient;
    use crate::writer::JjOut;
    use async_trait::async_trait;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Mock JJ executor
    // -----------------------------------------------------------------------

    struct MockExec {
        responses: HashMap<String, JjOut>,
        default: JjOut,
    }

    impl MockExec {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
                default: JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            }
        }

        fn with(mut self, key: &str, out: JjOut) -> Self {
            self.responses.insert(key.to_owned(), out);
            self
        }
    }

    #[async_trait]
    impl JjExecutorLike for MockExec {
        async fn exec(&self, args: &[&str]) -> hox_core::Result<JjOut> {
            let key = args.join(" ");
            Ok(self
                .responses
                .get(&key)
                .cloned()
                .unwrap_or_else(|| self.default.clone()))
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn valid_prd_md() -> String {
        let prd = Prd {
            version: "v1".to_owned(),
            title: "Test Feature".to_owned(),
            vision: "Enable users to do X efficiently.".to_owned(),
            actors: vec!["Developer".to_owned()],
            success_criteria: vec!["Feature completes within 200ms for 99% of requests".to_owned()],
            scope_in: vec!["Core implementation".to_owned()],
            scope_out: vec![],
            requirements: vec![Requirement {
                id: "REQ-001".to_owned(),
                description: "System must do the thing".to_owned(),
                kind: RequirementKind::Functional,
            }],
            constraints: vec![],
            decomposition_hints: vec![DecompositionHint {
                name: "Core".to_owned(),
                description: "Core slice".to_owned(),
                estimated_files: vec!["crates/core/".to_owned()],
                requirements: vec!["REQ-001".to_owned()],
            }],
        };
        prd.to_markdown()
    }

    fn make_executor(change_id: &str) -> MockExec {
        MockExec::new().with(
            "log -r @ -T change_id --no-graph",
            JjOut {
                stdout: change_id.to_owned(),
                stderr: String::new(),
                success: true,
            },
        )
    }

    // -----------------------------------------------------------------------
    // auto_plan tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_auto_plan_valid_response() {
        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        let result = PlanningAgent::auto_plan("Test Feature", &llm)
            .await
            .expect("should succeed");

        assert_eq!(result.prd.title, "Test Feature");
        assert!(result.change_id.is_none());
        assert!(result.file_path.is_none());
        assert!(!result.trace.steps.is_empty());
    }

    #[tokio::test]
    async fn test_auto_plan_validation_error_blocks() {
        // Return an empty string → import_markdown falls back to a backfilled
        // Prd whose vision is a TODO placeholder, which is non-empty, so we
        // need something that produces a truly invalid Prd.
        // The easiest way: a sentinel-prefixed document missing required fields.
        let sparse = "hox:prd:v1 — Sparse\n\n## Vision\n\n(none)\n";
        let llm = MockLlmClient {
            response: sparse.to_owned(),
        };

        let result = PlanningAgent::auto_plan("Sparse", &llm).await;
        assert!(result.is_err(), "validation should block on empty vision");
    }

    // -----------------------------------------------------------------------
    // from_file tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_from_file_native_prd() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let md = valid_prd_md();
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(md.as_bytes()).expect("write");

        let result = PlanningAgent::from_file(tmp.path(), None)
            .await
            .expect("should succeed");

        assert_eq!(result.prd.title, "Test Feature");
        assert!(result.change_id.is_none());
    }

    #[tokio::test]
    async fn test_from_file_not_found_returns_err() {
        let result =
            PlanningAgent::from_file(Path::new("/nonexistent/prd.md"), None).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // interactive tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_interactive_valid_responses() {
        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        let responses = vec![
            ("vision".to_owned(), "Enable users to do X efficiently.".to_owned()),
            ("actors".to_owned(), "Developer".to_owned()),
            ("success_criteria".to_owned(), "200ms for 99% of requests".to_owned()),
            ("scope".to_owned(), "In: core. Out: nothing.".to_owned()),
            ("requirements".to_owned(), "REQ-001: do the thing".to_owned()),
            ("constraints".to_owned(), "none".to_owned()),
            ("decomposition".to_owned(), "Core slice".to_owned()),
        ];

        let result = PlanningAgent::interactive(&responses, &llm)
            .await
            .expect("should succeed");

        assert_eq!(result.prd.title, "Test Feature");
        assert!(!result.trace.steps.is_empty());
    }

    #[tokio::test]
    async fn test_interactive_synthesizes_responses() {
        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        // Title derived from vision response
        let responses = vec![
            ("vision".to_owned(), "My cool feature vision statement".to_owned()),
        ];

        let result = PlanningAgent::interactive(&responses, &llm)
            .await
            .expect("should succeed");

        // The mock returns the valid PRD with title "Test Feature" regardless
        assert!(!result.prd.title.is_empty());
    }

    // -----------------------------------------------------------------------
    // write_result tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_write_result_to_jj() {
        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        let mut result = PlanningAgent::auto_plan("Test Feature", &llm)
            .await
            .expect("auto_plan");

        let executor = make_executor("abcdef123456");
        PlanningAgent::write_result(&mut result, Some(&executor), None)
            .await
            .expect("write_result");

        assert_eq!(result.change_id.as_deref(), Some("abcdef123456"));
        assert!(result.file_path.is_none());
    }

    #[tokio::test]
    async fn test_write_result_to_file() {
        use tempfile::TempDir;

        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        let mut result = PlanningAgent::auto_plan("Test Feature", &llm)
            .await
            .expect("auto_plan");

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("prd.md");
        PlanningAgent::write_result(&mut result, None::<&MockExec>, Some(&path))
            .await
            .expect("write_result");

        assert!(path.exists());
        assert_eq!(result.file_path.as_deref(), Some(path.as_path()));
        assert!(result.change_id.is_none());
    }

    #[tokio::test]
    async fn test_write_result_to_both() {
        use tempfile::TempDir;

        let md = valid_prd_md();
        let llm = MockLlmClient { response: md };

        let mut result = PlanningAgent::auto_plan("Test Feature", &llm)
            .await
            .expect("auto_plan");

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("prd.md");
        let executor = make_executor("deadbeef");
        PlanningAgent::write_result(&mut result, Some(&executor), Some(&path))
            .await
            .expect("write_result");

        assert_eq!(result.change_id.as_deref(), Some("deadbeef"));
        assert!(path.exists());
    }

    // -----------------------------------------------------------------------
    // PlanningTrace
    // -----------------------------------------------------------------------

    #[test]
    fn test_trace_records_steps() {
        let mut trace = PlanningTrace::default();
        trace.push("step one");
        trace.push("step two");
        assert_eq!(trace.steps.len(), 2);
        assert_eq!(trace.steps[0], "step one");
    }
}
