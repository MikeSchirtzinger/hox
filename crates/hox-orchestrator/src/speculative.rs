//! Speculative execution manager for parallel strategy exploration
//!
//! This module enables:
//! - Trying multiple approaches to a task in parallel
//! - Maintaining complete audit trails via evolution logs
//! - Safe reversion without destructive history editing
//! - DAG cleanup after complex multi-agent operations

use hox_core::Result;
use hox_jj::{BookmarkManager, DagOperations, EvolutionEntry, JjExecutor};
use tracing::{debug, instrument};

/// Manager for speculative execution patterns
pub struct SpeculativeExecutor<E: JjExecutor> {
    dag_ops: DagOperations<E>,
    bookmark_manager: BookmarkManager<E>,
}

impl<E: JjExecutor + Clone> SpeculativeExecutor<E> {
    /// Create a new speculative executor
    pub fn new(executor: E) -> Self {
        let dag_ops = DagOperations::new(executor.clone());
        let bookmark_manager = BookmarkManager::new(executor);

        Self {
            dag_ops,
            bookmark_manager,
        }
    }

    /// Try multiple approaches to a task in parallel
    ///
    /// Creates N duplicates of the task change, each with a different strategy bookmark.
    /// This enables parallel exploration of different solution approaches.
    ///
    /// # Arguments
    /// * `change_id` - The task change to duplicate
    /// * `strategies` - Names for each strategy approach
    ///
    /// # Returns
    /// Vector of new change IDs, one for each strategy
    #[instrument(skip(self))]
    pub async fn try_approaches(
        &self,
        change_id: &str,
        strategies: &[String],
    ) -> Result<Vec<String>> {
        debug!("Creating {} parallel approaches for {}", strategies.len(), change_id);

        let mut duplicate_ids = Vec::new();

        for (i, strategy) in strategies.iter().enumerate() {
            // Duplicate the change
            let new_change_id = self.dag_ops.duplicate(change_id, None).await?;

            // Create bookmark for this strategy
            let bookmark_name = format!("strategy/{}/{}", change_id, strategy);
            self.bookmark_manager
                .create(&bookmark_name, &new_change_id)
                .await?;

            debug!("Created approach {} ({}/{}) -> {}",
                strategy, i + 1, strategies.len(), new_change_id);

            duplicate_ids.push(new_change_id);
        }

        Ok(duplicate_ids)
    }

    /// Get evolution history for a change (audit trail)
    ///
    /// Returns the complete evolution log showing all rewrites, amends, and
    /// modifications that led to the current state of the change.
    #[instrument(skip(self))]
    pub async fn audit_trail(&self, change_id: &str) -> Result<Vec<EvolutionEntry>> {
        debug!("Retrieving audit trail for {}", change_id);
        self.dag_ops.evolution_log(change_id).await
    }

    /// Safely revert a change without destructive history editing
    ///
    /// Creates a backout change that reverses the effects of the specified change,
    /// preserving the complete history for audit purposes.
    ///
    /// # Returns
    /// The change ID of the backout change
    #[instrument(skip(self))]
    pub async fn safe_revert(&self, change_id: &str) -> Result<String> {
        debug!("Creating safe revert for {}", change_id);
        self.dag_ops.backout(change_id).await
    }

    /// Clean up DAG after multi-agent merge
    ///
    /// Removes redundant parent relationships that can accumulate during
    /// complex orchestration scenarios with many parallel agents.
    #[instrument(skip(self))]
    pub async fn cleanup_dag(&self, change_id: &str) -> Result<()> {
        debug!("Cleaning up DAG for {}", change_id);
        self.dag_ops.simplify_parents(change_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_jj::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_try_approaches() {
        // For simplicity, we'll test with a single approach since MockJjExecutor
        // doesn't support different responses for the same command key
        let executor = MockJjExecutor::new()
            .with_response(
                "duplicate abc123",
                JjOutput {
                    stdout: "Created new change def456789abc\n".to_string(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with_response(
                "bookmark create strategy/abc123/approach-a -r def456789abc",
                JjOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            );

        let spec_exec = SpeculativeExecutor::new(executor);
        let strategies = vec!["approach-a".to_string()];
        let change_ids = spec_exec.try_approaches("abc123", &strategies).await.unwrap();

        assert_eq!(change_ids.len(), 1);
        assert_eq!(change_ids[0], "def456789abc");
    }

    #[tokio::test]
    async fn test_audit_trail() {
        let executor = MockJjExecutor::new().with_response(
            r#"evolog -r abc123 -T commit_id ++ "\t" ++ description.first_line() ++ "\t" ++ committer.timestamp() ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "abc123def456\tInitial commit\t2025-01-30 12:00:00\ndef456ghi789\tAmended message\t2025-01-30 12:30:00\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let spec_exec = SpeculativeExecutor::new(executor);
        let entries = spec_exec.audit_trail("abc123").await.unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].commit_id, "abc123def456");
        assert_eq!(entries[1].commit_id, "def456ghi789");
    }

    #[tokio::test]
    async fn test_safe_revert() {
        let executor = MockJjExecutor::new().with_response(
            "backout -r abc123",
            JjOutput {
                stdout: "Created backout change xyz987654abc\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let spec_exec = SpeculativeExecutor::new(executor);
        let backout_id = spec_exec.safe_revert("abc123").await.unwrap();

        assert_eq!(backout_id, "xyz987654abc");
    }

    #[tokio::test]
    async fn test_cleanup_dag() {
        let executor = MockJjExecutor::new().with_response(
            "simplify-parents -r abc123",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let spec_exec = SpeculativeExecutor::new(executor);
        let result = spec_exec.cleanup_dag("abc123").await;

        assert!(result.is_ok());
    }
}
