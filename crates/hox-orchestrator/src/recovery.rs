//! Recovery manager for rolling back bad agent output
//!
//! This module provides recovery capabilities for Hox orchestration:
//! - Rolling back operations after bad agent iterations
//! - Creating recovery points before risky operations
//! - Restoring from saved recovery points
//! - Cleaning up agent workspaces after rollback

use chrono::{DateTime, Utc};
use hox_core::Result;
use hox_jj::{JjExecutor, OpManager};
use std::path::PathBuf;
use tracing::{info, warn};

/// A recovery point representing a known-good state
#[derive(Debug, Clone)]
pub struct RecoveryPoint {
    pub operation_id: String,
    pub created_at: DateTime<Utc>,
    pub description: String,
}

impl RecoveryPoint {
    /// Create a new recovery point
    pub fn new(operation_id: String, description: String) -> Self {
        Self {
            operation_id,
            created_at: Utc::now(),
            description,
        }
    }
}

/// Result of a rollback operation
#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub operations_undone: usize,
    pub agent_cleaned: bool,
    pub workspace_removed: bool,
}

impl RollbackResult {
    /// Create a result with no actions taken
    pub fn none() -> Self {
        Self {
            operations_undone: 0,
            agent_cleaned: false,
            workspace_removed: false,
        }
    }
}

/// Manager for recovery operations
///
/// This provides the ability to rollback bad agent output by:
/// - Restoring to a previous operation state
/// - Cleaning up agent workspaces
/// - Managing recovery points
pub struct RecoveryManager<E: JjExecutor> {
    op_manager: OpManager<E>,
    workspaces_dir: PathBuf,
}

impl<E: JjExecutor> RecoveryManager<E> {
    /// Create a new recovery manager
    pub fn new(executor: E, repo_root: PathBuf) -> Self {
        let op_manager = OpManager::new(executor);
        let workspaces_dir = repo_root
            .parent()
            .unwrap_or(&repo_root)
            .join(".hox-workspaces");

        Self {
            op_manager,
            workspaces_dir,
        }
    }

    /// Create a recovery point at the current operation
    ///
    /// Returns a recovery point that can be used to restore state later.
    pub async fn create_recovery_point(&self, description: String) -> Result<RecoveryPoint> {
        let operation_id = self.op_manager.snapshot().await?;

        info!("Created recovery point: {} ({})", operation_id, description);

        Ok(RecoveryPoint::new(operation_id, description))
    }

    /// Restore from a recovery point
    ///
    /// This restores the repository to the state captured in the recovery point,
    /// discarding all operations that came after it.
    pub async fn restore_from(&self, recovery_point: &RecoveryPoint) -> Result<RollbackResult> {
        info!(
            "Restoring from recovery point: {} ({})",
            recovery_point.operation_id, recovery_point.description
        );

        // Count operations that will be undone
        let current_ops = self.op_manager.recent_operations(100).await?;
        let mut operations_undone = 0;

        for op in &current_ops {
            if op.id == recovery_point.operation_id {
                break;
            }
            operations_undone += 1;
        }

        // Restore to the recovery point
        self.op_manager
            .restore(&recovery_point.operation_id)
            .await?;

        info!("Restored to operation {}", recovery_point.operation_id);

        Ok(RollbackResult {
            operations_undone,
            agent_cleaned: false,
            workspace_removed: false,
        })
    }

    /// Roll back the last N operations
    ///
    /// This is useful when you know how many operations to undo but don't
    /// have a specific recovery point.
    pub async fn rollback_operations(&self, count: usize) -> Result<RollbackResult> {
        info!("Rolling back {} operations", count);

        if count == 0 {
            warn!("Rollback count is 0, nothing to do");
            return Ok(RollbackResult::none());
        }

        // Undo operations one by one
        // Note: We could optimize this by getting the Nth operation and restoring to it
        for i in 0..count {
            match self.op_manager.undo().await {
                Ok(_) => {
                    info!("Undone operation {}/{}", i + 1, count);
                }
                Err(e) => {
                    warn!("Failed to undo operation {}/{}: {}", i + 1, count, e);
                    return Ok(RollbackResult {
                        operations_undone: i,
                        agent_cleaned: false,
                        workspace_removed: false,
                    });
                }
            }
        }

        Ok(RollbackResult {
            operations_undone: count,
            agent_cleaned: false,
            workspace_removed: false,
        })
    }

