# jj-beads-rs

A Rust rewrite of the beads file-based issue tracking system.

## Overview

Beads is a file-based issue tracking system that uses `.task.json` and `.deps.json` files to track issues and their dependencies directly in your repository.

## Project Structure

This is a Cargo workspace with the following crates:

- **bd-core**: Core types and traits (Issue, Dependency, Comment)
- **bd-storage**: Database persistence layer using libsql (SQLite)
- **bd-vcs**: Version control system abstraction (Git/Jujutsu)
- **bd-daemon**: File watcher daemon for automatic syncing
- **bd-cli**: Command-line interface binary

## Building

```bash
cargo build
```

## Running

```bash
# Build and install the CLI
cargo install --path crates/bd-cli

# Or run directly
cargo run --bin beads -- --help
```

## Development

```bash
# Check all crates
cargo check

# Run tests
cargo test

# Run specific crate
cargo check -p bd-core
```

## Architecture

The system follows a layered architecture:

1. **Core Layer** (bd-core): Domain types and schemas
2. **Storage Layer** (bd-storage): Database operations
3. **VCS Layer** (bd-vcs): Version control integration
4. **Daemon Layer** (bd-daemon): File watching and syncing
5. **CLI Layer** (bd-cli): User interface

## License

MIT OR Apache-2.0
