# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Core Paradigm

**Tasks ARE jj changes.** This is a JJ-native orchestration system where:
- Tasks = jj changes (change IDs are primary identifiers)
- Dependencies = DAG ancestry (parent-child relationships)
- Assignments = bookmarks (e.g., `agent-42/task-xyz`)
- Metadata = structured descriptions + `.tasks/metadata.jsonl`

The VCS itself is the source of truth. SQLite (Turso) serves as a query cache, not the authoritative data store.

## Build Commands

```bash
cargo build                    # Build all crates
cargo build --release          # Release build
cargo test                     # Run all tests
cargo test -p hox-orchestrator # Test specific crate
cargo run --bin hox -- --help  # Run CLI
cargo install --path crates/hox-cli  # Install CLI globally
```

Run tests with logging:
```bash
RUST_LOG=debug cargo test -- --nocapture
```

## Workspace Structure

| Crate | Purpose |
|-------|---------|
| `hox-core` | Core types: `Task`, `TaskStatus`, `Priority`, `HoxError`, `AgentId` |
| `hox-jj` | JJ integration: metadata parsing, revsets, workspace management |
| `hox-orchestrator` | Orchestration: agent spawning, communication, handoffs |
| `hox-agent` | Agent runtime: circuit breaker, promises, file execution, artifacts |
| `hox-browser` | Browser automation: CDP sessions, screenshots, verification |
| `hox-viz` | 3D cyberpunk orchestration visualization |
| `hox-dashboard` | Ratatui-based observability TUI for orchestration monitoring |
| `hox-planning` | PRD generation, task decomposition, plan templates |
| `hox-validation` | Plan validation and constraint checking |
| `hox-metrics` | Metrics collection and reporting |
| `hox-evolution` | Plan evolution and refinement |
| `hox-isolation` | Pluggable isolation backends + safety rules |
| `hox-cli` | `hox` binary - CLI interface |

## Key Types

```rust
// hox-core/src/types.rs
pub enum TaskStatus { Open, InProgress, Blocked, Review, Done, Abandoned }
pub enum Priority { Critical=0, High=1, Medium=2, Low=3 }
pub struct Task { /* change-based task */ }
pub struct AgentId { orchestrator, id, name }
pub struct HandoffContext { /* agent state for handoffs */ }

// hox-core/src/config.rs
pub struct HoxConfig { /* repository-level configuration */ }

// hox-core/src/error.rs
pub type Result<T> = std::result::Result<T, HoxError>;

// hox-agent/src/types.rs
pub struct ToolCall { /* structured tool invocation */ }
pub struct ToolResult { /* tool execution result */ }
pub struct AgentResponse { /* structured agent output */ }

// hox-orchestrator/src/state_machine.rs
pub enum State { /* orchestrator state */ }
pub enum Event { /* state machine event */ }
pub enum Action { /* orchestrator action */ }

// hox-orchestrator/src/hooks.rs
pub trait PostToolsHook { /* hook pipeline trait */ }

// hox-orchestrator/src/backpressure.rs
pub struct BackpressureEngine { /* calibrated check scheduling */ }

// hox-evolution/src/patterns.rs
pub struct PatternExtractor { /* auto-extract patterns from traces */ }

// hox-jj/src/repo_mode.rs
pub enum RepoMode { Colocated, Native, NotInitialized }
pub struct ForkFeatures { metadata_only, read_only, forked_op_heads }

// hox-jj/src/batch_metadata.rs
pub struct MetadataBatch { /* batched metadata transactions */ }

// hox-planning/src/hox_prd.rs
pub struct Prd { /* flat PRD type with hox:prd:v1 sentinel */ }
pub struct Requirement { /* REQ-*/NFR-* typed requirement */ }
pub struct DecompositionHint { /* slice hint with file estimates */ }

// hox-planning/src/validator.rs
pub struct ValidationResult { errors, warnings }

// hox-planning/src/decomposition.rs
pub struct DecompositionResult { slices, shared_contracts, dependency_edges }

// hox-planning/src/agent.rs
pub struct PlanningAgent { /* three-mode planning orchestrator */ }
pub struct PlanningResult { prd, change_id, file_path, trace }

// hox-isolation/src/lib.rs
pub trait IsolationBackend { create, destroy, list }
pub struct IsolatedEnv { agent_id, workspace_path }

// hox-agent/src/claude_cli.rs
pub struct ClaudeCliBackend { /* claude CLI subprocess backend */ }

// hox-core/src/config.rs
pub enum AgentBackend { AnthropicApi, ClaudeCli }

// hox-orchestrator/src/dag_optimization.rs
pub struct DagOptimizer { /* parallelizable task grouping */ }
```

