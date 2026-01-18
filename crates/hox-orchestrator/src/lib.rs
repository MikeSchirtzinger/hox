//! # hox-orchestrator
//!
//! Multi-agent orchestration engine for Hox.
//!
//! This crate provides:
//! - Orchestrator implementation with workspace management
//! - Phase-based task decomposition
//! - Agent spawning and coordination
//! - Communication protocol handling

#![allow(dead_code)]

mod communication;
mod orchestrator;
mod phases;
mod workspace;

pub use communication::{Message, MessageRouter};
pub use orchestrator::{Orchestrator, OrchestratorConfig, OrchestratorState};
pub use phases::{PhaseManager, PhaseStatus};
pub use workspace::WorkspaceManager;
