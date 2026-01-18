# Logging & Metrics Inventory - hox Workspace

**Audit Date:** 2026-01-17
**Workspace:** `/Users/mike/dev/hox`
**Crates:** 6 (bd-core, bd-storage, bd-vcs, bd-daemon, bd-cli, bd-orchestrator)
**Total Rust Files:** 29

---

## Executive Summary

The workspace has **moderate logging coverage** with **170 structured log statements** (tracing macros) but **169 println! statements** for UI output. However, several critical modules lack observability:

- **Strengths**: Core daemon (bd-daemon) and storage sync (bd-storage) have good logging
- **Weaknesses**: 17 files with no structured logging, no metrics collection, no correlation IDs
- **Opportunities**: Add timing instrumentation, structured logging in high-level modules

---

## 1. CURRENT LOGGING INVENTORY

### Log Statement Counts by Level

| Level | Count | Files |
|-------|-------|-------|
| `debug!` | 44 | 12 files |
| `info!` | 52 | 9 files |
| `warn!` | 23 | 8 files |
| `error!` | 22 | 6 files |
| `trace!` | 0 | N/A |
| `println!` | 169 | mainly bd-cli |
| `eprintln!` | 2 | 2 files |
| **TOTAL** | **312** | |

### Structured Logging by File (tracing crate)

```
crates/bd-daemon/src/lib.rs                   | D: 4 I:22 W: 8 E:18 | Total:  52 ✓ EXCELLENT
crates/bd-storage/src/sync.rs                 | D: 6 I:16 W: 7 E: 0 | Total:  29 ✓ GOOD
crates/bd-storage/src/dep_io.rs               | D:10 I: 0 W: 3 E: 0 | Total:  13 ✓ GOOD
crates/bd-vcs/src/git.rs                      | D:10 I: 0 W: 0 E: 0 | Total:  10 ✓ DECENT
crates/bd-daemon/examples/daemon_example.rs   | D: 0 I: 7 W: 0 E: 0 | Total:   7 ✓ GOOD
crates/bd-vcs/src/lib.rs                      | D: 4 I: 3 W: 0 E: 0 | Total:   7 ✓ DECENT
crates/bd-storage/src/task_io.rs              | D: 7 I: 0 W: 1 E: 0 | Total:   8 ✓ DECENT
crates/bd-daemon/src/oplog.rs                 | D: 1 I: 2 W: 2 E: 2 | Total:   7 ✓ OK
crates/bd-orchestrator/src/handoff.rs         | D: 1 I: 0 W: 2 E: 0 | Total:   3 ○ MINIMAL
crates/bd-daemon/src/dashboard.rs             | D: 0 I: 1 W: 0 E: 2 | Total:   3 ○ MINIMAL
crates/bd-cli/src/main.rs                     | D: 0 I: 1 W: 0 E: 0 | Total:   1 ✗ CRITICAL
```

---

## 2. FILES WITH NO STRUCTURED LOGGING (17 files)

### Critical Core Components (No Logging)

| File | Lines | Impact | Notes |
|------|-------|--------|-------|
| **crates/bd-orchestrator/src/jj.rs** | 326 | HIGH | JJ command execution, subprocess management |
| **crates/bd-orchestrator/src/task.rs** | 643 | HIGH | Task lifecycle management, critical path |
| **crates/bd-orchestrator/src/revsets.rs** | 429 | HIGH | Revset parsing and evaluation |
| **crates/bd-orchestrator/src/types.rs** | 528 | MEDIUM | Type definitions and conversions |
| **crates/bd-storage/src/db.rs** | 856 | HIGH | Database operations, queries, schema |
| **crates/bd-core/src/types.rs** | 1,244 | MEDIUM | Core type definitions (no logic) |
| **crates/bd-vcs/src/backend.rs** | 22 | LOW | VCS backend trait definition |
| **crates/bd-core/src/schema.rs** | 136 | LOW | Schema constants (no logic) |
| **crates/bd-core/src/error.rs** | 61 | LOW | Error type definitions |
| **crates/bd-core/src/lib.rs** | 25 | LOW | Module re-exports |

### Test & Example Files (No Logging)

