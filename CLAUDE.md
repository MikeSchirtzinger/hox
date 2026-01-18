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
cargo test -p bd-orchestrator  # Test specific crate
cargo run --bin beads -- --help # Run CLI
cargo install --path crates/bd-cli  # Install CLI globally
```

Run tests with logging:
```bash
RUST_LOG=debug cargo test -- --nocapture
```

## Workspace Structure

| Crate | Purpose |
|-------|---------|
| `bd-core` | Core types: `Task`, `TaskStatus`, `Priority`, `HoxError` |
| `bd-storage` | Turso (SQLite) database + file I/O for tasks/deps |
| `bd-vcs` | VCS abstraction (Git via gix, Jujutsu planned) |
| `bd-daemon` | File watcher daemon with jj oplog support |
| `bd-orchestrator` | JJ-native task management, revsets, agent handoffs |
| `bd-cli` | `beads` binary - CLI interface |

## Key Types

```rust
// bd-core/src/types.rs
pub enum TaskStatus { Open, InProgress, Blocked, Review, Done, Abandoned }
pub enum Priority { Critical=0, High=1, Medium=2, Low=3 }
pub struct Task { /* change-based task */ }
pub struct HandoffContext { /* agent state for handoffs */ }

// bd-core/src/error.rs
pub type Result<T> = std::result::Result<T, HoxError>;
```

All errors use the unified `HoxError` enum. Never panic in production code.

## Architecture Layers

```
CLI (bd-cli)
    ↓
Orchestration (bd-orchestrator) - revsets, handoffs, task management
    ↓
Storage (bd-storage) - Turso DB + file sync
    ↓
VCS (bd-vcs) - Git/Jujutsu abstraction
    ↓
Core (bd-core) - Types and schemas
```

The daemon (`bd-daemon`) runs horizontally, watching files and syncing to the database.

## Database

Uses Turso (the Rust SQLite rewrite, NOT libSQL cloud). Location: `.beads/turso.db`

WAL mode enabled for concurrent reads during writes.

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

Key integration test files:
- `bd-storage/tests/db_test.rs` - Database CRUD
- `bd-daemon/tests/oplog_integration.rs` - Daemon operations

## VCS Abstraction

The `Vcs` type auto-detects Git or Jujutsu. Uses pure Rust `gix` for Git (no external binary dependency).

```rust
let vcs = Vcs::open(path)?;  // Auto-detects backend
let commit = vcs.current_commit().await?;
```

Prefers Git when both `.git` and `.jj` exist (jj+git cohabitation).
