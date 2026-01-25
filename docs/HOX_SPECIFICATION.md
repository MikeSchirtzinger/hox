# Hox Specification

**Version:** 0.1.0
**Status:** Draft
**Last Updated:** 2026-01-17

## Overview

Hox is a JJ-native multi-agent orchestration system inspired by Hox genes - the master regulatory genes that control body structure in organisms. Like biological Hox genes, orchestrator decisions shape the structure of work, and agents differentiate within that structure but cannot override it.

**Core Paradigm:**
- Tasks ARE jj changes (change IDs are primary identifiers)
- Dependencies ARE DAG ancestry (no separate dependency graph)
- Assignments ARE bookmarks
- Shared context IS workspace inheritance
- Communication IS first-class metadata

---

## 1. JJ-Native Architecture

### 1.1 Why JJ?

> "jj IS a graph database. The VCS DAG replaces the need for a separate dependency graph."

Traditional orchestration systems maintain separate state:
- SQLite for task metadata
- Git for code
- Sync logic to keep them aligned

Hox eliminates this by making JJ the single source of truth. The version control DAG *is* the task graph.

### 1.2 Core Mappings

| Concept | JJ Implementation |
|---------|-------------------|
| Task | JJ change |
| Task ID | Change ID |
| Dependencies | DAG ancestry (parent-child) |
| Assignment | Bookmark (`O-A-1/agent-42`) |
| Status | First-class metadata field |
| Priority | First-class metadata field |
| Ready work | Revset query |
| Agent isolation | JJ workspace |

### 1.3 No External Database

Hox maximizes JJ-native capabilities to avoid external databases:

- **Structured queries**: Via revsets (not SQL)
- **Filtering**: Via first-class metadata fields (not database columns)
- **Full-text search**: Via description search revsets
- **Audit trail**: Via JJ operation log

If analytics require aggregation across many runs, telemetry can be feature-flagged to use external storage (Turso or SurrealDB), but this is optional.

---

## 2. First-Class Metadata (JJ Fork Enhancement)

### 2.1 New Fields on Commits

Extend the JJ `Commit` struct with optional Hox metadata:

```rust
pub struct Commit {
    // ... existing fields ...

    // Hox metadata (optional)
    pub hox_priority: Option<Priority>,
    pub hox_status: Option<TaskStatus>,
    pub hox_agent: Option<String>,
    pub hox_orchestrator: Option<String>,
    pub hox_msg_to: Option<String>,      // Messaging target (supports wildcards)
    pub hox_msg_type: Option<MsgType>,   // mutation, info, align-request
}

pub enum Priority {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

pub enum TaskStatus {
    Open,
    InProgress,
    Blocked,
    Review,
    Done,
    Abandoned,
}

pub enum MsgType {
    Mutation,      // Structural decision from orchestrator
    Info,          // Informational message
    AlignRequest,  // Request for alignment decision
}
```

### 2.2 Protobuf Schema Extension

**File**: `lib/src/protos/simple_store.proto`

Extend the existing `Commit` message with fields 11-18:

```protobuf
message Commit {
  // ... existing fields (1-10) ...
  repeated bytes parents = 1;
  repeated bytes predecessors = 2;
  repeated bytes root_tree = 3;
  bytes change_id = 4;
  string description = 5;
  Signature author = 6;
  Signature committer = 7;
  optional bytes secure_sig = 9;
  repeated string conflict_labels = 10;

  // Hox metadata (fields 11-18)
  optional int32 hox_priority = 11;           // 0=Critical, 1=High, 2=Medium, 3=Low
  optional string hox_status = 12;            // "open", "in_progress", "blocked", "review", "done", "abandoned"
  optional string hox_agent = 13;             // Agent identifier (e.g., "agent-42")
  optional string hox_orchestrator = 14;      // Orchestrator identifier (e.g., "O-A-1")
  optional string hox_msg_to = 15;            // Message target (supports glob patterns)
  optional string hox_msg_type = 16;          // "mutation", "info", "align_request"
  optional uint32 hox_loop_iteration = 17;    // Current loop iteration (for Ralph-style loops)
  optional uint32 hox_loop_max_iterations = 18; // Maximum loop iterations allowed
}
```

### 2.3 JJ Source File Locations

**Critical files to modify in the jj fork:**

