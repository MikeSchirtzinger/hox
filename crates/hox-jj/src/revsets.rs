//! Revset query helpers for Hox orchestration

use hox_core::{ChangeId, Result};

use crate::command::{JjExecutor, JjOutput};
use crate::validate::{validate_identifier, validate_path, validate_revset};

/// Helper for building and executing revset queries
pub struct RevsetQueries<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> RevsetQueries<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Execute a revset query and return matching change IDs
    pub async fn query(&self, revset: &str) -> Result<Vec<ChangeId>> {
        let output = self
            .executor
            .exec(&["log", "-r", revset, "-T", "change_id ++ \"\\n\"", "--no-graph"])
            .await?;

        Ok(parse_change_ids(&output))
    }

    /// Find ready tasks (open status, no conflicts, at heads)
    ///
    /// Revset: `heads(description(glob:"Status: open")) - conflicts()`
    ///
    /// Note: When jj-dev is complete, this becomes:
    /// `heads(status(open)) - conflicts()`
    pub async fn ready_tasks(&self) -> Result<Vec<ChangeId>> {
        self.query("heads(description(glob:\"Status: open\")) - conflicts()")
            .await
    }

    /// Find tasks assigned to a specific orchestrator
    ///
    /// Note: When jj-dev is complete, this becomes:
    /// `orchestrator("O-A-1")`
    pub async fn by_orchestrator(&self, orchestrator: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(orchestrator, "orchestrator")?;
        let revset = format!("description(glob:\"Orchestrator: {}\")", orchestrator);
        self.query(&revset).await
    }

    /// Find tasks assigned to a specific agent
    pub async fn by_agent(&self, agent: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(agent, "agent")?;
        let revset = format!("description(glob:\"Agent: {}\")", agent);
        self.query(&revset).await
    }

    /// Find messages addressed to a target (supports wildcards)
    ///
    /// Note: When jj-dev is complete with glob support for msg_to:
    /// `msg_to("O-A-*")`
    pub async fn messages_to(&self, target: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(target, "message target")?;
        // For now, we need to handle wildcards in application code
        // JJ's glob support in description() is limited
        let revset = format!("description(glob:\"Msg-To: {}\")", target);
        self.query(&revset).await
    }

    /// Find mutation messages (structural decisions from orchestrators)
    pub async fn mutations(&self) -> Result<Vec<ChangeId>> {
        self.query("description(glob:\"Msg-Type: mutation\")")
            .await
    }

    /// Find alignment requests
    pub async fn align_requests(&self) -> Result<Vec<ChangeId>> {
        self.query("description(glob:\"Msg-Type: align_request\")")
            .await
    }

    /// Find ancestors of a change (what blocks this task)
    pub async fn ancestors(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        validate_identifier(change_id, "change_id")?;
        let revset = format!("ancestors({}) & mutable()", change_id);
        self.query(&revset).await
    }

    /// Find descendants of a change (what this task blocks)
    pub async fn descendants(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        validate_identifier(change_id, "change_id")?;
        let revset = format!("descendants({})", change_id);
        self.query(&revset).await
    }

    /// Find tasks by priority
    pub async fn by_priority(&self, priority: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(priority, "priority")?;
        let revset = format!("description(glob:\"Priority: {}\")", priority);
        self.query(&revset).await
    }

    /// Find tasks by status
    pub async fn by_status(&self, status: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(status, "status")?;
        let revset = format!("description(glob:\"Status: {}\")", status);
        self.query(&revset).await
    }

    /// Find changes with conflicts
    pub async fn conflicts(&self) -> Result<Vec<ChangeId>> {
        self.query("conflicts()").await
    }

    /// Get current working copy change
    pub async fn current(&self) -> Result<Option<ChangeId>> {
        let changes = self.query("@").await?;
        Ok(changes.into_iter().next())
    }

    // ============================================================================
    // Bookmark-based queries (O(1) lookup via JJ's bookmark index)
    // ============================================================================

    /// Find tasks by bookmark (fast path using bookmark index)
    ///
    /// Revset: `bookmarks(glob:"task/*")`
    pub async fn all_tasks_by_bookmark(&self) -> Result<Vec<ChangeId>> {
        self.query(r#"bookmarks(glob:"task/*")"#).await
    }

    /// Find tasks assigned to a specific agent by bookmark (fast path)
    ///
    /// Revset: `bookmarks(glob:"agent/{name}/task/*")`
    pub async fn agent_tasks_by_bookmark(&self, agent_name: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(agent_name, "agent_name")?;
        let revset = format!(r#"bookmarks(glob:"agent/{}/task/*")"#, agent_name);
        self.query(&revset).await
    }

    /// Find orchestrator by bookmark (fast path)
    ///
    /// Revset: `bookmarks(glob:"orchestrator/{id}")`
    pub async fn orchestrator_by_bookmark(&self, orch_id: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(orch_id, "orchestrator_id")?;
        let revset = format!(r#"bookmarks(glob:"orchestrator/{}")"#, orch_id);
        self.query(&revset).await
    }

    /// Find all orchestrators by bookmark prefix
    ///
    /// Revset: `bookmarks(glob:"orchestrator/*")`
    pub async fn all_orchestrators_by_bookmark(&self) -> Result<Vec<ChangeId>> {
        self.query(r#"bookmarks(glob:"orchestrator/*")"#).await
    }

    /// Find session by bookmark
    ///
    /// Revset: `bookmarks(glob:"session/{id}")`
    pub async fn session_by_bookmark(&self, session_id: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(session_id, "session_id")?;
        let revset = format!(r#"bookmarks(glob:"session/{}")"#, session_id);
        self.query(&revset).await
    }

    // ============================================================================
    // Power queries (Phase 6 - Advanced Revsets)
    // ============================================================================

    /// Find ready tasks: bookmarked, no conflicts, no conflicting ancestors
    ///
    /// Revset: `heads(bookmarks(glob:"task/*")) - conflicts() - ancestors(conflicts())`
    pub async fn ready_tasks_v2(&self) -> Result<Vec<ChangeId>> {
        self.query(r#"heads(bookmarks(glob:"task/*")) - conflicts() - ancestors(conflicts())"#)
            .await
    }

    /// Find agent's active work via bookmarks
    ///
    /// Revset: `bookmarks(glob:"agent/{name}/*") & ~description(glob:"Status: done")`
    pub async fn agent_active_work(&self, agent_name: &str) -> Result<Vec<ChangeId>> {
        validate_identifier(agent_name, "agent_name")?;
        let revset = format!(
            r#"bookmarks(glob:"agent/{}/*") & ~description(glob:"Status: done")"#,
            agent_name
        );
        self.query(&revset).await
    }

    /// Find parallelizable tasks (independent heads, no merges, no conflicts)
    ///
    /// Revset: `heads(mutable()) & ~merges() & ~conflicts()`
    pub async fn parallelizable_tasks(&self) -> Result<Vec<ChangeId>> {
        self.query("heads(mutable()) & ~merges() & ~conflicts()")
            .await
    }

    /// Find what blocks a specific task (conflicting ancestors)
    ///
    /// Revset: `ancestors({change_id}) & mutable() & conflicts()`
    pub async fn blocking_conflicts(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        validate_identifier(change_id, "change_id")?;
        let revset = format!("ancestors({}) & mutable() & conflicts()", change_id);
        self.query(&revset).await
    }

    /// Find empty changes (abandoned tasks)
    ///
    /// Revset: `empty() & mutable()`
    pub async fn empty_changes(&self) -> Result<Vec<ChangeId>> {
        self.query("empty() & mutable()").await
    }

    /// Find changes touching specific files
    ///
    /// Revset: `file("{path}")`
    pub async fn changes_touching_file(&self, path: &str) -> Result<Vec<ChangeId>> {
        validate_path(path, "file_path")?;
        let revset = format!(r#"file("{}")"#, path);
        self.query(&revset).await
    }

    /// Safe reference that doesn't error if change is missing
    ///
    /// Revset: `present({change_id})`
    pub async fn present(&self, change_id: &ChangeId) -> Result<Option<ChangeId>> {
        validate_identifier(change_id, "change_id")?;
        let revset = format!("present({})", change_id);
        let results = self.query(&revset).await?;
        Ok(results.into_iter().next())
    }

    /// Find connected component (task subgraph)
    ///
    /// Revset: `connected({change_id})`
    pub async fn connected_component(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        validate_identifier(change_id, "change_id")?;
        let revset = format!("connected({})", change_id);
        self.query(&revset).await
    }

    /// Find most recent N changes matching criteria
    ///
    /// Revset: `latest({revset}, {count})`
    pub async fn latest(&self, revset: &str, count: usize) -> Result<Vec<ChangeId>> {
        validate_revset(revset)?;
        let query = format!("latest({}, {})", revset, count);
        self.query(&query).await
    }
}

/// Parse change IDs from JJ output
fn parse_change_ids(output: &JjOutput) -> Vec<ChangeId> {
    output
        .stdout
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_query_parsing() {
        let executor = MockJjExecutor::new().with_response(
            "log -r @ -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "abc123\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.query("@").await.unwrap();

        assert_eq!(result, vec!["abc123"]);
    }

    #[tokio::test]
    async fn test_ready_tasks_v2_revset() {
        let executor = MockJjExecutor::new().with_response(
            r#"log -r heads(bookmarks(glob:"task/*")) - conflicts() - ancestors(conflicts()) -T change_id ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "task1\ntask2\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.ready_tasks_v2().await.unwrap();

        assert_eq!(result, vec!["task1", "task2"]);
    }

    #[tokio::test]
    async fn test_agent_active_work_revset() {
        let executor = MockJjExecutor::new().with_response(
            r#"log -r bookmarks(glob:"agent/agent-42/*") & ~description(glob:"Status: done") -T change_id ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "work1\nwork2\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.agent_active_work("agent-42").await.unwrap();

        assert_eq!(result, vec!["work1", "work2"]);
    }

    #[tokio::test]
    async fn test_parallelizable_tasks_revset() {
        let executor = MockJjExecutor::new().with_response(
            "log -r heads(mutable()) & ~merges() & ~conflicts() -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "task1\ntask2\ntask3\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.parallelizable_tasks().await.unwrap();

        assert_eq!(result, vec!["task1", "task2", "task3"]);
    }

    #[tokio::test]
    async fn test_blocking_conflicts_revset() {
        let executor = MockJjExecutor::new().with_response(
            "log -r ancestors(abc123) & mutable() & conflicts() -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "conflict1\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.blocking_conflicts(&"abc123".to_string()).await.unwrap();

        assert_eq!(result, vec!["conflict1"]);
    }

    #[tokio::test]
    async fn test_empty_changes_revset() {
        let executor = MockJjExecutor::new().with_response(
            "log -r empty() & mutable() -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "empty1\nempty2\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.empty_changes().await.unwrap();

        assert_eq!(result, vec!["empty1", "empty2"]);
    }

    #[tokio::test]
    async fn test_changes_touching_file_revset() {
        let executor = MockJjExecutor::new().with_response(
            r#"log -r file("src/main.rs") -T change_id ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "change1\nchange2\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.changes_touching_file("src/main.rs").await.unwrap();

        assert_eq!(result, vec!["change1", "change2"]);
    }

    #[tokio::test]
    async fn test_present_existing_change() {
        let executor = MockJjExecutor::new().with_response(
            "log -r present(abc123) -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "abc123\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.present(&"abc123".to_string()).await.unwrap();

        assert_eq!(result, Some("abc123".to_string()));
    }

    #[tokio::test]
    async fn test_present_missing_change() {
        let executor = MockJjExecutor::new().with_response(
            "log -r present(missing) -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.present(&"missing".to_string()).await.unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_connected_component_revset() {
        let executor = MockJjExecutor::new().with_response(
            "log -r connected(abc123) -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "abc123\ndef456\nghi789\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.connected_component(&"abc123".to_string()).await.unwrap();

        assert_eq!(result, vec!["abc123", "def456", "ghi789"]);
    }

    #[tokio::test]
    async fn test_latest_revset() {
        let executor = MockJjExecutor::new().with_response(
            r#"log -r latest(mutable(), 5) -T change_id ++ "\n" --no-graph"#,
            JjOutput {
                stdout: "recent1\nrecent2\nrecent3\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.latest("mutable()", 5).await.unwrap();

        assert_eq!(result, vec!["recent1", "recent2", "recent3"]);
    }
}
