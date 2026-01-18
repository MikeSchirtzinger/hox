# Logging & Metrics Audit - Document Index

**Audit Date:** 2026-01-17
**Workspace:** `/Users/mike/dev/hox`
**Status:** COMPLETE

## Quick Links

### Start Here
- **[LOGGING_INVENTORY_SUMMARY.txt](./LOGGING_INVENTORY_SUMMARY.txt)** - Visual overview (5 min read)
  - Key statistics and metrics
  - Biggest problems highlighted
  - Top recommendations
  - Observability score

### Detailed Analysis
- **[LOGGING_METRICS_INVENTORY.md](./LOGGING_METRICS_INVENTORY.md)** - Full audit report (20 min read)
  - Comprehensive inventory by file
  - Critical gaps identified
  - Error handling analysis
  - Implementation recommendations with code examples

### Implementation Guide
- **[LOGGING_QUICK_REFERENCE.md](./LOGGING_QUICK_REFERENCE.md)** - How-to guide (10 min read)
  - How to enable logging
  - Common patterns
  - Examples for each situation
  - What to do next

---

## Key Findings Summary

### The Good
- ✓ Logging infrastructure in place (tracing crate)
- ✓ Core daemon (bd-daemon) well-instrumented (52 statements)
- ✓ Storage sync (bd-storage) has good coverage (29 statements)
- ✓ RUST_LOG environment variable works

### The Bad
- ✗ 3 critical files with ZERO logging (1,825 lines of code)
- ✗ 334 silent errors (? operator without logging)
- ✗ No metrics collection
- ✗ No trace IDs or correlation IDs
- ✗ No observability infrastructure

### The Ugly
- bd-orchestrator/src/jj.rs: Subprocess execution failures invisible
- bd-storage/src/db.rs: Database operation failures invisible
- bd-orchestrator/src/task.rs: Task state mutations completely untracked

---

## Action Plan

### Week 1 - Critical Logging
- [ ] Add logging to `bd-orchestrator/src/jj.rs` (2-3 hours)
- [ ] Add logging to `bd-storage/src/db.rs` (3-4 hours)
- [ ] Add logging to `bd-orchestrator/src/task.rs` (2-3 hours)

**Reference patterns:** `crates/bd-storage/src/sync.rs` and `crates/bd-daemon/src/lib.rs`

### Week 2 - Error Context
- [ ] Wrap silent errors with logging context
- [ ] Add request correlation IDs (UUID per CLI invocation)
- [ ] Create error logging helper functions

### Week 3 - Metrics Foundation
- [ ] Implement basic counters (tasks processed, syncs completed)
- [ ] Add timing measurements to critical paths
- [ ] Create metrics exporter (console for now)

### Week 4 - Advanced Observability
- [ ] Add OpenTelemetry spans
- [ ] Implement distributed tracing
- [ ] Add health check endpoint
- [ ] Prometheus integration (optional)

---

## Enable Logging Now

```bash
# Debug everything
RUST_LOG=debug beads sync

# Info level only
RUST_LOG=info beads list

# Specific modules
RUST_LOG=bd_storage=debug,bd_daemon=info beads

# Just the problematic modules (once fixed)
RUST_LOG=bd_orchestrator=debug beads
```

---

## Statistics at a Glance

| Metric | Value | Status |
|--------|-------|--------|
| Files Analyzed | 29 | - |
| Total Statements | 312 | - |
| Structured Logs | 141 | ⚠️ Incomplete |
| Files with Logging | 12 | ⚠️ 41% of codebase |
| Critical Gap Files | 3 | 🔴 URGENT |
| Error Contexts Missing | 334 | 🔴 URGENT |
| Metrics Collection | 0 | 🔴 MISSING |
| Observability Score | 3.2/10 | 🔴 Below Standard |

---

## Best Practices (From This Audit)

