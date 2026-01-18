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

mod auth;
mod circuit_breaker;
mod client;
mod file_executor;
mod types;

pub use auth::get_auth_token;
pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use client::{spawn_agent, AgentClient};
pub use file_executor::{
    execute_file_operations, file_operation_instructions, validate_path, ExecutionResult,
    FileOperation,
};
pub use types::*;
