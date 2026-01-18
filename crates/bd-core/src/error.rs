//! Error types for the hox orchestration system.
//!
//! This module provides unified error types for the JJ-native task orchestration.

use thiserror::Error;

use crate::types::TaskStatus;

/// Unified error type for hox operations.
#[derive(Error, Debug)]
pub enum HoxError {
    // =========================================================================
    // Task Errors
    // =========================================================================
    /// Task not found by change ID or other identifier.
    #[error("task not found: {0}")]
    TaskNotFound(String),

    /// Invalid status transition attempted.
    #[error("invalid status transition: {from} -> {to}")]
    InvalidStatusTransition { from: TaskStatus, to: TaskStatus },

    /// Validation error for task or other data.
    #[error("validation error: {0}")]
    ValidationError(String),

    // =========================================================================
    // JJ/VCS Errors
    // =========================================================================
    /// JJ command execution failed.
    #[error("jj command failed: {0}")]
    JjError(String),

    /// General VCS error.
    #[error("VCS error: {0}")]
    Vcs(String),

    /// Not in a VCS repository.
    #[error("not in a VCS repository")]
    NotInVcs,

    /// Repository not found at specified path.
    #[error("repository not found at path: {0}")]
    RepoNotFound(String),

    /// Invalid commit or change reference.
    #[error("invalid commit reference: {0}")]
    InvalidCommit(String),

    /// Git-specific error (via gix).
    #[error("git error: {0}")]
    Git(#[from] gix::open::Error),

    /// Git reference error.
    #[error("git reference error: {0}")]
    GitRef(String),

    /// Git traversal error.
    #[error("git traversal error: {0}")]
    GitTraversal(String),

    // =========================================================================
    // Storage/Database Errors
    // =========================================================================
    /// Database operation failed.
    #[error("database error: {0}")]
    DbError(String),

    /// Database error (alias for DbError for backwards compatibility).
    #[error("database error: {0}")]
    Database(String),

    /// Schema validation failed.
    #[error("schema validation error: {0}")]
    SchemaValidation(String),

    // =========================================================================
    // Serialization Errors
    // =========================================================================
    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Parse error for structured data.
    #[error("parse error: {0}")]
    Parse(String),

    // =========================================================================
    // I/O Errors
    // =========================================================================
    /// General I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// File watcher error.
    #[error("file watcher error: {0}")]
    Watcher(String),

    // =========================================================================
    // Dependency/Graph Errors
    // =========================================================================
    /// Dependency cycle detected in task graph.
    #[error("dependency cycle detected")]
    DependencyCycle,

    // =========================================================================
    // Agent/Orchestration Errors
    // =========================================================================
    /// Agent not found.
    #[error("agent not found: {0}")]
    AgentNotFound(String),

    /// Handoff failed.
    #[error("handoff failed: {0}")]
    HandoffFailed(String),

    /// Task already assigned to another agent.
    #[error("task {task_id} already assigned to agent {agent}")]
    TaskAlreadyAssigned { task_id: String, agent: String },
}

impl HoxError {
    /// Create a new task not found error.
    pub fn task_not_found(id: impl Into<String>) -> Self {
        HoxError::TaskNotFound(id.into())
    }

    /// Create a new JJ error.
    pub fn jj_error(msg: impl Into<String>) -> Self {
        HoxError::JjError(msg.into())
    }

    /// Create a new database error.
    pub fn db_error(msg: impl Into<String>) -> Self {
        HoxError::DbError(msg.into())
    }

    /// Create a new validation error.
    pub fn validation(msg: impl Into<String>) -> Self {
        HoxError::ValidationError(msg.into())
    }

    /// Create a new parse error.
    pub fn parse(msg: impl Into<String>) -> Self {
        HoxError::Parse(msg.into())
    }

    /// Returns true if this is a "not found" type error.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            HoxError::TaskNotFound(_) | HoxError::AgentNotFound(_) | HoxError::RepoNotFound(_)
        )
    }

    /// Returns true if this is a validation error.
    pub fn is_validation(&self) -> bool {
        matches!(
            self,
            HoxError::ValidationError(_) | HoxError::SchemaValidation(_)
        )
    }

    /// Returns true if this is a JJ/VCS related error.
    pub fn is_vcs(&self) -> bool {
        matches!(
            self,
            HoxError::JjError(_)
                | HoxError::Vcs(_)
                | HoxError::NotInVcs
                | HoxError::RepoNotFound(_)
                | HoxError::InvalidCommit(_)
                | HoxError::Git(_)
                | HoxError::GitRef(_)
                | HoxError::GitTraversal(_)
        )
    }
}

/// Result type alias using the HoxError type.
pub type Result<T> = std::result::Result<T, HoxError>;

// ============================================================================
// Legacy Error type alias for backwards compatibility
// ============================================================================

/// Legacy error type alias.
/// Prefer using HoxError directly in new code.
pub type Error = HoxError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = HoxError::TaskNotFound("abc123".to_string());
        assert_eq!(err.to_string(), "task not found: abc123");

        let err = HoxError::InvalidStatusTransition {
            from: TaskStatus::Open,
            to: TaskStatus::Done,
        };
        assert_eq!(err.to_string(), "invalid status transition: open -> done");
    }

    #[test]
    fn test_error_constructors() {
        let err = HoxError::task_not_found("abc123");
        assert!(matches!(err, HoxError::TaskNotFound(_)));

        let err = HoxError::jj_error("command failed");
        assert!(matches!(err, HoxError::JjError(_)));

        let err = HoxError::validation("invalid title");
        assert!(matches!(err, HoxError::ValidationError(_)));
    }

    #[test]
    fn test_error_classification() {
        let err = HoxError::TaskNotFound("abc".to_string());
        assert!(err.is_not_found());
        assert!(!err.is_validation());
        assert!(!err.is_vcs());

        let err = HoxError::ValidationError("bad input".to_string());
        assert!(!err.is_not_found());
        assert!(err.is_validation());
        assert!(!err.is_vcs());

        let err = HoxError::JjError("failed".to_string());
        assert!(!err.is_not_found());
        assert!(!err.is_validation());
        assert!(err.is_vcs());
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: HoxError = io_err.into();
        assert!(matches!(err, HoxError::Io(_)));
    }

    #[test]
    fn test_from_json_error() {
        let json_err = serde_json::from_str::<String>("invalid json").unwrap_err();
        let err: HoxError = json_err.into();
        assert!(matches!(err, HoxError::Json(_)));
    }
}