    /// Roll back an agent's work to a specific snapshot
    ///
    /// This does three things:
    /// 1. Restores to the snapshot operation
    /// 2. Marks the agent as cleaned
    /// 3. Optionally removes the agent's workspace directory
    pub async fn rollback_agent(
        &self,
        agent_name: &str,
        snapshot_op_id: &str,
        remove_workspace: bool,
    ) -> Result<RollbackResult> {
        info!(
            "Rolling back agent {} to operation {}",
            agent_name, snapshot_op_id
        );

        // Count operations that will be undone (must happen BEFORE restore)
        let current_ops = self.op_manager.recent_operations(100).await?;
        let mut operations_undone = 0;

        for op in &current_ops {
            if op.id == snapshot_op_id {
                break;
            }
            operations_undone += 1;
        }

        // Restore to snapshot
        self.op_manager.restore(snapshot_op_id).await?;

        // Clean up workspace if requested
        let workspace_removed = if remove_workspace {
            let workspace_path = self.workspaces_dir.join(agent_name);
            if workspace_path.exists() {
                match tokio::fs::remove_dir_all(&workspace_path).await {
                    Ok(_) => {
                        info!("Removed workspace: {:?}", workspace_path);
                        true
                    }
                    Err(e) => {
                        warn!("Failed to remove workspace {:?}: {}", workspace_path, e);
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        Ok(RollbackResult {
            operations_undone,
            agent_cleaned: true,
            workspace_removed,
        })
    }

    /// Get recent operations for inspection
    pub async fn recent_operations(&self, count: usize) -> Result<Vec<hox_jj::OperationInfo>> {
        self.op_manager.recent_operations(count).await
    }

    /// Take a snapshot of the current state
    ///
    /// This is a shorthand for creating a recovery point with a default description.
    pub async fn snapshot(&self) -> Result<String> {
        self.op_manager.snapshot().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_jj::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_create_recovery_point() {
        let executor = MockJjExecutor::new().with_response(
            "op log -n 1 -T operation_id ++ \"\\t\" ++ description ++ \"\\t\" ++ time ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "snapshot-123\tcurrent state\t2024-01-01 12:00:00\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = RecoveryManager::new(executor, PathBuf::from("/tmp/repo"));
        let point = manager
            .create_recovery_point("Before risky operation".to_string())
            .await
            .unwrap();

        assert_eq!(point.operation_id, "snapshot-123");
        assert_eq!(point.description, "Before risky operation");
    }

    #[tokio::test]
    async fn test_rollback_operations() {
        let executor = MockJjExecutor::new()
            .with_response(
                "undo",
                JjOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with_response(
                "undo",
                JjOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            );

        let manager = RecoveryManager::new(executor, PathBuf::from("/tmp/repo"));
        let result = manager.rollback_operations(2).await.unwrap();

        assert_eq!(result.operations_undone, 2);
        assert!(!result.agent_cleaned);
        assert!(!result.workspace_removed);
    }

    #[tokio::test]
    async fn test_rollback_operations_zero() {
        let executor = MockJjExecutor::new();
        let manager = RecoveryManager::new(executor, PathBuf::from("/tmp/repo"));
        let result = manager.rollback_operations(0).await.unwrap();

        assert_eq!(result.operations_undone, 0);
    }

    #[test]
    fn test_recovery_point_creation() {
        let point = RecoveryPoint::new("op-123".to_string(), "Test point".to_string());

        assert_eq!(point.operation_id, "op-123");
        assert_eq!(point.description, "Test point");
        // created_at should be close to now
        assert!((Utc::now() - point.created_at).num_seconds() < 5);
    }

    #[test]
    fn test_rollback_result_none() {
        let result = RollbackResult::none();

        assert_eq!(result.operations_undone, 0);
        assert!(!result.agent_cleaned);
        assert!(!result.workspace_removed);
    }
}
