//! Unified error types for Hox

use thiserror::Error;

/// Unified error type for all Hox operations
#[derive(Error, Debug)]
pub enum HoxError {
    // JJ errors
    #[error("JJ command failed: {0}")]
    JjCommand(String),

    #[error("JJ workspace error: {0}")]
    JjWorkspace(String),

    #[error("JJ revset error: {0}")]
    JjRevset(String),

    #[error("Change not found: {0}")]
    ChangeNotFound(String),

    // Orchestrator errors
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    #[error("Invalid orchestrator name: {0}")]
    InvalidOrchestratorName(String),

    #[error("Phase error: {0}")]
    Phase(String),

    // Agent errors
    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    // Communication errors
    #[error("Message routing error: {0}")]
    MessageRouting(String),

    #[error("Invalid message target: {0}")]
    InvalidMessageTarget(String),

    // Conflict errors
    #[error("Mutation conflict: {0}")]
    MutationConflict(String),

    #[error("Merge conflict: {0}")]
    MergeConflict(String),

    // Validation errors
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Consensus not reached: {0}")]
    ConsensusNotReached(String),

    // Metrics errors
    #[error("Metrics error: {0}")]
    Metrics(String),

    // Evolution errors
    #[error("Pattern error: {0}")]
    Pattern(String),

    // API errors
    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("API rate limit: {0}")]
    ApiLimit(String),

    // Path validation errors
    #[error("Path validation failed: {0}")]
    PathValidation(String),

    // I/O errors
    #[error("I/O error: {0}")]
    Io(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // Generic
    #[error("{0}")]
    Other(String),
}

/// Result type alias using HoxError
pub type Result<T> = std::result::Result<T, HoxError>;
