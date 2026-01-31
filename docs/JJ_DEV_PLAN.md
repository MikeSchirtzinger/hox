# JJ Development Enhancement Plan

**Target Repository:** ~/dev/hox/jj-dev (forked from martinvonz/jj)
**Purpose:** Add first-class Hox metadata support

---

## Overview

This document outlines the specific changes needed to the JJ development fork (jj-dev) to support Hox orchestration metadata as first-class citizens.

## Files to Modify

### 1. lib/src/backend.rs

**Add to Commit struct (~line 173):**

```rust
pub struct Commit {
    pub parents: Vec<CommitId>,
    pub predecessors: Vec<CommitId>,
    pub root_tree: Merge<TreeId>,
    pub conflict_labels: Merge<String>,
    pub change_id: ChangeId,
    pub description: String,
    pub author: Signature,
    pub committer: Signature,
    pub secure_sig: Option<SecureSig>,

    // Hox metadata (new fields)
    pub priority: Option<u8>,           // 0=Critical, 1=High, 2=Medium, 3=Low
    pub status: Option<String>,         // open, in_progress, blocked, review, done, abandoned
    pub agent: Option<String>,          // Agent identifier
    pub orchestrator: Option<String>,   // Orchestrator identifier (O-A-1 format)
    pub msg_to: Option<String>,         // Message target (supports wildcards)
    pub msg_type: Option<String>,       // mutation, info, align-request
}
```

**Update Default/Clone implementations** to include new fields.

### 2. lib/src/protos/simple_store.proto

**Add fields 11-16 to Commit message:**

```protobuf
message Commit {
  repeated bytes parents = 1;
  repeated bytes predecessors = 2;
  repeated bytes root_tree = 3;
  repeated string conflict_labels = 10;
  bytes change_id = 4;
  string description = 5;

  message Timestamp {
    int64 millis_since_epoch = 1;
    int32 tz_offset = 2;
  }
  message Signature {
    string name = 1;
    string email = 2;
    Timestamp timestamp = 3;
  }

  Signature author = 6;
  Signature committer = 7;
  optional bytes secure_sig = 9;

  // Hox metadata (fields 11-16)
  optional uint32 priority = 11;
  optional string status = 12;
  optional string agent = 13;
  optional string orchestrator = 14;
  optional string msg_to = 15;
  optional string msg_type = 16;
}
```

### 3. lib/src/simple_backend.rs

**Update commit_to_proto function:**

```rust
fn commit_to_proto(commit: &Commit) -> crate::protos::simple_store::Commit {
    let mut proto = crate::protos::simple_store::Commit::default();
    // ... existing serialization ...

    // Hox metadata
    proto.priority = commit.priority.map(|p| p as u32);
    proto.status = commit.status.clone();
    proto.agent = commit.agent.clone();
    proto.orchestrator = commit.orchestrator.clone();
    proto.msg_to = commit.msg_to.clone();
    proto.msg_type = commit.msg_type.clone();

    proto
}
```

**Update commit_from_proto function:**

```rust
fn commit_from_proto(proto: crate::protos::simple_store::Commit) -> Commit {
    // ... existing deserialization ...

    Commit {
        // ... existing fields ...

        // Hox metadata
        priority: proto.priority.map(|p| p as u8),
        status: proto.status,
        agent: proto.agent,
        orchestrator: proto.orchestrator,
        msg_to: proto.msg_to,
        msg_type: proto.msg_type,
    }
}
```

### 4. lib/src/git_backend.rs

**Store Hox metadata in extra_metadata_store:**

The Git backend stores jj-specific data in a separate stacked table. Add Hox fields there:

```rust
// In read_commit, after reading git commit
if let Some(extra) = self.extra_metadata_store.get(&id)? {
    commit.priority = extra.priority;
    commit.status = extra.status;
    commit.agent = extra.agent;
    commit.orchestrator = extra.orchestrator;
    commit.msg_to = extra.msg_to;
    commit.msg_type = extra.msg_type;
}

// In write_commit, store to extra_metadata_store
let extra = ExtraCommitMetadata {
    // ... existing fields ...
    priority: commit.priority,
    status: commit.status.clone(),
    agent: commit.agent.clone(),
    orchestrator: commit.orchestrator.clone(),
    msg_to: commit.msg_to.clone(),
    msg_type: commit.msg_type.clone(),
};
```

### 5. lib/src/revset.rs

**Add RevsetFilterPredicate variants (~line 200):**

```rust
pub enum RevsetFilterPredicate {
    // ... existing variants ...

    // Hox predicates
    Priority(StringExpression),
    Status(StringExpression),
    Agent(StringExpression),
    Orchestrator(StringExpression),
    MsgTo(StringExpression),
    MsgType(StringExpression),
}
```

**Register functions in BUILTIN_FUNCTION_MAP (~line 763+):**

```rust
map.insert("priority", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::Priority(expr)))
});

map.insert("status", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::Status(expr)))
});

map.insert("agent", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::Agent(expr)))
});

map.insert("orchestrator", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::Orchestrator(expr)))
});

map.insert("msg_to", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::MsgTo(expr)))
});

map.insert("msg_type", |diagnostics, function, context| {
    let [arg] = function.expect_exact_arguments()?;
    let expr = expect_string_expression(diagnostics, arg, context)?;
    Ok(RevsetExpression::filter(RevsetFilterPredicate::MsgType(expr)))
});
```

**Implement filter evaluation in revset_engine:**