| File Path | Purpose | What to Add |
|-----------|---------|-------------|
| `lib/src/backend.rs` | Core `Commit` struct definition | Add Hox metadata fields to `pub struct Commit` (line ~156) |
| `lib/src/protos/simple_store.proto` | Protobuf schema for commit storage | Add fields 11-18 as shown above |
| `lib/src/simple_backend.rs` | Proto ↔ Rust conversion | Update `commit_to_proto()` and `commit_from_proto()` functions |
| `lib/src/revset.rs` | Revset function definitions | Add predicates to `BUILTIN_FUNCTION_MAP` (starts at line ~460) |
| `lib/src/commit_builder.rs` | Commit builder API | Add setter methods for Hox metadata |
| `cli/src/commands/describe.rs` | CLI describe command | Add `--set-priority`, `--set-status`, etc. flags |
| `cli/src/commit_templater.rs` | Template rendering for commits | Register `hox_priority`, `hox_status`, etc. as template keywords |

### 2.4 Rust Struct Extension

**File**: `lib/src/backend.rs`

Extend the existing `Commit` struct (around line 156):

```rust
#[derive(ContentHash, Debug, PartialEq, Eq, Clone, serde::Serialize)]
pub struct Commit {
    pub parents: Vec<CommitId>,
    #[serde(skip)] // deprecated
    pub predecessors: Vec<CommitId>,
    #[serde(skip)]
    pub root_tree: Merge<TreeId>,
    #[serde(skip)]
    pub conflict_labels: Merge<String>,
    pub change_id: ChangeId,
    pub description: String,
    pub author: Signature,
    pub committer: Signature,
    #[serde(skip)]
    pub secure_sig: Option<SecureSig>,

    // Hox metadata (all optional for backwards compatibility)
    pub hox_priority: Option<Priority>,
    pub hox_status: Option<TaskStatus>,
    pub hox_agent: Option<String>,
    pub hox_orchestrator: Option<String>,
    pub hox_msg_to: Option<String>,
    pub hox_msg_type: Option<MsgType>,
    pub hox_loop_iteration: Option<u32>,
    pub hox_loop_max_iterations: Option<u32>,
}
```

### 2.5 New Revset Predicates

**File**: `lib/src/revset.rs`

Add to `BUILTIN_FUNCTION_MAP` (around line 460):

```rust
// Priority filter: priority(high), priority(critical)
map.insert("priority", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let priority_str = expect_literal("string", arg)?;
    let priority = Priority::from_str(&priority_str)
        .map_err(|_| RevsetParseError::new("Invalid priority value"))?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxPriority(priority)))
});

// Status filter: status(in_progress), status(blocked)
map.insert("status", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let status_str = expect_literal("string", arg)?;
    let status = TaskStatus::from_str(&status_str)
        .map_err(|_| RevsetParseError::new("Invalid status value"))?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxStatus(status)))
});

// Agent filter: agent("agent-42")
map.insert("agent", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let pattern = expect_literal("string", arg)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxAgent(
        StringPattern::from_glob(&pattern)
    )))
});

// Orchestrator filter: orchestrator("O-A-1")
map.insert("orchestrator", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let pattern = expect_literal("string", arg)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxOrchestrator(
        StringPattern::from_glob(&pattern)
    )))
});

// Message target filter with glob support: msg_to("O-A-*")
map.insert("msg_to", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let pattern = expect_literal("string", arg)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxMsgTo(
        StringPattern::from_glob(&pattern)
    )))
});

// Message type filter: msg_type(mutation)
map.insert("msg_type", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let msg_type_str = expect_literal("string", arg)?;
    let msg_type = MsgType::from_str(&msg_type_str)
        .map_err(|_| RevsetParseError::new("Invalid message type"))?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::HoxMsgType(msg_type)))
});
```

### 2.6 CLI Commands

**File**: `cli/src/commands/describe.rs`

Add Hox-specific flags to `DescribeArgs`:

