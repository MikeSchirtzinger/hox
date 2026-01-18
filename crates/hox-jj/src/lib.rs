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

mod command;
mod metadata;
pub mod oplog;
mod revsets;

pub use command::{JjCommand, JjExecutor, JjOutput};
pub use metadata::MetadataManager;
pub use oplog::{OpLogEvent, OpLogWatcher, OpLogWatcherConfig};
pub use revsets::RevsetQueries;
