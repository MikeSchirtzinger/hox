# JJ-Native Agent Orchestration

## Core Insight

jj IS a graph database. Instead of building a parallel dependency graph in SQLite,
we use the VCS itself as the source of truth for:
- Task state (changes)
- Dependencies (DAG ancestry)
- Agent assignment (bookmarks)
- Context handoff (descriptions)
- Ready work queries (revsets)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        JJ Repository                            │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Change Graph (DAG)                    │   │
│  │                                                          │   │
│  │    root ──► task-1 ──► task-2 ──► task-3                │   │
│  │               │                      │                   │   │
│  │               └──► subtask-1a        └──► subtask-3a    │   │
│  │                                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                      Bookmarks                           │   │
│  │                                                          │   │
│  │  agent-42/task-1 ──────────────────► change xyz123      │   │
│  │  agent-99/subtask-1a ──────────────► change abc456      │   │
│  │  ready/task-2 ─────────────────────► change def789      │   │
│  │                                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    .tasks/metadata.jsonl                 │   │
│  │                                                          │   │
│  │  {"change_id":"xyz123","priority":1,"labels":["bug"]}   │   │
│  │  {"change_id":"abc456","due":"2026-01-15","agent":"42"} │   │
│  │                                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Mapping Concepts

| Beads/SQLite | JJ Native | Notes |
|--------------|-----------|-------|
| Issue | Change | Each task = a change with task file |
| `blocks` edge | Parent-child ancestry | Natural in DAG |
| `in_progress` | Bookmark `agent-{id}/{task}` | Agent owns the bookmark |
| `ready` query | Revset `heads() - conflicts()` | Leaf nodes with no blockers |
| `checked_out_by` | Bookmark namespace | `agent-42/*` = agent 42's work |
| Task metadata | `.tasks/metadata.jsonl` + description | Hybrid storage |
| Dependency graph | JJ's DAG | Already exists, no sync needed |

## Change Description Format

Each task change has a structured description:

```
Task: Implement VCS abstraction layer
Priority: 1
Status: in_progress
Agent: agent-42

## Context
Working on the VCS interface. Need to support both git and jj.

## Progress
- [x] Designed interface
- [x] Implemented git backend
- [ ] Implementing jj backend

## Next Steps
- Fix bookmark parsing
- Add workspace support

## Blockers
None currently

## Files
- internal/vcs/vcs.go
- internal/vcs/git/
- internal/vcs/jj/
```

## Revset Queries

### Ready Work (unblocked tasks)
```bash
# Tasks with no children depending on them
jj log -r 'heads(bookmarks(glob:"task-*")) - conflicts()'
```

### Agent's Assigned Work
```bash
# All tasks assigned to agent-42
jj log -r 'bookmarks(glob:"agent-42/*")'
```

### Blocked Tasks
```bash
# Tasks that have incomplete ancestors
jj log -r 'ancestors(bookmark) & ~immutable()'
```

### Task Dependencies
```bash
# What blocks task-xyz?
jj log -r 'ancestors(task-xyz) - immutable()'

# What does task-xyz block?
jj log -r 'descendants(task-xyz)'
```

## Agent Handoff Flow

```
┌─────────────────────────────────────────────────────────────────┐
│ Agent A running, context getting low                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 1. Agent A updates change description with context              │
│    jj describe -m "$(structured_handoff_context)"               │
│    - Progress made                                              │
│    - Current focus                                              │
│    - Next steps                                                 │
│    - Open questions                                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. All file changes already tracked by jj (auto-tracking)      │
│    No explicit commit needed - working state is captured        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. Agent A exits / context exhausted                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. New Agent B spawned with same bookmark                       │
│    jj log -r 'agent-42/task-xyz' --no-graph                    │
│    jj diff -r 'root()..agent-42/task-xyz'                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. Agent B has:                                                 │
│    - Full code diff (what changed)                              │
│    - Structured context (why, what's next)                      │
│    - Task metadata (priority, dependencies)                     │
│    - Change history (evolution of work)                         │
└─────────────────────────────────────────────────────────────────┘
```

## File Structure

```
.tasks/
├── metadata.jsonl       # Priority, labels, due dates, non-DAG metadata
└── (that's it - the graph IS jj)

internal/orchestrator/
├── DESIGN.md           # This file
├── task.go             # Task operations (create, update, query)
├── agent.go            # Agent assignment and handoff
├── revsets.go          # Revset query helpers
└── handoff.go          # Context handoff utilities
```

## Key Advantages Over SQLite

1. **No sync problem** - JJ IS the source of truth
2. **Distributed native** - Multiple agents, concurrent work, conflict resolution
3. **Natural dependencies** - Code deps = task deps
4. **Change evolution** - Rebase, split, squash - graph auto-updates
5. **Workspaces** - Native agent isolation
6. **Audit trail** - Operation log shows all actions

## Implementation Plan

### Phase 1: Core Task Model
- [ ] Define task change format (description structure)
- [ ] Implement task creation (new change + bookmark)
- [ ] Implement task query (revset wrappers)

### Phase 2: Agent Assignment
- [ ] Bookmark naming conventions
- [ ] Agent workspace creation
- [ ] Assignment/unassignment operations

### Phase 3: Context Handoff
- [ ] Handoff context generation (Haiku summarization)
- [ ] Context embedding in description
- [ ] Handoff context parsing for new agents

### Phase 4: Ready Work Queries
- [ ] "What's ready" revset
- [ ] "What's blocked" revset
- [ ] Priority ordering from metadata.jsonl