```rust
#[derive(clap::Args, Clone, Debug)]
pub(crate) struct DescribeArgs {
    // ... existing fields ...

    /// Set task priority (critical, high, medium, low)
    #[arg(long = "set-priority", value_name = "PRIORITY")]
    set_priority: Option<String>,

    /// Set task status (open, in_progress, blocked, review, done, abandoned)
    #[arg(long = "set-status", value_name = "STATUS")]
    set_status: Option<String>,

    /// Set agent assignment
    #[arg(long = "set-agent", value_name = "AGENT")]
    set_agent: Option<String>,

    /// Set orchestrator
    #[arg(long = "set-orchestrator", value_name = "ORCHESTRATOR")]
    set_orchestrator: Option<String>,

    /// Set message target (supports glob patterns)
    #[arg(long = "set-msg-to", value_name = "TARGET")]
    set_msg_to: Option<String>,

    /// Set message type (mutation, info, align_request)
    #[arg(long = "set-msg-type", value_name = "TYPE")]
    set_msg_type: Option<String>,

    /// Mark as mutation commit (shorthand for --set-msg-type mutation)
    #[arg(long = "mutation")]
    mutation: bool,

    /// Set loop iteration
    #[arg(long = "set-loop-iteration", value_name = "N")]
    set_loop_iteration: Option<u32>,

    /// Set max loop iterations
    #[arg(long = "set-loop-max", value_name = "N")]
    set_loop_max: Option<u32>,
}
```

**Usage examples:**

```bash
# Set metadata
jj describe --set-priority high --set-status in_progress

# Assign to agent under orchestrator
jj describe --set-orchestrator "O-A-1" --set-agent "agent-42"

# Create mutation decision
jj describe --mutation -m "MUTATION: user_id is the standard field name"

# Set loop metadata for Ralph-style workflows
jj describe --set-loop-iteration 3 --set-loop-max 10

# Query with metadata
jj log -r 'priority(high) & status(open)'

# Find ready tasks
jj log -r 'heads(status(open)) - conflicts()'

# Find messages for this orchestrator (exact or wildcard)
jj log -r 'msg_to("O-A-1") | msg_to("O-A-*")'
```

### 2.7 Template Support

**File**: `cli/src/commit_templater.rs`

Add template keywords for Hox metadata:

```rust
// In CommitTemplateBuildFnTable::builtin()
build_commit_method(method_table, "hox_priority", |commit| {
    commit.hox_priority.map(|p| p.to_string()).unwrap_or_default()
});
build_commit_method(method_table, "hox_status", |commit| {
    commit.hox_status.map(|s| s.to_string()).unwrap_or_default()
});
build_commit_method(method_table, "hox_agent", |commit| {
    commit.hox_agent.clone().unwrap_or_default()
});
build_commit_method(method_table, "hox_orchestrator", |commit| {
    commit.hox_orchestrator.clone().unwrap_or_default()
});
build_commit_method(method_table, "hox_msg_to", |commit| {
    commit.hox_msg_to.clone().unwrap_or_default()
});
build_commit_method(method_table, "hox_msg_type", |commit| {
    commit.hox_msg_type.map(|m| m.to_string()).unwrap_or_default()
});
build_commit_method(method_table, "hox_loop_iteration", |commit| {
    commit.hox_loop_iteration.map(|i| i.to_string()).unwrap_or_default()
});
build_commit_method(method_table, "hox_loop_max_iterations", |commit| {
    commit.hox_loop_max_iterations.map(|i| i.to_string()).unwrap_or_default()
});
```

**Usage examples:**

```bash
# Show Hox metadata in log
jj log -T 'change_id ++ " [" ++ hox_status ++ "/" ++ hox_priority ++ "] " ++ description.first_line()'

# Custom log for orchestrator view
jj log -T 'if(hox_agent, hox_agent ++ ": ", "") ++ description.first_line()'
```

### 2.8 Backwards Compatibility

**Critical requirement**: Hox-enhanced jj (jj-dev) must remain compatible with vanilla jj.

| Scenario | Behavior |
|----------|----------|
| Vanilla jj reads Hox commit | Hox metadata fields ignored (protobuf `optional` semantics) |
| jj-dev reads vanilla commit | All `hox_*` fields are `None` |
| Mixed environment | Works correctly; Hox metadata preserved when written by jj-dev |

**Implementation notes:**

1. All Hox fields in protobuf are `optional` - unknown fields are preserved by protobuf
2. All Hox fields in Rust struct are `Option<T>` with `Default` implementations
3. The `make_root_commit()` function sets all Hox fields to `None`
4. Proto field numbers 11-18 chosen to avoid conflicts with future jj additions (leaves gap after 10)

