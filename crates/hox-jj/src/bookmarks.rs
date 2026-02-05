//! Bookmark management for Hox assignments
//!
//! This module provides helpers for managing JJ bookmarks as the primary
//! mechanism for task assignments. Bookmarks follow these naming conventions:
//!
//! - `task/{change-id-prefix}` — Task bookmark
//! - `agent/{agent-name}/task/{id}` — Agent assignment
//! - `orchestrator/{orch-id}` — Orchestrator base
//! - `session/{session-id}` — Session tracking

use hox_core::{ChangeId, HoxError, Result};
use std::collections::HashMap;
use tracing::{debug, instrument};

use crate::command::JjExecutor;
use crate::validate::validate_identifier;

/// Information about a bookmark
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkInfo {
    /// Bookmark name
    pub name: String,
    /// Change ID the bookmark points to
    pub change_id: String,
    /// Remote tracking info (if any)
    pub tracking: Option<String>,
}

/// Manager for bookmark operations
pub struct BookmarkManager<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> BookmarkManager<E> {
    /// Create a new bookmark manager
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Create a new bookmark pointing to a change
    ///
    /// Executes: `jj bookmark create {name} -r {change_id}`
    #[instrument(skip(self))]
    pub async fn create(&self, name: &str, change_id: &ChangeId) -> Result<()> {
        debug!("Creating bookmark {} -> {}", name, change_id);

        let output = self
            .executor
            .exec(&["bookmark", "create", name, "-r", change_id])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to create bookmark {}: {}",
                name, output.stderr
            )));
        }

        Ok(())
    }

    /// Set (move) an existing bookmark to a change
    ///
    /// Executes: `jj bookmark set {name} -r {change_id}`
    #[instrument(skip(self))]
    pub async fn set(&self, name: &str, change_id: &ChangeId) -> Result<()> {
        debug!("Setting bookmark {} -> {}", name, change_id);

        let output = self
            .executor
            .exec(&["bookmark", "set", name, "-r", change_id])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to set bookmark {}: {}",
                name, output.stderr
            )));
        }

        Ok(())
    }

    /// Delete a bookmark
    ///
    /// Executes: `jj bookmark delete {name}`
    #[instrument(skip(self))]
    pub async fn delete(&self, name: &str) -> Result<()> {
        debug!("Deleting bookmark {}", name);

        let output = self.executor.exec(&["bookmark", "delete", name]).await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to delete bookmark {}: {}",
                name, output.stderr
            )));
        }

        Ok(())
    }

    /// List all bookmarks, optionally filtered by glob pattern
    ///
    /// Executes: `jj bookmark list --all -T {template}`
    #[instrument(skip(self))]
    pub async fn list(&self, glob_pattern: Option<&str>) -> Result<Vec<BookmarkInfo>> {
        debug!("Listing bookmarks with pattern: {:?}", glob_pattern);

        // Use template to output parseable format
        // Format: name|change_id|tracking
        let template = r#"name ++ "|" ++ change_id ++ "|" ++ if(tracked, remote_name, "") ++ "\n""#;

        let output = self
            .executor
            .exec(&["bookmark", "list", "--all", "-T", template])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to list bookmarks: {}",
                output.stderr
            )));
        }

        let mut bookmarks = Vec::new();

        for line in output.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 2 {
                continue;
            }

            let name = parts[0].trim().to_string();
            let change_id = parts[1].trim().to_string();
            let tracking = if parts.len() > 2 && !parts[2].is_empty() {
                Some(parts[2].trim().to_string())
            } else {
                None
            };

            // Filter by glob pattern if provided
            if let Some(pattern) = glob_pattern {
                if !glob_match(&name, pattern) {
                    continue;
                }
            }

            bookmarks.push(BookmarkInfo {
                name,
                change_id,
                tracking,
            });
        }

        Ok(bookmarks)
    }

    /// Assign a task to an agent by creating a bookmark
    ///
    /// Creates bookmark: `agent/{agent_name}/task/{change_id_prefix}`
    #[instrument(skip(self))]
    pub async fn assign_task(&self, agent_name: &str, change_id: &ChangeId) -> Result<()> {
        validate_identifier(agent_name, "agent_name")?;
        let change_id_prefix = get_change_id_prefix(change_id);
        let bookmark_name = format!("agent/{}/task/{}", agent_name, change_id_prefix);

        debug!("Assigning task {} to agent {}", change_id, agent_name);
        self.create(&bookmark_name, change_id).await
    }

    /// Unassign a task from an agent by deleting the bookmark
    ///
    /// Deletes bookmark: `agent/{agent_name}/task/{change_id_prefix}`
    #[instrument(skip(self))]
    pub async fn unassign_task(&self, agent_name: &str, change_id: &ChangeId) -> Result<()> {
        validate_identifier(agent_name, "agent_name")?;
        let change_id_prefix = get_change_id_prefix(change_id);
        let bookmark_name = format!("agent/{}/task/{}", agent_name, change_id_prefix);

        debug!("Unassigning task {} from agent {}", change_id, agent_name);
        self.delete(&bookmark_name).await
    }

    /// Get all tasks assigned to an agent
    ///
    /// Lists bookmarks matching: `agent/{agent_name}/task/*`
    #[instrument(skip(self))]
    pub async fn agent_tasks(&self, agent_name: &str) -> Result<HashMap<String, ChangeId>> {
        validate_identifier(agent_name, "agent_name")?;
        let pattern = format!("agent/{}/task/*", agent_name);
        let bookmarks = self.list(Some(&pattern)).await?;

        let mut tasks = HashMap::new();
        for bookmark in bookmarks {
            // Extract task ID from bookmark name
            // Format: agent/{name}/task/{task_id}
            if let Some(task_id) = bookmark.name.rsplit('/').next() {
                tasks.insert(task_id.to_string(), bookmark.change_id);
            }
        }

        Ok(tasks)
    }

    /// Find which agent (if any) owns a task
    ///
    /// Reverse lookup: searches for bookmarks matching `agent/*/task/{change_id_prefix}`
    #[instrument(skip(self))]
    pub async fn task_agent(&self, change_id: &ChangeId) -> Result<Option<String>> {
        validate_identifier(change_id, "change_id")?;
        let change_id_prefix = get_change_id_prefix(change_id);
        let pattern = format!("agent/*/task/{}", change_id_prefix);

        let bookmarks = self.list(Some(&pattern)).await?;

        if let Some(bookmark) = bookmarks.first() {
            // Extract agent name from bookmark: agent/{name}/task/{id}
            let parts: Vec<&str> = bookmark.name.split('/').collect();
            if parts.len() >= 2 && parts[0] == "agent" {
                return Ok(Some(parts[1].to_string()));
            }
        }

        Ok(None)
    }

    /// Mark a change as an orchestrator base
    ///
    /// Creates bookmark: `orchestrator/{orch_id}`
    #[instrument(skip(self))]
    pub async fn mark_orchestrator(&self, orch_id: &str, change_id: &ChangeId) -> Result<()> {
        validate_identifier(orch_id, "orchestrator_id")?;
        let bookmark_name = format!("orchestrator/{}", orch_id);
        debug!("Marking orchestrator {} at {}", orch_id, change_id);
        self.create(&bookmark_name, change_id).await
    }

    /// Create a session tracking bookmark
    ///
    /// Creates bookmark: `session/{session_id}`
    #[instrument(skip(self))]
    pub async fn session_bookmark(&self, session_id: &str, change_id: &ChangeId) -> Result<()> {
        validate_identifier(session_id, "session_id")?;
        let bookmark_name = format!("session/{}", session_id);
        debug!("Creating session bookmark {} at {}", session_id, change_id);
        self.create(&bookmark_name, change_id).await
    }

    /// Create a task bookmark
    ///
    /// Creates bookmark: `task/{change_id_prefix}`
    #[instrument(skip(self))]
    pub async fn mark_task(&self, change_id: &ChangeId) -> Result<()> {
        let change_id_prefix = get_change_id_prefix(change_id);
        let bookmark_name = format!("task/{}", change_id_prefix);
        debug!("Marking task {}", change_id);
        self.create(&bookmark_name, change_id).await
    }

    /// List all tasks (bookmarks matching `task/*`)
    #[instrument(skip(self))]
    pub async fn all_tasks(&self) -> Result<Vec<BookmarkInfo>> {
        self.list(Some("task/*")).await
    }

    /// List all orchestrators (bookmarks matching `orchestrator/*`)
    #[instrument(skip(self))]
    pub async fn all_orchestrators(&self) -> Result<Vec<BookmarkInfo>> {
        self.list(Some("orchestrator/*")).await
    }
}