All errors use the unified `HoxError` enum. Never panic in production code.

## Architecture Layers

```
CLI (hox-cli)
    ↓
Planning (hox-planning) - PRD discovery, validation, decomposition → feeds orchestration
    ↓
Orchestration (hox-orchestrator) - state machine, hooks, backpressure, DAG optimization, agent spawning
    ↓
Isolation (hox-isolation) - pluggable backends (filesystem), safety rules enforcement
    ↓
Agent (hox-agent) - Anthropic API (tool_use) OR Claude CLI subprocess, circuit breaker
    ↓
JJ Integration (hox-jj) - metadata, revsets, workspaces, batch metadata, fork detection
    ├── jj-lib (feature-gated, reads only) → ReadonlyRepo for hot-path queries
    │       ↕ fallback
    └── CLI subprocess (writes + fallback) → Preserves oplog contract
    ↓
Core (hox-core) - Types, config (.hox/config.toml), fail-open utilities, errors
```

**jj-lib read/write policy:** jj-lib for reads, CLI for writes. One CLI invocation = one oplog entry. Reads that fail via jj-lib automatically fall back to CLI subprocess. The `jj-lib-integration` feature flag gates all jj-lib code; without it, everything uses CLI subprocess.

**jj-dev fork feature detection:** At runtime, hox probes the installed `jj` binary for fork capabilities. Two isolation tiers:
- **Tier 1 (upstream jj):** Workspace isolation + scheduling discipline + batch `jj describe` (reduces oplog writes)
- **Tier 2 (jj-dev fork):** All of Tier 1 plus W7 `--metadata-only` (~80% oplog staleness eliminated), W8 `--read-only` (safe orchestrator polling), W6 `ForkedOpHeadsStore` (100% per-agent oplog isolation). Auto-detected at runtime; no config change needed.

## Revset Patterns

Finding ready tasks:
```
heads(bookmarks(glob:"task-*")) - conflicts()
```

Finding what blocks a task:
```
ancestors(task-xyz) & mutable()
```

Finding what a task blocks:
```
descendants(task-xyz)
```

Finding agent's tasks:
```
bookmarks(glob:"agent-{id}/*")
```

## Structured Description Format

Task metadata lives in jj change descriptions:
```
Task: Implement VCS abstraction layer
Priority: 1
Status: in_progress
Agent: agent-42

## Context
Working on the VCS interface.

## Progress
- [x] Designed interface
- [ ] Implementing backend

## Files Touched
internal/vcs/vcs.go
```

## CLI Commands

```bash
hox plan [--description TEXT] [--auto] [--from-file FILE] [--output FILE]
                               # Run planning agent, produce PRD change
hox plan-validate <change-id>  # Validate PRD for completeness before orchestration
hox orchestrate --from-plan <change-id>  # Execute an existing PRD change
hox orchestrate --dry-run      # Show phases without executing
hox orchestrate --backend filesystem     # Explicit isolation backend selection
```

## Configuration

**`.hox/config.toml`** — repository-level config:
```toml
[workspace]
workspace_dir = ".hox-workspaces/"  # agent workspace root (default)

[agent]
backend = "anthropic-api"  # or "claude-cli" to use local claude subprocess
```

**`.hox/safety-rules.toml`** — isolation safety enforcement:
```toml
[rules]
deny_paths = ["*.env", "*.secrets", ".git/**", ".jj/**"]
deny_commands = ["rm -rf /", "curl .* | sh"]
max_file_size_bytes = 1_048_576  # 1MB
```

Missing safety rules file = no enforcement (fail-open). Protected file violations produce `HoxError::ProtectedFile`.

## Testing

- Unit tests: inline with `#[cfg(test)]`
- Integration tests: `crates/*/tests/`
- Examples: `crates/*/examples/`

## Agent System

Agents are identified hierarchically via `AgentId`:
- Format: `{orchestrator}/{agent-name}` (e.g., `O-A-1/agent-abc123`)
- Each agent gets an isolated JJ workspace at `.hox-workspaces/{agent-name}/`
- Task assignments use bookmarks: `agent-{id}/task-{name}`

Communication protocol:
- `Mutation` - Orchestrator decisions agents MUST follow
- `Info` - Informational broadcasts agents MAY read
- `AlignRequest` - Agent asks for guidance
