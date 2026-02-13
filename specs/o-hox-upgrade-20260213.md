# Plan: o-hox-upgrade-20260213

## Objective
Implement the full Hox upgrade plan (10 improvements across 5 phases) to transform Hox from a research prototype into a production-ready standalone JJ-native multi-agent orchestration tool.

## Critical Path Corrections
- Loop engine is at `crates/hox-orchestrator/src/loop_engine.rs` (NOT hox-agent)
- File executor is at `crates/hox-agent/src/file_executor.rs` (NOT hox-orchestrator)

## Adjustments from Review
1. Protected files: Fully configurable via .hox/config.toml, no hardcoded baseline
2. Budget tracking: Use existing `total_usage` accumulation, add enforcement checks. No AtomicUsize.
3. Extra crates (hox-planning, hox-browser, hox-dashboard, hox-viz): Leave alone
4. jj-lib (Phase 5): Keep in scope, feature-flagged

## Team Members
| Name | Role | Agent Type | Model | Tasks |
|------|------|-----------|-------|-------|
| core-types | Create shared types in hox-core | engineer | sonnet | 1 |
| budget-enforcer | Add budget enforcement | engineer | sonnet | 1 |
| fail-open-agent | Apply fail-open wrappers | engineer | sonnet | 1 |
| config-agent | Add config file system | engineer | sonnet | 1 |
| state-hooks | Create state machine + hooks | engineer | sonnet | 1 |
| integrator-p2 | Wire state machine into orchestrator | engineer | sonnet | 1 |
| structured-output | Replace XML with tool_use | engineer | sonnet | 1 |
| backpressure-cal | Calibrate backpressure | engineer | sonnet | 1 |
| pattern-extractor | Add pattern extraction | engineer | sonnet | 1 |
| auto-advancer | Add phase auto-advancement | engineer | sonnet | 1 |
| jj-lib-agent | Add jj-lib integration | engineer | sonnet | 1 |
| final-validator | Validate full workspace | validator | haiku | 1 |

## Tasks

### Task 0: Create foundational types in hox-core
- **ID**: core-types
- **Phase**: 0 (blocking)
- **Dependencies**: none
- **Owner**: core-types
- **Scope**:
  - READ: `crates/hox-core/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-core/src/config.rs`, `crates/hox-core/src/fail_open.rs`, `crates/hox-core/src/error.rs`, `crates/hox-core/src/lib.rs`
  - FORBIDDEN: All other crates
- **Acceptance Criteria**:
  - New file `crates/hox-core/src/config.rs` with `HoxConfig`, `LoopDefaults`, `BackpressureConfig`, `SlowCheck`, `ModelConfig` structs
  - All structs derive `Debug, Clone, Serialize, Deserialize`
  - `HoxConfig::load_or_default(repo_root: &Path) -> Result<Self>` loads from `.hox/config.toml`
  - `HoxConfig::write_default(repo_root: &Path) -> Result<()>` writes default config
  - Sensible defaults: max_iterations=20, default model="claude-sonnet-4", fast_checks=["cargo check", "cargo clippy"]
  - Protected files are fully configurable (default: .git, .jj, .env, Cargo.lock, .secrets, .gitignore) — NO hardcoded baseline
  - New file `crates/hox-core/src/fail_open.rs` with `fail_open()` async wrapper and `fail_open_with_retries()` with exponential backoff
  - `fail_open` logs warnings via `tracing::warn!` on failure, returns `Ok(None)`
  - `fail_open_with_retries` retries up to N times with exponential backoff (100ms * attempt)
  - `crates/hox-core/src/error.rs` has new variants: `BudgetExceeded(String)`, `InvalidToolInput(String)`, `UnknownTool(String)`, `ProtectedFile(String)`
  - `crates/hox-core/src/lib.rs` re-exports `config` and `fail_open` modules
  - `cargo check -p hox-core` passes
  - Add `toml` dependency to hox-core Cargo.toml

### Task 1: Budget enforcement in loop engine
- **ID**: budget-enforce
- **Phase**: 1
- **Dependencies**: core-types
- **Owner**: budget-enforcer
- **Scope**:
  - READ: `crates/hox-orchestrator/src/loop_engine.rs`, `crates/hox-agent/src/types.rs`, `crates/hox-core/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/loop_engine.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`
