//! BD Storage - Database layer for jj-beads-rs
//!
//! This crate provides libSQL database integration for the jj-beads-rs project,
//! implementing the query cache layer for the jj-turso architecture.
//!
//! # Overview
//!
//! The database uses libSQL (Turso's production-ready SQLite fork) with:
//! - WAL mode for concurrent reads during writes
//! - Connection pooling for performance
//! - Full-text search capabilities
//! - Schema versioning and migrations
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │         Application Layer                   │
//! │  (CLI, Daemon, API)                         │
//! └─────────────────┬───────────────────────────┘
//!                   │
//! ┌─────────────────▼───────────────────────────┐
//! │         BD Storage (this crate)             │
//! │  • Database struct                          │
//! │  • CRUD operations                          │
//! │  • Query helpers                            │
//! │  • Schema management                        │
//! └─────────────────┬───────────────────────────┘
//!                   │
//! ┌─────────────────▼───────────────────────────┐
//! │         LibSQL Database                     │
//! │  • .beads/turso.db                          │
//! │  • WAL mode                                 │
//! │  • Tables: tasks, deps, blocked_cache       │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Example Usage
//!
//! ```no_run
//! use bd_storage::db::{Database, ReadyTasksOptions};
//! use bd_core::TaskFile;
//! use chrono::Utc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Open database connection
//! let db = Database::open(".beads/turso.db").await?;
//! db.init_schema().await?;
//!
//! // Create a task
//! let task = TaskFile {
//!     id: "task-001".to_string(),
//!     title: "Implement feature X".to_string(),
//!     description: Some("Add new feature".to_string()),
//!     task_type: "feature".to_string(),
//!     status: "open".to_string(),
//!     priority: 1,
//!     assigned_agent: Some("agent-123".to_string()),
//!     tags: vec!["backend".to_string()],
//!     created_at: Utc::now(),
//!     updated_at: Utc::now(),
//!     due_at: None,
//!     defer_until: None,
//! };
//!
//! // Insert task
//! db.upsert_task(&task).await?;
//!
//! // Query ready tasks
//! let ready_tasks = db.get_ready_tasks(ReadyTasksOptions {
//!     include_deferred: false,
//!     limit: 10,
//!     assigned_agent: None,
//! }).await?;
//!
//! println!("Found {} ready tasks", ready_tasks.len());
//! # Ok(())
//! # }
//! ```

pub mod db;
pub mod dep_io;
pub mod sync;
pub mod task_io;

// Re-export commonly used types
pub use db::{Database, DbError, ListTasksFilter, ReadyTasksOptions, Result};

// Re-export dep_io functions for convenience
pub use dep_io::{
    delete_dep_file, find_deps_for_task, read_all_dep_files, read_dep_file, write_dep_file,
};

// Re-export sync types for convenience
pub use sync::{ExportStats, SyncManager, SyncStats};

// Re-export task_io functions for convenience
pub use task_io::{delete_task_file, read_all_task_files, read_task_file, write_task_file};
