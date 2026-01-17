//! Error types for the beads core library.

use thiserror::Error;

/// Core error types for beads operations.
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Issue not found: {0}")]
    IssueNotFound(String),

    #[error("Invalid issue ID: {0}")]
    InvalidIssueId(String),

    #[error("Invalid issue status: {0}")]
    InvalidStatus(String),

    #[error("Dependency cycle detected")]
    DependencyCycle,

    #[error("Schema validation error: {0}")]
    SchemaValidation(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("VCS error: {0}")]
    Vcs(String),

    #[error("Not in a VCS repository")]
    NotInVcs,

    #[error("Repository not found at path: {0}")]
    RepoNotFound(String),

    #[error("Invalid commit reference: {0}")]
    InvalidCommit(String),

    #[error("Git error: {0}")]
    Git(#[from] gix::open::Error),

    #[error("Git reference error: {0}")]
    GitRef(String),

    #[error("Git traversal error: {0}")]
    GitTraversal(String),

    #[error("File watcher error: {0}")]
    Watcher(String),

    #[error("Database error: {0}")]
    Database(String),
}

/// Result type alias using the beads Error type.
pub type Result<T> = std::result::Result<T, Error>;
