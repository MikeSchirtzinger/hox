# jj OpLog Watcher

The `bd-daemon` crate now includes an efficient operation log watcher for jj repositories. Instead of watching the file system for changes, the daemon can poll jj's operation log to detect changes to task and dependency files.

## Why OpLog Watching?

For jj repositories, watching the operation log is more efficient than file system watching because:

1. **Fewer false positives**: Only actual committed changes are detected, not editor temp files or other noise
2. **Batch operations**: Multiple file changes in a single jj operation are processed together
3. **Atomic changes**: Changes are detected after the jj operation completes, ensuring consistency
4. **Less overhead**: No need to watch every file in the repository

## Usage

### Enable OpLog Watching in Daemon

```rust
use bd_daemon::{Daemon, DaemonConfig};
use bd_storage::Database;
use std::sync::Arc;
use std::time::Duration;

let db = Database::open(".beads/turso.db").await?;
let storage = Arc::new(db);

let config = DaemonConfig {
    debounce_interval: Duration::from_millis(100),
    blocked_cache_refresh_interval: Duration::from_secs(5),
    use_oplog_watcher: true,  // Enable oplog watching
    oplog_poll_interval: Duration::from_millis(100),
};

let mut daemon = Daemon::new_with_config(storage, ".beads", config);
daemon.run().await?;
```

The daemon will automatically:
- Check if jj is available in PATH
- Check if the repository is a jj repository
- Fall back to file system watching if jj is not available

### Standalone OpLog Watcher

You can also use the oplog watcher directly without the daemon:

```rust
use bd_daemon::oplog::{OpLogWatcher, OpLogWatcherConfig};
use std::time::Duration;

let config = OpLogWatcherConfig {
    repo_path: ".".into(),
    poll_interval: Duration::from_millis(100),
    tasks_dir: "tasks".to_string(),
    deps_dir: "deps".to_string(),
    last_op_id: None,
};

let watcher = OpLogWatcher::new(config)?;

watcher.watch(|entries| {
    for entry in entries {
        println!("Operation: {} - {}", &entry.id[..12], entry.description);
        for file in &entry.affected_files {
            println!("  Changed: {}", file.display());
        }
    }
    Ok(())
}).await?;
```

## How It Works

1. **Poll Operation Log**: The watcher periodically runs `jj op log` to get recent operations
2. **Detect New Operations**: Compares operation IDs to find new operations since last poll
3. **Parse Affected Files**: For each new operation, runs `jj op show` to get changed files
4. **Filter Relevant Files**: Only task/*.json and deps/*.json files are processed
5. **Callback Invocation**: The callback is called with all new operations in chronological order

## Configuration

- `use_oplog_watcher`: Enable oplog watching (default: false)
- `oplog_poll_interval`: How often to poll the operation log (default: 100ms)
- `tasks_dir`: Directory containing task files (default: "tasks")
- `deps_dir`: Directory containing dependency files (default: "deps")

## Requirements

- jj must be installed and available in PATH
- The repository must be a jj repository
- The daemon will gracefully fall back to file system watching if these requirements are not met

## Performance

The oplog watcher is designed to be efficient:
- Minimal overhead: Only runs `jj op log` periodically
- Smart polling: Only processes operations newer than last seen
- Batch processing: All files from an operation are processed together
- No file system events: Avoids the overhead of file system watching

## Limitations

- Only works with jj repositories
- Requires jj to be installed
- May have a slight delay (poll interval) before detecting changes
- Does not detect uncommitted changes (by design)