/// Get a stable prefix from a change ID (first 12 chars)
fn get_change_id_prefix(change_id: &str) -> &str {
    if change_id.len() > 12 {
        &change_id[..12]
    } else {
        change_id
    }
}

/// Simple glob matching for bookmark names
/// Supports `*` wildcard only
fn glob_match(text: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return text == pattern;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    // Pattern starts with *
    if pattern.starts_with('*')
        && parts.len() == 2 && parts[0].is_empty() {
            return text.ends_with(parts[1]);
        }

    // Pattern ends with *
    if pattern.ends_with('*')
        && parts.len() == 2 && parts[1].is_empty() {
            return text.starts_with(parts[0]);
        }

    // Pattern has * in middle
    if parts.len() == 2 {
        return text.starts_with(parts[0]) && text.ends_with(parts[1]);
    }

    // More complex patterns - basic implementation
    // For production, consider using a proper glob library
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            // First part must match at start
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            // Last part must match at end
            return text[pos..].ends_with(part);
        } else {
            // Middle parts
            if let Some(idx) = text[pos..].find(part) {
                pos += idx + part.len();
            } else {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    #[test]
    fn test_get_change_id_prefix() {
        assert_eq!(
            get_change_id_prefix("abc123def456ghi789"),
            "abc123def456"
        );
        assert_eq!(get_change_id_prefix("short"), "short");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("agent/foo/task/abc", "agent/*/task/*"));
        assert!(glob_match("agent/foo/task/abc", "agent/foo/*"));
        assert!(glob_match("agent/foo/task/abc", "*/task/*"));
        assert!(glob_match("task/abc123", "task/*"));
        assert!(!glob_match("task/abc123", "agent/*"));
        assert!(glob_match("exact-match", "exact-match"));
    }

    #[tokio::test]
    async fn test_create_bookmark() {
        let executor = MockJjExecutor::new().with_response(
            "bookmark create test-bookmark -r abc123",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = BookmarkManager::new(executor);
        let change_id = "abc123".to_string();
        let result = manager.create("test-bookmark", &change_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assign_task() {
        let executor = MockJjExecutor::new().with_response(
            "bookmark create agent/agent-42/task/abc123def456 -r abc123def456ghi789",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = BookmarkManager::new(executor);
        let change_id = "abc123def456ghi789".to_string();
        let result = manager.assign_task("agent-42", &change_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_bookmarks() {
        let executor = MockJjExecutor::new().with_response(
            r#"bookmark list --all -T name ++ "|" ++ change_id ++ "|" ++ if(tracked, remote_name, "") ++ "\n""#,
            JjOutput {
                stdout: "task/abc123|abc123def456|\nagent/foo/task/xyz|xyz789abc|\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = BookmarkManager::new(executor);
        let bookmarks = manager.list(None).await.unwrap();

        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].name, "task/abc123");
        assert_eq!(bookmarks[0].change_id, "abc123def456");
        assert_eq!(bookmarks[1].name, "agent/foo/task/xyz");
        assert_eq!(bookmarks[1].change_id, "xyz789abc");
    }

    #[tokio::test]
    async fn test_task_agent() {
        let executor = MockJjExecutor::new().with_response(
            r#"bookmark list --all -T name ++ "|" ++ change_id ++ "|" ++ if(tracked, remote_name, "") ++ "\n""#,
            JjOutput {
                stdout: "agent/agent-42/task/abc123def456|abc123def456ghi789|\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = BookmarkManager::new(executor);
        let change_id = "abc123def456ghi789".to_string();
        let agent = manager.task_agent(&change_id).await.unwrap();

        assert_eq!(agent, Some("agent-42".to_string()));
    }

    #[tokio::test]
    async fn test_agent_tasks() {
        let executor = MockJjExecutor::new().with_response(
            r#"bookmark list --all -T name ++ "|" ++ change_id ++ "|" ++ if(tracked, remote_name, "") ++ "\n""#,
            JjOutput {
                stdout: "agent/agent-42/task/abc123|abc123def456|\nagent/agent-42/task/xyz789|xyz789abc123|\n"
                    .to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let manager = BookmarkManager::new(executor);
        let tasks = manager.agent_tasks("agent-42").await.unwrap();

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks.get("abc123"), Some(&"abc123def456".to_string()));
        assert_eq!(tasks.get("xyz789"), Some(&"xyz789abc123".to_string()));
    }
}
