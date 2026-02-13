//! Hook pipeline for post-iteration callbacks
//!
//! This module provides a fail-open hook system that allows registering
//! callbacks to run after agent tool execution. Hooks are fail-open:
//! failures are logged but never propagate errors.

use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

/// Context passed to hooks
#[derive(Debug, Clone)]
pub struct HookContext {
    /// Change ID being worked on
    pub change_id: String,
    /// Path to the workspace
    pub workspace_path: PathBuf,
    /// Current iteration number
    pub iteration: usize,
}

/// Result from hook execution
#[derive(Debug, Clone)]
pub struct HookResult {
    /// Whether the hook succeeded
    pub success: bool,
    /// Message describing what happened
    pub message: String,
}

impl HookResult {
    /// Create a success result
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// Create a failure result
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

/// Trait for post-tools hooks
#[async_trait]
pub trait PostToolsHook: Send + Sync {
    /// Execute the hook
    ///
    /// Returns HookResult indicating success or failure.
    /// Implementations should handle their own errors and return
    /// failure results rather than propagating errors.
    async fn execute(&self, context: &HookContext) -> HookResult;
}

/// Pipeline for executing multiple hooks in sequence
pub struct HookPipeline {
    hooks: Vec<Box<dyn PostToolsHook + Send + Sync>>,
}

impl HookPipeline {
    /// Create a new empty pipeline
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Add a hook to the pipeline
    pub fn add_hook(&mut self, hook: Box<dyn PostToolsHook + Send + Sync>) {
        self.hooks.push(hook);
    }

    /// Execute all hooks in order (fail-open)
    ///
    /// Runs each hook sequentially. If a hook fails, logs a warning
    /// but continues executing remaining hooks. Never propagates errors.
    pub async fn execute_all(&self, context: &HookContext) -> Vec<HookResult> {
        let mut results = Vec::new();

        for (idx, hook) in self.hooks.iter().enumerate() {
            info!(
                "Executing hook {} of {} for change {}",
                idx + 1,
                self.hooks.len(),
                context.change_id
            );

            let result = hook.execute(context).await;

            if result.success {
                info!("Hook {} succeeded: {}", idx + 1, result.message);
            } else {
                warn!("Hook {} failed (continuing): {}", idx + 1, result.message);
            }

            results.push(result);
        }

        results
    }

    /// Get number of hooks in pipeline
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Check if pipeline is empty
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }
}

impl Default for HookPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Auto-commit hook (placeholder implementation)
///
/// In production, this would create a JJ snapshot after each iteration.
/// Currently logs that it would commit.
pub struct AutoCommitHook;

#[async_trait]
impl PostToolsHook for AutoCommitHook {
    async fn execute(&self, context: &HookContext) -> HookResult {
        info!(
            "AutoCommitHook: Would create snapshot for change {} at iteration {}",
            context.change_id, context.iteration
        );

        // TODO: Actually create JJ snapshot
        // jj describe -r <change_id> -m "Auto-snapshot iteration <iteration>"

        HookResult::success(format!(
            "Auto-commit for iteration {} (placeholder)",
            context.iteration
        ))
    }
}

/// Snapshot hook (placeholder implementation)
///
/// In production, this would create a JJ operation snapshot.
/// Currently logs that a snapshot was taken.
pub struct SnapshotHook;

