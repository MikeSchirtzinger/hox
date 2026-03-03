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

pub mod batch_metadata;
mod bookmarks;
mod command;
mod dag;
mod metadata;
pub mod oplog;
pub mod repo_mode;
mod revsets;
mod validate;

#[cfg(feature = "jj-lib-integration")]
pub mod lib_backend;

pub use batch_metadata::MetadataBatch;
pub use bookmarks::{BookmarkInfo, BookmarkManager};
pub use command::{JjCommand, JjExecutor, JjOutput, MockJjExecutor};
pub use dag::{AbsorbResult, DagOperations, EvolutionEntry, ParallelizeResult, SplitResult};
pub use metadata::MetadataManager;
pub use oplog::{OpLogEvent, OpLogWatcher, OpLogWatcherConfig, OpManager, OperationInfo};
pub use revsets::RevsetQueries;
pub use repo_mode::{detect_fork_features, detect_repo_mode, ForkFeatures, RepoMode};
pub use validate::{validate_identifier, validate_path, validate_revset};

#[cfg(feature = "jj-lib-integration")]
pub use lib_backend::JjLibExecutor;
