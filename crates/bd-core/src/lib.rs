// Allow result_large_err at crate level: the Git error variant contains
// gix::open::Error which is 128+ bytes. Boxing would require invasive changes
// throughout the codebase. This is idiomatic for external library error types.
#![allow(clippy::result_large_err)]

//! Core types and traits for the hox JJ-native task orchestration system.
//!
//! This crate provides the unified type system for JJ-native orchestration:
//! - Tasks ARE jj changes
//! - Dependencies ARE ancestry in the jj DAG
//! - Assignments ARE bookmarks
//!
//! # Key Types
//!
//! - [`Task`] - Core task representation tied to a jj change
//! - [`TaskStatus`] - Workflow states (Open, InProgress, Blocked, etc.)
//! - [`Priority`] - Task priority levels (Critical, High, Medium, Low)
//! - [`HandoffContext`] - Agent state for seamless transitions
//! - [`AgentHandoff`] - Complete handoff package for agent takeover
//! - [`HoxError`] - Unified error type for all operations
//!
//! # Example
//!
//! ```rust
//! use bd_core::{Task, TaskStatus, Priority, HandoffContext};
//!
//! // Create a new task
//! let mut task = Task::new("abc123", "Implement feature X");
//! task.status = TaskStatus::InProgress;
//! task.priority = Priority::High;
//! task.agent = Some("agent-1".to_string());
//!
//! // Add handoff context
//! let mut ctx = HandoffContext::new("Working on the parser");
//! ctx.add_progress("Completed lexer");
//! ctx.add_next_step("Implement AST");
//! task.context = Some(ctx);
//!
//! // Format for jj change description
//! let description = task.format_description();
//! ```

pub mod error;
pub mod schema;
pub mod types;

// ============================================================================
// Primary exports - the unified JJ-native types
// ============================================================================

pub use error::{Error, HoxError, Result};

pub use types::{
    // Core task types
    AgentHandoff,
    ChangeEntry,
    HandoffContext,
    Priority,
    Task,
    TaskMetadata,
    TaskStatus,
};

// ============================================================================
// Schema exports - file format types
// ============================================================================

pub use schema::{DepFile, TaskFile};