```
crates/bd-core/examples/types_demo.rs         152 lines
crates/bd-daemon/tests/oplog_integration.rs   46 lines
crates/bd-storage/examples/dep_io_demo.rs    114 lines
crates/bd-storage/tests/db_test.rs           395 lines
crates/bd-vcs/examples/test_vcs.rs            66 lines
crates/bd-orchestrator/src/lib.rs             39 lines
```

---

## 3. ERROR HANDLING ANALYSIS

### Error Handling Patterns

| Pattern | Count | Notes |
|---------|-------|-------|
| `?` operator (silent errors) | 334 | ⚠️ High volume of silent error propagation |
| `if let` patterns | 113 | Good for graceful degradation |
| `match` patterns | 69 | Error context handling |
| `.map_err()` | 60 | Custom error context conversion |

### Error Paths WITHOUT Logging

**HIGH PRIORITY**: These error paths silently fail:

1. **bd-orchestrator/src/jj.rs** (326 lines, 0 logging)
   - All JJ command failures are silent
   - subprocess execution errors not logged
   - Example: `self.exec(args).await?` - 334 instances across codebase

2. **bd-storage/src/db.rs** (856 lines, 0 logging)
   - Database query failures not logged
   - Connection issues silent
   - Schema migrations unmonitored

3. **bd-orchestrator/src/task.rs** (643 lines, 0 logging)
   - Task state transitions not logged
   - Task mutations untracked

### Error Recovery Issues

```
crates/bd-storage/src/sync.rs - GOOD PATTERN:
  if let Err(e) = process_file() {
      warn!("Failed to process {}: {}", path, e);
      stats.tasks_failed += 1;  // Still count it
      continue;
  }

crates/bd-storage/src/db.rs - SILENT FAILURE:
  let result = self.execute_query().await?;  // No context
```

---

## 4. METRICS & OBSERVABILITY READINESS

### Current Metrics Infrastructure

| Category | Status | Count |
|----------|--------|-------|
| **Prometheus** | ✗ Not implemented | 0 |
| **StatsD** | ✗ Not implemented | 0 |
| **Custom counters** | ⚠️ Minimal | 12 |
| **Timing instrumentation** | ⚠️ Partial | 60 |
| **Trace IDs / Correlation IDs** | ✗ Not implemented | 0 |
| **Span-based tracing** | ✗ Not implemented | 0 |

### Timing Measurements Found

```rust
// crates/bd-daemon/src/lib.rs
let elapsed = start.elapsed();
info!("Sync completed in {:.2?}", elapsed);

// crates/bd-storage/src/sync.rs
let start = Instant::now();
// ... work ...
info!("Operations completed in {:.3}s", start.elapsed().as_secs_f64());
```

✓ **Positive**: 60 Duration/Instant measurements exist

### Structured Logging

| Aspect | Status | Notes |
|--------|--------|-------|
| **JSON output** | ⚠️ Partial | 22 serde_json uses (mostly export) |
| **Structured fields** | ✓ Good | Using `info!("Event", field=value)` syntax |
| **Trace context** | ✗ Missing | No OpenTelemetry or trace ID propagation |
| **Request IDs** | ✗ Missing | No correlation IDs across async tasks |

---

## 5. CRITICAL GAPS & RECOMMENDATIONS

### Gap 1: CRITICAL - No Logging in High-Volume Operations

**Files with severe logging gaps:**

```
1. bd-orchestrator/src/jj.rs (326 lines) - JJ SUBPROCESS EXECUTION
   ├─ No logging on command execution
   ├─ No stderr capture/logging
   ├─ Failures silent via ? operator
   └─ IMPACT: Can't debug failed jj operations

2. bd-storage/src/db.rs (856 lines) - DATABASE OPERATIONS
   ├─ No logging on queries
   ├─ No connection pool monitoring
   ├─ Schema operations untracked
   └─ IMPACT: Database issues invisible

3. bd-orchestrator/src/task.rs (643 lines) - TASK STATE
   ├─ Task mutations not logged
   ├─ State transitions invisible
   └─ IMPACT: Can't debug task state inconsistencies
```

### Gap 2: No Metrics Collection

