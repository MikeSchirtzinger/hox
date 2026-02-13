//! # hox-evolution
//!
//! Self-improvement and pattern learning for Hox orchestration.
//!
//! This crate provides:
//! - Pattern capture from successful runs
//! - Pattern storage in hox-patterns branch
//! - Review gate implementation
//! - Pattern loading at startup

#![allow(dead_code)]

mod patterns;
mod review;

pub use patterns::{
    builtin_patterns, AgentPerformance, OrchestrationTrace, Pattern, PatternCategory,
    PatternExtractor, PatternStore, Suggestion, TaskContext,
};
pub use review::{ReviewGate, ReviewResult};