### 2.9 Wildcard Matching Semantics

The `msg_to()` revset predicate supports **glob patterns** (not regex):

| Pattern | Matches |
|---------|---------|
| `O-A-1` | Exact match only |
| `O-A-*` | All Level A orchestrators (O-A-1, O-A-2, O-A-99) |
| `O-*` | All orchestrators at any level |
| `agent-*` | All agents |
| `O-A-1/*` | All agents under O-A-1 |

**Implementation**: Uses jj's existing `StringPattern::from_glob()` which converts glob to regex internally.

### 2.10 Testing Strategy

**Unit tests** (in respective source files):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hox_priority_roundtrip() {
        let commit = create_test_commit();
        let proto = commit_to_proto(&commit);
        let restored = commit_from_proto(proto);
        assert_eq!(commit.hox_priority, restored.hox_priority);
    }

    #[test]
    fn test_backwards_compat_no_hox_fields() {
        // Proto without Hox fields should deserialize with None
        let proto = crate::protos::simple_store::Commit::default();
        let commit = commit_from_proto(proto);
        assert!(commit.hox_priority.is_none());
        assert!(commit.hox_status.is_none());
    }

    #[test]
    fn test_revset_priority_filter() {
        // Test that priority(high) matches commits with Priority::High
        // Setup: create commits with different priorities
        // Assert: revset evaluates correctly
    }
}
```

**Integration tests** (in `cli/tests/`, using [bats](https://bats-core.readthedocs.io/)):

```bash
# test_hox_metadata.bats
@test "set and query priority" {
    jj describe --set-priority high
    result=$(jj log -r 'priority(high)' --no-graph -T 'change_id')
    assert_eq "$result" "$(jj log -r @ --no-graph -T 'change_id')"
}

@test "wildcard msg_to matching" {
    jj describe --set-msg-to "O-A-1"
    assert_eq "$(jj log -r 'msg_to("O-A-*")' --count)" "1"
    assert_eq "$(jj log -r 'msg_to("O-B-*")' --count)" "0"
}

@test "backwards compat with vanilla commits" {
    # Create commit with vanilla jj (simulated by not setting Hox fields)
    jj describe -m "vanilla commit"
    # Should have no Hox metadata
    assert_eq "$(jj log -r @ -T 'hox_priority')" ""
}
```

---

## 3. Orchestrator Architecture

### 3.1 Naming Convention

Orchestrators use hierarchical naming: `O-{level}-{number}`

```
O-A-1    # Level A, Orchestrator 1 (root level)
O-A-2    # Level A, Orchestrator 2 (peer)
O-B-1    # Level B, under some Level A orchestrator
O-B-2    # Level B, under some Level A orchestrator
O-C-1    # Level C, under some Level B orchestrator
```

Levels represent depth in the decomposition tree:
- **Level A**: Root orchestrators (spawned from plan)
- **Level B**: Sub-orchestrators for complex phases
- **Level C+**: Further decomposition as needed

### 3.2 Agent Metadata

Every agent change includes metadata flagging its orchestrator:

```bash
jj describe --set-orchestrator "O-A-1" --set-agent "agent-42"
```

This enables:
- Agents know their parent orchestrator immediately
- Oplog filtering to just their orchestrator's scope
- Revset queries: `orchestrator("O-A-1")` returns all work under that orchestrator

### 3.3 Orchestrator Responsibilities

1. **Task Decomposition**: Break work into phases respecting dependencies
2. **Agent Spawning**: Create workspaces for agents, set initial metadata
3. **Oplog Watching**: Monitor for alignment requests and completed work
4. **Contract Decisions**: Make structural decisions when agents request alignment
5. **Mutation Commits**: Commit decisions to workspace (auto-rebases agents)
6. **Integration**: Handle merge conflicts between parallel agent work
7. **Validation**: Spawn validator agents for quality checks

### 3.4 Workspace as Shared Context

The orchestrator's workspace IS the shared context for all agents under it:

```
O-A-1 workspace (orchestrator)
    │
    ├── agent-1 (branches from O-A-1, inherits context)
    │
    ├── agent-2 (branches from O-A-1, inherits context)
    │
    └── agent-3 (branches from O-A-1, inherits context)