- **Acceptance Criteria**:
  - After each iteration in `LoopEngine::run()`, check cumulative `total_usage` against `LoopConfig::max_tokens`
  - Check cumulative cost against `LoopConfig::max_budget_usd` using pricing: input=$3/MTok, output=$15/MTok for Sonnet
  - Return `HoxError::BudgetExceeded` with message showing actual vs limit when exceeded
  - Context freshness warning at 60% of context window (log warning, don't error)
  - Usage summary logged at end of loop
  - `cargo check -p hox-orchestrator` passes

### Task 2: Fail-open audit
- **ID**: fail-open-audit
- **Phase**: 1
- **Dependencies**: core-types
- **Owner**: fail-open-agent
- **Scope**:
  - READ: `crates/hox-orchestrator/src/activity_logger.rs`, `crates/hox-jj/src/oplog.rs`, `crates/hox-core/src/fail_open.rs`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/activity_logger.rs`, `crates/hox-jj/src/oplog.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - Activity logger operations wrapped with `fail_open()` — logging failures don't crash the tool
  - OpLog polling wrapped with `fail_open()` — poll failures result in retry after delay, not crash
  - Agent execution does NOT fail-open (this is business logic)
  - Backpressure checks do NOT fail-open (this is correctness)
  - All wrapped operations log warnings via `tracing::warn!`
  - `cargo check -p hox-orchestrator -p hox-jj` passes

### Task 3: Config file system
- **ID**: config-system
- **Phase**: 1
- **Dependencies**: core-types
- **Owner**: config-agent
- **Scope**:
  - READ: `crates/hox-agent/src/file_executor.rs`, `crates/hox-cli/src/`, `crates/hox-core/src/config.rs`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-agent/src/file_executor.rs`, `crates/hox-cli/src/main.rs`, `crates/hox-agent/Cargo.toml`
  - FORBIDDEN: `crates/hox-orchestrator/`, `crates/hox-jj/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - `file_executor.rs` loads protected files from `HoxConfig` instead of hardcoded `PROTECTED_FILES` const
  - `execute_file_operations()` accepts config parameter or loads config from workspace path
  - `hox init` subcommand writes default `.hox/config.toml` to current directory
  - Config is loaded once at startup and passed through to components that need it
  - `cargo check -p hox-agent -p hox-cli` passes

### Task 4: State machine and hooks
- **ID**: state-hooks
- **Phase**: 2
- **Dependencies**: budget-enforce, fail-open-audit, config-system (all Phase 1)
- **Owner**: state-hooks
- **Scope**:
  - READ: `crates/hox-orchestrator/src/orchestrator.rs`, `crates/hox-orchestrator/src/loop_engine.rs`, `crates/hox-orchestrator/src/phases.rs`, `crates/hox-core/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/state_machine.rs`, `crates/hox-orchestrator/src/hooks.rs`, `crates/hox-orchestrator/src/lib.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - New `state_machine.rs` with `State`, `Phase`, `Event`, `Action` enums
  - Pure `transition(state: State, event: Event) -> (State, Vec<Action>)` function — NO I/O
  - States: Idle, Planning, Executing, Integrating, Validating, Complete, Failed
  - Events: StartOrchestration, PlanningComplete, PhaseComplete, AllTasksComplete, IntegrationConflict, IntegrationClean, ValidationPassed, ValidationFailed, Error
  - Actions: SpawnPlanningAgent, SpawnTaskAgent, CreateMerge, ResolveConflicts, SpawnValidator, LogActivity, RecordPattern
  - All invalid transitions go to Failed state (never panic)
  - Unit tests for happy path, error paths, invalid transitions
  - New `hooks.rs` with `PostToolsHook` trait, `HookContext`, `HookResult`, `HookPipeline`
  - `AutoCommitHook` and `SnapshotHook` implementations
  - `HookPipeline::execute_all()` runs hooks in order, fail-open
  - Both new modules exported from `crates/hox-orchestrator/src/lib.rs`
  - `cargo check -p hox-orchestrator` passes

### Task 5: Wire state machine and hooks into orchestrator
- **ID**: integrate-p2
- **Phase**: 2b
- **Dependencies**: state-hooks
- **Owner**: integrator-p2
- **Scope**:
  - READ: `crates/hox-orchestrator/src/state_machine.rs`, `crates/hox-orchestrator/src/hooks.rs`, `crates/hox-orchestrator/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/orchestrator.rs`, `crates/hox-orchestrator/src/loop_engine.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - `Orchestrator` struct uses `State` from state_machine.rs
  - `Orchestrator::run()` follows event loop: get event → pure transition → execute actions
  - Terminal states (Complete, Failed) break the loop
  - `LoopEngine` uses `HookPipeline` instead of inline JJ operations
  - AutoCommitHook and SnapshotHook registered in pipeline
  - `cargo check -p hox-orchestrator` passes
  - Existing behavior preserved (no regressions)

### Task 6: Structured output (tool_use API)
- **ID**: structured-output
- **Phase**: 3
- **Dependencies**: integrate-p2
- **Owner**: structured-output
- **Scope**:
  - READ: `crates/hox-agent/src/client.rs`, `crates/hox-agent/src/file_executor.rs`, `crates/hox-agent/src/types.rs`, `crates/hox-agent/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-agent/src/client.rs`, `crates/hox-agent/src/file_executor.rs`, `crates/hox-agent/src/types.rs`, `crates/hox-agent/src/lib.rs`
  - FORBIDDEN: `crates/hox-orchestrator/`, `crates/hox-jj/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - Tool definitions for: read_file, write_file, edit_file, run_command
  - `send_message_with_tools()` method on `AgentClient` uses Anthropic tool_use API
  - New types: `ToolCall { id, name, input }`, `ToolResult { tool_id, success, output }`, `AgentResponse { thinking, tool_calls, usage }`
  - `FileExecutor` has `execute_tools(tool_calls: &[ToolCall]) -> Result<Vec<ToolResult>>` method
  - Protected file check via `check_protected()` before writes
  - No XML parsing remains in file_executor.rs
  - `cargo check -p hox-agent` passes

### Task 7: Backpressure calibration
- **ID**: backpressure-cal
- **Phase**: 3
- **Dependencies**: integrate-p2
- **Owner**: backpressure-cal
- **Scope**:
  - READ: `crates/hox-orchestrator/src/backpressure.rs`, `crates/hox-core/src/config.rs`, `crates/hox-agent/src/types.rs`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/backpressure.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - `BackpressureEngine` struct with `config: BackpressureConfig` and `CheckHistory`
  - Fast checks run every iteration
  - Slow checks run on configurable schedule (`every_n_iterations`)
  - Adaptive escalation: if 2+ fast check failures in last 3 iterations, force slow checks
  - Force run if 2x normal interval since last slow check
  - Track check timing (fast vs slow duration)
  - `format_for_prompt()` formats failures for agent consumption
  - Language-aware defaults: `detect_language()` checks for Cargo.toml, pyproject.toml, package.json
  - Default configs for Rust (cargo check/clippy fast, cargo test slow), Python (ruff/mypy fast, pytest slow), JS (npm lint fast, npm test slow)
  - `cargo check -p hox-orchestrator` passes

### Task 8: Pattern extraction
- **ID**: pattern-extract
- **Phase**: 4
- **Dependencies**: structured-output, backpressure-cal (all Phase 3)
- **Owner**: pattern-extractor
- **Scope**:
  - READ: `crates/hox-evolution/src/patterns.rs`, `crates/hox-evolution/src/`, `crates/hox-core/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-evolution/src/patterns.rs`, `crates/hox-evolution/src/lib.rs`
  - FORBIDDEN: `crates/hox-orchestrator/`, `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`
- **Acceptance Criteria**:
  - `PatternExtractor` struct with reference to `PatternStore`
  - `extract_from_trace(trace: &OrchestrationTrace) -> Vec<Pattern>` identifies:
    - Fast convergence patterns (< 10 iterations)
    - Effective agent assignment patterns (> 80% success rate)
  - `suggest(context: &TaskContext) -> Vec<Suggestion>` recommends patterns with > 60% confidence
  - `Pattern` struct with name, description, confidence, applicable_contexts
  - `Suggestion` struct with pattern_name, message, actionable flag
  - `cargo check -p hox-evolution` passes

### Task 9: Phase auto-advancement
- **ID**: auto-advance
- **Phase**: 4
- **Dependencies**: structured-output, backpressure-cal (all Phase 3)
- **Owner**: auto-advancer
- **Scope**:
  - READ: `crates/hox-orchestrator/src/phases.rs`, `crates/hox-orchestrator/src/state_machine.rs`, `crates/hox-core/src/`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-orchestrator/src/phases.rs`
  - FORBIDDEN: `crates/hox-agent/`, `crates/hox-jj/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - `PhaseManager::check_auto_advance()` returns true when all phase tasks are Done
  - `PhaseManager::maybe_advance()` advances to next phase if current is complete
  - Returns `Option<Phase>` — Some if advanced, None if not ready
  - Logs phase transitions via `tracing::info!`
  - Integrates with state machine `Event::PhaseComplete`
  - `cargo check -p hox-orchestrator` passes

### Task 10: jj-lib direct integration
- **ID**: jj-lib
- **Phase**: 5
- **Dependencies**: pattern-extract, auto-advance (all Phase 4)
- **Owner**: jj-lib-agent
- **Scope**:
  - READ: `crates/hox-jj/src/`, `crates/hox-jj/Cargo.toml`, `docs/IMPLEMENTATION_PLAN.md`
  - WRITE: `crates/hox-jj/src/lib_backend.rs`, `crates/hox-jj/src/lib.rs`, `crates/hox-jj/Cargo.toml`
  - FORBIDDEN: `crates/hox-orchestrator/`, `crates/hox-agent/`, `crates/hox-cli/`, `crates/hox-core/`
- **Acceptance Criteria**:
  - New `lib_backend.rs` with `JjLibExecutor` struct behind `#[cfg(feature = "jj-lib-integration")]`
  - Implements `JjExecutor` trait
  - Hot path (describe, log) uses jj-lib directly
  - Fallback to subprocess for unsupported commands
  - `jj-lib` as optional dependency in Cargo.toml: `jj-lib = { version = "0.20", optional = true }`
  - Feature flag: `jj-lib-integration = ["dep:jj-lib"]`
  - When feature is disabled, `JjLibExecutor` type aliases to `JjCommand` (subprocess fallback)
  - `cargo check -p hox-jj` passes (without feature flag)
  - `cargo check -p hox-jj --features jj-lib-integration` passes (if jj-lib is available)

### Task 11: Final integration validation
- **ID**: final-validate
- **Phase**: 6
- **Dependencies**: jj-lib
- **Owner**: final-validator
- **Scope**:
  - READ: entire workspace
  - WRITE: none (read-only validation)
- **Acceptance Criteria**:
  - `cargo check --workspace` passes
  - `cargo test --workspace` passes (or failures are pre-existing)
  - `cargo clippy --workspace` has no new warnings
  - All new files exist and are non-empty
  - No XML parsing remains in file_executor.rs
  - No hardcoded protected files (configurable only)

## Relevant Files

### New Files (created by this plan)
- `crates/hox-core/src/config.rs`
- `crates/hox-core/src/fail_open.rs`
- `crates/hox-orchestrator/src/state_machine.rs`
- `crates/hox-orchestrator/src/hooks.rs`
- `crates/hox-jj/src/lib_backend.rs`

### Modified Files
- `crates/hox-core/src/error.rs`
- `crates/hox-core/src/lib.rs`
- `crates/hox-core/Cargo.toml`
- `crates/hox-orchestrator/src/loop_engine.rs`
- `crates/hox-orchestrator/src/orchestrator.rs`
- `crates/hox-orchestrator/src/backpressure.rs`
- `crates/hox-orchestrator/src/activity_logger.rs`
- `crates/hox-orchestrator/src/phases.rs`
- `crates/hox-orchestrator/src/lib.rs`
- `crates/hox-agent/src/file_executor.rs`
- `crates/hox-agent/src/client.rs`
- `crates/hox-agent/src/types.rs`
- `crates/hox-agent/src/lib.rs`
- `crates/hox-agent/Cargo.toml`
- `crates/hox-jj/src/lib.rs`
- `crates/hox-jj/Cargo.toml`
- `crates/hox-evolution/src/patterns.rs`
- `crates/hox-evolution/src/lib.rs`
- `crates/hox-cli/src/main.rs`
