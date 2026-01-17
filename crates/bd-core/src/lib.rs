//! Core types and traits for the beads issue tracking system.
//!
//! This crate provides the fundamental data structures and interfaces
//! used throughout the beads system.

pub mod error;
pub mod schema;
pub mod types;

pub use error::{Error, Result};
pub use schema::{DepFile, TaskFile};

// Re-export main types for convenience
pub use types::{
    AgentState, BlockedIssue, BondRef, Comment, Dependency, DependencyCounts, DependencyType,
    EntityRef, EpicStatus, Event, EventType, Issue, IssueDetails, IssueType,
    IssueWithCounts, IssueWithDependencyMetadata, Label, MolType, MoleculeProgressStats,
    RequiredSection, Statistics, Status, TreeNode, Validation, ValidationError, WaitsForMeta,
    BOND_TYPE_CONDITIONAL, BOND_TYPE_PARALLEL, BOND_TYPE_ROOT, BOND_TYPE_SEQUENTIAL,
    FAILURE_CLOSE_KEYWORDS, VALIDATION_ACCEPTED, VALIDATION_REJECTED,
    VALIDATION_REVISION_REQUESTED, WAITS_FOR_ALL_CHILDREN, WAITS_FOR_ANY_CHILDREN,
};

// Re-export utility functions
pub use types::is_failure_close;
