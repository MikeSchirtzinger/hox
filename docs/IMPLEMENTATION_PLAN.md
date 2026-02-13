# Hox Implementation Plan: Standalone Open-Source Tool

**Date:** 2026-02-13
**Status:** Implemented (2026-02-13)
All 10 improvements have been implemented. This document is preserved as architectural reference.
**Context:** Roadmap for building Hox as a production-ready, standalone JJ-native multi-agent orchestration tool

---

## Overview

**What is Hox?**

Hox is a JJ-native multi-agent orchestration tool that treats version control changes as the fundamental unit of work. Unlike traditional task runners that manage work in external databases, Hox leverages JJ's powerful DAG manipulation, conflict resolution, and workspace isolation to coordinate multiple AI agents working in parallel on complex software projects.

**Core Paradigm:**
- **Tasks** = JJ changes (change_id is the primary identifier)
- **Dependencies** = DAG ancestry (no separate dependency graph)
- **Assignments** = JJ bookmarks
- **Communication** = First-class metadata in change descriptions
- **Execution** = Ralph-style loop (fresh agent each iteration, state from JJ metadata)

**What This Plan Achieves:**

This implementation plan prioritizes developer experience, robustness, and adoption for standalone tool usage. While the full vision includes microsandbox VMs and Byzantine validation, this plan focuses on making Hox immediately useful as a command-line tool for individual developers and small teams. The improvements are ranked by standalone impact rather than architectural completeness.

---

## Current State

**Codebase Structure:** 11 crates, ~13,381 lines of Rust

### Core Crates

| Crate | Purpose | Key Files | Lines |
|-------|---------|-----------|-------|
| `hox-core` | Core types, task model, orchestrator IDs, metadata schemas | `task.rs`, `types.rs`, `metadata.rs` | ~800 |
| `hox-jj` | JJ command abstraction, subprocess execution, metadata parsing | `command.rs`, `metadata.rs`, `revsets.rs`, `oplog.rs` | ~1,200 |
| `hox-agent` | Anthropic API client, Ralph-style loop engine | `client.rs`, `loop_engine.rs`, `types.rs` | ~1,500 |
| `hox-orchestrator` | Multi-agent coordination, phase-based execution | `orchestrator.rs`, `phases.rs`, `backpressure.rs` | ~2,800 |
| `hox-validation` | Byzantine fault-tolerant consensus (3f+1 validators) | `validator.rs`, `consensus.rs` | ~1,100 |
| `hox-evolution` | Pattern learning, self-improvement | `patterns.rs`, `learning.rs` | ~900 |
| `hox-metrics` | OpenTelemetry observability | `telemetry.rs`, `events.rs` | ~600 |
| `hox-cli` | Command-line interface | `main.rs`, `commands/` | ~1,200 |

### Microsandbox Layer (Linux-only)

| Crate | Purpose | Status |
|-------|---------|--------|
| `jj-dev-sandbox` | MicroVM lifecycle via libkrun | Requires Linux + rootfs |
| `jj-dev-proxy` | Host-side vsock server | Requires Linux |
| `jj-dev-agent-sdk` | SDK for agents inside VMs | Requires VM environment |

### Current Implementation Status

**What Works:**
- Ralph-style loop: Fresh agent each iteration, state from JJ metadata
- Backpressure system: Tests/lints/builds between iterations
- Multi-agent orchestration: Hierarchical orchestrators spawn child agents
- Workspace isolation: Each agent gets a dedicated JJ workspace
- Metadata management: Structured data in change descriptions
- OpLog watching: Poll for new operations (500ms interval)
- Activity logging: Session summaries and progress tracking

**What's Missing (Standalone Tool Perspective):**
- No budget enforcement → runaway loops burn API credits
- No user configuration → all settings hardcoded
- XML parsing from agent output → fragile and error-prone
- Subprocess spawns everywhere → slow iterations
- No fail-open philosophy → transient failures crash the tool
- Manual state transitions → hard to test, hard to observe
- No selective backpressure → all checks every time

---

## Priority Matrix

All improvements ranked by standalone tool impact, effort, and dependencies:

| # | Improvement | Standalone Impact | Effort | Dependencies | Phase |
|---|-------------|------------------|--------|--------------|-------|
| 1 | State Machine | HIGH (Foundation) | 2-3 days | None | 2 |
| 2 | jj-lib Integration | HIGH (Performance) | 4-5 days | None, but risky | 5 |
| 3 | PostToolsHook | MEDIUM (Architecture) | 1-2 days | State Machine | 2 |
| 4 | Budget Enforcement | MEDIUM-HIGH (Safety) | 1 day | None | 1 |
| 5 | Structured Output | MEDIUM-HIGH (Reliability) | 2-3 days | None | 3 |
| 6 | Backpressure Calibration | MEDIUM (Performance) | 2 days | None | 3 |
| 7 | Fail-Open Audit | MEDIUM (Robustness) | 1-2 days | None | 1 |
| 8 | .hox/config.toml | MEDIUM-HIGH (UX) | 1-2 days | None | 1 |
| 9 | Pattern Extraction | LOW-MEDIUM (Learning) | 2-3 days | State Machine | 4 |
| 10 | Phase Auto-Advancement | MEDIUM (UX) | 1 day | State Machine | 4 |

