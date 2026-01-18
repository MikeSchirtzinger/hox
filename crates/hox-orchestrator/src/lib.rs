//! # hox-orchestrator
//!
//! Multi-agent orchestration engine for Hox.
//!
//! This crate provides:
//! - Orchestrator implementation with workspace management
//! - Phase-based task decomposition
//! - Agent spawning and coordination
//! - Communication protocol handling
//! - Ralph-style loop engine for autonomous iteration

#![allow(dead_code)]

mod backpressure;
mod communication;
mod loop_engine;
mod orchestrator;
mod phases;
mod prompt;
mod workspace;

pub use backpressure::{format_errors_for_prompt, run_all_checks};
pub use communication::{Message, MessageRouter};
pub use loop_engine::LoopEngine;
pub use orchestrator::{Orchestrator, OrchestratorConfig, OrchestratorState};
pub use phases::{PhaseManager, PhaseStatus};
pub use prompt::{build_iteration_prompt, build_simple_prompt, parse_context_update};
pub use workspace::WorkspaceManager;
