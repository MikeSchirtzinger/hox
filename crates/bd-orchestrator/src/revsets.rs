//! Revset query helpers for task orchestration.
//!
//! This module provides common revset patterns for querying task state,
//! dependencies, and assignment in a JJ repository-based task system.

use std::process::Command;
use thiserror::Error;

/// Errors that can occur during revset queries.
#[derive(Debug, Error)]
pub enum RevsetError {
    #[error("JJ command failed: {0}")]
    CommandFailed(String),

    #[error("Invalid output from JJ: {0}")]
    InvalidOutput(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RevsetError>;

/// Trait for executing JJ commands.
///
/// This abstraction allows for testing and different JJ execution strategies.
#[async_trait::async_trait]
pub trait JjExecutor: Send + Sync {
    /// Execute a JJ command with the given arguments.
    async fn exec(&self, args: &[&str]) -> Result<Vec<u8>>;
}

/// Default JJ executor that runs jj commands via subprocess.
#[derive(Debug, Clone)]
pub struct DefaultJjExecutor {
    repo_path: std::path::PathBuf,
}

impl DefaultJjExecutor {
    /// Create a new executor for the given repository path.
    pub fn new(repo_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
        }
    }
}

#[async_trait::async_trait]
impl JjExecutor for DefaultJjExecutor {
    async fn exec(&self, args: &[&str]) -> Result<Vec<u8>> {
        let repo_path = self.repo_path.clone();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let output = tokio::task::spawn_blocking(move || {
            Command::new("jj")
                .current_dir(&repo_path)
                .args(&args)
                .output()
        })
        .await
        .map_err(|e| RevsetError::CommandFailed(e.to_string()))?
        .map_err(|e| RevsetError::Io(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RevsetError::CommandFailed(stderr.to_string()));
        }

        Ok(output.stdout)
    }
}

/// Provides common revset patterns for task orchestration.
pub struct RevsetQueries<E: JjExecutor> {
    jj: E,
}

impl<E: JjExecutor> RevsetQueries<E> {
    /// Create a new revset query helper with the given executor.
    pub fn new(jj: E) -> Self {
        Self { jj }
    }

    /// Returns tasks that have no incomplete dependencies.
    ///
    /// These are leaf nodes in the task DAG that aren't in conflict.
    /// Uses revset: `heads(bookmarks(glob:"task-*")) - conflicts()`
    pub async fn ready_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"heads(bookmarks(glob:"task-*")) - conflicts()"#;
        self.query_change_ids(revset).await
    }

    /// Returns tasks that have incomplete ancestors.
    ///
    /// This finds tasks whose dependencies aren't done yet.
    /// Uses revset: `bookmarks(glob:"task-*") & descendants(mutable())`
    pub async fn blocked_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"task-*") & descendants(mutable())"#;
        self.query_change_ids(revset).await
    }

    /// Returns all tasks assigned to a specific agent.
    ///
    /// Uses revset: `bookmarks(glob:"agent-{id}/*")`
    pub async fn agent_tasks(&self, agent_id: &str) -> Result<Vec<String>> {
        let revset = format!(r#"bookmarks(glob:"agent-{}/*")"#, agent_id);
        self.query_change_ids(&revset).await
    }

    /// Returns tasks with no agent bookmark.
    ///
    /// Uses revset: `bookmarks(glob:"task-*") - bookmarks(glob:"agent-*/*")`
    pub async fn unassigned_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"task-*") - bookmarks(glob:"agent-*/*")"#;
        self.query_change_ids(revset).await
    }

    /// Returns all changes that must complete before the given task.
    ///
    /// All ancestors of this change (except immutable/root).
    /// Uses revset: `ancestors({id}) & mutable()`
    pub async fn task_dependencies(&self, change_id: &str) -> Result<Vec<String>> {
        let revset = format!("ancestors({}) & mutable()", change_id);
        self.query_change_ids(&revset).await
    }

    /// Returns all changes that depend on the given task.
    ///
    /// Uses revset: `descendants({id}) - {id}`
    pub async fn dependent_tasks(&self, change_id: &str) -> Result<Vec<String>> {
        let revset = format!("descendants({}) - {}", change_id, change_id);
        self.query_change_ids(&revset).await
    }

    /// Returns tasks that have conflicts.
    ///
    /// Uses revset: `bookmarks(glob:"task-*") & conflicts()`
    pub async fn conflicting_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"task-*") & conflicts()"#;
        self.query_change_ids(revset).await
    }

    /// Returns all task changes.
    ///
    /// Uses revset: `bookmarks(glob:"task-*")`
    pub async fn all_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"task-*")"#;
        self.query_change_ids(revset).await
    }

    /// Returns tasks that are actively being worked on.
    ///
    /// These have agent bookmarks pointing to them.
    /// Uses revset: `bookmarks(glob:"agent-*/*")`
    pub async fn in_progress_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"agent-*/*")"#;
        self.query_change_ids(revset).await
    }

    /// Executes a revset query and returns change IDs.
    ///
    /// This is a private helper method that handles the common pattern
    /// of running `jj log` with a revset and extracting change IDs.
    async fn query_change_ids(&self, revset: &str) -> Result<Vec<String>> {
        let output = self
            .jj
            .exec(&[
                "log",
                "-r",
                revset,
                "--no-graph",
                "-T",
                r#"change_id ++ "\n""#,
            ])
            .await?;

        let output_str = String::from_utf8_lossy(&output);

        // Empty result or "no matching" is not an error, just return empty vec
        if output_str.trim().is_empty() || output_str.contains("no matching") {
            return Ok(Vec::new());
        }

        let ids: Vec<String> = output_str
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();

        Ok(ids)
    }
}

