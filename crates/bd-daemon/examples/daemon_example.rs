//! Example of using the bd-daemon file watcher.
//!
//! This example demonstrates how to:
//! 1. Create a database connection
//! 2. Initialize the daemon
//! 3. Start watching for file changes
//! 4. Handle graceful shutdown
//!
//! Run with:
//! ```bash
//! cargo run --package bd-daemon --example daemon_example
//! ```

use bd_daemon::Daemon;
use bd_storage::Database;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Get watch path from args or use current directory
    let watch_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".".to_string());

    info!("Beads file watcher daemon starting...");
    info!("Watch path: {}", watch_path);

    // Open database connection
    let db_path = format!("{}/.beads/turso.db", watch_path);
    let db = Database::open(&db_path).await?;
    db.init_schema().await?;

    info!("Database initialized at: {}", db_path);

    // Create daemon instance
    let mut daemon = Daemon::new(Arc::new(RwLock::new(db)), &watch_path);

    // Spawn daemon in background task
    let daemon_handle = tokio::spawn(async move {
        if let Err(e) = daemon.run().await {
            eprintln!("Daemon error: {}", e);
        }
    });

    info!("Daemon running. Press Ctrl+C to stop.");
    info!("Watching for changes to tasks/*.json and deps/*.json files...");

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    info!("Received shutdown signal, stopping daemon...");

    // Wait for daemon to finish
    daemon_handle.await?;

    info!("Daemon stopped gracefully");
    Ok(())
}
