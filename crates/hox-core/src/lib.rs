//! # hox-core
//!
//! Core types for the Hox JJ-native orchestration system.
//!
//! Hox is inspired by Hox genes - the master regulatory genes that control body structure.
//! Like biological Hox genes, orchestrator decisions shape the structure of work,
//! and agents differentiate within that structure but cannot override it.
//!
//! ## Core Paradigm
//!
//! - Tasks ARE jj changes (change IDs are primary identifiers)
//! - Dependencies ARE DAG ancestry (no separate dependency graph)
//! - Assignments ARE bookmarks
//! - Shared context IS workspace inheritance
//! - Communication IS first-class metadata

#![allow(dead_code)]

mod error;
mod types;

pub use error::{HoxError, Result};
pub use types::*;