```

When the orchestrator commits a decision:
1. JJ auto-rebases all descendant changes
2. Agents automatically see the new context
3. If agents already made conflicting decisions → mutation conflict

---

## 4. Communication Protocol

### 4.1 Agent → Orchestrator

Agents signal needs via metadata on their changes:

```bash
# Request alignment on API syntax
jj describe \
  --set-msg-to "O-A-1" \
  --set-msg-type align-request \
  -m "ALIGN: need syntax for user API - userId vs user_id"
```

Orchestrator detects this by watching oplog for changes with `msg_to` matching its name.

### 4.2 Orchestrator → Agents

Orchestrator commits decisions to its workspace. Since agents branch from it:
- Decision appears in their tree via auto-rebase
- No explicit message needed - it's structural

For explicit notifications, orchestrator can set `msg_to`:

```bash
jj describe \
  --set-msg-to "O-A-1/*" \    # All agents under O-A-1
  --set-msg-type mutation \
  -m "MUTATION: user_id is the standard field name"
```

### 4.3 Orchestrator → Orchestrator

Peer orchestrators communicate via workspace addressing:

```bash
# O-A-1 messages O-A-2
jj describe \
  --set-msg-to "O-A-2" \
  --set-msg-type mutation \
  -m "MUTATION: Shared type User uses user_id field"
```

Wildcard addressing for broadcast:

```bash
# Message all Level A orchestrators
jj describe --set-msg-to "O-A-*" --set-msg-type info -m "..."

# Message all orchestrators
jj describe --set-msg-to "O-*" --set-msg-type info -m "..."
```

### 4.4 Message Types

| Type | Meaning | Response |
|------|---------|----------|
| `mutation` | Structural decision from orchestrator | Agent MUST conform |
| `info` | Informational, no action required | Agent MAY read |
| `align-request` | Agent needs alignment decision | Orchestrator SHOULD respond |

---

## 5. Conflict Handling (The Hox Gene Model)

### 5.1 Two Types of Conflicts

| Conflict Type | Cause | Who Resolves |
|---------------|-------|--------------|
| **Mutation** | Orchestrator structural decision | Agent corrects to match |
| **Merge** | Code overlap between parallel work | Orchestrator in integration |

### 5.2 Mutation Conflicts

When an orchestrator commits a structural decision (e.g., "use `user_id`"), JJ rebases agent work. If an agent already used `userId`:

1. JJ marks the change as having conflicts
2. Hox flags this as a **mutation conflict** (via `msg_type: mutation` on the source)
3. Agent is responsible for fixing: change `userId` to `user_id`
4. Agent continues work after resolving

**JJ Fork Enhancement**: Add `--mutation` flag to mark commits as structural decisions. Descendant conflicts from mutations are flagged differently than merge conflicts.

### 5.3 Merge Conflicts

When parallel agents' work overlaps at integration:

1. Orchestrator receives all completed agent work
2. JJ merge may produce conflicts
3. Orchestrator (or dedicated integration agent) resolves
4. Final integrated change has no conflicts

### 5.4 The Hox Principle

> Orchestrator decisions are like Hox genes - they determine structure.
> Agents differentiate within that structure but cannot override it.

If an agent disagrees with an orchestrator decision, they can:
1. Signal concern via `align-request` message
2. Continue working per the decision
3. Trust the orchestrator to revise if needed

Agents NEVER override mutation decisions unilaterally.

---

## 6. Observability & Metrics

### 6.1 Telemetry Collection

Granular metrics per agent:

- Tool calls (count, types)
- Failed tool calls
- Time per phase
- Conflict rate (mutations encountered)
- Alignment requests made
- Validation pass/fail

### 6.2 Storage (Feature-Flagged)

Two storage modes, A/B tested at scale:

**Mode A - JJ-Native:**
```bash
jj describe \
  --set-hox-metrics '{"tool_calls": 47, "failures": 2, "time_ms": 34500}'
