//! # hox-agent
//!
//! Anthropic API client and file executor for Hox orchestration.
//!
//! This crate implements the Ralph-style loop pattern:
//! - Fresh agent spawning (no conversation history)
//! - Circuit breaker for rate limit protection
//! - XML-based file operations parsed from agent output
//!
//! ## Key Pattern
//!
//! Each iteration spawns a completely fresh agent. State comes from:
//! - JJ change descriptions (HandoffContext, metadata)
//! - Backpressure errors (test/lint/build failures)
//! - Previous iteration logs (from `jj log`)
//!
//! This prevents context compaction/drift that plagues long-running agents.

pub mod artifact_manager;
mod auth;
mod circuit_breaker;
mod client;
mod file_executor;
mod promise;
mod types;

pub use artifact_manager::{
    artifact_capture_instructions, capture_screenshot_cdp, ArtifactManager, ArtifactType,
    ValidationArtifact,
};
pub use auth::get_auth_token;
pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use client::{spawn_agent, AgentClient};
pub use file_executor::{
    execute_file_operations, file_operation_instructions, validate_path, ExecutionResult,
    FileOperation,
};
pub use promise::CompletionPromise;
pub use types::*;
