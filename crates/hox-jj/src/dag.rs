//! DAG manipulation operations for task restructuring
//!
//! This module provides JJ DAG operations that enable powerful workflow transformations:
//! - Parallelize: Convert sequential changes into parallel siblings
//! - Absorb: Auto-distribute working copy changes to correct ancestor commits
//! - Split: Divide a change into multiple changes by file groups
//! - Squash: Fold changes into parents or targets
//!
//! These operations are safe because bookmarks auto-track through DAG rewrites (Phase 1).

use hox_core::{HoxError, Result};
use tracing::{debug, instrument};

use crate::command::JjExecutor;

/// Result from parallelize operation
#[derive(Debug, Clone)]
pub struct ParallelizeResult {
    /// Number of changes restructured
    pub changes_restructured: usize,
    /// Whether the operation completed without conflicts
    pub clean: bool,
    /// List of conflicts encountered (empty if clean)
    pub conflicts: Vec<String>,
}

/// Result from absorb operation
#[derive(Debug, Clone)]
pub struct AbsorbResult {
    /// Number of hunks absorbed into ancestor commits
    pub hunks_absorbed: usize,
    /// Change IDs affected by absorption
    pub affected_changes: Vec<String>,
}

/// Result from split operation
#[derive(Debug, Clone)]
pub struct SplitResult {
    /// New change IDs created from the split
    pub new_changes: Vec<String>,
}

/// Entry from evolution log (change history)
#[derive(Debug, Clone)]
pub struct EvolutionEntry {
    pub commit_id: String,
    pub description: String,
    pub timestamp: String,
}