/// Result of a detailed task query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryResult {
    pub change_id: String,
    pub bookmark: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
}

impl<E: JjExecutor> RevsetQueries<E> {
    /// Executes a revset and returns detailed task information.
    ///
    /// This uses a custom template to extract structured data including
    /// bookmarks, description, author, and timestamp.
    pub async fn query_tasks(&self, revset: &str) -> Result<Vec<QueryResult>> {
        let template = r#"change_id ++ "|" ++ bookmarks ++ "|" ++ description.first_line() ++ "|" ++ author ++ "|" ++ committer.timestamp() ++ "\n""#;

        let output = self
            .jj
            .exec(&["log", "-r", revset, "--no-graph", "-T", template])
            .await?;

        let output_str = String::from_utf8_lossy(&output);

        let results: Vec<QueryResult> = output_str
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(5, '|').collect();
                if parts.len() < 5 {
                    return None;
                }

                Some(QueryResult {
                    change_id: parts[0].trim().to_string(),
                    bookmark: parts[1].trim().to_string(),
                    description: parts[2].trim().to_string(),
                    author: parts[3].trim().to_string(),
                    timestamp: parts[4].trim().to_string(),
                })
            })
            .collect();

        Ok(results)
    }
}

/// Node in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    pub change_id: String,
    pub label: String,
    pub status: TaskStatus,
}

/// Edge in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
}

/// Task status in the dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Blocked,
    InProgress,
    Conflict,
}

/// Type of dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeType {
    Blocks,
    ParentChild,
}

/// Complete dependency graph for visualization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl<E: JjExecutor> RevsetQueries<E> {
    /// Builds a dependency graph for visualization.
    ///
    /// This creates a complete picture of all tasks, their statuses,
    /// and their dependencies for visualization tools.
    pub async fn build_dependency_graph(&self) -> Result<DependencyGraph> {
        let mut graph = DependencyGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
        };

        // Get all tasks with details
        let tasks = self.query_tasks(r#"bookmarks(glob:"task-*")"#).await?;

        // Build status sets for quick lookup
        let ready = self.ready_tasks().await.unwrap_or_default();
        let in_progress = self.in_progress_tasks().await.unwrap_or_default();
        let conflicts = self.conflicting_tasks().await.unwrap_or_default();

        let ready_set: std::collections::HashSet<_> = ready.into_iter().collect();
        let in_progress_set: std::collections::HashSet<_> = in_progress.into_iter().collect();
        let conflict_set: std::collections::HashSet<_> = conflicts.into_iter().collect();

        // Build nodes and edges
        for task in tasks {
            // Determine status
            let status = if conflict_set.contains(&task.change_id) {
                TaskStatus::Conflict
            } else if in_progress_set.contains(&task.change_id) {
                TaskStatus::InProgress
            } else if ready_set.contains(&task.change_id) {
                TaskStatus::Ready
            } else {
                TaskStatus::Blocked
            };

            graph.nodes.push(GraphNode {
                change_id: task.change_id.clone(),
                label: task.description.clone(),
                status,
            });

            // Get dependencies for edges
            if let Ok(deps) = self.task_dependencies(&task.change_id).await {
                for dep in deps {
                    graph.edges.push(GraphEdge {
                        from: dep,
                        to: task.change_id.clone(),
                        edge_type: EdgeType::Blocks,
                    });
                }
            }
        }

        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock executor for testing.
    struct MockExecutor {
        responses: std::collections::HashMap<String, Vec<u8>>,
    }

    impl MockExecutor {
        fn new() -> Self {
            Self {
                responses: std::collections::HashMap::new(),
            }
        }

        fn add_response(&mut self, args_key: &str, response: &str) {
            self.responses
                .insert(args_key.to_string(), response.as_bytes().to_vec());
        }
    }

    #[async_trait::async_trait]
    impl JjExecutor for MockExecutor {
        async fn exec(&self, args: &[&str]) -> Result<Vec<u8>> {
            let key = args.join(" ");
            self.responses
                .get(&key)
                .cloned()
                .ok_or_else(|| RevsetError::CommandFailed(format!("No mock for: {}", key)))
        }
    }

    #[tokio::test]
    async fn test_ready_tasks() {
        let mut executor = MockExecutor::new();
        executor.add_response(
            r#"log -r heads(bookmarks(glob:"task-*")) - conflicts() --no-graph -T change_id ++ "\n""#,
            "abc123\ndef456\n",
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.ready_tasks().await.unwrap();

        assert_eq!(result, vec!["abc123", "def456"]);
    }

    #[tokio::test]
    async fn test_empty_result() {
        let mut executor = MockExecutor::new();
        executor.add_response(
            r#"log -r bookmarks(glob:"task-*") --no-graph -T change_id ++ "\n""#,
            "",
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.all_tasks().await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_agent_tasks() {
        let mut executor = MockExecutor::new();
        executor.add_response(
            r#"log -r bookmarks(glob:"agent-alice/*") --no-graph -T change_id ++ "\n""#,
            "task001\ntask002\n",
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.agent_tasks("alice").await.unwrap();

        assert_eq!(result, vec!["task001", "task002"]);
    }

    #[tokio::test]
    async fn test_task_dependencies() {
        let mut executor = MockExecutor::new();
        executor.add_response(
            r#"log -r ancestors(abc123) & mutable() --no-graph -T change_id ++ "\n""#,
            "dep1\ndep2\nabc123\n",
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.task_dependencies("abc123").await.unwrap();

        assert_eq!(result, vec!["dep1", "dep2", "abc123"]);
    }
}
