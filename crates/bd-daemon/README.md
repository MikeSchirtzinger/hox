# bd-daemon

File watcher daemon for the jj-beads-rs task tracking system.

## Overview

`bd-daemon` provides automatic synchronization between JSON task/dependency files and the libSQL database. It watches for changes to files in the `tasks/` and `deps/` directories and keeps the database up-to-date in real-time.

## Features

- **File Watching**: Uses the `notify` crate to efficiently watch for filesystem changes
- **Auto-sync**: Automatically syncs task and dependency files to the database
- **Real-time Updates**: Processes file changes as they occur
- **Blocked Cache**: Automatically refreshes the blocked task cache when dependencies change
- **Graceful Shutdown**: Handles shutdown signals cleanly
- **Error Recovery**: Continues processing even if individual file operations fail

## Architecture

```text
┌─────────────────────────────────────────┐
│     File System (tasks/ & deps/)        │
│  • *.json files                         │
└─────────────┬───────────────────────────┘
              │
              │ File system events
              ▼
┌─────────────────────────────────────────┐
│          bd-daemon                      │
│  • File watcher (notify)                │
│  • Event processing                     │
│  • File I/O (task_io, dep_io)           │
└─────────────┬───────────────────────────┘
              │
              │ Database operations
              ▼
┌─────────────────────────────────────────┐
│        libSQL Database                  │
│  • tasks table                          │
│  • deps table                           │
│  • blocked_cache table                  │
└─────────────────────────────────────────┘
```

## Usage

### Basic Example

```rust
use bd_daemon::Daemon;
use bd_storage::Database;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open database
    let db = Database::open(".beads/turso.db").await?;
    db.init_schema().await?;

    // Create and run daemon
    let mut daemon = Daemon::new(Arc::new(db), ".");
    daemon.run().await?;

    Ok(())
}
```

### With Graceful Shutdown

```rust
use bd_daemon::Daemon;
use bd_storage::Database;
use std::sync::Arc;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open(".beads/turso.db").await?;
    db.init_schema().await?;

    let mut daemon = Daemon::new(Arc::new(db), ".");

    // Spawn daemon in background
    let daemon_handle = tokio::spawn(async move {
        daemon.run().await
    });

    // Wait for Ctrl+C
    signal::ctrl_c().await?;

    // Daemon stops when dropped
    daemon_handle.await??;

    Ok(())
}
```

## Event Handling

The daemon processes these file system events:

### Create/Modify Events
- **Task Files** (`tasks/*.json`):
  1. Read and parse task file
  2. Validate task data
  3. Upsert to `tasks` table
  4. Refresh blocked cache

- **Dep Files** (`deps/*.json`):
  1. Read and parse dependency file
  2. Validate dependency data
  3. Upsert to `deps` table
  4. Refresh blocked cache

### Delete Events
- **Task Files**:
  1. Extract task ID from filename
  2. Delete from `tasks` table (cascades to deps and blocked_cache)
  3. Refresh blocked cache

- **Dep Files**:
  1. Parse filename: `{from}--{type}--{to}.json`
  2. Delete matching row from `deps` table
  3. Refresh blocked cache

## Error Handling

The daemon uses resilient error handling:

- **File Read Errors**: Logged as warnings, daemon continues
- **Database Errors**: Logged as errors, daemon continues
- **Watcher Errors**: Logged and propagated
- **Invalid Files**: Skipped with warning

This ensures that one bad file doesn't stop the entire sync process.

## Testing

The crate includes comprehensive unit tests:

```bash
# Run all tests
cargo test --package bd-daemon

# Run with logging
RUST_LOG=debug cargo test --package bd-daemon -- --nocapture
```

## Example Program

A complete example daemon is included:

```bash
# Run the daemon example (watches current directory)
cargo run --package bd-daemon --example daemon_example

# Watch a specific directory
cargo run --package bd-daemon --example daemon_example -- /path/to/watch
```

## Integration with jj-turso

The daemon is designed to work with the jj-turso architecture:

1. **Files are source of truth**: The daemon syncs FROM files TO database
2. **Database is query cache**: Fast queries without parsing all files
3. **Real-time sync**: Database stays current as files change
4. **Blocked cache**: Automatically maintains transitive dependency blocking

## Performance

The daemon is designed for efficiency:

- **Event Batching**: Uses async channels to batch file system events
- **Selective Processing**: Only processes `.json` files in watched directories
- **Lazy Refresh**: Blocked cache is only refreshed when dependencies change
- **Non-blocking**: File I/O and database operations run asynchronously

## Dependencies

- `notify` - File system watching
- `tokio` - Async runtime
- `bd-storage` - Database and file I/O operations
- `bd-core` - Core types and error handling
- `tracing` - Structured logging

## License

MIT OR Apache-2.0
