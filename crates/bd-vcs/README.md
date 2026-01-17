# bd-vcs - VCS Abstraction Layer

Version control system abstraction layer for jj-beads-rs. Provides a unified interface for Git and Jujutsu operations.

## Features

- **VCS Backend Trait**: Unified interface for version control operations
- **Git Support**: Full implementation using the `gix` crate
- **Jujutsu Support**: Detection logic with placeholder for future implementation
- **Auto-detection**: Automatically detects repository type (prefers Git when both exist)
- **Thread-safe**: Uses `Arc<Mutex<>>` for safe concurrent access

## API Overview

### Core Types

- `Vcs` - Main interface that auto-detects and delegates to appropriate backend
- `VcsBackend` trait - Interface that all backends must implement
- `GitBackend` - Git implementation using gix
- `JjBackend` - Jujutsu placeholder (to be implemented)

### Operations

```rust
use bd_vcs::Vcs;

// Open a repository (auto-detects Git or Jujutsu)
let vcs = Vcs::open("/path/to/repo")?;

// Get current commit ID
let commit = vcs.current_commit()?;

// Find files matching a glob pattern
let files = vcs.find_files("*.rs")?;

// Check if a file is tracked
let tracked = vcs.is_tracked("README.md")?;

// Get changed files since a commit
let changed = vcs.changed_files("HEAD~5")?;

// Get repository root
let root = vcs.repo_root();
```

## Implementation Details

### GitBackend

The Git backend is implemented using the `gix` crate (pure Rust Git implementation). Key features:

- **Repository Discovery**: Walks up directory tree to find Git repos
- **Commit Operations**: Read HEAD, resolve commit references
- **Tree Traversal**: Breadth-first tree walking for file discovery
- **Diff Operations**: Compare trees to find changed files
- **File Tracking**: Lookup files in Git index

### Thread Safety

`gix::Repository` contains `RefCell` internally and is not `Sync`. We wrap it in `Arc<Mutex<>>` to make it thread-safe, allowing the VCS backend to be used across threads as required by the trait bounds.

### Detection Order

When both `.git` and `.jj` directories exist (common in cohabitation setups), the system prefers Git. This is because:

1. Jujutsu can coexist with Git (it's a common workflow)
2. Git is more mature and fully implemented
3. Users can explicitly use JjBackend if needed

## Testing

```bash
# Run all tests
cargo test --package bd-vcs

# Run with a specific test
cargo test --package bd-vcs test_vcs_detection_git

# Run the example
cargo run --package bd-vcs --example test_vcs -- /path/to/repo
```

## Examples

See `examples/test_vcs.rs` for a complete demonstration of all VCS operations.

## Error Handling

All operations return `bd_core::Result<T>` with these error types:

- `Error::NotInVcs` - Not in a VCS repository
- `Error::Vcs(String)` - General VCS error
- `Error::Git(..)` - Git-specific errors
- `Error::GitRef(String)` - Git reference errors
- `Error::GitTraversal(String)` - Git tree traversal errors
- `Error::InvalidCommit(String)` - Invalid commit reference

## Future Work

- [ ] Complete JjBackend implementation using jj library
- [ ] Add changed_files support for working directory (not just commits)
- [ ] Add staging area operations for Git
- [ ] Performance optimization for large repositories
- [ ] Caching layer for frequently accessed data