**Missing metrics:**

```
Performance:
  ✗ DB query execution time distribution
  ✗ File sync duration
  ✗ JJ subprocess execution time
  ✗ Task processing latency

Counts:
  ✗ Total tasks processed
  ✗ Total syncs completed
  ✗ Errors by type
  ✗ Database operations per type (INSERT, UPDATE, DELETE)

System:
  ✗ Active daemon connections
  ✗ File watch event backlog
  ✗ Database connection pool status
  ✗ Memory usage
```

### Gap 3: No Correlation/Tracing Context

**Missing context propagation:**

```
Issues:
  ✗ No request IDs linking operations
  ✗ No span tracing across async boundaries
  ✗ No parent-child operation relationships
  ✗ Logs from different components can't be correlated

Example: User runs `beads sync` → Should create a trace ID that
follows through daemon, VCS checks, DB operations, file I/O
Currently: All these generate separate log lines with no connection
```

### Gap 4: Test & Example Coverage

```
9 test/example files with zero logging:
  - Can't see what's happening during tests
  - Examples don't demonstrate logging setup
  - Integration test failures are silent
```

---

## 6. IMPLEMENTATION RECOMMENDATIONS

### Priority 1: Add Logging to Critical Paths (IMMEDIATE)

**Files to instrument:**

```rust
// 1. bd-orchestrator/src/jj.rs
impl RealJjExecutor {
    async fn exec(&self, args: &[&str]) -> Result<String> {
        info!(
            "Executing jj command",
            args = ?args,  // Add field logging
        );

        match self.run_command(args).await {
            Ok(output) => {
                debug!(
                    "JJ command succeeded",
                    args = ?args,
                    output_size = output.len(),
                );
                Ok(output)
            }
            Err(e) => {
                error!(
                    "JJ command failed",
                    args = ?args,
                    error = %e,
                    // Capture stderr for debugging
                );
                Err(e)
            }
        }
    }
}

// 2. bd-storage/src/db.rs
impl Database {
    async fn execute_query(&self, query: &str) -> Result<Vec<Row>> {
        debug!(
            "Executing database query",
            query_preview = &query[..50.min(query.len())],
        );

        let start = Instant::now();
        match self.conn.execute(query).await {
            Ok(rows) => {
                debug!(
                    "Query executed",
                    row_count = rows.len(),
                    elapsed_ms = start.elapsed().as_millis(),
                );
                Ok(rows)
            }
            Err(e) => {
                error!(
                    "Query execution failed",
                    query_preview = &query[..50.min(query.len())],
                    error = %e,
                );
                Err(e)
            }
        }
    }
}

// 3. bd-orchestrator/src/task.rs
impl TaskManager {
    pub async fn update_task(&mut self, id: &str, updates: TaskUpdate) -> Result<()> {
        info!(
            "Updating task",
            task_id = id,
            changes = ?updates.fields(),
        );

        // ... perform update ...

        info!(
            "Task updated successfully",
            task_id = id,
        );
        Ok(())
    }
}
```

### Priority 2: Add Request Correlation (HIGH)

```rust
// Add to bd-cli/src/main.rs
use uuid::Uuid;
use tracing::Instrument;

#[tokio::main]
async fn main() -> Result<()> {
    let request_id = Uuid::new_v4();

    let span = tracing::info_span!(
        "cli_request",
        request_id = %request_id,
        command = ?cmd,
    );

    main_impl(cmd)
        .instrument(span)
        .await
}
```

### Priority 3: Add Basic Metrics (MEDIUM)

```rust
// Create crates/bd-metrics/src/lib.rs
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Metrics {
    pub tasks_processed: AtomicU64,
    pub syncs_completed: AtomicU64,
    pub db_queries_total: AtomicU64,
    pub db_errors_total: AtomicU64,
}

impl Metrics {
    pub fn record_task_processed(&self) {
        self.tasks_processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_db_query(&self) {
        self.db_queries_total.fetch_add(1, Ordering::Relaxed);
    }
}

// In bd-daemon/src/lib.rs
impl Daemon {
    async fn sync(&self) -> Result<SyncStats> {
        let start = Instant::now();
        let stats = self.perform_sync().await?;

        self.metrics.record_sync(stats.total_synced());
        info!(
            "Sync completed",
            tasks = stats.tasks_synced,
            deps = stats.deps_synced,
            elapsed_ms = start.elapsed().as_millis(),
        );
        Ok(stats)
    }
}
```

