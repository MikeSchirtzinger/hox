//! # hox-dashboard
//!
//! Terminal-based observability dashboard for Hox orchestration.
//!
//! This crate provides a Ratatui-based TUI for monitoring:
//! - Agent progress and status
//! - JJ operation log
//! - Global metrics (tool calls, success rates, timing)
//! - Visual orchestration graph
//!
//! ## Usage
//!
//! ```bash
//! hox dashboard              # Watch current orchestration
//! hox dashboard --metrics    # Focus on metrics view
//! ```
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  HOX DASHBOARD                           [q]uit [r]efresh   │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ORCHESTRATION PROGRESS                                     │
//! │   Phase 0 ──▶ Phase 1 (Parallel) ──▶ Phase 2               │
//! │   [████]     [▓▓░░] [▓▓▓░] [▓░░░]     [░░░░]               │
//! ├─────────────────────────────────────────────────────────────┤
//! │  GLOBAL METRICS                                             │
//! │  Tool Calls: 1,234    Failures: 12    Success: 99.0%        │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ACTIVE AGENTS                                              │
//! │  │ Agent   │ Task           │ Progress │ Status  │          │
//! ├─────────────────────────────────────────────────────────────┤
//! │  JJ OPLOG                                                   │
//! │  09:15:23  + new commit abc123                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

// Phase 0: Shared types (contracts)
mod state;

pub use state::{
    AgentNode, AgentStatus, DashboardConfig, DashboardState, GlobalMetrics, JjOpType,
    JjOplogEntry, OrchestrationSession, PhaseProgress, PhaseStatus,
};

// Phase 1a: Error types
mod error;

pub use error::{DashboardError, Result};
pub use jj_source::JjDataSource;

// Phase 1 modules (to be implemented by parallel agents)
mod widgets;      // Agent 1b: Ratatui widgets ✓
mod jj_source;    // Agent 1c: JJ oplog data source

pub use widgets::{AgentGraphWidget, AgentTableWidget, EventLogWidget, MetricsPanelWidget};

// Phase 2 modules (integration)
mod app;          // Main application state
mod event;        // Event handling
mod terminal;     // Terminal setup/teardown
mod ui;           // UI layout and rendering
mod run;          // Main run loop

pub use app::{App, TabSelection};
pub use run::run;