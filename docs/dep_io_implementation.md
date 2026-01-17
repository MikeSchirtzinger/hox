# DepFile I/O Implementation

**Status:** ✅ Complete - Phase 2 of jj-beads-rs
**Date:** 2026-01-14
**Crate:** `bd-storage`

## Overview

Complete implementation of dependency file I/O operations in the `bd-storage` crate, matching the Go reference implementation from `jj-beads`.

## Implementation Details

### Module: `bd-storage/src/dep_io.rs`

Production-quality async Rust implementation with the following functions:

#### Public API

1. **`read_dep_file(path: &Path) -> Result<DepFile>`**
   - Reads and parses a single dependency file
   - Validates the parsed dependency
   - Returns error if file cannot be read, parsed, or validation fails

2. **`write_dep_file(deps_dir: &Path, dep: &DepFile) -> Result<()>`**
   - Writes dependency file with validation
   - Creates deps directory if it doesn't exist
   - Filename format: `{from}--{dep_type}--{to}.json`
   - Pretty-prints JSON for readability

3. **`read_all_dep_files(deps_dir: &Path) -> Result<Vec<DepFile>>`**
   - Batch reads all dependency files from directory
   - Skips invalid files with warnings (not errors)
   - Returns empty vector if directory doesn't exist

4. **`delete_dep_file(deps_dir: &Path, from: &str, dep_type: &str, to: &str) -> Result<()>`**
   - Deletes a dependency file by components
   - Idempotent - returns Ok(()) even if file doesn't exist

5. **`find_deps_for_task(deps_dir: &Path, task_id: &str) -> Result<Vec<DepFile>>`**
   - Finds all dependencies involving a specific task
   - Returns deps where task is either 'from' or 'to'
   - Parses filenames to avoid reading all files

#### Internal Helpers

- **`parse_dep_filename(filename: &str) -> Result<(String, String, String)>`**
  - Parses dependency filename into components
  - Validates format: `{from}--{type}--{to}.json`
  - Returns error for invalid formats

## Test Coverage

Comprehensive test suite with 6 tests covering:

1. ✅ Filename parsing (valid and invalid cases)
2. ✅ Write and read round-trip
3. ✅ Read all dependency files
4. ✅ Find dependencies for specific task
5. ✅ Delete dependency file
6. ✅ Idempotent deletion
7. ✅ Non-existent directory handling

**All tests passing**: 6 passed; 0 failed

## Usage Example

See `crates/bd-storage/examples/dep_io_demo.rs` for a complete demonstration:

```rust
use bd_storage::{write_dep_file, read_dep_file, find_deps_for_task};
use bd_core::DepFile;
use chrono::Utc;

// Create a dependency
let dep = DepFile {
    from: "task-001".to_string(),
    to: "task-002".to_string(),
    dep_type: "blocks".to_string(),
    created_at: Utc::now(),
};

// Write it
write_dep_file(&deps_dir, &dep).await?;

// Read it back
let path = deps_dir.join(dep.to_filename());
let read_dep = read_dep_file(&path).await?;

// Find all deps for a task
let deps = find_deps_for_task(&deps_dir, "task-001").await?;
```

## Integration

The module is properly integrated into `bd-storage`:

- ✅ Added to `lib.rs` with public module declaration
- ✅ Functions re-exported for convenient access
- ✅ Uses `bd_core::DepFile` type from Phase 1
- ✅ Follows async Rust patterns with tokio
- ✅ Proper error handling with `bd_core::Error`

## Key Features

1. **Async/Await**: Full async implementation using tokio
2. **Error Handling**: Comprehensive error handling with meaningful messages
3. **Validation**: Validates all dependencies before writing
4. **Idempotency**: Delete operations are idempotent
5. **Robustness**: Handles missing directories, invalid files gracefully
6. **Logging**: Uses tracing for debug and warning logs
7. **Testing**: Comprehensive test suite with tempfile for isolation

## Compatibility

This implementation matches the Go reference implementation from:
- `jj-beads/internal/turso/schema/dep.go`

Key compatibility points:
- Same filename format: `{from}--{type}--{to}.json`
- Same validation rules
- Same error handling approach (skip invalid files in batch operations)
- Same idempotent deletion behavior

## Next Steps

Phase 2 is complete. Ready for Phase 3:
- Implement TaskFile I/O operations (similar pattern)
- Or integrate with database layer for persistence
- Add VCS integration for git hooks
