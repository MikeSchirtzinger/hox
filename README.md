# Hox

**Hox** — named after [Hox genes](https://en.wikipedia.org/wiki/Hox_gene), the master regulators that orchestrated the Cambrian explosion's burst of body plan diversity. Just as Hox genes coordinate development by telling cells where they are and what to become, this system orchestrates AI agents by telling them what to do and when.

A JJ-native multi-agent orchestration system where **tasks ARE jj changes**.

## Core Paradigm

- **Tasks = jj changes** (change IDs are primary identifiers)
- **Dependencies = DAG ancestry** (parent-child relationships)
- **Assignments = bookmarks** (e.g., `agent-42/task-xyz`)
- **Metadata = structured descriptions**

The VCS itself is the source of truth.

## Project Structure

```
crates/
├── hox-core          # Core types: Task, AgentId, HandoffContext
├── hox-jj            # JJ integration: metadata, revsets, workspaces
├── hox-agent         # Anthropic API client, file executor
├── hox-orchestrator  # Agent spawning, loop engine, backpressure
├── hox-validation    # Plan validation and constraints
├── hox-metrics       # Metrics collection
├── hox-evolution     # Plan evolution and refinement
└── hox-cli           # CLI binary
```

## Building

```bash
cargo build
cargo test
cargo install --path crates/hox-cli
```

## Usage

```bash
# Run Ralph-style autonomous loop on a task
hox loop start <change-id> --max-iterations 20 --model sonnet

# Check loop status
hox loop status <change-id>

# Stop a running loop
hox loop stop <change-id>
```

## Architecture

The Ralph-style loop pattern spawns **fresh agents per iteration** with no conversation history. State flows through:

1. JJ change descriptions (HandoffContext, metadata)
2. Backpressure signals (test/lint/build failures)
3. Previous iteration logs

This prevents context drift that plagues long-running agents.

## License

MIT OR Apache-2.0
