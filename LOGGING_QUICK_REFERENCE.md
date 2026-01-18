# Logging Quick Reference - hox Workspace

## Current Logging Setup

The workspace uses the **tracing** crate for structured logging:

```toml
# In Cargo.toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## Enable Logging

```bash
# Debug level (all logs)
RUST_LOG=debug beads sync

# Info level (important events)
RUST_LOG=info beads list

# Specific module
RUST_LOG=bd_storage=debug,bd_daemon=info beads

# Filter by module
RUST_LOG=bd_daemon::sync=debug beads
```

## Logging by Module

### Best Instrumented (Use as Reference)

1. **bd-daemon/src/lib.rs** - 52 log statements
   ```rust
   info!("Daemon started");
   debug!("File changed: {}", path);
   warn!("Failed to process: {}", error);
   ```

2. **bd-storage/src/sync.rs** - 29 log statements
   ```rust
   info!("Syncing {} tasks", count);
   warn!("Failed to sync task {}: {}", id, error);
   ```

### Critical Gaps (Needs Logging)

1. **bd-orchestrator/src/jj.rs** (326 lines)
   - No logging on jj command execution
   - Can't debug subprocess failures

2. **bd-storage/src/db.rs** (856 lines)
   - No logging on database queries
   - Query failures are silent

3. **bd-orchestrator/src/task.rs** (643 lines)
   - No logging on task mutations
   - Can't track state changes

## Adding Logging

### Pattern 1: Entry/Exit Logging

```rust
use tracing::{debug, info};

pub async fn my_function(id: &str) -> Result<()> {
    info!("Starting operation", id = id);

    // Do work...

    info!("Operation completed", id = id);
    Ok(())
}
```

### Pattern 2: Error Context

```rust
pub async fn process_file(path: &Path) -> Result<()> {
    match read_file(path).await {
        Ok(data) => {
            debug!("File read successfully", path = %path, size = data.len());
            Ok(())
        }
        Err(e) => {
            warn!("Failed to read file", path = %path, error = %e);
            Err(e)
        }
    }
}
```

### Pattern 3: Performance Timing

```rust
use std::time::Instant;

pub async fn expensive_operation() -> Result<()> {
    let start = Instant::now();
    info!("Starting expensive operation");

    // Do work...

    info!(
        "Operation completed",
        elapsed_ms = start.elapsed().as_millis(),
    );
    Ok(())
}
```

### Pattern 4: Structured Fields

```rust
info!(
    "Task processed",
    task_id = &task.id,
    priority = task.priority,
    duration_ms = elapsed,
);
```

## Error Handling Without Logging (⚠️ Problem!)

```rust
// BAD: Silent error
let result = database.query().await?;

// GOOD: With context
match database.query().await {
    Ok(result) => {
        debug!("Query succeeded", rows = result.len());
        Ok(result)
    }
    Err(e) => {
        warn!("Query failed", error = %e);
        Err(e)
    }
}
```

## Log Levels Guide

| Level | Use Case | Example |
|-------|----------|---------|
| **error!** | Failures, errors | `error!("Failed to save: {}", e)` |
| **warn!** | Recoverable issues, retries | `warn!("Retry attempt 3")` |
| **info!** | Important events, milestones | `info!("Sync completed", count=10)` |
| **debug!** | Development details, flow tracking | `debug!("Processing task: {}", id)` |
| **trace!** | Low-level details (rarely used) | `trace!("Entering function")` |

## Files That Need Attention

### Priority 1: Add Error Logging

```
bd-orchestrator/src/jj.rs - Subprocess execution failures
bd-storage/src/db.rs - Database operation failures
bd-orchestrator/src/task.rs - Task mutation failures
```

### Priority 2: Add Performance Logging

```
bd-daemon/src/lib.rs - Sync timing (partially done)
bd-storage/src/sync.rs - File processing timing
bd-vcs/src/git.rs - Git command timing
```

### Priority 3: Add State Logging

```
bd-orchestrator/src/task.rs - Task state transitions
bd-daemon/src/oplog.rs - OpLog processing
```

## Helpful Patterns

### Logging with Request ID

```rust
use uuid::Uuid;

let request_id = Uuid::new_v4();

info!(
    "Processing request",
    request_id = %request_id,
    command = "sync",
);
```

### Structured Errors

```rust
match operation.await {
    Ok(v) => {
        info!("Operation succeeded");
        Ok(v)
    }
    Err(e) => {
        error!(
            "Operation failed",
            error_type = std::any::type_name_of_val(&e),
            error_message = %e,
        );
        Err(e)
    }
}
```

### Conditional Logging

```rust
if log::log_enabled!(log::Level::Debug) {
    debug!("Expensive debug computation: {:?}", expensive_operation());
}
```

## Testing with Logging

```rust
#[tokio::test]
async fn test_sync() {
    // Initialize logging in tests
    let subscriber = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Your test code...
}
```

## Metrics to Track

Currently missing:
- Task processing count
- Sync duration histogram
- Database query count
- Error rate by type
- File watch event backlog
- JJ subprocess success rate

Should be added to `bd-daemon/src/lib.rs` for global visibility.

## Next Steps

1. Add logging to jj.rs (subprocess execution)
2. Add logging to db.rs (database queries)
3. Add logging to task.rs (state mutations)
4. Add request correlation IDs
5. Add basic metrics collection
6. Set up test logging utilities

## References

- [tracing crate docs](https://docs.rs/tracing/)
- [structured logging guide](https://docs.rs/tracing/latest/tracing/#recording-fields)
- Current example: `crates/bd-storage/src/sync.rs`