```
- Stored as metadata on agent's final change
- Queryable via revsets
- Travels with the work

**Mode B - External:**
- Append-only metrics file or Turso/SurrealDB
- Easier aggregation across runs
- Better for historical analytics

### 6.3 Scoring Weights

For self-evolution evaluation:

| Metric | Weight | Rationale |
|--------|--------|-----------|
| Quality | 0.35 | Correctness matters most |
| Completeness | 0.30 | Did it finish the task? |
| Time | 0.20 | Efficiency matters |
| Efficiency | 0.15 | Resource usage |

---

## 7. Validation System

### 7.1 Byzantine Consensus

To tolerate `f` faulty agents, use `3f+1` validator agents.

Example: To tolerate 1 faulty validator, spawn 4 validators.

### 7.2 Validator Agents

Dedicated validator agents (not peer review):

1. Spawned by orchestrator after phase completion
2. Review work against requirements
3. Check for:
   - Compilation/tests passing
   - Mutation compliance
   - Contract adherence
   - Quality metrics
4. Report pass/fail with details

### 7.3 Configurable

Validation strategy is configurable to evolve based on data:

```yaml
validation:
  strategy: dedicated_validators  # or: peer_review, historical_compare
  validator_count: 4              # 3f+1 where f=1
  consensus_threshold: 0.75       # 3/4 must agree
```

---

## 8. Evolution System

### 8.1 Pattern Storage

Successful orchestration patterns stored in `hox-patterns` branch:

```
main
  └── hox-patterns (separate branch, not in task DAG)
        ├── patterns/decomposition/
        ├── patterns/communication/
        ├── patterns/validation/
        └── patterns/integration/
```

### 8.2 Pattern Lifecycle

1. **Capture**: After successful run, orchestrator proposes pattern
2. **Review**: Validator agents or human review proposal
3. **Merge**: Approved patterns merged to `hox-patterns`
4. **Load**: Future orchestrators load patterns at startup

### 8.3 Prompt Evolution

Patterns influence orchestrator prompts:

```markdown
## Learned Patterns

### Decomposition
- Types-first: Always define shared types before parallel implementation
- Integration phase: Always plan integration agent as final phase

### Communication
- Early alignment: Request alignment at phase start, not mid-work
- Mutation clarity: Include rationale in mutation messages
```

### 8.4 Review Gates

Patterns require approval before merge:

| Gate | Approver | Criteria |
|------|----------|----------|
| Automated | Validator agents | Pattern is well-formed, tested |
| Human | Mike | Pattern aligns with system goals |

No pattern enters `hox-patterns` without passing gates.

---

## 9. Phased Execution

### 9.1 Phase Structure

Orchestrators decompose work into phases. Phases are flexible, not rigid:

```
Phase 0: Contracts (blocking)
    - Define shared types, interfaces
    - All agents wait for this

Phase 1-N: Parallel Work
    - Independent agents work concurrently
    - Communicate via alignment requests

Phase N+1: Integration
    - Dedicated integration agent
    - Resolve merge conflicts
    - Unify work

Phase N+2: Validation
    - Validator agents check work
    - Byzantine consensus on quality