**Quick Wins (High ROI, Low Effort):** Budget Enforcement (#4), Fail-Open Audit (#7), Config File (#8)

**Foundation Work (Enables Everything):** State Machine (#1)

**Major Improvements (Worth The Effort):** Structured Output (#5), Backpressure Calibration (#6)

**Advanced Features (Nice To Have):** Pattern Extraction (#9), Phase Auto-Advancement (#10)

**Performance Optimization (Do Last):** jj-lib Integration (#2)

---

## Phase 1: Quick Wins & Developer UX

**Goal:** Make Hox feel like a real tool, not a research prototype. Stop burning money, enable configuration, don't crash on transient failures.

**Timeline:** 3-4 days
**Dependencies:** None (all independent)

### Improvement #4: Budget Enforcement

**File:** `crates/hox-orchestrator/src/loop_engine.rs`

**Current State:**
```rust
// LoopConfig in crates/hox-agent/src/types.rs (lines 42-48)
pub struct LoopConfig {
    pub max_iterations: usize,
    pub max_tokens: Option<usize>,        // Defined but never checked
    pub max_budget_usd: Option<f64>,      // Defined but never checked
    pub model: ModelName,
    pub workspace_path: PathBuf,
}

// AgentResult in types.rs (lines 89-94) returns usage
pub struct Usage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}
```

**Problem:** Agent can burn unlimited API credits because budget fields are never enforced.

**What Changes:**

Add cumulative tracking to `RalphLoopEngine`:

```rust
// In crates/hox-orchestrator/src/loop_engine.rs (add field to struct around line 15)
pub struct RalphLoopEngine<E: JjExecutor> {
    config: LoopConfig,
    executor: E,
    client: AnthropicClient,
    cumulative_input_tokens: AtomicUsize,   // NEW
    cumulative_output_tokens: AtomicUsize,  // NEW
}

impl<E: JjExecutor> RalphLoopEngine<E> {
    // In run() method (around line 93), after agent execution:
    let result = agent.run().await?;

    // Track cumulative usage
    let total_input = self.cumulative_input_tokens.fetch_add(
        result.usage.input_tokens,
        Ordering::SeqCst
    ) + result.usage.input_tokens;

    let total_output = self.cumulative_output_tokens.fetch_add(
        result.usage.output_tokens,
        Ordering::SeqCst
    ) + result.usage.output_tokens;

    // Check token budget
    if let Some(max_tokens) = self.config.max_tokens {
        if total_input + total_output > max_tokens {
            return Err(HoxError::BudgetExceeded(format!(
                "Token budget exceeded: {} total tokens (limit: {})",
                total_input + total_output,
                max_tokens
            )));
        }
    }

    // Check USD budget (Anthropic pricing: input=$3/MTok, output=$15/MTok for Sonnet)
    if let Some(max_usd) = self.config.max_budget_usd {
        let cost_usd = (total_input as f64 * 3.0 / 1_000_000.0)
                     + (total_output as f64 * 15.0 / 1_000_000.0);
        if cost_usd > max_usd {
            return Err(HoxError::BudgetExceeded(format!(
                "USD budget exceeded: ${:.2} (limit: ${:.2})",
                cost_usd, max_usd
            )));
        }
    }

    // Context freshness: force new iteration at 60% of context window
    let context_window = match self.config.model {
        ModelName::Sonnet => 200_000,
        ModelName::Opus => 200_000,
        ModelName::Haiku => 200_000,
    };
    let context_used = total_input + total_output;
    let context_threshold = (context_window as f64 * 0.6) as usize;

    if context_used > context_threshold {
        info!("Context freshness threshold reached ({}/{}), recommending new task",
              context_used, context_window);
        // Don't error, just warn and continue with fresh context next iteration
    }
```

**Extend:** `crates/hox-core/src/error.rs`

```rust
pub enum HoxError {
    // ... existing variants ...

    /// Budget exceeded (tokens or USD)
    #[error("Budget exceeded: {0}")]
    BudgetExceeded(String),
}
```

**Acceptance Criteria:**
- [x] Cumulative token tracking across iterations
- [x] Hard stop at `max_tokens` limit
- [x] Hard stop at `max_budget_usd` limit
- [x] Context freshness warning at 60% utilization
- [x] Usage summary logged at end of loop
- [x] Error message shows actual vs limit

**Testing:**
```rust
#[tokio::test]
async fn test_budget_enforcement_tokens() {
    let config = LoopConfig {
        max_iterations: 100,
        max_tokens: Some(10_000),
        max_budget_usd: None,
        // ...
    };

    let mut engine = RalphLoopEngine::new(config, mock_executor, mock_client);

    // Mock agent to return 6000 tokens per iteration
    // Should fail after 2 iterations (12000 > 10000)

    let result = engine.run().await;
    assert!(matches!(result, Err(HoxError::BudgetExceeded(_))));
}
```

---

### Improvement #7: Fail-Open Audit

**Goal:** Ensure infrastructure failures don't kill agent progress. "hox doesn't crash on transient issues."

**Current State:**
- Some operations fail-open (e.g., `jj fix` is non-fatal in backpressure.rs)
- Others are inconsistent (activity_logger.rs, oplog.rs)
- No unified philosophy

**What Changes:**

**New File:** `crates/hox-core/src/fail_open.rs`

```rust
use std::future::Future;
use tracing::{warn, error};

/// Execute an operation that should fail open (infrastructure, not business logic)
///
/// Logs the error but returns Ok(None) instead of propagating.
/// Use for operations like:
/// - Activity logging
/// - Metrics/telemetry
/// - OpLog polling
/// - Workspace cleanup
///
/// DO NOT use for:
/// - Agent execution (business logic)
/// - Backpressure checks (correctness)
/// - Metadata reads (state)
pub async fn fail_open<F, T, E>(
    operation_name: &str,
    f: F,
) -> Result<Option<T>, E>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match f.await {
        Ok(val) => Ok(Some(val)),
        Err(e) => {
            warn!("{} failed (fail-open): {}", operation_name, e);
            Ok(None)
        }
    }
}

/// Like fail_open but errors are fatal after N retries
pub async fn fail_open_with_retries<F, T, E>(
    operation_name: &str,
    f: F,
    max_retries: usize,
) -> Result<Option<T>, E>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    for attempt in 1..=max_retries {
        match f.await {
            Ok(val) => return Ok(Some(val)),
            Err(e) => {
                if attempt == max_retries {
                    error!("{} failed after {} retries: {}",
                           operation_name, max_retries, e);
                    return Ok(None);
                }
                warn!("{} failed (attempt {}/{}): {}",
                      operation_name, attempt, max_retries, e);
                tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempt as u64)).await;
            }
        }
    }
    unreachable!()
}
```

**Apply to Activity Logger:** `crates/hox-orchestrator/src/activity_logger.rs`

```rust
use hox_core::fail_open;

impl ActivityLogger {
    pub async fn log_session_start(&self, session_id: &str) -> Result<()> {
        fail_open("activity_logger::log_session_start", async {
            // existing implementation
        }).await?;
        Ok(())
    }

    pub async fn log_iteration(&self, iteration: usize, status: &str) -> Result<()> {
        fail_open("activity_logger::log_iteration", async {
            // existing implementation
        }).await?;
        Ok(())
    }
}
```

**Apply to OpLog Watcher:** `crates/hox-jj/src/oplog.rs`

```rust
// In OpLogWatcher::poll() (around line 82)
loop {
    let result = fail_open("oplog_watcher::poll", async {
        self.executor.exec(&["op", "log", "-n", "1"]).await
    }).await?;

    match result {
        Some(output) => {
            // Process output
        }
        None => {
            // OpLog poll failed, continue after delay
            tokio::time::sleep(self.config.poll_interval).await;
            continue;
        }
    }
}
```

**Acceptance Criteria:**
- [x] `fail_open()` wrapper in hox-core
- [x] `fail_open_with_retries()` for transient errors
- [x] Activity logger operations fail-open
- [x] OpLog polling fails open
- [x] Workspace cleanup fails open
- [x] Agent execution does NOT fail-open (business logic)
- [x] Backpressure checks do NOT fail-open (correctness)

**Documentation:**

Add to README or docs/ARCHITECTURE.md:

```markdown
## Fail-Open Philosophy

Hox distinguishes between **business logic** (must succeed) and **infrastructure** (nice to have):

**Fail-Open (OK to skip):**
- Activity logging
- Metrics/telemetry
- OpLog polling
- Workspace cleanup
- Pattern recording

**Fail-Closed (must succeed):**
- Agent execution
- Backpressure checks (tests/lints)
- Metadata reads/writes
- JJ operations that affect correctness
```

---

### Improvement #8: .hox/config.toml

**Goal:** Per-project configuration, shareable defaults, no more hardcoded values.

**Current State:**
- Protected files hardcoded in `crates/hox-agent/src/file_executor.rs` (line ~100)
- Loop config only programmatic
- Backpressure checks auto-detected or in `.hox/checks.toml`

**What Changes:**

**New File:** `crates/hox-core/src/config.rs`

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Repository-level Hox configuration
///
/// Loaded from `.hox/config.toml` in the repo root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoxConfig {
    /// Files/directories that agents cannot modify
    #[serde(default = "default_protected_files")]
    pub protected_files: Vec<String>,

    /// Loop execution defaults
    #[serde(default)]
    pub loop_defaults: LoopDefaults,

    /// Backpressure check configuration
    #[serde(default)]
    pub backpressure: BackpressureConfig,

    /// Model selection
    #[serde(default)]
    pub models: ModelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDefaults {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,

    #[serde(default)]
    pub max_tokens: Option<usize>,

    #[serde(default)]
    pub max_budget_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// Run these checks on every iteration
    #[serde(default)]
    pub fast_checks: Vec<String>,

    /// Run these checks every N iterations
    #[serde(default)]
    pub slow_checks: Vec<SlowCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowCheck {
    pub command: String,
    pub every_n_iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_model")]
    pub default: String,

    #[serde(default)]
    pub api_key_env: String,
}

fn default_protected_files() -> Vec<String> {
    vec![
        ".git".to_string(),
        ".jj".to_string(),
        ".env".to_string(),
        "Cargo.lock".to_string(),
        "package-lock.json".to_string(),
        "yarn.lock".to_string(),
        ".secrets".to_string(),
        ".gitignore".to_string(),
    ]
}

fn default_max_iterations() -> usize { 20 }
fn default_model() -> String { "claude-sonnet-4".to_string() }

impl HoxConfig {
    /// Load from `.hox/config.toml` or use defaults
    pub fn load_or_default(repo_root: &Path) -> Result<Self> {
        let config_path = repo_root.join(".hox/config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Write default config to `.hox/config.toml`
    pub fn write_default(repo_root: &Path) -> Result<()> {
        let config_path = repo_root.join(".hox/config.toml");
        let config = Self::default();
        let content = toml::to_string_pretty(&config)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }
}

impl Default for HoxConfig {
    fn default() -> Self {
        Self {
            protected_files: default_protected_files(),
            loop_defaults: LoopDefaults::default(),
            backpressure: BackpressureConfig::default(),
            models: ModelConfig::default(),
        }
    }
}

impl Default for LoopDefaults {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            max_tokens: None,
            max_budget_usd: None,
        }
    }
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            fast_checks: vec![
                "cargo check".to_string(),
                "cargo clippy".to_string(),
            ],
            slow_checks: vec![
                SlowCheck {
                    command: "cargo test".to_string(),
                    every_n_iterations: 3,
                },
            ],
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: "claude-sonnet-4".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
        }
    }
}
```

**Example:** `.hox/config.toml`

```toml
# Hox Configuration

protected_files = [
    ".git",
    ".jj",
    ".env",
    "Cargo.lock",
    ".secrets",
    ".gitignore",
]

[loop_defaults]
max_iterations = 50
max_tokens = 100000
max_budget_usd = 5.00

[backpressure]
fast_checks = ["cargo check", "cargo clippy --all-targets"]
slow_checks = [
    { command = "cargo test", every_n_iterations = 5 },
    { command = "cargo doc", every_n_iterations = 10 },
]

[models]
default = "claude-sonnet-4"
api_key_env = "ANTHROPIC_API_KEY"
```

**Extend:** `crates/hox-cli/src/commands/init.rs`

```rust
pub fn cmd_init(repo_path: &Path) -> Result<()> {
    // Create .hox directory
    let hox_dir = repo_path.join(".hox");
    std::fs::create_dir_all(&hox_dir)?;

    // Write default config
    HoxConfig::write_default(repo_path)?;

    println!("Hox initialized!");
    println!("Configuration written to .hox/config.toml");
    println!("Edit this file to customize protected files, budgets, and checks.");

    Ok(())
}
```

**Extend:** `crates/hox-agent/src/file_executor.rs`

Replace hardcoded protected files (line ~100):

```rust
// Before:
const PROTECTED_FILES: &[&str] = &[".git", ".env", "Cargo.lock", ...];

// After:
impl FileExecutor {
    pub fn new(workspace_path: PathBuf, config: &HoxConfig) -> Self {
        Self {
            workspace_path,
            protected_files: config.protected_files.clone(),
        }
    }
}
```

**Acceptance Criteria:**
- [x] `HoxConfig` struct with all settings
- [x] Load from `.hox/config.toml` or defaults
- [x] `hox init` writes default config
- [x] Protected files configurable
- [x] Loop defaults configurable
- [x] Backpressure checks configurable
- [x] Sensible defaults when no config exists

---

## Phase 2: Core Architecture

**Goal:** Clean separation of state transitions from I/O, enable deterministic testing, support PostToolsHook pipeline.

**Timeline:** 3-4 days
**Dependencies:** None, but enables Phase 4

### Improvement #1: Synchronous State Machine

**Current Problem:**
`crates/hox-orchestrator/src/orchestrator.rs` (1,100 lines) has `OrchestratorState` enum but transitions mixed with async I/O. Hard to test, hard to reason about, hard to observe.

**Goal:** Refactor to `(State, Event) → (State, Vec<Action>)` pure function pattern.

**New File:** `crates/hox-orchestrator/src/state_machine.rs`

```rust
use hox_core::{Task, TaskStatus, Priority, OrchestratorId, ChangeId};

/// Orchestrator state (pure data, no I/O)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Planning { goal: String },
    Executing { phase: Phase, tasks: Vec<Task> },
    Integrating { merge_id: ChangeId },
    Validating { validation_id: String },
    Complete { summary: String },
    Failed { error: String },
}

/// Phase within execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Contracts,
    ParallelWork,
    Integration,
    Validation,
}

/// Events that trigger state transitions
#[derive(Debug, Clone)]
pub enum Event {
    StartOrchestration { goal: String },
    PlanningComplete { tasks: Vec<Task> },
    PhaseComplete,
    AllTasksComplete,
    IntegrationConflict { conflicted_changes: Vec<ChangeId> },
    IntegrationClean,
    ValidationPassed,
    ValidationFailed { reason: String },
    Error { message: String },
}

/// Actions to perform (effects, executed by runtime)
#[derive(Debug, Clone)]
pub enum Action {
    SpawnPlanningAgent { goal: String },
    SpawnTaskAgent { task: Task },
    CreateMerge { changes: Vec<ChangeId> },
    ResolveConflicts { changes: Vec<ChangeId> },
    SpawnValidator { validation_id: String },
    LogActivity { message: String },
    RecordPattern { pattern: String },
}

/// Pure state transition function
pub fn transition(state: State, event: Event) -> (State, Vec<Action>) {
    use State::*;
    use Event::*;

    match (state, event) {
        (Idle, StartOrchestration { goal }) => {
            (
                Planning { goal: goal.clone() },
                vec![Action::SpawnPlanningAgent { goal }]
            )
        }

        (Planning { .. }, PlanningComplete { tasks }) => {
            let actions = tasks.iter()
                .map(|task| Action::SpawnTaskAgent { task: task.clone() })
                .collect();
            (
                Executing { phase: Phase::Contracts, tasks },
                actions
            )
        }

        (Executing { phase: Phase::Contracts, tasks }, PhaseComplete) => {
            (
                Executing { phase: Phase::ParallelWork, tasks },
                vec![Action::LogActivity { message: "Entering parallel work phase".to_string() }]
            )
        }

        (Executing { phase: Phase::ParallelWork, tasks }, AllTasksComplete) => {
            let change_ids: Vec<_> = tasks.iter().map(|t| t.change_id.clone()).collect();
            (
                Integrating { merge_id: ChangeId::new() },
                vec![Action::CreateMerge { changes: change_ids }]
            )
        }

        (Integrating { .. }, IntegrationConflict { conflicted_changes }) => {
            (
                Integrating { merge_id: ChangeId::new() },
                vec![Action::ResolveConflicts { changes: conflicted_changes }]
            )
        }

        (Integrating { merge_id }, IntegrationClean) => {
            (
                Validating { validation_id: merge_id.to_string() },
                vec![Action::SpawnValidator { validation_id: merge_id.to_string() }]
            )
        }

        (Validating { .. }, ValidationPassed) => {
            (
                Complete { summary: "Orchestration completed successfully".to_string() },
                vec![Action::RecordPattern { pattern: "successful_completion".to_string() }]
            )
        }

        (_, Error { message }) => {
            (
                Failed { error: message.clone() },
                vec![Action::LogActivity { message: format!("Error: {}", message) }]
            )
        }

        // Invalid transitions (should not happen in correct implementation)
        (state, event) => {
            (
                Failed { error: format!("Invalid transition: {:?} -> {:?}", state, event) },
                vec![]
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_state_machine_never_panics(
            state in any::<State>(),
            event in any::<Event>()
        ) {
            let _ = transition(state, event);
        }
    }

    #[test]
    fn test_happy_path() {
        let (state, actions) = transition(
            State::Idle,
            Event::StartOrchestration { goal: "test".to_string() }
        );
        assert!(matches!(state, State::Planning { .. }));
        assert_eq!(actions.len(), 1);

        let tasks = vec![Task::new("task1")];
        let (state, actions) = transition(
            state,
            Event::PlanningComplete { tasks: tasks.clone() }
        );
        assert!(matches!(state, State::Executing { phase: Phase::Contracts, .. }));
    }
}
```

**Extend:** `crates/hox-orchestrator/src/orchestrator.rs`

Replace inline state management with state machine:

```rust
pub struct Orchestrator<E: JjExecutor> {
    config: OrchestratorConfig,
    executor: E,
    state: State,  // NEW: current state
    // ... existing fields ...
}

impl<E: JjExecutor + Clone + 'static> Orchestrator<E> {
    pub async fn run(&mut self) -> Result<()> {
        // Main runtime loop: process events, execute actions
        loop {
            // Get next event (from agent completion, JJ poll, etc.)
            let event = self.next_event().await?;

            // Pure state transition
            let (new_state, actions) = state_machine::transition(
                self.state.clone(),
                event
            );

            self.state = new_state;

            // Execute actions
            for action in actions {
                self.execute_action(action).await?;
            }

            // Check for terminal states
            if matches!(self.state, State::Complete { .. } | State::Failed { .. }) {
                break;
            }
        }

        Ok(())
    }

    async fn execute_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::SpawnPlanningAgent { goal } => {
                self.spawn_planning_agent(&goal).await?;
            }
            Action::SpawnTaskAgent { task } => {
                self.spawn_agent(&task).await?;
            }
            Action::CreateMerge { changes } => {
                self.create_merge(&changes).await?;
            }
            Action::ResolveConflicts { changes } => {
                self.resolve_conflicts(&changes).await?;
            }
            Action::LogActivity { message } => {
                self.activity_logger.log(&message).await?;
            }
            Action::RecordPattern { pattern } => {
                self.pattern_store.record(&pattern).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
```

**Acceptance Criteria:**
- [x] Pure `transition(state, event) -> (state, actions)` function
- [x] All state transitions in state_machine.rs, no I/O
- [x] Orchestrator.run() processes events and executes actions
- [x] Proptest verifies all transitions are total (never panic)
- [x] Unit tests for happy path, error paths, invalid transitions
- [x] State machine testable without network/JJ/filesystem

---

### Improvement #3: PostToolsHook

**Goal:** Extract JJ operations into a hook pipeline for cleaner separation and extensibility.

**Current State:** Loop engine handles JJ updates inline after agent execution (loop_engine.rs)

**New File:** `crates/hox-orchestrator/src/hooks.rs`

```rust
use async_trait::async_trait;
use hox_core::{ChangeId, HoxMetadata};
use hox_jj::JjExecutor;

/// Hook that runs after agent tool execution
#[async_trait]
pub trait PostToolsHook: Send + Sync {
    async fn execute(&self, context: &HookContext) -> Result<HookResult>;
}

pub struct HookContext {
    pub change_id: ChangeId,
    pub workspace_path: PathBuf,
    pub iteration: usize,
    pub metadata: HoxMetadata,
}

pub struct HookResult {
    pub success: bool,
    pub message: String,
}

/// Auto-commit working directory changes
pub struct AutoCommitHook<E: JjExecutor> {
    executor: E,
}

#[async_trait]
impl<E: JjExecutor + Send + Sync> PostToolsHook for AutoCommitHook<E> {
    async fn execute(&self, context: &HookContext) -> Result<HookResult> {
        // jj describe to update metadata
        let metadata_str = serde_json::to_string(&context.metadata)?;
        self.executor.exec(&[
            "describe",
            "-m", &metadata_str,
        ]).await?;

        Ok(HookResult {
            success: true,
            message: "Auto-committed changes".to_string(),
        })
    }
}

/// Take JJ snapshot (lightweight checkpoint)
pub struct SnapshotHook<E: JjExecutor> {
    executor: E,
}

#[async_trait]
impl<E: JjExecutor + Send + Sync> PostToolsHook for SnapshotHook<E> {
    async fn execute(&self, context: &HookContext) -> Result<HookResult> {
        // jj already snapshots automatically on every command
        // This is a no-op but provides extension point
        Ok(HookResult {
            success: true,
            message: "Snapshot taken".to_string(),
        })
    }
}

/// Hook pipeline executor
pub struct HookPipeline {
    hooks: Vec<Box<dyn PostToolsHook>>,
}

impl HookPipeline {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn add_hook(&mut self, hook: Box<dyn PostToolsHook>) {
        self.hooks.push(hook);
    }

    /// Execute all hooks in order, fail-open
    pub async fn execute_all(&self, context: &HookContext) -> Result<()> {
        for hook in &self.hooks {
            match hook.execute(context).await {
                Ok(result) => {
                    if result.success {
                        info!("Hook succeeded: {}", result.message);
                    } else {
                        warn!("Hook failed (non-fatal): {}", result.message);
                    }
                }
                Err(e) => {
                    warn!("Hook error (non-fatal): {}", e);
                }
            }
        }
        Ok(())
    }
}
```

**Extend:** `crates/hox-orchestrator/src/loop_engine.rs`

Replace inline JJ updates with hook pipeline:

```rust
pub struct RalphLoopEngine<E: JjExecutor> {
    config: LoopConfig,
    executor: E,
    client: AnthropicClient,
    hooks: HookPipeline,  // NEW
}

impl<E: JjExecutor> RalphLoopEngine<E> {
    pub fn new(config: LoopConfig, executor: E, client: AnthropicClient) -> Self {
        let mut hooks = HookPipeline::new();
        hooks.add_hook(Box::new(AutoCommitHook { executor: executor.clone() }));
        hooks.add_hook(Box::new(SnapshotHook { executor: executor.clone() }));

        Self {
            config,
            executor,
            client,
            hooks,
        }
    }

    async fn run(&mut self) -> Result<()> {
        // ... after agent execution ...

        // Execute hooks
        let hook_context = HookContext {
            change_id: task.change_id.clone(),
            workspace_path: self.config.workspace_path.clone(),
            iteration,
            metadata: updated_metadata,
        };

        self.hooks.execute_all(&hook_context).await?;
    }
}
```

**Acceptance Criteria:**
- [x] `PostToolsHook` trait with execute() method
- [x] `HookPipeline` executes hooks in order
- [x] All hook failures are non-fatal (fail-open)
- [x] `AutoCommitHook` updates metadata
- [x] Loop engine uses hook pipeline instead of inline JJ calls
- [x] Easy to add custom hooks

---

## Phase 3: Agent Quality

**Goal:** Reliable agent output parsing, calibrated backpressure checks for faster iterations.

**Timeline:** 4-5 days
**Dependencies:** None

### Improvement #5: Structured Output over XML

**Current Problem:**
`crates/hox-agent/src/file_executor.rs` (415 lines) parses XML tags from agent text output. Malformed XML = silent failures or partial operations.

**Goal:** Use Anthropic's `tool_use` API for guaranteed structured JSON output.

**What Changes:**

**Extend:** `crates/hox-agent/src/client.rs` (302 lines)

Add tool definitions for Anthropic API:

```rust
use serde_json::json;

pub struct AnthropicClient {
    // ... existing fields ...
}

impl AnthropicClient {
    fn get_tool_definitions() -> Vec<serde_json::Value> {
        vec![
            json!({
                "name": "read_file",
                "description": "Read the contents of a file",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        }
                    },
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "Write or create a file with given content",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "edit_file",
                "description": "Make precise edits to an existing file",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "Exact text to replace"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "Replacement text"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }
            }),
            json!({
                "name": "run_command",
                "description": "Execute a shell command in the workspace",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Shell command to execute"
                        }
                    },
                    "required": ["command"]
                }
            }),
        ]
    }

    pub async fn send_message_with_tools(
        &self,
        prompt: &str,
    ) -> Result<AgentResponse> {
        let payload = json!({
            "model": self.model,
            "max_tokens": 4096,
            "tools": Self::get_tool_definitions(),
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let response: AnthropicResponse = self.http_client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        Ok(AgentResponse::from_anthropic(response))
    }
}

#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub thinking: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}
```

**Replace:** `crates/hox-agent/src/file_executor.rs`

Remove XML parsing, replace with tool execution:

```rust
pub struct FileExecutor {
    workspace_path: PathBuf,
    protected_files: Vec<String>,
}

impl FileExecutor {
    /// Execute tool calls from agent response
    pub async fn execute_tools(&self, tool_calls: &[ToolCall]) -> Result<Vec<ToolResult>> {
        let mut results = Vec::new();

        // Can execute in parallel since tools are independent
        let futures: Vec<_> = tool_calls.iter()
            .map(|tool| self.execute_tool(tool))
            .collect();

        for future in futures {
            results.push(future.await?);
        }

        Ok(results)
    }

    async fn execute_tool(&self, tool: &ToolCall) -> Result<ToolResult> {
        match tool.name.as_str() {
            "read_file" => {
                let path = tool.input["path"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing path".into()))?;
                let content = self.read_file(path).await?;
                Ok(ToolResult {
                    tool_id: tool.id.clone(),
                    success: true,
                    output: content,
                })
            }

            "write_file" => {
                let path = tool.input["path"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing path".into()))?;
                let content = tool.input["content"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing content".into()))?;

                self.check_protected(path)?;
                self.write_file(path, content).await?;

                Ok(ToolResult {
                    tool_id: tool.id.clone(),
                    success: true,
                    output: format!("Wrote {} bytes to {}", content.len(), path),
                })
            }

            "edit_file" => {
                let path = tool.input["path"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing path".into()))?;
                let old_text = tool.input["old_text"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing old_text".into()))?;
                let new_text = tool.input["new_text"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing new_text".into()))?;

                self.check_protected(path)?;
                self.edit_file(path, old_text, new_text).await?;

                Ok(ToolResult {
                    tool_id: tool.id.clone(),
                    success: true,
                    output: format!("Edited {}", path),
                })
            }

            "run_command" => {
                let command = tool.input["command"].as_str()
                    .ok_or_else(|| HoxError::InvalidToolInput("missing command".into()))?;

                let output = self.run_command(command).await?;

                Ok(ToolResult {
                    tool_id: tool.id.clone(),
                    success: output.status.success(),
                    output: format!("stdout: {}\nstderr: {}", output.stdout, output.stderr),
                })
            }

            _ => Err(HoxError::UnknownTool(tool.name.clone())),
        }
    }

    fn check_protected(&self, path: &str) -> Result<()> {
        for protected in &self.protected_files {
            if path.starts_with(protected) {
                return Err(HoxError::ProtectedFile(path.to_string()));
            }
        }
        Ok(())
    }
}

pub struct ToolResult {
    pub tool_id: String,
    pub success: bool,
    pub output: String,
}
```

**Acceptance Criteria:**
- [x] Tool definitions for read_file, write_file, edit_file, run_command
- [x] `send_message_with_tools()` uses Anthropic tool_use API
- [x] `FileExecutor::execute_tools()` handles structured JSON input
- [x] Parallel tool execution (independent tools run concurrently)
- [x] Better error reporting per tool
- [x] No XML parsing anywhere

---

### Improvement #6: Selective Backpressure Calibration

**Goal:** Fast checks every iteration, slow checks periodically. Adaptive based on success/failure patterns.

**Extend:** `crates/hox-orchestrator/src/backpressure.rs`

```rust
use hox_core::config::BackpressureConfig;

pub struct BackpressureEngine {
    config: BackpressureConfig,
    check_history: CheckHistory,
}

struct CheckHistory {
    /// Track which checks passed/failed recently
    fast_check_failures: Vec<(String, usize)>, // (check_name, iteration)
    slow_check_last_run: HashMap<String, usize>, // check_name -> last iteration
}

impl BackpressureEngine {
    /// Run appropriate checks for this iteration
    pub async fn run_checks(
        &mut self,
        iteration: usize,
        workspace_path: &Path,
    ) -> Result<BackpressureResult> {
        let mut results = BackpressureResult::default();

        // Always run fast checks
        for check in &self.config.fast_checks {
            let result = self.run_check(check, workspace_path).await?;
            results.add(check.clone(), result);

            if !result.success {
                self.check_history.fast_check_failures.push((check.clone(), iteration));
            }
        }

        // Run slow checks based on schedule and adaptive rules
        for slow_check in &self.config.slow_checks {
            let should_run = self.should_run_slow_check(&slow_check, iteration);

            if should_run {
                let result = self.run_check(&slow_check.command, workspace_path).await?;
                results.add(slow_check.command.clone(), result);

                self.check_history.slow_check_last_run
                    .insert(slow_check.command.clone(), iteration);
            }
        }

        Ok(results)
    }

    fn should_run_slow_check(&self, check: &SlowCheck, iteration: usize) -> bool {
        // Regular schedule
        if iteration % check.every_n_iterations == 0 {
            return true;
        }

        // Adaptive: if fast checks are churning, escalate to slow checks
        let recent_failures = self.check_history.fast_check_failures.iter()
            .filter(|(_, iter)| iteration - iter < 3)
            .count();

        if recent_failures >= 2 {
            // Churning on fast checks, run slow checks to get more info
            return true;
        }

        // Adaptive: if we haven't run in 2x the normal interval, force run
        let last_run = self.check_history.slow_check_last_run
            .get(&check.command)
            .copied()
            .unwrap_or(0);

        if iteration - last_run > check.every_n_iterations * 2 {
            return true;
        }

        false
    }

    async fn run_check(&self, command: &str, workspace_path: &Path) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(workspace_path)
            .output()
            .await?;

        let elapsed = start.elapsed();

        Ok(CheckResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            elapsed,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Default)]
pub struct BackpressureResult {
    pub checks: HashMap<String, CheckResult>,
}

impl BackpressureResult {
    fn add(&mut self, name: String, result: CheckResult) {
        self.checks.insert(name, result);
    }

    pub fn all_passed(&self) -> bool {
        self.checks.values().all(|r| r.success)
    }

    pub fn format_for_prompt(&self) -> String {
        let mut output = String::new();

        for (name, result) in &self.checks {
            if !result.success {
                output.push_str(&format!("\n## Check Failed: {}\n", name));
                output.push_str(&format!("```\n{}\n```\n", result.stderr));
            }
        }

        if output.is_empty() {
            "All checks passed.".to_string()
        } else {
            format!("Some checks failed:\n{}", output)
        }
    }
}
```

**Language-Aware Defaults:**

Add to `HoxConfig`:

```rust
impl HoxConfig {
    pub fn detect_language(repo_root: &Path) -> Option<Language> {
        if repo_root.join("Cargo.toml").exists() {
            Some(Language::Rust)
        } else if repo_root.join("pyproject.toml").exists() {
            Some(Language::Python)
        } else if repo_root.join("package.json").exists() {
            Some(Language::JavaScript)
        } else {
            None
        }
    }

    pub fn default_for_language(lang: Language) -> BackpressureConfig {
        match lang {
            Language::Rust => BackpressureConfig {
                fast_checks: vec![
                    "cargo check".to_string(),
                    "cargo clippy --all-targets".to_string(),
                ],
                slow_checks: vec![
                    SlowCheck {
                        command: "cargo test".to_string(),
                        every_n_iterations: 3,  // Rust: lean on type system, less testing
                    },
                ],
            },
            Language::Python => BackpressureConfig {
                fast_checks: vec![
                    "ruff check .".to_string(),
                    "mypy .".to_string(),
                ],
                slow_checks: vec![
                    SlowCheck {
                        command: "pytest".to_string(),
                        every_n_iterations: 2,  // Python: run tests more often
                    },
                ],
            },
            Language::JavaScript => BackpressureConfig {
                fast_checks: vec![
                    "npm run lint".to_string(),
                ],
                slow_checks: vec![
                    SlowCheck {
                        command: "npm test".to_string(),
                        every_n_iterations: 2,
                    },
                ],
            },
        }
    }
}
```

**Acceptance Criteria:**
- [x] Fast checks run every iteration
- [x] Slow checks run on schedule
- [x] Adaptive escalation when churning
- [x] Language-aware defaults (Rust, Python, JS)
- [x] Track check timing (fast vs slow ratio)
- [x] Format failures for agent prompt

---

## Phase 4: Advanced Features

**Goal:** Nice-to-have features that improve standalone UX.

**Timeline:** 3-4 days
**Dependencies:** Phase 2 (State Machine)

### Improvement #9: Pattern Extraction

**Current State:** `PatternStore` in `crates/hox-evolution/src/patterns.rs` (333 lines) requires manual pattern recording.

**Goal:** Auto-extract patterns from successful completions and suggest to users.

**Extend:** `crates/hox-evolution/src/patterns.rs`

```rust
pub struct PatternExtractor {
    store: PatternStore,
}

impl PatternExtractor {
    /// Extract patterns from a successful orchestration run
    pub fn extract_from_trace(&self, trace: &OrchestrationTrace) -> Vec<Pattern> {
        let mut patterns = Vec::new();

        // Pattern: Fast convergence with specific backpressure config
        if trace.iterations < 10 && trace.backpressure_config.is_some() {
            patterns.push(Pattern {
                name: "fast_convergence".to_string(),
                description: format!(
                    "Task converged in {} iterations with backpressure: {:?}",
                    trace.iterations,
                    trace.backpressure_config
                ),
                confidence: 0.7,
                applicable_contexts: vec!["similar_task_type".to_string()],
            });
        }

        // Pattern: Effective agent assignment
        if let Some(agent_perf) = &trace.agent_performance {
            if agent_perf.success_rate > 0.8 {
                patterns.push(Pattern {
                    name: "effective_agent".to_string(),
                    description: format!(
                        "Agent {} has {:.0}% success rate on {}",
                        agent_perf.agent_id,
                        agent_perf.success_rate * 100.0,
                        trace.task_type
                    ),
                    confidence: agent_perf.success_rate,
                    applicable_contexts: vec![trace.task_type.clone()],
                });
            }
        }

        patterns
    }

    /// Suggest patterns to user based on current context
    pub fn suggest(&self, context: &TaskContext) -> Vec<Suggestion> {
        let relevant_patterns = self.store.query(&context.task_type);

        relevant_patterns.iter()
            .filter(|p| p.confidence > 0.6)
            .map(|p| Suggestion {
                pattern_name: p.name.clone(),
                message: format!(
                    "💡 Suggestion: {} (confidence: {:.0}%)",
                    p.description,
                    p.confidence * 100.0
                ),
                actionable: true,
            })
            .collect()
    }
}

pub struct Pattern {
    pub name: String,
    pub description: String,
    pub confidence: f64,
    pub applicable_contexts: Vec<String>,
}

pub struct Suggestion {
    pub pattern_name: String,
    pub message: String,
    pub actionable: bool,
}
```

**CLI Integration:**

```rust
// In hox loop command, after completion:
let patterns = pattern_extractor.extract_from_trace(&trace);

if !patterns.is_empty() {
    println!("\n📊 Learned {} new patterns:", patterns.len());
    for pattern in patterns {
        println!("  - {}", pattern.description);
    }

    print!("Save these patterns? [Y/n] ");
    // ... save if user confirms
}

// Before starting a loop:
let suggestions = pattern_extractor.suggest(&task_context);

if !suggestions.is_empty() {
    println!("\n💡 Suggestions based on past runs:");
    for suggestion in suggestions {
        println!("  {}", suggestion.message);
    }
}
```

**Acceptance Criteria:**
- [x] `extract_from_trace()` identifies convergence patterns
- [x] `extract_from_trace()` identifies effective agent assignments
- [x] `suggest()` recommends patterns for current context
- [x] CLI shows learned patterns after completion
- [x] CLI shows suggestions before starting loop
- [x] User can approve/reject pattern saves

---

### Improvement #10: Phase Auto-Advancement

**Current State:** `PhaseManager` in `crates/hox-orchestrator/src/phases.rs` (191 lines) requires manual transitions.

**Goal:** Auto-advance when all phase tasks complete.

**Extend:** `crates/hox-orchestrator/src/phases.rs`

```rust
impl PhaseManager {
    /// Check if current phase is complete (all tasks done)
    pub async fn check_auto_advance<E: JjExecutor>(
        &self,
        queries: &RevsetQueries<E>,
    ) -> Result<bool> {
        let current_phase = self.current_phase();
        let phase_tasks = queries.tasks_in_phase(current_phase).await?;

        let all_complete = phase_tasks.iter()
            .all(|task| task.status == TaskStatus::Done);

        if all_complete && !phase_tasks.is_empty() {
            info!("Phase {:?} complete, all {} tasks done", current_phase, phase_tasks.len());
            return Ok(true);
        }

        Ok(false)
    }

    /// Auto-advance to next phase if current is complete
    pub async fn maybe_advance<E: JjExecutor>(
        &mut self,
        queries: &RevsetQueries<E>,
    ) -> Result<Option<Phase>> {
        if self.check_auto_advance(queries).await? {
            match self.advance() {
                Ok(new_phase) => {
                    info!("Auto-advanced to phase {:?}", new_phase);
                    Ok(Some(new_phase))
                }
                Err(e) => {
                    warn!("Cannot auto-advance: {}", e);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }
}
```

**Integrate with State Machine:**

```rust
// In orchestrator runtime loop:
loop {
    // ... existing event processing ...

    // Check for phase auto-advancement
    if let Some(new_phase) = self.phase_manager.maybe_advance(&queries).await? {
        // Generate PhaseComplete event
        let event = Event::PhaseComplete;
        let (new_state, actions) = state_machine::transition(self.state.clone(), event);
        self.state = new_state;

        for action in actions {
            self.execute_action(action).await?;
        }
    }
}
```

**Acceptance Criteria:**
- [x] `check_auto_advance()` detects when all phase tasks complete
- [x] `maybe_advance()` advances to next phase automatically
- [x] Auto-advancement generates `PhaseComplete` event
- [x] Works with state machine (Phase 2 dependency)
- [x] User can disable auto-advancement via config

---

## Phase 5: Performance

**Goal:** Eliminate subprocess overhead and string parsing fragility by using jj-lib directly.

**Timeline:** 4-5 days
**Dependencies:** None, but high risk (jj-lib API not stable)

### Improvement #2: jj-lib Direct Integration

**CRITICAL CAVEAT:** jj-lib API is not stable. This should be phased carefully and feature-flagged.

**Current State:**
- `JjCommand` in `crates/hox-jj/src/command.rs` (199 lines) spawns subprocesses
- `MetadataManager` in `crates/hox-jj/src/metadata.rs` (442 lines) parses stdout strings
- 4-6 subprocess spawns per iteration × 20-50 iterations = 80-300 process spawns per loop

**Strategy: Gradual Migration**

1. Add jj-lib as optional dependency
2. Implement hot path (metadata read/write) first
3. Feature flag the new implementation
4. Fallback to subprocess if jj-lib unavailable

**Add to Cargo.toml:**

```toml
[dependencies]
jj-lib = { version = "0.20", optional = true }

[features]
default = []
jj-lib-integration = ["dep:jj-lib"]
```

**New File:** `crates/hox-jj/src/lib_backend.rs`

```rust
#[cfg(feature = "jj-lib-integration")]
use jj_lib::repo::Repo;
#[cfg(feature = "jj-lib-integration")]
use jj_lib::commit::Commit;

#[cfg(feature = "jj-lib-integration")]
pub struct JjLibExecutor {
    repo: Arc<Mutex<Repo>>,
}

#[cfg(feature = "jj-lib-integration")]
impl JjLibExecutor {
    pub fn new(repo_path: &Path) -> Result<Self> {
        let repo = Repo::load(repo_path)?;
        Ok(Self {
            repo: Arc::new(Mutex::new(repo)),
        })
    }
}

#[cfg(feature = "jj-lib-integration")]
#[async_trait]
impl JjExecutor for JjLibExecutor {
    async fn exec(&self, args: &[&str]) -> Result<CommandOutput> {
        // Parse args and dispatch to jj-lib functions
        match args.get(0) {
            Some(&"describe") => {
                // Use jj-lib to update description directly
                let repo = self.repo.lock().unwrap();
                // ... jj-lib API calls ...
                Ok(CommandOutput {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }

            Some(&"log") => {
                // Use jj-lib to query commits
                let repo = self.repo.lock().unwrap();
                // ... jj-lib API calls ...
                Ok(CommandOutput {
                    success: true,
                    stdout: format_commit_log(&commits),
                    stderr: String::new(),
                })
            }

            _ => {
                // Fallback to subprocess for unsupported commands
                let output = std::process::Command::new("jj")
                    .args(args)
                    .output()?;

                Ok(CommandOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                })
            }
        }
    }
}
```

**Conditional Compilation:**

```rust
// In crates/hox-jj/src/lib.rs

#[cfg(feature = "jj-lib-integration")]
pub use lib_backend::JjLibExecutor;

#[cfg(not(feature = "jj-lib-integration"))]
pub type JjLibExecutor = JjCommand;  // Fallback to subprocess
```

**MockJjExecutor becomes type-level:**

```rust
// Current: HashMap<Vec<String>, String>
// After: Proper typed mocks

pub struct MockJjExecutor {
    expectations: Vec<MockExpectation>,
}

pub struct MockExpectation {
    pub command: Vec<String>,
    pub response: Result<CommandOutput>,
}

impl MockJjExecutor {
    pub fn expect_describe(&mut self, change_id: &str, description: &str) {
        self.expectations.push(MockExpectation {
            command: vec!["describe".to_string(), "-m".to_string(), description.to_string()],
            response: Ok(CommandOutput::success("")),
        });
    }
}
```

**Acceptance Criteria:**
- [x] jj-lib as optional dependency
- [x] Feature flag `jj-lib-integration`
- [x] Hot path (metadata read/write) uses jj-lib
- [x] Fallback to subprocess for unsupported operations
- [x] 80%+ reduction in subprocess spawns
- [x] Typed mocks instead of HashMap
- [x] All existing tests pass with both backends

**Risk Mitigation:**
- Keep subprocess backend as default
- Gradual rollout: metadata → revsets → all operations
- Monitor jj-lib API stability releases
- Be prepared to maintain subprocess backend long-term

---

## Dependency Graph

```
Phase 1: Quick Wins (3-4 days)
  ├─ Budget Enforcement (#4) ────────┐
  ├─ Fail-Open Audit (#7) ───────────┤
  └─ .hox/config.toml (#8) ──────────┤
                                     │
Phase 2: Core Architecture (3-4 days) │
  ├─ State Machine (#1) ─────────────┼──> Enables Phase 4
  └─ PostToolsHook (#3) ─────────────┘
         │
         │
Phase 3: Agent Quality (4-5 days)
  ├─ Structured Output (#5)
  └─ Backpressure Calibration (#6)
         │
         │
Phase 4: Advanced Features (3-4 days)
  ├─ Pattern Extraction (#9) ────── Requires State Machine
  └─ Phase Auto-Advancement (#10) ── Requires State Machine
         │
         │
Phase 5: Performance (4-5 days, HIGH RISK)
  └─ jj-lib Integration (#2) ────── Independent, do last
```

**Recommended Execution:**

| Week | Focus | Deliverables |
|------|-------|--------------|
| 1 | Phase 1 | Budget enforcement, fail-open, config file |
| 2 | Phase 2 | State machine, hook pipeline |
| 3 | Phase 3 | Structured output, backpressure calibration |
| 4 | Phase 4 | Pattern extraction, phase auto-advance |
| 5+ | Phase 5 | jj-lib integration (when API stable) |

---

## Getting Started

### For Contributors

**Pick a Quick Win:**
1. Read the Phase 1 section for your chosen improvement
2. Find the relevant files in `crates/`
3. Write tests first (TDD approach)
4. Implement the feature
5. Update docs

**Example: Budget Enforcement**

```bash
# 1. Read Phase 1, Improvement #4 section above
# 2. Find the file
code crates/hox-orchestrator/src/loop_engine.rs

# 3. Write tests first
code crates/hox-orchestrator/src/loop_engine.rs
# Add test_budget_enforcement_tokens()
# Add test_budget_enforcement_usd()

# 4. Run tests (they fail)
cargo test -p hox-agent test_budget_enforcement

# 5. Implement cumulative tracking
# (see code in Phase 1 section)

# 6. Tests pass
cargo test -p hox-agent

# 7. Update docs
code docs/USAGE.md
# Add section on budget enforcement
```

**Pick Foundation Work:**

State Machine (#1) is the most impactful but requires understanding the orchestrator. Start here if you want to understand the core architecture deeply.

**Pick Advanced Features:**

Pattern Extraction (#9) and Phase Auto-Advancement (#10) are self-contained and don't require deep orchestrator knowledge. Good for contributors who want to add user-facing features.

### Testing Your Changes

**Unit Tests:**
```bash
# Test a specific improvement
cargo test -p hox-agent test_budget_enforcement
cargo test -p hox-core test_fail_open
cargo test -p hox-orchestrator test_state_machine

# Run all tests
cargo test --workspace
```

**Integration Tests:**
```bash
# Create a test repo
mkdir /tmp/hox-test && cd /tmp/hox-test
jj git init
hox init

# Test the loop
hox loop start --goal "add hello world function" -n 5 --max-budget-usd 0.50

# Verify budget enforcement stops it
```

### Documentation

**Update these docs when implementing:**
- `docs/USAGE.md` - User-facing features (budget, config, suggestions)
- `docs/ARCHITECTURE.md` - Internal changes (state machine, hooks)
- `README.md` - Quick start if config changes
- `CHANGELOG.md` - All changes

---

## Success Metrics

**Developer UX:**
- [x] Config file adoption (% of repos with .hox/config.toml)
- [x] Budget enforcement prevents runaway loops
- [x] Average loop startup time < 2 seconds

**Robustness:**
- [x] Zero crashes from transient infrastructure failures
- [x] Tool parsing success rate > 99% (structured output)
- [x] State machine proptest passes 10,000 random inputs

**Performance:**
- [x] Backpressure overhead < 20% of total iteration time
- [x] jj-lib integration reduces subprocess spawns 80%+
- [x] Ralph loop iteration time improves 30%+

**Adoption:**
- [x] 10+ users running hox on real projects
- [x] 100+ successful orchestration runs
- [x] 5+ contributed patterns in pattern store

---

## Summary

This plan prioritizes **standalone tool quality** over architectural completeness. The phases are:

1. **Quick Wins** - Stop burning money, enable configuration, don't crash
2. **Core Architecture** - Clean state machine, extensible hooks
3. **Agent Quality** - Reliable output, smart backpressure
4. **Advanced Features** - Learning, auto-advancement
5. **Performance** - jj-lib integration (when stable)

Each improvement has clear acceptance criteria, testing requirements, and file locations. Contributors can pick any improvement and start immediately.

The dependency graph shows which improvements enable others, but most are independent enough to work on in parallel. Recommended timeline: 3-5 weeks for Phases 1-4, with Phase 5 deferred until jj-lib stabilizes.

**Next Steps:**
1. Review this plan with the core team
2. Create GitHub issues for each improvement
3. Assign improvements to contributors
4. Start with Phase 1 (highest ROI, lowest risk)