### Good Error Logging
```rust
// From bd-storage/src/sync.rs
match process_file().await {
    Ok(v) => {
        info!("File processed");
        Ok(v)
    }
    Err(e) => {
        warn!("Failed to process: {}", e);
        Err(e)  // Still return error for caller
    }
}
```

### Performance Tracking
```rust
// From bd-daemon/src/lib.rs
let start = Instant::now();
info!("Starting operation");
// ... do work ...
info!("Completed in {:.2?}", start.elapsed());
```

### Structured Fields
```rust
info!(
    "Task processed",
    task_id = &id,
    priority = priority,
    elapsed_ms = elapsed,
);
```

---

## File-by-File Status

### Excellent (50+ statements)
- [x] bd-daemon/src/lib.rs - 52 statements

### Good (20-50 statements)
- [x] bd-storage/src/sync.rs - 29 statements

### Decent (10-20 statements)
- [x] bd-storage/src/dep_io.rs - 13 statements
- [x] bd-vcs/src/git.rs - 10 statements
- [x] bd-storage/src/task_io.rs - 8 statements
- [x] bd-vcs/src/lib.rs - 7 statements
- [x] bd-daemon/src/oplog.rs - 7 statements

### Minimal (1-10 statements)
- [ ] bd-orchestrator/src/handoff.rs - 3 statements
- [ ] bd-daemon/src/dashboard.rs - 3 statements
- [ ] bd-cli/src/main.rs - 1 statement

### Critical (0 statements, high priority)
- [ ] bd-orchestrator/src/jj.rs - 0 statements
- [ ] bd-storage/src/db.rs - 0 statements
- [ ] bd-orchestrator/src/task.rs - 0 statements

### Secondary (0 statements, lower priority)
- [ ] bd-orchestrator/src/revsets.rs - 0 statements
- [ ] bd-orchestrator/src/types.rs - 0 statements
- [ ] bd-core/src/types.rs - 0 statements
- [ ] bd-core/src/schema.rs - 0 statements
- [ ] bd-core/src/error.rs - 0 statements
- [ ] bd-core/src/lib.rs - 0 statements
- [ ] bd-vcs/src/backend.rs - 0 statements

---

## Implementation Examples

### Pattern 1: Entry/Exit
See: `crates/bd-daemon/src/lib.rs`

### Pattern 2: Error Context
See: `crates/bd-storage/src/sync.rs`

### Pattern 3: Timing
See: `crates/bd-daemon/src/lib.rs`

### Pattern 4: Structured Fields
See: `crates/bd-storage/src/sync.rs`

---

## Questions Answered by This Audit

**Q: Is the logging good enough for production?**
A: No. Score is 3.2/10. Critical modules have zero logging.

**Q: What are the biggest gaps?**
A: (1) JJ subprocess execution, (2) Database operations, (3) Task state mutations

**Q: How much work to fix?**
A: ~8-10 hours for critical fixes, ~40 hours for full observability.

**Q: Can I enable logging now?**
A: Yes, use `RUST_LOG=debug beads <cmd>` but you won't see critical operation details.

**Q: What should I prioritize?**
A: jj.rs > db.rs > task.rs (these are blocking debugging)

---

## Related Documentation

- [Tracing crate docs](https://docs.rs/tracing/)
- [RUST_LOG format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
- [Structured logging best practices](https://github.com/tokio-rs/tracing)

---

## Report Metadata

- **Created:** 2026-01-17
- **Duration:** Complete audit
- **Files Scanned:** 29 Rust sources
- **Lines Analyzed:** ~5,700
- **Report Format:** Markdown
- **Recommendations:** 20+ actionable items

---

## Next Steps

1. Read LOGGING_INVENTORY_SUMMARY.txt (5 minutes)
2. Review LOGGING_METRICS_INVENTORY.md (20 minutes)
3. Check LOGGING_QUICK_REFERENCE.md for patterns
4. Start implementing Week 1 fixes in jj.rs
5. Use bd-storage/src/sync.rs as your template

**Estimated effort:** 40-50 hours for full production-grade observability

Good luck with the improvements!