```rust
// In evaluate_predicate or similar
RevsetFilterPredicate::Priority(expr) => {
    let priority_str = match commit.priority {
        Some(0) => "critical",
        Some(1) => "high",
        Some(2) => "medium",
        Some(3) => "low",
        _ => return false,
    };
    expr.matches(priority_str)
}

RevsetFilterPredicate::Status(expr) => {
    commit.status.as_ref().map_or(false, |s| expr.matches(s))
}

// Similar for agent, orchestrator, msg_to, msg_type
// Note: msg_to should support glob matching for wildcards like "O-A-*"
```

### 6. cli/src/commands/describe.rs

**Add CLI arguments for setting Hox metadata:**

```rust
#[derive(clap::Args, Clone, Debug)]
pub struct DescribeArgs {
    // ... existing args ...

    /// Set Hox priority (critical, high, medium, low)
    #[arg(long, value_name = "PRIORITY")]
    set_priority: Option<String>,

    /// Set Hox status (open, in_progress, blocked, review, done, abandoned)
    #[arg(long, value_name = "STATUS")]
    set_status: Option<String>,

    /// Set Hox agent identifier
    #[arg(long, value_name = "AGENT")]
    set_agent: Option<String>,

    /// Set Hox orchestrator identifier
    #[arg(long, value_name = "ORCHESTRATOR")]
    set_orchestrator: Option<String>,

    /// Set message target (supports wildcards like O-A-*)
    #[arg(long, value_name = "TARGET")]
    set_msg_to: Option<String>,

    /// Set message type (mutation, info, align-request)
    #[arg(long, value_name = "TYPE")]
    set_msg_type: Option<String>,
}
```

**Update describe command to apply metadata:**

```rust
// In cmd_describe
if let Some(priority) = &args.set_priority {
    let p = match priority.to_lowercase().as_str() {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => return Err(user_error("Invalid priority")),
    };
    new_commit.priority = Some(p);
}

if let Some(status) = &args.set_status {
    // Validate status
    let valid = ["open", "in_progress", "blocked", "review", "done", "abandoned"];
    if !valid.contains(&status.as_str()) {
        return Err(user_error("Invalid status"));
    }
    new_commit.status = Some(status.clone());
}

// Similar for agent, orchestrator, msg_to, msg_type
```

### 7. lib/src/commit_builder.rs

**Add builder methods for Hox metadata:**

```rust
impl CommitBuilder {
    // ... existing methods ...

    pub fn set_priority(mut self, priority: Option<u8>) -> Self {
        self.commit.priority = priority;
        self
    }

    pub fn set_status(mut self, status: Option<String>) -> Self {
        self.commit.status = status;
        self
    }

    pub fn set_agent(mut self, agent: Option<String>) -> Self {
        self.commit.agent = agent;
        self
    }

    pub fn set_orchestrator(mut self, orchestrator: Option<String>) -> Self {
        self.commit.orchestrator = orchestrator;
        self
    }

    pub fn set_msg_to(mut self, msg_to: Option<String>) -> Self {
        self.commit.msg_to = msg_to;
        self
    }

    pub fn set_msg_type(mut self, msg_type: Option<String>) -> Self {
        self.commit.msg_type = msg_type;
        self
    }
}
```

---

## Testing Plan

### Unit Tests

1. **Serialization round-trip**: Commit with Hox metadata → proto → Commit
2. **Revset parsing**: Parse `priority(high)`, `status(open)`, etc.
3. **Revset evaluation**: Filter commits by Hox predicates
4. **Wildcard matching**: `msg_to("O-A-*")` matches `O-A-1`, `O-A-2`

### Integration Tests

1. **CLI workflow**:
   ```bash
   jj new -m "Test task"
   jj describe --set-priority high --set-status open
   jj log -r 'priority(high)'  # Should show the commit
   ```

2. **Git backend persistence**:
   ```bash
   jj git push
   jj git fetch
   # Hox metadata should survive round-trip
   ```

3. **Rebase preservation**:
   ```bash
   jj describe --set-orchestrator "O-A-1"
   jj rebase -d main
   # Metadata should be preserved
   ```

---

## Implementation Order

1. **Backend types** (backend.rs, commit_builder.rs)
   - Add fields to Commit
   - Add builder methods

2. **Simple backend** (simple_backend.rs, simple_store.proto)
   - Update proto
   - Implement serialization

3. **Git backend** (git_backend.rs)
   - Store in extra_metadata_store

4. **Revsets** (revset.rs)
   - Add predicates
   - Register functions
   - Implement evaluation

5. **CLI** (describe.rs)
   - Add arguments
   - Wire up to commit builder

6. **Tests**
   - Unit tests for each component
   - Integration tests for full workflow

---

## Backward Compatibility

- New fields are all `Option<T>` - existing commits have `None`
- Proto fields 11-16 are new, won't conflict with existing data
- Revset functions are additive, don't change existing behavior
- CLI args are additive, existing workflows unchanged

---

## Future Enhancements

### Mutation Conflict Detection

Flag commits that cause conflicts in descendants when they're marked as mutations:

```rust
pub struct Commit {
    // ...
    pub is_mutation: bool,  // If true, descendant conflicts are "mutation conflicts"
}
```

### Structured Metrics Storage

If metrics become complex, add a dedicated field:

```rust
pub metrics: Option<Metrics>,

pub struct Metrics {
    pub tool_calls: u32,
    pub failures: u32,
    pub time_ms: u64,
    pub quality_score: Option<f32>,
}
```

### Workspace Metadata

Store orchestrator state at workspace level, not just commit level.

---

*End of JJ Development Plan*
