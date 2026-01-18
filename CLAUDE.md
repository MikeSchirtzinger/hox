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
| `hox-validation` | Plan validation and constraint checking |
| `hox-metrics` | Metrics collection and reporting |
| `hox-evolution` | Plan evolution and refinement |
| `hox-cli` | `hox` binary - CLI interface |

## Key Types

```rust
// hox-core/src/types.rs
pub enum TaskStatus { Open, InProgress, Blocked, Review, Done, Abandoned }
pub enum Priority { Critical=0, High=1, Medium=2, Low=3 }
pub struct Task { /* change-based task */ }
pub struct AgentId { orchestrator, id, name }
pub struct HandoffContext { /* agent state for handoffs */ }

// hox-core/src/error.rs
pub type Result<T> = std::result::Result<T, HoxError>;
```

All errors use the unified `HoxError` enum. Never panic in production code.

## Architecture Layers

```
CLI (hox-cli)
    ↓
Orchestration (hox-orchestrator) - agent spawning, communication, handoffs
    ↓
JJ Integration (hox-jj) - metadata, revsets, workspaces
    ↓
Core (hox-core) - Types and schemas
```

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
