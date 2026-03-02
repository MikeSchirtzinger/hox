//! PRD writer: persist a [`Prd`] to a file or JJ change description.
//!
//! ## JJ workflow (two-step)
//!
//! 1. `jj new` — creates a new empty change on top of the current working copy.
//! 2. `jj describe -m <markdown>` — sets the PRD markdown as the change description.
//! 3. `jj log -r @ -T change_id --no-graph` — reads back the new change ID.
//!
//! The two-step approach keeps shell argument handling consistent with how the
//! rest of hox-jj manages descriptions; passing the full markdown as a single
//! `argv` entry to `describe -m` is safe because [`JjExecutor::exec`] takes a
//! pre-split `&[&str]` slice.

use std::path::Path;

use async_trait::async_trait;
use hox_core::{HoxError, Result};

use crate::hox_prd::Prd;

// ---------------------------------------------------------------------------
// File writer (sync — no JJ dependency)
// ---------------------------------------------------------------------------

/// Write a PRD to a markdown file on disk.
///
/// Creates or overwrites `path`. Parent directories are created if absent.
pub fn write_to_file(prd: &Prd, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| HoxError::Io(format!("failed to create dirs: {}", e)))?;
        }
    }
    let md = prd.to_markdown();
    std::fs::write(path, md).map_err(|e| HoxError::Io(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// JJ writer (async)
// ---------------------------------------------------------------------------

/// Write a PRD as a new JJ change description.
///
/// Creates a new JJ change with `jj new`, sets its description to the PRD
/// markdown with `jj describe -m`, then returns the change ID of `@`.
///
/// # Errors
///
/// Returns [`HoxError::JjCommand`] if any JJ command fails.
pub async fn write_to_jj<E: JjExecutorLike>(prd: &Prd, executor: &E) -> Result<String> {
    let md = prd.to_markdown();

    // Step 1: new child change on top of the current working copy.
    let new_out = executor.exec(&["new"]).await?;
    if !new_out.success {
        return Err(HoxError::JjCommand(format!(
            "jj new failed: {}",
            new_out.stderr
        )));
    }

    // Step 2: set the PRD markdown as the change description.
    let describe_out = executor.exec(&["describe", "-m", &md]).await?;
    if !describe_out.success {
        return Err(HoxError::JjCommand(format!(
            "jj describe failed: {}",
            describe_out.stderr
        )));
    }

    // Step 3: read back the change ID of the current working copy.
    let id_out = executor
        .exec(&["log", "-r", "@", "-T", "change_id", "--no-graph"])
        .await?;
    if !id_out.success {
        return Err(HoxError::JjCommand(format!(
            "jj log failed while reading change id: {}",
            id_out.stderr
        )));
    }

    let change_id = id_out.stdout.trim().to_owned();
    if change_id.is_empty() {
        return Err(HoxError::JjCommand(
            "jj log returned empty change_id".to_owned(),
        ));
    }

    Ok(change_id)
}

// ---------------------------------------------------------------------------
// JjExecutorLike — minimal mirror of hox_jj::JjExecutor
// ---------------------------------------------------------------------------

/// Minimal output type mirroring [`hox_jj::JjOutput`].
#[derive(Debug, Clone)]
pub struct JjOut {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Minimal async executor interface used by [`write_to_jj`].
///
/// Any type that implements [`hox_jj::JjExecutor`] also satisfies this trait
/// via the blanket impl below.
#[async_trait]
pub trait JjExecutorLike: Send + Sync {
    async fn exec(&self, args: &[&str]) -> Result<JjOut>;
}

// Blanket impl: any hox_jj::JjExecutor also satisfies JjExecutorLike.
#[async_trait]
impl<E> JjExecutorLike for E
where
    E: hox_jj::JjExecutor + Send + Sync,
{
    async fn exec(&self, args: &[&str]) -> Result<JjOut> {
        let out = hox_jj::JjExecutor::exec(self, args).await?;
        Ok(JjOut {
            stdout: out.stdout,
            stderr: out.stderr,
            success: out.success,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hox_prd::{DecompositionHint, Prd, Requirement, RequirementKind};
    use std::collections::HashMap;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Minimal mock executor (no hox_jj dependency)
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
        async fn exec(&self, args: &[&str]) -> Result<JjOut> {
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

    fn minimal_prd() -> Prd {
        Prd {
            version: "v1".to_owned(),
            title: "Test PRD".to_owned(),
            vision: "A test vision statement.".to_owned(),
            actors: vec!["Developer".to_owned()],
            success_criteria: vec!["System builds without errors".to_owned()],
            scope_in: vec!["Core implementation".to_owned()],
            scope_out: vec![],
            requirements: vec![Requirement {
                id: "REQ-001".to_owned(),
                description: "System must compile.".to_owned(),
                kind: RequirementKind::Functional,
            }],
            constraints: vec!["Must use existing infrastructure.".to_owned()],
            decomposition_hints: vec![DecompositionHint {
                name: "Core".to_owned(),
                description: "Core slice.".to_owned(),
                estimated_files: vec!["crates/core/".to_owned()],
                requirements: vec!["REQ-001".to_owned()],
            }],
        }
    }

    // -----------------------------------------------------------------------
    // write_to_file tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_write_to_file_creates_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("prd.md");

        write_to_file(&minimal_prd(), &path).expect("write_to_file");

        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).expect("read file");
        assert!(contents.starts_with("hox:prd:v1"));
        assert!(contents.contains("Test PRD"));
    }

    #[test]
    fn test_write_to_file_creates_parent_dirs() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nested").join("deep").join("prd.md");
        write_to_file(&minimal_prd(), &path).expect("write with nested parents");
        assert!(path.exists());
    }

    #[test]
    fn test_write_to_file_round_trips_prd() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("prd.md");
        let prd = minimal_prd();

        write_to_file(&prd, &path).expect("write_to_file");

        let contents = std::fs::read_to_string(&path).expect("read file");
        let parsed = Prd::from_markdown(&contents).expect("parse written file");

        assert_eq!(parsed.title, prd.title);
        assert_eq!(parsed.version, prd.version);
        assert_eq!(parsed.vision, prd.vision);
        assert_eq!(parsed.actors.len(), prd.actors.len());
        assert_eq!(parsed.requirements.len(), prd.requirements.len());
        assert_eq!(
            parsed.decomposition_hints.len(),
            prd.decomposition_hints.len()
        );
    }

    #[test]
    fn test_write_to_file_overwrites_existing() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("prd.md");

        write_to_file(&minimal_prd(), &path).expect("first write");

        let mut prd2 = minimal_prd();
        prd2.title = "Updated PRD Title".to_owned();
        write_to_file(&prd2, &path).expect("second write");

        let contents = std::fs::read_to_string(&path).expect("read file");
        assert!(contents.contains("Updated PRD Title"));
        assert!(!contents.contains("\"Test PRD\""));
    }

    // -----------------------------------------------------------------------
    // write_to_jj tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_write_to_jj_success() {
        let expected_id = "kpqvuttsabcdefghijklmnopqrstuv";
        let prd = minimal_prd();
        let md = prd.to_markdown();
        let describe_key = format!("describe -m {}", md);

        let mock = MockExec::new()
            .with(
                "new",
                JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with(
                &describe_key,
                JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with(
                "log -r @ -T change_id --no-graph",
                JjOut {
                    stdout: expected_id.to_owned(),
                    stderr: String::new(),
                    success: true,
                },
            );

        let change_id = write_to_jj(&prd, &mock).await.expect("write_to_jj");
        assert_eq!(change_id, expected_id);
    }

    #[tokio::test]
    async fn test_write_to_jj_new_fails() {
        let mock = MockExec::new().with(
            "new",
            JjOut {
                stdout: String::new(),
                stderr: "fatal: not a jj repo".to_owned(),
                success: false,
            },
        );

        let result = write_to_jj(&minimal_prd(), &mock).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("jj new failed"), "got: {}", msg);
    }

    #[tokio::test]
    async fn test_write_to_jj_describe_fails() {
        let prd = minimal_prd();
        let describe_key = format!("describe -m {}", prd.to_markdown());

        let mock = MockExec::new()
            .with(
                "new",
                JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with(
                &describe_key,
                JjOut {
                    stdout: String::new(),
                    stderr: "permission denied".to_owned(),
                    success: false,
                },
            );

        let result = write_to_jj(&prd, &mock).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("jj describe failed"), "got: {}", msg);
    }

    #[tokio::test]
    async fn test_write_to_jj_empty_change_id_fails() {
        let prd = minimal_prd();
        let describe_key = format!("describe -m {}", prd.to_markdown());

        let mock = MockExec::new()
            .with(
                "new",
                JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with(
                &describe_key,
                JjOut {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with(
                "log -r @ -T change_id --no-graph",
                JjOut {
                    stdout: String::new(), // empty — error case
                    stderr: String::new(),
                    success: true,
                },
            );

        let result = write_to_jj(&prd, &mock).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("empty change_id"), "got: {}", msg);
    }

    #[test]
    fn test_prd_markdown_starts_with_sentinel() {
        let prd = minimal_prd();
        let md = prd.to_markdown();
        assert!(md.starts_with("hox:prd:v1"));
    }
}
