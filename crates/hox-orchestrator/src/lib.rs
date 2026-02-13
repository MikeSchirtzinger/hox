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

mod activity_logger;
mod backpressure;
mod communication;
mod conflict_resolver;
mod hooks;
mod loop_engine;
mod loop_external;
mod orchestrator;
mod phases;
mod prompt;
mod recovery;
mod speculative;
mod state_machine;
mod workspace;

pub use activity_logger::ActivityLogger;
pub use backpressure::{
    detect_checks, format_errors_for_prompt, run_all_checks, run_checks, run_failed_checks,
    CheckCommand,
};
pub use communication::{Message, MessageRouter};
pub use conflict_resolver::{
    ConflictInfo, ConflictResolver, ConflictSide, ResolutionReport, ResolutionStrategy,
};
pub use hooks::{AutoCommitHook, HookContext, HookPipeline, HookResult, PostToolsHook, SnapshotHook};
pub use loop_engine::LoopEngine;
pub use loop_external::{
    create_initial_state, load_state, run_external_iteration, save_state, ExternalIterationConfig,
};
pub use orchestrator::{Orchestrator, OrchestratorConfig, OrchestratorState};
pub use phases::{PhaseManager, PhaseStatus};
pub use prompt::{build_iteration_prompt, build_simple_prompt, parse_context_update};
pub use recovery::{RecoveryManager, RecoveryPoint, RollbackResult};
pub use speculative::SpeculativeExecutor;
pub use state_machine::{transition, Action, Event, State};
pub use workspace::WorkspaceManager;
