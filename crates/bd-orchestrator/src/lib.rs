//! bd-orchestrator: jj-native task and agent orchestration.
//!
//! This crate provides task management using jj's native change DAG as the task graph.
//! Instead of maintaining a separate dependency graph in a database, this leverages:
//! - Tasks are changes
//! - Dependencies are ancestry
//! - Assignments are bookmarks
//!
//! This approach provides version control for task state, natural dependency tracking,
//! and seamless integration with development workflows.
//!
//! # Core Concepts
//!
//! - **Task**: A work item tracked as a jj change
//! - **Priority**: Task priority levels (Critical to Backlog)
//! - **TaskStatus**: Current state of a task (Pending, InProgress, Blocked, Completed)
//! - **HandoffContext**: State information for seamless agent handoffs
//! - **TaskMetadata**: Non-DAG metadata stored separately in .tasks/metadata.jsonl
//!
//! # Modules
//!
//! - [`types`]: Core data structures (Task, Priority, TaskStatus, HandoffContext)
//! - [`jj`]: JJ command execution abstraction
//! - [`revsets`]: Revset query helpers for task discovery
//! - [`handoff`]: Agent handoff context generation
//! - [`task`]: Task management and metadata storage

pub mod handoff;
pub mod jj;
pub mod revsets;
pub mod task;
pub mod types;

// Re-export commonly used types from bd-core
pub use bd_core::{
    AgentHandoff, ChangeEntry, HandoffContext, HoxError, Priority, Result, Task, TaskMetadata,
    TaskStatus,
};

// Re-export orchestrator-specific types and modules
pub use handoff::HandoffGenerator;
pub use jj::{JjCommand, JjExecutor};
pub use revsets::RevsetQueries;
pub use task::{MetadataStore, TaskManager};
pub use types::HandoffSummary;