#[async_trait]
impl PostToolsHook for SnapshotHook {
    async fn execute(&self, context: &HookContext) -> HookResult {
        info!(
            "SnapshotHook: Would create operation snapshot for change {} at workspace {:?}",
            context.change_id, context.workspace_path
        );

        // TODO: Actually create operation snapshot
        // This would use JjExecutor to create an operation-level snapshot

        HookResult::success(format!(
            "Snapshot at iteration {} (placeholder)",
            context.iteration
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;

    // Test hook that tracks execution
    struct TestHook {
        name: String,
        should_succeed: bool,
        execution_count: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl PostToolsHook for TestHook {
        async fn execute(&self, context: &HookContext) -> HookResult {
            let mut count = self.execution_count.lock().unwrap();
            *count += 1;

            if self.should_succeed {
                HookResult::success(format!(
                    "{} executed for change {} (execution #{})",
                    self.name, context.change_id, *count
                ))
            } else {
                HookResult::failure(format!(
                    "{} failed for change {} (execution #{})",
                    self.name, context.change_id, *count
                ))
            }
        }
    }

    #[tokio::test]
    async fn test_hook_pipeline_empty() {
        let pipeline = HookPipeline::new();
        assert_eq!(pipeline.len(), 0);
        assert!(pipeline.is_empty());

        let context = HookContext {
            change_id: "test-123".to_string(),
            workspace_path: PathBuf::from("/tmp/test"),
            iteration: 1,
        };

        let results = pipeline.execute_all(&context).await;
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_hook_pipeline_single_success() {
        let mut pipeline = HookPipeline::new();
        let execution_count = Arc::new(Mutex::new(0));

        pipeline.add_hook(Box::new(TestHook {
            name: "TestHook1".to_string(),
            should_succeed: true,
            execution_count: execution_count.clone(),
        }));

        let context = HookContext {
            change_id: "test-456".to_string(),
            workspace_path: PathBuf::from("/tmp/test"),
            iteration: 2,
        };

        let results = pipeline.execute_all(&context).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].message.contains("TestHook1"));

        let count = execution_count.lock().unwrap();
        assert_eq!(*count, 1);
    }

    #[tokio::test]
    async fn test_hook_pipeline_fail_open() {
        let mut pipeline = HookPipeline::new();
        let count1 = Arc::new(Mutex::new(0));
        let count2 = Arc::new(Mutex::new(0));
        let count3 = Arc::new(Mutex::new(0));

        // Add three hooks: success, failure, success
        pipeline.add_hook(Box::new(TestHook {
            name: "Hook1".to_string(),
            should_succeed: true,
            execution_count: count1.clone(),
        }));

        pipeline.add_hook(Box::new(TestHook {
            name: "Hook2".to_string(),
            should_succeed: false, // This one fails
            execution_count: count2.clone(),
        }));

        pipeline.add_hook(Box::new(TestHook {
            name: "Hook3".to_string(),
            should_succeed: true,
            execution_count: count3.clone(),
        }));

        let context = HookContext {
            change_id: "test-789".to_string(),
            workspace_path: PathBuf::from("/tmp/test"),
            iteration: 3,
        };

        let results = pipeline.execute_all(&context).await;
        assert_eq!(results.len(), 3);

        // All hooks executed despite middle one failing
        assert!(results[0].success);
        assert!(!results[1].success);
        assert!(results[2].success);

        // Verify all hooks were executed
        assert_eq!(*count1.lock().unwrap(), 1);
        assert_eq!(*count2.lock().unwrap(), 1);
        assert_eq!(*count3.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_auto_commit_hook() {
        let hook = AutoCommitHook;
        let context = HookContext {
            change_id: "abc123".to_string(),
            workspace_path: PathBuf::from("/tmp/workspace"),
            iteration: 5,
        };

        let result = hook.execute(&context).await;
        assert!(result.success);
        assert!(result.message.contains("iteration 5"));
    }

    #[tokio::test]
    async fn test_snapshot_hook() {
        let hook = SnapshotHook;
        let context = HookContext {
            change_id: "def456".to_string(),
            workspace_path: PathBuf::from("/tmp/workspace"),
            iteration: 7,
        };

        let result = hook.execute(&context).await;
        assert!(result.success);
        assert!(result.message.contains("iteration 7"));
    }

    #[tokio::test]
    async fn test_pipeline_with_real_hooks() {
        let mut pipeline = HookPipeline::new();
        pipeline.add_hook(Box::new(AutoCommitHook));
        pipeline.add_hook(Box::new(SnapshotHook));

        assert_eq!(pipeline.len(), 2);
        assert!(!pipeline.is_empty());

        let context = HookContext {
            change_id: "real-123".to_string(),
            workspace_path: PathBuf::from("/tmp/real"),
            iteration: 10,
        };

        let results = pipeline.execute_all(&context).await;
        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);
    }

    #[tokio::test]
    async fn test_hook_result_creation() {
        let success = HookResult::success("Everything worked");
        assert!(success.success);
        assert_eq!(success.message, "Everything worked");

        let failure = HookResult::failure("Something broke");
        assert!(!failure.success);
        assert_eq!(failure.message, "Something broke");
    }

    #[tokio::test]
    async fn test_hook_context_creation() {
        let context = HookContext {
            change_id: "context-test".to_string(),
            workspace_path: PathBuf::from("/test/path"),
            iteration: 42,
        };

        assert_eq!(context.change_id, "context-test");
        assert_eq!(context.workspace_path, PathBuf::from("/test/path"));
        assert_eq!(context.iteration, 42);
    }
}