/// DAG manipulation operations for task restructuring
pub struct DagOperations<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> DagOperations<E> {
    /// Create a new DAG operations manager
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Convert sequential changes into parallel siblings
    ///
    /// Executes: `jj parallelize {revset}`
    ///
    /// This is useful for converting sequentially-planned tasks into parallel execution.
    /// After planning, an orchestrator can parallelize independent tasks to run concurrently.
    #[instrument(skip(self))]
    pub async fn parallelize(&self, revset: &str) -> Result<ParallelizeResult> {
        debug!("Parallelizing changes in revset: {}", revset);

        let output = self.executor.exec(&["parallelize", revset]).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to parallelize: {}",
                output.stderr
            )));
        }

        // Parse output to determine success
        // JJ parallelize outputs nothing on success, non-zero exit on failure
        // Count the number of changes in the revset as an approximation
        let clean = !output.stderr.contains("conflict");
        let conflicts = if clean {
            Vec::new()
        } else {
            // Extract conflict information from stderr if present
            output
                .stderr
                .lines()
                .filter(|l| l.contains("conflict"))
                .map(String::from)
                .collect()
        };

        Ok(ParallelizeResult {
            changes_restructured: 1, // Conservative estimate without querying revset
            clean,
            conflicts,
        })
    }

    /// Auto-distribute working copy changes to correct ancestor commits
    ///
    /// Executes: `jj absorb [paths...]`
    ///
    /// This is useful after integration testing - fixes can be automatically absorbed
    /// back into the agent branches that introduced the bugs.
    #[instrument(skip(self))]
    pub async fn absorb(&self, paths: Option<&[&str]>) -> Result<AbsorbResult> {
        debug!("Absorbing changes, paths: {:?}", paths);

        let mut args = vec!["absorb"];
        if let Some(path_list) = paths {
            args.extend(path_list.iter().copied());
        }

        let output = self.executor.exec(&args).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to absorb: {}",
                output.stderr
            )));
        }

        // Parse output: "Absorbed 3 hunks into commit abc123"
        let hunks_absorbed = output
            .stdout
            .lines()
            .filter_map(|line| {
                if line.contains("Absorbed") && line.contains("hunks") {
                    // Extract number from "Absorbed N hunks"
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<usize>().ok())
                } else {
                    None
                }
            })
            .sum();

        // Extract affected change IDs
        let affected_changes = output
            .stdout
            .lines()
            .filter_map(|line| {
                if line.contains("commit") {
                    // Extract change ID after "commit"
                    line.split_whitespace().last().map(String::from)
                } else {
                    None
                }
            })
            .collect();

        Ok(AbsorbResult {
            hunks_absorbed,
            affected_changes,
        })
    }

    /// Split a change into multiple changes by file groups
    ///
    /// Executes: `jj split -r {change_id} --siblings {files...}` for each group
    ///
    /// This is useful when an agent reports a task is too large. The orchestrator
    /// can decompose it into smaller file-based subtasks.
    ///
    /// Note: Uses `--siblings` flag for non-interactive file-based splitting.
    #[instrument(skip(self))]
    pub async fn split_by_files(
        &self,
        change_id: &str,
        file_groups: &[Vec<String>],
    ) -> Result<SplitResult> {
        debug!(
            "Splitting change {} into {} groups",
            change_id,
            file_groups.len()
        );

        if file_groups.is_empty() {
            return Err(HoxError::JjCommand(
                "No file groups provided for split".to_string(),
            ));
        }

        // For now, perform a simple split
        // TODO: Implement multi-group splitting with --siblings flag
        // JJ split is interactive by default - need to use --siblings for automation

        let mut args = vec!["split", "-r", change_id, "--siblings"];

        // Add files from first group as a starting point
        if let Some(first_group) = file_groups.first() {
            for file in first_group {
                args.push(file.as_str());
            }
        }

        let output = self.executor.exec(&args).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to split change: {}",
                output.stderr
            )));
        }

        // Parse output to extract new change IDs
        // JJ split outputs the new change IDs
        let new_changes = output
            .stdout
            .lines()
            .filter_map(|line| {
                // Look for lines containing change IDs
                if line.contains("Created") || line.contains("change") {
                    // Extract potential change ID (simplified parsing)
                    line.split_whitespace()
                        .find(|s| s.len() > 8 && s.chars().all(|c| c.is_alphanumeric()))
                        .map(String::from)
                } else {
                    None
                }
            })
            .collect();

        Ok(SplitResult { new_changes })
    }

    /// Fold a change into its parent
    ///
    /// Executes: `jj squash -r {change_id}`
    ///
    /// This is useful for simplifying the DAG after agent work is complete,
    /// combining related changes before final integration.
    #[instrument(skip(self))]
    pub async fn squash(&self, change_id: &str) -> Result<()> {
        debug!("Squashing change {} into parent", change_id);

        let output = self.executor.exec(&["squash", "-r", change_id]).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to squash change: {}",
                output.stderr
            )));
        }

        Ok(())
    }

    /// Squash specific files from source into target
    ///
    /// Executes: `jj squash --from {source} --into {target} [paths...]`
    ///
    /// This is useful for selectively moving work between changes, such as
    /// moving documentation updates to a separate commit or consolidating
    /// related fixes.
    #[instrument(skip(self))]
    pub async fn squash_into(
        &self,
        source: &str,
        target: &str,
        paths: Option<&[&str]>,
    ) -> Result<()> {
        debug!(
            "Squashing from {} into {}, paths: {:?}",
            source, target, paths
        );

        let mut args = vec!["squash", "--from", source, "--into", target];
        if let Some(path_list) = paths {
            args.extend(path_list.iter().copied());
        }

        let output = self.executor.exec(&args).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to squash into target: {}",
                output.stderr
            )));
        }

        Ok(())
    }

    /// Duplicate a change for speculative execution
    ///
    /// Executes: `jj duplicate {change_id} [-d {destination}]`
    ///
    /// This creates a copy of a change at a new location, useful for trying
    /// multiple approaches to a task in parallel (speculative execution).
    #[instrument(skip(self))]
    pub async fn duplicate(
        &self,
        change_id: &str,
        destination: Option<&str>,
    ) -> Result<String> {
        debug!("Duplicating change {}, destination: {:?}", change_id, destination);

        let mut args = vec!["duplicate", change_id];
        if let Some(dest) = destination {
            args.push("-d");
            args.push(dest);
        }

        let output = self.executor.exec(&args).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to duplicate change: {}",
                output.stderr
            )));
        }

        // Parse output to extract new change ID
        // JJ duplicate outputs something like "Created new change abc123"
        let new_change_id = output
            .stdout
            .lines()
            .filter_map(|line| {
                if line.contains("Created") || line.contains("change") {
                    line.split_whitespace()
                        .find(|s| s.len() > 8 && s.chars().all(|c| c.is_alphanumeric()))
                        .map(String::from)
                } else {
                    None
                }
            })
            .next()
            .ok_or_else(|| HoxError::JjCommand("Failed to parse duplicate output".to_string()))?;

        Ok(new_change_id)
    }

    /// Create a change that undoes the effect of another
    ///
    /// Executes: `jj backout -r {change_id}`
    ///
    /// This creates a new change that reverses the changes made by the specified
    /// change, providing a safe way to revert without destructive history editing.
    #[instrument(skip(self))]
    pub async fn backout(&self, change_id: &str) -> Result<String> {
        debug!("Creating backout change for {}", change_id);

        let output = self.executor.exec(&["backout", "-r", change_id]).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to backout change: {}",
                output.stderr
            )));
        }

        // Parse output to extract backout change ID
        let backout_change_id = output
            .stdout
            .lines()
            .filter_map(|line| {
                if line.contains("Created") || line.contains("change") {
                    line.split_whitespace()
                        .find(|s| s.len() > 8 && s.chars().all(|c| c.is_alphanumeric()))
                        .map(String::from)
                } else {
                    None
                }
            })
            .next()
            .ok_or_else(|| HoxError::JjCommand("Failed to parse backout output".to_string()))?;

        Ok(backout_change_id)
    }

    /// Get the evolution log for a change (all rewrites, amends)
    ///
    /// Executes: `jj evolog -r {change_id} -T {template} --no-graph`
    ///
    /// This returns the complete history of how a change evolved through rewrites,
    /// providing a full audit trail for agent operations.
    #[instrument(skip(self))]
    pub async fn evolution_log(&self, change_id: &str) -> Result<Vec<EvolutionEntry>> {
        debug!("Getting evolution log for {}", change_id);

        let template = r#"commit_id ++ "\t" ++ description.first_line() ++ "\t" ++ committer.timestamp() ++ "\n""#;

        let output = self
            .executor
            .exec(&["evolog", "-r", change_id, "-T", template, "--no-graph"])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to get evolution log: {}",
                output.stderr
            )));
        }

        let entries = output
            .stdout
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }

                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 3 {
                    return None;
                }

                Some(EvolutionEntry {
                    commit_id: parts[0].trim().to_string(),
                    description: parts[1].trim().to_string(),
                    timestamp: parts[2].trim().to_string(),
                })
            })
            .collect();

        Ok(entries)
    }

    /// Clean up redundant parent relationships
    ///
    /// Executes: `jj simplify-parents -r {change_id}`
    ///
    /// This removes redundant parent relationships that can occur after complex
    /// multi-agent merge operations, simplifying the DAG structure.
    #[instrument(skip(self))]
    pub async fn simplify_parents(&self, change_id: &str) -> Result<()> {
        debug!("Simplifying parents for {}", change_id);

        let output = self
            .executor
            .exec(&["simplify-parents", "-r", change_id])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to simplify parents: {}",
                output.stderr
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_parallelize() {
        let executor = MockJjExecutor::new().with_response(
            "parallelize heads(bookmarks(glob:\"task-*\"))",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops
            .parallelize("heads(bookmarks(glob:\"task-*\"))")
            .await
            .unwrap();

        assert!(result.clean);
        assert_eq!(result.conflicts.len(), 0);
    }

    #[tokio::test]
    async fn test_parallelize_with_conflicts() {
        let executor = MockJjExecutor::new().with_response(
            "parallelize some-revset",
            JjOutput {
                stdout: String::new(),
                stderr: "Warning: conflict detected in file.rs\n".to_string(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops.parallelize("some-revset").await.unwrap();

        assert!(!result.clean);
        assert_eq!(result.conflicts.len(), 1);
    }

    #[tokio::test]
    async fn test_absorb() {
        let executor = MockJjExecutor::new().with_response(
            "absorb",
            JjOutput {
                stdout: "Absorbed 3 hunks into commit abc123\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops.absorb(None).await.unwrap();

        assert_eq!(result.hunks_absorbed, 3);
        assert_eq!(result.affected_changes.len(), 1);
        assert_eq!(result.affected_changes[0], "abc123");
    }

    #[tokio::test]
    async fn test_absorb_with_paths() {
        let executor = MockJjExecutor::new().with_response(
            "absorb src/main.rs src/lib.rs",
            JjOutput {
                stdout: "Absorbed 5 hunks into commit xyz789\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops
            .absorb(Some(&["src/main.rs", "src/lib.rs"]))
            .await
            .unwrap();

        assert_eq!(result.hunks_absorbed, 5);
    }

    #[tokio::test]
    async fn test_split_by_files() {
        let executor = MockJjExecutor::new().with_response(
            "split -r abc123 --siblings src/main.rs",
            JjOutput {
                stdout: "Created new change def456789abc\nCreated new change ghi789012def\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let file_groups = vec![vec!["src/main.rs".to_string()]];
        let result = dag_ops.split_by_files("abc123", &file_groups).await.unwrap();

        assert_eq!(result.new_changes.len(), 2);
        assert_eq!(result.new_changes[0], "def456789abc");
        assert_eq!(result.new_changes[1], "ghi789012def");
    }

    #[tokio::test]
    async fn test_squash() {
        let executor = MockJjExecutor::new().with_response(
            "squash -r abc123",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops.squash("abc123").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_squash_into() {
        let executor = MockJjExecutor::new().with_response(
            "squash --from abc123 --into xyz789",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops.squash_into("abc123", "xyz789", None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_squash_into_with_paths() {
        let executor = MockJjExecutor::new().with_response(
            "squash --from abc123 --into xyz789 src/docs.md",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops
            .squash_into("abc123", "xyz789", Some(&["src/docs.md"]))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_duplicate() {
        let executor = MockJjExecutor::new().with_response(
            "duplicate abc123",
            JjOutput {
                stdout: "Created new change def456789abc\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let new_change_id = dag_ops.duplicate("abc123", None).await.unwrap();

        assert_eq!(new_change_id, "def456789abc");
    }

    #[tokio::test]
    async fn test_duplicate_with_destination() {
        let executor = MockJjExecutor::new().with_response(
            "duplicate abc123 -d xyz789",
            JjOutput {
                stdout: "Created new change ghi012345def\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let new_change_id = dag_ops.duplicate("abc123", Some("xyz789")).await.unwrap();

        assert_eq!(new_change_id, "ghi012345def");
    }

    #[tokio::test]
    async fn test_backout() {
        let executor = MockJjExecutor::new().with_response(
            "backout -r abc123",
            JjOutput {
                stdout: "Created backout change def456789abc\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let backout_id = dag_ops.backout("abc123").await.unwrap();

        assert_eq!(backout_id, "def456789abc");
    }

    #[tokio::test]
    async fn test_evolution_log() {
        let executor = MockJjExecutor::new().with_response(
            r#"evolog -r abc123 -T commit_id ++ "\t" ++ description.first_line() ++ "\t" ++ committer.timestamp() ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "abc123def456\tInitial commit\t2025-01-30 12:00:00\ndef456ghi789\tAmended message\t2025-01-30 12:30:00\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let entries = dag_ops.evolution_log("abc123").await.unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].commit_id, "abc123def456");
        assert_eq!(entries[0].description, "Initial commit");
        assert_eq!(entries[0].timestamp, "2025-01-30 12:00:00");
        assert_eq!(entries[1].commit_id, "def456ghi789");
        assert_eq!(entries[1].description, "Amended message");
    }

    #[tokio::test]
    async fn test_simplify_parents() {
        let executor = MockJjExecutor::new().with_response(
            "simplify-parents -r abc123",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let dag_ops = DagOperations::new(executor);
        let result = dag_ops.simplify_parents("abc123").await;

        assert!(result.is_ok());
    }
}
