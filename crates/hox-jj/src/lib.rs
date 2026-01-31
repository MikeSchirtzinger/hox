//! # hox-jj
//!
//! JJ integration layer for Hox orchestration.
//!
//! This crate provides:
//! - JJ command execution abstraction
//! - Hox metadata read/write operations
//! - Revset query helpers
//! - Operation log watching

#![allow(dead_code)]

mod bookmarks;
mod command;
mod dag;
mod metadata;
pub mod oplog;
mod revsets;

pub use bookmarks::{BookmarkInfo, BookmarkManager};
pub use command::{JjCommand, JjExecutor, JjOutput, MockJjExecutor};
pub use dag::{AbsorbResult, DagOperations, EvolutionEntry, ParallelizeResult, SplitResult};
pub use metadata::MetadataManager;
pub use oplog::{OpLogEvent, OpLogWatcher, OpLogWatcherConfig, OpManager, OperationInfo};
pub use revsets::RevsetQueries;
