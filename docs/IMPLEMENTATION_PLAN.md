# Hox Implementation Plan: JJ Integration Gap Closure

**Date:** 2026-01-30
**Status:** Draft
**Context:** Systematic closure of gaps between Hox's JJ-native design philosophy and its current implementation
**Prerequisite:** jj-dev fork cleanup complete (directory renamed, metadata fields renamed)

---

## Table of Contents

1. [Current State Assessment](#current-state-assessment)
2. [Phase 1: Bookmark Management (CRITICAL)](#phase-1-bookmark-management)
3. [Phase 2: Operation Rollback & Recovery](#phase-2-operation-rollback--recovery)
4. [Phase 3: Conflict Resolution Pipeline](#phase-3-conflict-resolution-pipeline)
5. [Phase 4: DAG Manipulation Commands](#phase-4-dag-manipulation-commands)
6. [Phase 5: Backpressure Enhancement (jj fix)](#phase-5-backpressure-enhancement)
7. [Phase 6: Advanced Revsets & Query Migration](#phase-6-advanced-revsets--query-migration)
8. [Phase 7: Speculative Execution & Audit Trails](#phase-7-speculative-execution--audit-trails)
9. [Cross-Cutting: Dual Metadata Path](#cross-cutting-dual-metadata-path)
10. [Dependency Map](#dependency-map)
11. [Testing Strategy](#testing-strategy)

---

## Current State Assessment

**11 crates, ~13,381 lines of Rust.**

### JJ Commands Currently Used

| Command | Location | Purpose |
|---------|----------|---------|
| `jj new` | `orchestrator.rs:141`, `orchestrator.rs:195` | Create changes for orchestrators and agents |
| `jj describe` | `metadata.rs:155`, `loop_engine.rs:366` | Write metadata into descriptions |
| `jj log` | `metadata.rs:113`, `revsets.rs:21`, `oplog.rs:82` | Read descriptions, query revsets, poll oplog |
| `jj root` | `command.rs:54` | Detect repository root |
| `jj workspace add/forget/list` | `workspace.rs:49-103` | Agent workspace isolation |
| `jj op log` | `oplog.rs:82` | Poll for new operations (500ms interval) |

### JJ Commands NOT Used (Should Be)

| Command | Priority | Gap |
|---------|----------|-----|
| `jj bookmark create/set/list/delete` | CRITICAL | No bookmark code exists at all |
| `jj op restore/undo` | HIGH | OpLogWatcher polls but never manipulates |
| `jj parallelize` | HIGH | No DAG restructuring |
| `jj absorb` | HIGH | No megamerge distribution |
| `jj resolve` | HIGH | Conflict detection only, no resolution |
| `jj split` | MEDIUM-HIGH | No task decomposition via VCS |
| `jj squash` | MEDIUM-HIGH | No task consolidation via VCS |
| `jj fix` | MEDIUM | Backpressure runs cargo/pytest, not jj fix |
| `jj duplicate` | MEDIUM | No speculative execution |
| `jj backout` | LOW-MEDIUM | No safe reversion |
| `jj evolog` | LOW | No change evolution tracking |

---

## Phase 1: Bookmark Management

**Priority:** CRITICAL
**Business Value:** Bookmarks are the architectural linchpin. The design docs state "Assignments ARE bookmarks" but zero bookmark code exists. This blocks O(1) task lookups, proper agent assignment tracking, and bookmark-based revset queries. Every other phase benefits from this.
**Estimated Effort:** 2-3 days

### What Changes

#### New File: `crates/hox-jj/src/bookmarks.rs`

This module wraps all `jj bookmark` subcommands and provides Hox-specific bookmark conventions.

```rust
/// Bookmark naming conventions for Hox:
///   task/{change-id-prefix}           - Task bookmark
///   agent/{agent-name}/task/{id}      - Agent assignment
///   orchestrator/{orch-id}            - Orchestrator base
///   session/{session-id}              - Session tracking
pub struct BookmarkManager<E: JjExecutor> {
    executor: E,
}
```

**Key functions to implement:**

```rust
impl<E: JjExecutor> BookmarkManager<E> {
    pub fn new(executor: E) -> Self;

    // Core bookmark operations
    pub async fn create(&self, name: &str, change_id: &ChangeId) -> Result<()>;
    pub async fn set(&self, name: &str, change_id: &ChangeId) -> Result<()>;
    pub async fn delete(&self, name: &str) -> Result<()>;
    pub async fn list(&self, glob: Option<&str>) -> Result<Vec<BookmarkInfo>>;

    // Hox-specific operations
    pub async fn assign_task(&self, agent_name: &str, change_id: &ChangeId) -> Result<()>;
    pub async fn unassign_task(&self, agent_name: &str, change_id: &ChangeId) -> Result<()>;
    pub async fn agent_tasks(&self, agent_name: &str) -> Result<Vec<ChangeId>>;
    pub async fn task_agent(&self, change_id: &ChangeId) -> Result<Option<String>>;
    pub async fn mark_orchestrator(&self, orch_id: &OrchestratorId, change_id: &ChangeId) -> Result<()>;
    pub async fn session_bookmark(&self, session_id: &str, change_id: &ChangeId) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct BookmarkInfo {
    pub name: String,
    pub change_id: ChangeId,
    pub tracking: Option<String>, // Remote tracking info
}
```

**JJ commands used:**
```bash
jj bookmark create {name} -r {change_id}
jj bookmark set {name} -r {change_id}
jj bookmark delete {name}
jj bookmark list --all          # or with -T for template output
```

#### Extend: `crates/hox-jj/src/lib.rs`

```rust
mod bookmarks;
pub use bookmarks::{BookmarkManager, BookmarkInfo};
```

#### Extend: `crates/hox-orchestrator/src/orchestrator.rs`

Update `spawn_agent()` (line 174) to create bookmarks on task assignment:

```rust
// After creating agent change and setting metadata...
let bookmark_mgr = BookmarkManager::new(self.executor.clone());
bookmark_mgr.assign_task(&agent_name, &change_id).await?;
```

Update `initialize()` (line 134) to create orchestrator bookmark:

```rust
let bookmark_mgr = BookmarkManager::new(self.executor.clone());
bookmark_mgr.mark_orchestrator(&self.config.id, change_id).await?;
```

#### Extend: `crates/hox-jj/src/revsets.rs`

Add bookmark-based query methods alongside existing description-based ones:

```rust
/// Find agent's tasks via bookmarks (O(1) vs description grep O(n))
/// Revset: bookmarks(glob:"agent/{agent_name}/task/*")
pub async fn agent_tasks_by_bookmark(&self, agent_name: &str) -> Result<Vec<ChangeId>> {
    let revset = format!("bookmarks(glob:\"agent/{}/task/*\")", agent_name);
    self.query(&revset).await
}

/// Find all tasks with bookmarks
pub async fn all_tasks_by_bookmark(&self) -> Result<Vec<ChangeId>> {
    self.query("bookmarks(glob:\"task/*\")").await
}

/// Find orchestrator base changes
pub async fn orchestrator_by_bookmark(&self, orch_id: &str) -> Result<Vec<ChangeId>> {
    let revset = format!("bookmarks(glob:\"orchestrator/{}\")", orch_id);
    self.query(&revset).await
}
```

#### Extend: `crates/hox-cli/src/main.rs`

Add bookmark management CLI commands:

```rust
/// Bookmark management
Bookmark {
    #[command(subcommand)]
    action: BookmarkCommands,
},

enum BookmarkCommands {
    /// Assign current change to an agent
    Assign { agent: String },
    /// List task bookmarks
    List {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Show which agent owns a task
    Owner { change_id: String },
}
```

### Acceptance Criteria

- [ ] `BookmarkManager` struct with all CRUD operations
- [ ] Hox naming convention enforced: `task/`, `agent/`, `orchestrator/`, `session/`
- [ ] `spawn_agent()` creates bookmark on assignment
- [ ] `initialize()` creates orchestrator bookmark
- [ ] Bookmark-based revset queries in `RevsetQueries`
- [ ] CLI subcommand `hox bookmark assign/list/owner`
- [ ] Unit tests with `MockJjExecutor` covering all operations
- [ ] Existing description-based queries still work (dual path)

### Testing Checklist

- [ ] Mock test: `BookmarkManager::create()` calls `jj bookmark create` with correct args
- [ ] Mock test: `BookmarkManager::assign_task()` creates `agent/{name}/task/{id}` bookmark
- [ ] Mock test: `BookmarkManager::agent_tasks()` returns correct change IDs
- [ ] Mock test: `RevsetQueries::agent_tasks_by_bookmark()` uses `bookmarks(glob:...)` revset
- [ ] Integration test (requires real JJ repo): full assign/query/unassign cycle

---

## Phase 2: Operation Rollback & Recovery

**Priority:** HIGH
**Business Value:** When an agent produces bad output, there is currently no way to roll back. The `OpLogWatcher` detects operations but cannot undo them. This is essential for agent reliability -- without rollback, a single bad agent iteration can corrupt the task state permanently.
**Estimated Effort:** 2 days
**Depends on:** None (independent of Phase 1)

### What Changes

#### Extend: `crates/hox-jj/src/oplog.rs`

Add manipulation capabilities to the existing watch-only module:

```rust
/// Operation management (undo/restore/revert)
pub struct OpManager<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> OpManager<E> {
    pub fn new(executor: E) -> Self;

    /// Get the N most recent operation IDs
    pub async fn recent_operations(&self, count: usize) -> Result<Vec<OperationInfo>>;

    /// Undo the most recent operation
    pub async fn undo(&self) -> Result<()>;

    /// Restore repo to a specific operation state
    pub async fn restore(&self, operation_id: &str) -> Result<()>;

    /// Revert a specific (non-recent) operation
    pub async fn revert(&self, operation_id: &str) -> Result<()>;

    /// Snapshot current operation ID (for later rollback)
    pub async fn snapshot(&self) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub id: String,
    pub description: String,
    pub timestamp: String,
}
```

**JJ commands used:**
```bash
jj op log -n {count} -T 'operation_id ++ "\t" ++ description ++ "\t" ++ time ++ "\n"' --no-graph
jj undo
jj op restore {operation_id}
```

#### New File: `crates/hox-orchestrator/src/recovery.rs`

Agent recovery logic built on top of `OpManager`:

```rust
/// Recovery manager for handling agent failures
pub struct RecoveryManager<E: JjExecutor> {
    op_manager: OpManager<E>,
    bookmark_manager: BookmarkManager<E>, // Optional, if Phase 1 is done
}

impl<E: JjExecutor> RecoveryManager<E> {
    /// Roll back an agent's work to before it started
    ///
    /// 1. Find the operation ID from before the agent was spawned
    /// 2. Restore to that state
    /// 3. Clean up the agent's workspace
    /// 4. Remove agent bookmark (if bookmarks are implemented)
    pub async fn rollback_agent(
        &self,
        agent_name: &str,
        snapshot_op_id: &str,
    ) -> Result<RollbackResult>;

    /// Roll back the last N operations
    pub async fn rollback_operations(&self, count: usize) -> Result<()>;

    /// Create a recovery point before risky operations
    pub async fn create_recovery_point(&self) -> Result<RecoveryPoint>;

    /// Restore from a recovery point
    pub async fn restore_from(&self, point: &RecoveryPoint) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct RecoveryPoint {
    pub operation_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub operations_undone: usize,
    pub agent_cleaned: bool,
    pub workspace_removed: bool,
}
```

#### Extend: `crates/hox-orchestrator/src/loop_engine.rs`

Add recovery points around agent iterations (modify `run()` method, around line 93):

```rust
// Before spawning agent, create recovery point
let op_manager = OpManager::new(self.executor.clone());
let snapshot = op_manager.snapshot().await?;

// ... spawn agent, execute operations ...

// If agent output is clearly broken, rollback
if result.output.is_empty() || result.output.contains("[ERROR]") {
    warn!("Agent iteration {} produced bad output, rolling back", iteration);
    op_manager.restore(&snapshot).await?;
    continue; // retry iteration
}
```

#### Extend: `crates/hox-orchestrator/src/lib.rs`

```rust
mod recovery;
pub use recovery::{RecoveryManager, RecoveryPoint, RollbackResult};
```

#### Extend: `crates/hox-jj/src/lib.rs`

```rust
pub use oplog::{OpLogEvent, OpLogWatcher, OpLogWatcherConfig, OpManager, OperationInfo};
```

#### Extend: `crates/hox-cli/src/main.rs`

```rust
/// Rollback agent work
Rollback {
    /// Agent name to roll back
    #[arg(long)]
    agent: Option<String>,
    /// Operation ID to restore to
    #[arg(long)]
    operation: Option<String>,
    /// Undo last N operations
    #[arg(long)]
    count: Option<usize>,
},
```

### Acceptance Criteria

- [ ] `OpManager` with `undo()`, `restore()`, `revert()`, `snapshot()` methods
- [ ] `RecoveryManager` with `rollback_agent()` and recovery points
- [ ] Loop engine creates snapshot before each agent iteration
- [ ] Automatic rollback on empty/broken agent output
- [ ] CLI `hox rollback` command with --agent, --operation, --count flags
- [ ] Unit tests for all OpManager operations
- [ ] Recovery point lifecycle tested end-to-end

### Testing Checklist

- [ ] Mock test: `OpManager::snapshot()` calls `jj op log -n 1` and returns ID
- [ ] Mock test: `OpManager::restore()` calls `jj op restore {id}`
- [ ] Mock test: `OpManager::undo()` calls `jj undo`
- [ ] Mock test: `RecoveryManager::rollback_agent()` restores and cleans workspace
- [ ] Unit test: Loop engine retries after rollback
- [ ] Integration test: full snapshot -> bad work -> rollback -> verify clean state

---

## Phase 3: Conflict Resolution Pipeline

**Priority:** HIGH
**Business Value:** Currently Hox detects conflicts (`conflicts()` revset in `revsets.rs:101`) but does nothing. The orchestrator warns about conflicts and has `// TODO: Handle conflicts` comments (orchestrator.rs:378, orchestrator.rs:728). Multi-agent parallel work inevitably produces conflicts; automated resolution is the difference between "agents work in parallel" and "agents actually produce merged results."
**Estimated Effort:** 3-4 days
**Depends on:** Phase 2 (recovery points for failed resolution attempts)

### What Changes

#### New File: `crates/hox-orchestrator/src/conflict_resolver.rs`

```rust
/// Strategy for resolving a specific conflict
#[derive(Debug, Clone)]
pub enum ResolutionStrategy {
    /// Use jj fix to auto-resolve formatting conflicts
    JjFix,
    /// Use :ours or :theirs resolution tool
    PickSide { side: ConflictSide },
    /// Spawn a dedicated conflict-resolution agent
    SpawnAgent { prompt_context: String },
    /// Queue for human review
    HumanReview { reason: String },
}

#[derive(Debug, Clone)]
pub enum ConflictSide {
    Ours,
    Theirs,
}

/// Information about a detected conflict
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub change_id: ChangeId,
    pub files: Vec<String>,
    pub is_formatting_only: bool,
}

/// Pipeline for resolving conflicts
pub struct ConflictResolver<E: JjExecutor> {
    executor: E,
    recovery: RecoveryManager<E>,
}

impl<E: JjExecutor + Clone + 'static> ConflictResolver<E> {
    pub fn new(executor: E, recovery: RecoveryManager<E>) -> Self;

    /// Analyze a conflict and determine resolution strategy
    pub async fn analyze(&self, change_id: &ChangeId) -> Result<Vec<ConflictInfo>>;

    /// Determine best strategy for a given conflict
    pub fn recommend_strategy(&self, info: &ConflictInfo) -> ResolutionStrategy;

    /// Execute a resolution strategy
    pub async fn resolve(&self, info: &ConflictInfo, strategy: &ResolutionStrategy) -> Result<bool>;

    /// Run the full pipeline: detect -> analyze -> strategize -> resolve
    pub async fn resolve_all(&self) -> Result<ResolutionReport>;

    /// Resolve formatting-only conflicts using jj fix
    async fn resolve_with_jj_fix(&self, change_id: &ChangeId) -> Result<bool>;

    /// Resolve using pick-side strategy
    async fn resolve_with_pick_side(&self, change_id: &ChangeId, side: &ConflictSide) -> Result<bool>;

    /// Spawn a conflict-resolution agent
    async fn spawn_resolution_agent(&self, info: &ConflictInfo, context: &str) -> Result<bool>;
}

#[derive(Debug, Clone)]
pub struct ResolutionReport {
    pub total_conflicts: usize,
    pub auto_resolved: usize,
    pub agent_resolved: usize,
    pub needs_human: usize,
    pub failed: usize,
}
```

**JJ commands used:**
```bash
jj log -r '{change_id}' -T 'conflict' --no-graph   # Check if change has conflicts
jj diff -r {change_id}                                # Analyze conflict content
jj resolve -r {change_id} --tool :ours                # Pick-side resolution
jj resolve -r {change_id} --tool :theirs
jj fix -s {change_id}                                 # Format-only resolution
```

#### Extend: `crates/hox-orchestrator/src/orchestrator.rs`

Replace TODO comments with actual conflict resolution calls.

At line ~376 (inside `integrate()`):
```rust
// Before:
// TODO: Handle conflicts - spawn integration agent

// After:
let recovery = RecoveryManager::new(
    OpManager::new(self.executor.clone()),
    BookmarkManager::new(self.executor.clone()),
);
let resolver = ConflictResolver::new(self.executor.clone(), recovery);
let report = resolver.resolve_all().await?;
info!("Conflict resolution: {} auto, {} agent, {} human",
    report.auto_resolved, report.agent_resolved, report.needs_human);
if report.needs_human > 0 {
    warn!("{} conflicts need human review", report.needs_human);
}
```

At line ~728 (inside `integrate_child_work()`):
```rust
// Same pattern -- replace the warn! + spawn_agent with ConflictResolver pipeline
```

#### Extend: `crates/hox-orchestrator/src/lib.rs`

```rust
mod conflict_resolver;
pub use conflict_resolver::{ConflictResolver, ConflictInfo, ResolutionReport, ResolutionStrategy};
```

### Acceptance Criteria

- [ ] `ConflictInfo` analysis distinguishes formatting-only from semantic conflicts
- [ ] `jj fix` auto-resolves formatting conflicts
- [ ] Pick-side resolution works via `jj resolve --tool`
- [ ] Agent-based resolution spawns a conflict-resolution agent with proper context
- [ ] Recovery point created before each resolution attempt
- [ ] `ResolutionReport` summarizes what was resolved and how
- [ ] TODO comments in orchestrator.rs replaced with real code
- [ ] Pipeline falls through to human review for unresolvable conflicts

### Testing Checklist

- [ ] Mock test: `analyze()` parses `jj diff` output to find conflicted files
- [ ] Mock test: `resolve_with_jj_fix()` calls `jj fix -s {id}`
- [ ] Mock test: `resolve_with_pick_side()` calls `jj resolve --tool :ours`
- [ ] Unit test: `recommend_strategy()` returns JjFix for formatting-only conflicts
- [ ] Unit test: `recommend_strategy()` returns SpawnAgent for semantic conflicts
- [ ] Integration test: create two conflicting changes, run resolve_all, verify resolution

---

## Phase 4: DAG Manipulation Commands

**Priority:** HIGH
**Business Value:** JJ's DAG manipulation commands (`parallelize`, `absorb`, `split`, `squash`) are the features that make JJ uniquely powerful for agent orchestration. Without them, Hox can only create linear sequences -- it cannot restructure work for parallelism, decompose tasks, consolidate results, or distribute fixes to correct branches. This phase unlocks the "plan then optimize" workflow.
**Estimated Effort:** 3-4 days
**Depends on:** Phase 1 (bookmarks auto-track through rewrites)

### What Changes

#### New File: `crates/hox-jj/src/dag.rs`

All DAG manipulation commands in one module:

```rust
/// DAG manipulation operations for task restructuring
pub struct DagOperations<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> DagOperations<E> {
    pub fn new(executor: E) -> Self;

    // --- jj parallelize ---

    /// Convert sequential changes into parallel siblings
    ///
    /// Takes a range of changes (e.g., "abc..xyz") and restructures them
    /// so they all share the same parent instead of being sequential.
    ///
    /// Bookmarks auto-track through rewrites, so agent assignments stay stable.
    pub async fn parallelize(&self, revset: &str) -> Result<ParallelizeResult>;

    // --- jj absorb ---

    /// Auto-distribute working copy changes to correct ancestor commits
    ///
    /// Each hunk is routed to the ancestor where those lines were last modified.
    /// Essential for the megamerge pattern:
    ///   1. Agents work in parallel branches
    ///   2. Orchestrator creates merge for integration testing
    ///   3. Orchestrator (or agent) makes fixes on merge
    ///   4. `jj absorb` distributes fixes back to correct branches
    pub async fn absorb(&self, paths: Option<&[&str]>) -> Result<AbsorbResult>;

    // --- jj split ---

    /// Split a change into multiple smaller changes
    ///
    /// Uses file-based splitting (non-interactive).
    /// Agent realizes task is too large -> split into subtasks.
    pub async fn split_by_files(
        &self,
        change_id: &ChangeId,
        file_groups: &[Vec<String>],
    ) -> Result<SplitResult>;

    // --- jj squash ---

    /// Fold a change into its parent
    pub async fn squash(&self, change_id: &ChangeId) -> Result<()>;

    /// Squash specific files from a change into a target
    pub async fn squash_into(
        &self,
        source: &ChangeId,
        target: &ChangeId,
        paths: Option<&[&str]>,
    ) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct ParallelizeResult {
    /// Number of changes restructured
    pub changes_restructured: usize,
    /// Whether the operation succeeded cleanly
    pub clean: bool,
    /// Any conflicts introduced by restructuring
    pub conflicts: Vec<ChangeId>,
}

#[derive(Debug, Clone)]
pub struct AbsorbResult {
    /// Number of hunks absorbed
    pub hunks_absorbed: usize,
    /// Changes that received absorbed hunks
    pub affected_changes: Vec<ChangeId>,
}

#[derive(Debug, Clone)]
pub struct SplitResult {
    /// Change IDs of the new split changes
    pub new_changes: Vec<ChangeId>,
}
```

**JJ commands used:**
```bash
jj parallelize {revset}
jj absorb [paths...]
jj split -r {change_id} {files...}          # File-based split (non-interactive)
jj squash -r {change_id}                     # Squash into parent
jj squash --from {source} --into {target}    # Squash into specific target
```

#### Extend: `crates/hox-jj/src/lib.rs`

```rust
mod dag;
pub use dag::{DagOperations, ParallelizeResult, AbsorbResult, SplitResult};
```

#### Extend: `crates/hox-orchestrator/src/orchestrator.rs`

Add methods that use DAG operations for orchestration patterns:

```rust
impl<E: JjExecutor + Clone + 'static> Orchestrator<E> {
    /// After planning tasks sequentially, restructure independent ones for parallel execution
    pub async fn optimize_dag(&self, task_range: &str) -> Result<ParallelizeResult> {
        let dag = DagOperations::new(self.executor.clone());
        dag.parallelize(task_range).await
    }

    /// After integration testing, distribute fixes back to agent branches
    pub async fn absorb_fixes(&self, paths: Option<&[&str]>) -> Result<AbsorbResult> {
        let dag = DagOperations::new(self.executor.clone());
        dag.absorb(paths).await
    }

    /// Agent reports task is too large; split it
    pub async fn decompose_task(
        &self,
        change_id: &ChangeId,
        file_groups: &[Vec<String>],
    ) -> Result<SplitResult> {
        let dag = DagOperations::new(self.executor.clone());
        dag.split_by_files(change_id, file_groups).await
    }
}
```

#### Extend: `crates/hox-cli/src/main.rs`

```rust
/// DAG manipulation commands
Dag {
    #[command(subcommand)]
    action: DagCommands,
},

enum DagCommands {
    /// Make sequential changes parallel
    Parallelize {
        /// Revset range to parallelize (e.g., "abc..xyz")
        revset: String,
    },
    /// Absorb working copy fixes to correct ancestors
    Absorb {
        /// Specific files to absorb (empty = all)
        paths: Vec<String>,
    },
    /// Split a change by files
    Split {
        /// Change ID to split
        change_id: String,
        /// File patterns for first group (rest goes to second change)
        files: Vec<String>,
    },
    /// Squash a change into its parent
    Squash {
        /// Change ID to squash
        change_id: String,
    },
}
```

### Acceptance Criteria

- [ ] `DagOperations::parallelize()` restructures sequential changes into parallel
- [ ] `DagOperations::absorb()` distributes fixes to correct ancestor branches
- [ ] `DagOperations::split_by_files()` decomposes tasks by file groups
- [ ] `DagOperations::squash()` consolidates changes
- [ ] Orchestrator exposes `optimize_dag()`, `absorb_fixes()`, `decompose_task()`
- [ ] CLI commands for all DAG operations
- [ ] All operations create recovery points (Phase 2) before executing
- [ ] Bookmarks tracked through rewrites verified (Phase 1 dependency)

### Testing Checklist

- [ ] Mock test: `parallelize()` calls `jj parallelize {revset}`
- [ ] Mock test: `absorb()` calls `jj absorb` with optional path args
- [ ] Mock test: `split_by_files()` calls `jj split -r {id} {files}`
- [ ] Mock test: `squash()` calls `jj squash -r {id}`
- [ ] Mock test: `squash_into()` calls `jj squash --from {src} --into {tgt}`
- [ ] Unit test: `ParallelizeResult` parsing from jj output
- [ ] Integration test: create 3 sequential changes, parallelize, verify DAG structure

---

## Phase 5: Backpressure Enhancement

**Priority:** MEDIUM
**Business Value:** The current backpressure system (`crates/hox-orchestrator/src/backpressure.rs`) shells out to `cargo test`, `cargo clippy`, etc. directly. JJ's `jj fix` command can run formatters/linters retroactively on historical commits and auto-rebase descendants. Integrating `jj fix` eliminates formatting-only conflicts and keeps the entire commit chain clean.
**Estimated Effort:** 1-2 days
**Depends on:** None (can run in parallel with other phases)

### What Changes

#### Extend: `crates/hox-orchestrator/src/backpressure.rs`

Add `jj fix` as a step in the backpressure pipeline. Insert after existing checks:

```rust
use hox_jj::JjExecutor;

/// Run jj fix to auto-format all mutable commits
///
/// This applies configured formatters to the commit chain,
/// preventing formatting-only conflicts between agents.
pub async fn run_jj_fix<E: JjExecutor>(executor: &E, change_id: Option<&str>) -> Result<FixResult> {
    let args = match change_id {
        Some(id) => vec!["fix", "-s", id],
        None => vec!["fix"],
    };

    let output = executor.exec(&args).await?;

    Ok(FixResult {
        success: output.success,
        output: if output.success {
            output.stdout
        } else {
            output.stderr
        },
    })
}

#[derive(Debug, Clone)]
pub struct FixResult {
    pub success: bool,
    pub output: String,
}

/// Enhanced backpressure that includes jj fix
pub async fn run_all_checks_with_fix<E: JjExecutor>(
    workspace_path: &Path,
    executor: &E,
    change_id: Option<&str>,
) -> Result<BackpressureResult> {
    // Run jj fix FIRST to clean formatting
    let fix_result = run_jj_fix(executor, change_id).await;
    if let Err(e) = &fix_result {
        tracing::warn!("jj fix failed (non-fatal): {}", e);
    }

    // Then run standard checks (tests, lints, builds)
    run_all_checks(workspace_path)
}
```

#### Configuration Support

Add jj fix configuration guidance. When `hox init` runs, suggest adding fix configuration:

```rust
// In cmd_init(), add after creating .hox directory:
println!("\nTo enable auto-formatting with jj fix, add to .jj/repo/config.toml:");
println!("  [fix.tools.rustfmt]");
println!("  command = [\"rustfmt\", \"--edition\", \"2021\"]");
println!("  patterns = [\"glob:*.rs\"]");
```

#### Extend: `crates/hox-orchestrator/src/loop_engine.rs`

Replace the backpressure call (around line 87 and 184) to use the enhanced version:

```rust
// Before:
let mut backpressure = run_all_checks(&self.workspace_path)?;

// After:
let mut backpressure = run_all_checks_with_fix(
    &self.workspace_path,
    &self.executor,
    task.change_id.as_deref(),
).await?;
```

### Acceptance Criteria

- [ ] `run_jj_fix()` function wraps `jj fix` command
- [ ] `run_all_checks_with_fix()` runs jj fix before standard checks
- [ ] jj fix failures are non-fatal (warn and continue)
- [ ] Loop engine uses enhanced backpressure
- [ ] `hox init` prints jj fix configuration guidance
- [ ] Existing backpressure without jj fix still works (graceful fallback)

### Testing Checklist

- [ ] Mock test: `run_jj_fix()` calls `jj fix` or `jj fix -s {id}`
- [ ] Mock test: jj fix failure does not block other checks
- [ ] Unit test: `run_all_checks_with_fix()` runs fix then checks
- [ ] Integration test: create change with formatting issues, run fix, verify clean

---

## Phase 6: Advanced Revsets & Query Migration

**Priority:** MEDIUM
**Business Value:** Current revset queries use `description(glob:...)` which is O(n) -- every query scans all change descriptions. After Phase 1 (bookmarks), queries can use `bookmarks(glob:...)` which is indexed and O(1). This phase migrates all queries and adds power queries needed for sophisticated orchestration.
**Estimated Effort:** 2 days
**Depends on:** Phase 1 (bookmark-based queries)

### What Changes

#### Extend: `crates/hox-jj/src/revsets.rs`

The `RevsetQueries` struct currently has 12 methods, all using `description(glob:...)`. Add bookmark-based equivalents and deprecate the description-based ones:

```rust
impl<E: JjExecutor> RevsetQueries<E> {
    // ---- Bookmark-based queries (preferred when bookmarks are set up) ----

    /// Find ready tasks using bookmarks
    /// Revset: heads(bookmarks(glob:"task/*")) - conflicts() - ancestors(conflicts())
    pub async fn ready_tasks_v2(&self) -> Result<Vec<ChangeId>> {
        self.query("heads(bookmarks(glob:\"task/*\")) - conflicts() - ancestors(conflicts())")
            .await
    }

    /// Find agent's active work using bookmarks
    /// Revset: bookmarks(glob:"agent/{name}/*") & ~description(glob:"Status: done")
    pub async fn agent_active_work(&self, agent_name: &str) -> Result<Vec<ChangeId>> {
        let revset = format!(
            "bookmarks(glob:\"agent/{}/*\") & ~description(glob:\"Status: done\")",
            agent_name
        );
        self.query(&revset).await
    }

    /// Find parallelizable tasks (independent heads, no merges, no conflicts)
    pub async fn parallelizable_tasks(&self) -> Result<Vec<ChangeId>> {
        self.query("heads(mutable()) & ~merges() & ~conflicts()")
            .await
    }

    /// Find what blocks a specific task (conflicting ancestors)
    pub async fn blocking_conflicts(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        let revset = format!("ancestors({}) & mutable() & conflicts()", change_id);
        self.query(&revset).await
    }

    /// Find empty changes (abandoned tasks)
    pub async fn empty_changes(&self) -> Result<Vec<ChangeId>> {
        self.query("empty() & mutable()").await
    }

    /// Find changes touching specific files
    pub async fn changes_touching_file(&self, path: &str) -> Result<Vec<ChangeId>> {
        let revset = format!("file(\"{}\")", path);
        self.query(&revset).await
    }

    /// Safe reference that doesn't error if change is missing
    pub async fn present(&self, change_id: &ChangeId) -> Result<Option<ChangeId>> {
        let revset = format!("present({})", change_id);
        let results = self.query(&revset).await?;
        Ok(results.into_iter().next())
    }

    /// Find connected component (task subgraph)
    pub async fn connected_component(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        let revset = format!("connected({})", change_id);
        self.query(&revset).await
    }

    /// Find most recent N changes matching criteria
    pub async fn latest(&self, revset: &str, count: usize) -> Result<Vec<ChangeId>> {
        let full_revset = format!("latest({}, {})", revset, count);
        self.query(&full_revset).await
    }
}
```

#### Extend: `crates/hox-orchestrator/src/orchestrator.rs`

Migrate internal queries to use bookmark-based versions. For example, `check_align_requests()` (line 251) and `integrate()` (line 352):

```rust
// Before (in integrate):
let agent_changes = queries.by_orchestrator(&self.config.id.to_string()).await?;

// After:
let agent_changes = queries
    .orchestrator_by_bookmark(&self.config.id.to_string())
    .await
    .or_else(|_| {
        // Fallback to description-based query if bookmarks not set
        queries.by_orchestrator(&self.config.id.to_string())
    })?;
```

#### Extend: `crates/hox-cli/src/main.rs`

Enhance the `hox status` command with richer queries:

```rust
// In cmd_status():
// Add parallelizable task count
let parallelizable = queries.parallelizable_tasks().await?;
println!("Parallelizable: {}", parallelizable.len());

// Add empty/abandoned count
let empty = queries.empty_changes().await?;
if !empty.is_empty() {
    println!("Empty (abandoned): {}", empty.len());
}
```

### Acceptance Criteria

- [ ] All new bookmark-based query methods added to `RevsetQueries`
- [ ] Orchestrator migrated to prefer bookmark queries with description fallback
- [ ] Power queries: parallelizable, blocking_conflicts, empty, file-based, connected
- [ ] CLI status enhanced with new queries
- [ ] `present()` wrapper for safe references
- [ ] `latest()` wrapper for recency queries
- [ ] All existing description-based queries remain functional

### Testing Checklist

- [ ] Mock test: Each new revset method generates correct revset string
- [ ] Mock test: Fallback from bookmark to description queries works
- [ ] Unit test: `parallelizable_tasks()` revset string is correct
- [ ] Unit test: `blocking_conflicts()` revset includes conflicts() predicate
- [ ] Integration test: Create bookmarked tasks, query them with new methods

---

## Phase 7: Speculative Execution & Audit Trails

**Priority:** MEDIUM-LOW
**Business Value:** `jj duplicate` enables trying multiple approaches to the same task in parallel. `jj evolog` provides agent audit trails. `jj backout` enables safe reversion. These are "nice to have" features that improve observability and enable advanced orchestration patterns.
**Estimated Effort:** 2 days
**Depends on:** Phase 1 (bookmarks), Phase 2 (recovery)

### What Changes

#### Extend: `crates/hox-jj/src/dag.rs`

Add speculative execution and audit operations:

```rust
impl<E: JjExecutor> DagOperations<E> {
    // --- jj duplicate ---

    /// Duplicate a change for speculative execution
    ///
    /// Creates a copy that can be worked on independently.
    /// The original remains untouched.
    pub async fn duplicate(
        &self,
        change_id: &ChangeId,
        destination: Option<&ChangeId>,
    ) -> Result<ChangeId>;

    // --- jj backout ---

    /// Create a change that undoes the effect of another change
    ///
    /// Safer than rollback -- creates new history instead of rewriting.
    pub async fn backout(&self, change_id: &ChangeId) -> Result<ChangeId>;

    // --- jj evolog ---

    /// Get the evolution log for a change (all rewrites, amends, etc.)
    ///
    /// Useful for agent audit trails -- see how a task evolved.
    pub async fn evolution_log(&self, change_id: &ChangeId) -> Result<Vec<EvolutionEntry>>;

    // --- jj simplify-parents ---

    /// Clean up redundant parent relationships after merges
    pub async fn simplify_parents(&self, change_id: &ChangeId) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct EvolutionEntry {
    pub commit_id: CommitId,
    pub description: String,
    pub timestamp: String,
}
```

**JJ commands used:**
```bash
jj duplicate {change_id} [-d {destination}]
jj backout -r {change_id}
jj evolog -r {change_id} -T 'commit_id ++ "\t" ++ description ++ "\t" ++ committer.timestamp() ++ "\n"' --no-graph
jj simplify-parents -r {change_id}
```

#### New File: `crates/hox-orchestrator/src/speculative.rs`

```rust
/// Manager for speculative execution patterns
pub struct SpeculativeExecutor<E: JjExecutor> {
    dag: DagOperations<E>,
    bookmark_mgr: BookmarkManager<E>,
}

impl<E: JjExecutor + Clone> SpeculativeExecutor<E> {
    /// Try multiple approaches to a task in parallel
    ///
    /// 1. Duplicate the task N times
    /// 2. Assign each duplicate to a different agent with different strategies
    /// 3. Compare results and pick the best
    pub async fn try_approaches(
        &self,
        change_id: &ChangeId,
        strategies: &[String],
    ) -> Result<Vec<ChangeId>>;

    /// Compare speculative results and pick winner
    pub async fn evaluate_and_pick(
        &self,
        candidates: &[ChangeId],
    ) -> Result<ChangeId>;
}
```

### Acceptance Criteria

- [ ] `duplicate()` creates copy via `jj duplicate`
- [ ] `backout()` creates reverse change via `jj backout`
- [ ] `evolution_log()` returns change history via `jj evolog`
- [ ] `simplify_parents()` cleans DAG via `jj simplify-parents`
- [ ] `SpeculativeExecutor` can duplicate tasks for parallel approach testing
- [ ] CLI commands for duplicate, backout, evolog

### Testing Checklist

- [ ] Mock test: `duplicate()` calls `jj duplicate {id}`
- [ ] Mock test: `backout()` calls `jj backout -r {id}`
- [ ] Mock test: `evolution_log()` parses `jj evolog` output
- [ ] Unit test: `SpeculativeExecutor::try_approaches()` creates N duplicates

---

## Cross-Cutting: Dual Metadata Path

**Context:** Hox currently parses metadata from JJ change descriptions using regex (`crates/hox-jj/src/metadata.rs`). The jj-dev fork adds native metadata fields to JJ commits. Until jj-dev is complete and deployed, Hox must support both paths.

### Strategy: Trait-Based Abstraction

#### New File: `crates/hox-jj/src/metadata_provider.rs`

```rust
use async_trait::async_trait;

/// Abstraction over metadata storage backends
///
/// Two implementations:
/// 1. DescriptionMetadataProvider - Current regex parsing (bridge)
/// 2. NativeMetadataProvider - jj-dev native fields (future)
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    async fn read(&self, change_id: &ChangeId) -> Result<HoxMetadata>;
    async fn write(&self, change_id: &ChangeId, metadata: &HoxMetadata) -> Result<()>;
}

/// Current implementation: parse from description text
pub struct DescriptionMetadataProvider<E: JjExecutor> {
    manager: MetadataManager<E>,
}

#[async_trait]
impl<E: JjExecutor + Send + Sync> MetadataProvider for DescriptionMetadataProvider<E> {
    async fn read(&self, change_id: &ChangeId) -> Result<HoxMetadata> {
        self.manager.read(change_id).await
    }

    async fn write(&self, change_id: &ChangeId, metadata: &HoxMetadata) -> Result<()> {
        self.manager.set(change_id, metadata).await
    }
}

/// Future implementation: use jj-dev native metadata fields
/// When jj-dev ships, this will use:
///   jj describe --set-priority high --set-status in_progress
///   jj log -T 'priority ++ "\t" ++ status ++ ...'
pub struct NativeMetadataProvider<E: JjExecutor> {
    executor: E,
}

// Implementation stubbed -- will be filled when jj-dev ships
```

#### Migration Path

The trait is designed so that switching from description-based to native metadata requires:

1. Implement `NativeMetadataProvider` (fill the stub)
2. Change the provider construction from `DescriptionMetadataProvider::new()` to `NativeMetadataProvider::new()`
3. Run a one-time migration to move metadata from descriptions to native fields

No other code needs to change. Every consumer works through `dyn MetadataProvider`.

### Integration Points

Every module that currently uses `MetadataManager` directly should be updated to accept `dyn MetadataProvider`:

| File | Current Usage | Migration |
|------|--------------|-----------|
| `orchestrator.rs:160` | `MetadataManager::new(executor).set(...)` | Accept `&dyn MetadataProvider` |
| `orchestrator.rs:214` | `MetadataManager::new(executor).set(...)` | Accept `&dyn MetadataProvider` |
| `loop_engine.rs:326` | `MetadataManager::new(executor).read(...)` | Accept `&dyn MetadataProvider` |
| `loop_engine.rs:362` | `self.executor.exec(["describe"...])` | Use provider's `write()` |

**This migration can happen incrementally.** It is NOT a prerequisite for any phase -- it is a parallel improvement track.

---

## Dependency Map

```
Phase 1: Bookmarks     Phase 2: Rollback     Phase 5: jj fix
  (CRITICAL)              (HIGH)              (MEDIUM)
      |                     |                     |
      v                     v                     |
Phase 4: DAG Ops     Phase 3: Conflicts          |
  (HIGH)               (HIGH)                    |
      |                     |                     |
      v                     v                     v
Phase 6: Advanced Revsets         Phase 5 integrates
  (MEDIUM)                        into backpressure
      |
      v
Phase 7: Speculative
  (LOW-MEDIUM)

Cross-cutting: Dual Metadata Path (parallel, any time)
```

**Recommended execution order for maximum parallelism:**

| Slot | Agent A | Agent B |
|------|---------|---------|
| Week 1 | Phase 1 (Bookmarks) | Phase 2 (Rollback) |
| Week 2 | Phase 4 (DAG Ops) | Phase 3 (Conflicts) |
| Week 2 | -- | Phase 5 (jj fix) |
| Week 3 | Phase 6 (Revsets) | Cross-cutting: Metadata Provider |
| Week 3-4 | Phase 7 (Speculative) | -- |

---

## Testing Strategy

### Unit Tests (All Phases)

Every new function gets a mock test using `MockJjExecutor`. The mock verifies:
1. Correct JJ command arguments are constructed
2. Output is parsed correctly
3. Error cases are handled (command failure, malformed output)

### Integration Tests

Create a shared test fixture module at `crates/hox-jj/tests/common/mod.rs`:

```rust
/// Create a temporary JJ repository for integration testing
pub async fn create_test_repo() -> (TempDir, JjCommand) {
    let dir = TempDir::new().unwrap();
    // jj git init
    let executor = JjCommand::new(dir.path());
    executor.exec(&["git", "init"]).await.unwrap();
    (dir, executor)
}

/// Create a test repo with some changes and bookmarks
pub async fn create_test_repo_with_tasks() -> (TempDir, JjCommand) {
    let (dir, executor) = create_test_repo().await;
    // Create some changes with metadata
    executor.exec(&["new", "-m", "Task: First task\nPriority: high\nStatus: open"]).await.unwrap();
    executor.exec(&["bookmark", "create", "task/first"]).await.unwrap();
    // ...
    (dir, executor)
}
```

### CI Considerations

- Integration tests require `jj` binary installed
- Mark integration tests with `#[cfg(feature = "integration")]` or `#[ignore]`
- Run with `cargo test -- --ignored` in CI where jj is available

---

## File Summary

### New Files

| File | Phase | Purpose |
|------|-------|---------|
| `crates/hox-jj/src/bookmarks.rs` | 1 | Bookmark CRUD and Hox naming conventions |
| `crates/hox-orchestrator/src/conflict_resolver.rs` | 3 | Conflict analysis, strategy, and resolution pipeline |
| `crates/hox-jj/src/dag.rs` | 4, 7 | DAG manipulation (parallelize, absorb, split, squash, duplicate, backout, evolog) |
| `crates/hox-orchestrator/src/recovery.rs` | 2 | Agent rollback and recovery points |
| `crates/hox-orchestrator/src/speculative.rs` | 7 | Speculative execution patterns |
| `crates/hox-jj/src/metadata_provider.rs` | Cross-cutting | Trait abstraction for dual metadata path |
| `crates/hox-jj/tests/common/mod.rs` | All | Shared integration test fixtures |

### Extended Files

| File | Phases | Changes |
|------|--------|---------|
| `crates/hox-jj/src/lib.rs` | 1, 2, 4, 7 | Add module exports for bookmarks, dag, metadata_provider; extend oplog exports |
| `crates/hox-jj/src/oplog.rs` | 2 | Add `OpManager` with undo/restore/revert/snapshot |
| `crates/hox-jj/src/revsets.rs` | 1, 6 | Add bookmark-based queries, power queries, safe references |
| `crates/hox-orchestrator/src/lib.rs` | 2, 3, 7 | Export recovery, conflict_resolver, speculative modules |
| `crates/hox-orchestrator/src/orchestrator.rs` | 1, 3, 4, 6 | Bookmark creation on spawn, conflict resolution, DAG methods, query migration |
| `crates/hox-orchestrator/src/backpressure.rs` | 5 | Add `run_jj_fix()` and enhanced check pipeline |
| `crates/hox-orchestrator/src/loop_engine.rs` | 2, 5 | Recovery points around iterations, enhanced backpressure |
| `crates/hox-cli/src/main.rs` | 1, 2, 4 | Add bookmark, rollback, dag subcommands |
| `crates/hox-core/src/error.rs` | 2, 3 | Add `HoxError::Rollback`, `HoxError::ConflictResolution` variants |