```

### 9.2 Dependency Awareness

Before parallel decomposition:

1. **Scan imports**: What depends on what?
2. **Identify contracts**: Shared types, interfaces, APIs
3. **Phase 0**: Contracts that block everything
4. **Parallelize**: Only truly independent work
5. **Plan integration**: Always have integration phase

### 9.3 Orchestrator Count

Determined by:
- **Explicit flag**: User specifies N orchestrators
- **Root LLM decision**: Analyze plan complexity, suggest optimal N

```bash
hox run --orchestrators 3   # Explicit
hox run                     # Let system decide
```

---

## 10. Implementation Roadmap

### Phase 1: JJ Fork (Weeks 1-2)

1. Extend `Commit` struct with Hox metadata fields
2. Update protobuf schema (fields 11-16)
3. Modify simple_backend serialization
4. Add revset predicates (`priority()`, `status()`, etc.)
5. Add CLI commands (`--set-priority`, etc.)

**Deliverable**: JJ fork with first-class Hox metadata

### Phase 2: Core Hox (Weeks 3-4)

1. Orchestrator implementation
   - Workspace management
   - Oplog watching
   - Agent spawning
2. Communication protocol
   - Message routing
   - Wildcard addressing
3. Conflict handling
   - Mutation detection
   - Conflict flagging

**Deliverable**: Working orchestrator with agent coordination

### Phase 3: Validation & Metrics (Weeks 5-6)

1. Validator agent implementation
2. Byzantine consensus logic
3. Metrics collection (feature-flagged)
4. Telemetry storage (jj-native + external)

**Deliverable**: Validation system with metrics

### Phase 4: Evolution (Weeks 7-8)

1. Pattern capture logic
2. `hox-patterns` branch management
3. Review gate implementation
4. Pattern loading at startup

**Deliverable**: Self-improving orchestration

---

## 11. File Structure

```
hox/
├── Cargo.toml                    # Workspace root
├── CLAUDE.md                     # Claude Code guidance
├── docs/
│   └── HOX_SPECIFICATION.md      # This document
│
├── crates/
│   ├── hox-core/                 # Core types (Task, Priority, Status, Message)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs          # Core type definitions
│   │       └── error.rs          # HoxError
│   │
│   ├── hox-jj/                   # JJ integration layer
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── metadata.rs       # Metadata read/write
│   │       ├── revsets.rs        # Revset query helpers
│   │       └── oplog.rs          # Operation log watching
│   │
│   ├── hox-orchestrator/         # Orchestration engine
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── orchestrator.rs   # Main orchestrator
│   │       ├── workspace.rs      # Workspace management
│   │       ├── communication.rs  # Message routing
│   │       └── phases.rs         # Phase management
│   │
│   ├── hox-validation/           # Validation system
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── validator.rs      # Validator agent
│   │       └── consensus.rs      # Byzantine consensus
│   │
│   ├── hox-metrics/              # Observability
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── telemetry.rs      # Collection
│   │       ├── storage.rs        # Feature-flagged storage
│   │       └── scoring.rs        # Quality scoring
│   │
│   ├── hox-evolution/            # Self-improvement
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── patterns.rs       # Pattern capture/load
│   │       └── review.rs         # Review gates
│   │
│   └── hox-cli/                  # CLI binary
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
└── jj-fork/                      # Submodule or separate repo
    └── (modified jj with Hox metadata support)
```

---

## 12. Dependencies

### Rust Crates

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime |
| `serde` / `serde_json` | Serialization |
| `thiserror` | Error handling |
| `tracing` | Logging/telemetry |
| `clap` | CLI parsing |
| `notify` | File watching (for oplog) |

### Optional (Feature-Flagged)

| Crate | Purpose | Feature Flag |
|-------|---------|--------------|
| `turso` | External metrics storage | `metrics-turso` |
| `surrealdb` | External metrics storage | `metrics-surreal` |

### JJ Fork

The modified JJ is either:
- Git submodule at `jj-fork/`
- Separate repo with path dependency

---

## Appendix A: Revset Quick Reference

```bash
# Find ready tasks (no blockers, no conflicts)
jj log -r 'heads(status(open)) - conflicts()'

# Find all work under orchestrator O-A-1
jj log -r 'orchestrator("O-A-1")'

# Find high priority blocked tasks
jj log -r 'priority(high) & status(blocked)'

# Find messages for this orchestrator
jj log -r 'msg_to("O-A-1") | msg_to("O-A-*")'

# Find mutation decisions
jj log -r 'msg_type(mutation)'

# Find what blocks a task
jj log -r 'ancestors(@) & status(open)'

# Find what a task blocks
jj log -r 'descendants(@) & status(blocked)'
```

---

## Appendix B: Example Orchestration Flow

```
1. Plan created (external or by root LLM)

2. Root spawns O-A-1 (Level A orchestrator)
   - O-A-1 creates workspace
   - Analyzes plan, identifies contracts

3. O-A-1 commits Phase 0 (contracts)
   - Shared types, API definitions
   - All agents will inherit this

4. O-A-1 spawns agents for Phase 1
   - agent-1: Implement API
   - agent-2: Implement client
   - agent-3: Implement tests
   - Each branches from O-A-1 workspace

5. agent-2 needs alignment
   - Commits: "ALIGN: need user field naming"
   - O-A-1 sees in oplog

6. O-A-1 decides, commits mutation
   - "MUTATION: user_id is standard"
   - JJ rebases all agents
   - agent-1 had userId → mutation conflict
   - agent-1 fixes, continues

7. Agents complete, O-A-1 integrates
   - Spawns integration agent
   - Resolves any merge conflicts

8. O-A-1 spawns validators
   - 4 validators (tolerate 1 faulty)
   - Check compilation, tests, quality
   - 3/4 consensus → pass

9. Success captured
   - Metrics stored
   - Pattern proposed to hox-patterns
   - Review gate → merge
```

---

*End of Specification*