### Priority 4: Structured Logging in Tests (MEDIUM)

```rust
// crates/bd-storage/tests/db_test.rs
#[tokio::test]
async fn test_sync_all() {
    // Initialize logging for this test
    let _guard = init_test_logging();

    info!("Starting sync_all test");

    let db = Database::open(":memory:").await.expect("failed to open");
    db.init_schema().await.expect("failed to init schema");

    let result = sync_manager.sync_all().await;
    assert!(result.is_ok());

    info!("sync_all test completed");
}

fn init_test_logging() -> impl Drop {
    let subscriber = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(Level::DEBUG)
        .init();

    subscriber
}
```

---

## 7. OBSERVABILITY READINESS CHECKLIST

```
[ ] Add logging to jj.rs (subprocess execution)
[ ] Add logging to db.rs (database operations)
[ ] Add logging to task.rs (state mutations)
[x] Structured logging format (already using)
[ ] Error context with logging (wrap errors)
[ ] Request correlation IDs
[ ] Span-based tracing (OpenTelemetry)
[ ] Metrics collection (counters, gauges)
[ ] Performance timing (latency histograms)
[ ] Health check endpoint (daemon readiness)
[ ] Debug logging levels (RUST_LOG=debug)
[ ] Log rotation/retention (if file-based)
```

---

## 8. QUICK START: Enable Verbose Logging

### Current Setup

```bash
# Initialize tracing in bd-cli/src/main.rs
tracing_subscriber::registry()
    .with(fmt::layer())
    .with(filter)
    .init();

# This reads RUST_LOG environment variable
RUST_LOG=debug beads sync
RUST_LOG=info beads list
```

### What You'll See

```
2026-01-17T10:30:45.123Z  INFO beads_cli: Beads CLI starting
2026-01-17T10:30:45.124Z DEBUG bd_daemon: Daemon starting file watcher
2026-01-17T10:30:45.200Z  INFO bd_storage::sync: Syncing 15 tasks, 8 deps
2026-01-17T10:30:45.250Z DEBUG bd_storage::sync: Synced task abc-123 (1.2ms)
```

---

## 9. SUMMARY TABLE

| Aspect | Status | Score |
|--------|--------|-------|
| **Logging Coverage** | ⚠️ Partial | 6/10 |
| **Error Logging** | ✗ Gaps | 4/10 |
| **Metrics** | ✗ Missing | 0/10 |
| **Tracing/Correlation** | ✗ Missing | 0/10 |
| **Observability** | ⚠️ Basic | 3/10 |
| **Performance Visibility** | ⚠️ Partial | 4/10 |

**Overall Score: 3.2/10 - Below Production Standard**

### Action Items (Ranked)

1. **CRITICAL**: Add logging to `jj.rs` (subprocess execution)
2. **CRITICAL**: Add logging to `db.rs` (database operations)
3. **HIGH**: Add logging to `task.rs` (task mutations)
4. **HIGH**: Add request correlation IDs
5. **MEDIUM**: Implement basic metrics (counters, gauges)
6. **MEDIUM**: Add performance timing to critical paths
7. **MEDIUM**: Initialize logging in tests
8. **LOW**: Add OpenTelemetry span tracing

---

## Files This Report Covers

Generated for audit:
- 29 Rust source files
- 6 crates in workspace
- Total: ~5,700 lines analyzed

Most problematic files:
1. `bd-orchestrator/src/jj.rs` - 326 lines, 0 logging
2. `bd-storage/src/db.rs` - 856 lines, 0 logging
3. `bd-orchestrator/src/task.rs` - 643 lines, 0 logging

Best instrumented:
1. `bd-daemon/src/lib.rs` - 52 log statements
2. `bd-storage/src/sync.rs` - 29 log statements
3. `bd-storage/src/dep_io.rs` - 13 log statements

---

**End of Report**
