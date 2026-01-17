//! File watcher daemon for beads.
//!
//! This crate provides background file watching and automatic syncing
//! of .task.json and .deps.json files to the database.
//!
//! # Features
//!
//! - **Debouncing**: Batches rapid file changes to avoid excessive DB writes
//! - **Periodic cache refresh**: Background task that updates blocked cache
//! - **Full sync on startup**: Ensures database is up-to-date when daemon starts
//! - **Change queue**: Batches multiple changes to same file
//! - **Graceful shutdown**: Processes pending changes before exit
//! - **Optional Dashboard**: Real-time monitoring HTTP server (enable with `dashboard` feature)
//!
//! # Example
//!
//! ```no_run
//! use bd_daemon::{Daemon, DaemonConfig};
//! use bd_storage::Database;
//! use std::sync::Arc;
//! use std::time::Duration;
//! use tokio::sync::RwLock;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let db = Database::open(".beads/turso.db").await?;
//! let storage = Arc::new(RwLock::new(db));
//!
//! let config = DaemonConfig {
//!     debounce_interval: Duration::from_millis(100),
//!     blocked_cache_refresh_interval: Duration::from_secs(5),
//!     use_oplog_watcher: false,
//!     oplog_poll_interval: Duration::from_millis(100),
//! };
//!
//! let mut daemon = Daemon::new_with_config(storage, ".beads", config);
//! daemon.run().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # jj OpLog Watching
//!
//! For jj repositories, you can use the more efficient oplog watcher:
//!
//! ```no_run
//! use bd_daemon::oplog::{OpLogWatcher, OpLogWatcherConfig};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = OpLogWatcherConfig {
//!     repo_path: ".".into(),
//!     poll_interval: Duration::from_millis(100),
//!     tasks_dir: "tasks".to_string(),
//!     deps_dir: "deps".to_string(),
//!     last_op_id: None,
//! };
//!
//! let watcher = OpLogWatcher::new(config)?;
//! watcher.watch(|entries| {
//!     // Handle new operations
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Dashboard Feature
//!
//! Enable the `dashboard` feature to get real-time monitoring:
//!
//! ```toml
//! bd-daemon = { version = "0.1", features = ["dashboard"] }
//! ```
//!
//! Then use the dashboard:
//!
//! ```no_run
//! # #[cfg(feature = "dashboard")]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use bd_daemon::dashboard::{DashboardServer, DaemonStats};
//! use std::sync::Arc;
//!
//! let stats = Arc::new(DaemonStats::new());
//! let addr = "127.0.0.1:8080".parse().unwrap();
//! let server = DashboardServer::new(addr, stats, None);
//! server.start().await?;
//! # Ok(())
//! # }
//! ```

// OpLog watcher for jj repositories
pub mod oplog;

// Optional dashboard feature
#[cfg(feature = "dashboard")]
pub mod dashboard;

use bd_core::{Error, Result};
use bd_storage::Database;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// Configuration for the daemon.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// How long to wait before processing file changes.
    /// This batches rapid updates together to avoid excessive DB writes.
    /// Default: 100ms
    pub debounce_interval: Duration,

    /// How often to recompute the blocked cache in the background.
    /// Default: 5 seconds
    pub blocked_cache_refresh_interval: Duration,

    /// Enable jj oplog watching instead of file system watching.
    /// If enabled and jj is available, the daemon will poll jj's operation log
    /// for changes instead of using file system events.
    /// This is more efficient for jj repositories as it only processes actual changes.
    /// Default: false
    pub use_oplog_watcher: bool,

    /// Poll interval for jj oplog watcher (only used if use_oplog_watcher is true).
    /// Default: 100ms
    pub oplog_poll_interval: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            debounce_interval: Duration::from_millis(100),
            blocked_cache_refresh_interval: Duration::from_secs(5),
            use_oplog_watcher: false,
            oplog_poll_interval: Duration::from_millis(100),
        }
    }
}

/// File watcher daemon that monitors for changes to task/dep files.
pub struct Daemon {
    storage: Arc<RwLock<Database>>,
    watch_path: PathBuf,
    config: DaemonConfig,
    shutdown_tx: Option<mpsc::Sender<()>>,
    change_queue: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    #[cfg(feature = "dashboard")]
    stats: Option<Arc<dashboard::DaemonStats>>,
}

impl Daemon {
    /// Create a new daemon instance with default configuration.
    pub fn new(storage: Arc<RwLock<Database>>, watch_path: impl AsRef<Path>) -> Self {
        Self::new_with_config(storage, watch_path, DaemonConfig::default())
    }

    /// Create a new daemon instance with custom configuration.
    pub fn new_with_config(
        storage: Arc<RwLock<Database>>,
        watch_path: impl AsRef<Path>,
        config: DaemonConfig,
    ) -> Self {
        Self {
            storage,
            watch_path: watch_path.as_ref().to_path_buf(),
            config,
            shutdown_tx: None,
            change_queue: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(feature = "dashboard")]
            stats: None,
        }
    }

    /// Enable dashboard statistics tracking.
    #[cfg(feature = "dashboard")]
    pub fn with_stats(mut self, stats: Arc<dashboard::DaemonStats>) -> Self {
        self.stats = Some(stats);
        self
    }

    /// Get a reference to the dashboard statistics (if enabled).
    #[cfg(feature = "dashboard")]
    pub fn stats(&self) -> Option<Arc<dashboard::DaemonStats>> {
        self.stats.clone()
    }

    /// Perform full sync of all task and dependency files to database.
    async fn perform_full_sync(
        &self,
        tasks_dir: &Path,
        deps_dir: &Path,
    ) -> Result<bd_storage::sync::SyncStats> {
        // Read all task files
        let mut tasks_synced = 0;
        let mut tasks_failed = 0;

        if tasks_dir.exists() {
            let mut entries = tokio::fs::read_dir(tasks_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    match bd_storage::task_io::read_task_file(&path).await {
                        Ok(task) => {
                            let db = self.storage.read().await;
                            match db.upsert_task(&task).await {
                                Ok(_) => tasks_synced += 1,
                                Err(e) => {
                                    warn!("Failed to sync task {}: {}", task.id, e);
                                    tasks_failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read task file {}: {}", path.display(), e);
                            tasks_failed += 1;
                        }
                    }
                }
            }
        }

        // Read all dependency files
        let mut deps_synced = 0;
        let mut deps_failed = 0;

        if deps_dir.exists() {
            let mut entries = tokio::fs::read_dir(deps_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    match bd_storage::dep_io::read_dep_file(&path).await {
                        Ok(dep) => {
                            let db = self.storage.read().await;
                            match db.upsert_dep(&dep).await {
                                Ok(_) => deps_synced += 1,
                                Err(e) => {
                                    warn!("Failed to sync dep {}--{}-->{}: {}", dep.from, dep.dep_type, dep.to, e);
                                    deps_failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read dep file {}: {}", path.display(), e);
                            deps_failed += 1;
                        }
                    }
                }
            }
        }

        // Refresh blocked cache
        let mut db = self.storage.write().await;
        if let Err(e) = db.refresh_blocked_cache().await {
            error!("Failed to refresh blocked cache after full sync: {}", e);
        }

        Ok(bd_storage::sync::SyncStats {
            tasks_synced,
            tasks_failed,
            deps_synced,
            deps_failed,
            deleted: 0,
        })
    }

    /// Sync affected files from oplog entries.
    ///
    /// This processes a batch of files changed by jj operations and updates the database.
    async fn sync_affected_files(&self, files: &[PathBuf]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }

        info!("Syncing {} affected files from jj operation", files.len());

        for file in files {
            // Construct full path
            let full_path = self.watch_path.join(file);

            // Determine if this is a task or dep file
            let file_str = file.to_string_lossy();
            let is_task_file = file_str.starts_with("tasks/");
            let is_dep_file = file_str.starts_with("deps/");

            if is_task_file {
                if let Err(e) = Self::process_task_change(
                    &self.storage,
                    &full_path,
                    &self.watch_path.join("tasks"),
                )
                .await
                {
                    error!("Error processing task change {}: {}", full_path.display(), e);
                }
            } else if is_dep_file {
                if let Err(e) = Self::process_dep_change(
                    &self.storage,
                    &full_path,
                    &self.watch_path.join("deps"),
                )
                .await
                {
                    error!("Error processing dep change {}: {}", full_path.display(), e);
                }
            }
        }

        Ok(())
    }

    /// Start the daemon and watch for file changes.
    ///
    /// This method blocks until stop() is called or an error occurs.
    /// It performs the following steps:
    /// 1. Full sync of all files to database
    /// 2. Start watching for file changes (either oplog or file system)
    /// 3. Launch background tasks (debounce processor, cache refresh)
    /// 4. Process events until shutdown
    pub async fn run(&mut self) -> Result<()> {
        info!("Starting daemon, watching: {}", self.watch_path.display());

        let tasks_dir = self.watch_path.join("tasks");
        let deps_dir = self.watch_path.join("deps");

        // Create directories if they don't exist
        tokio::fs::create_dir_all(&tasks_dir).await?;
        tokio::fs::create_dir_all(&deps_dir).await?;

        // Perform full sync on startup
        info!("Performing full sync on startup...");
        match self.perform_full_sync(&tasks_dir, &deps_dir).await {
            Ok(stats) => {
                info!(
                    "Full sync complete: {} tasks, {} deps synced ({} errors)",
                    stats.tasks_synced,
                    stats.deps_synced,
                    stats.total_failed()
                );
            }
            Err(e) => {
                error!("Full sync failed: {}", e);
                return Err(Error::Database(format!("Full sync failed: {}", e)));
            }
        }

        // Check if we should use oplog watcher
        if self.config.use_oplog_watcher {
            // Check if jj is available and this is a jj repo
            if oplog::OpLogWatcher::is_jj_available().await
                && oplog::OpLogWatcher::is_jj_repo(&self.watch_path).await
            {
                info!("Using jj oplog watcher for change detection");
                return self.run_with_oplog().await;
            } else {
                warn!(
                    "OpLog watcher requested but jj not available or not a jj repo, falling back to file system watcher"
                );
            }
        }

        // Create channels for file system events
        let (fs_tx, fs_rx) = std::sync::mpsc::channel();

        // Create async channel for event processing
        let (event_tx, mut event_rx) = mpsc::channel(100);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Create file watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Err(e) = fs_tx.send(res) {
                    error!("Failed to send file event: {}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| Error::Watcher(e.to_string()))?;

        // Watch both directories
        watcher
            .watch(&tasks_dir, RecursiveMode::Recursive)
            .map_err(|e| Error::Watcher(e.to_string()))?;
        watcher
            .watch(&deps_dir, RecursiveMode::Recursive)
            .map_err(|e| Error::Watcher(e.to_string()))?;

        info!("Watching tasks: {}", tasks_dir.display());
        info!("Watching deps: {}", deps_dir.display());

        // Spawn task to forward events from sync channel to async channel
        let event_tx_clone = event_tx.clone();
        tokio::task::spawn_blocking(move || {
            while let Ok(res) = fs_rx.recv() {
                match res {
                    Ok(event) => {
                        if event_tx_clone.blocking_send(event).is_err() {
                            break; // Channel closed
                        }
                    }
                    Err(e) => {
                        error!("File watcher error: {}", e);
                    }
                }
            }
        });

        // Spawn debounce processor task
        let debounce_handle = {
            let storage = self.storage.clone();
            let change_queue = self.change_queue.clone();
            let debounce_interval = self.config.debounce_interval;
            let tasks_dir = tasks_dir.clone();
            let deps_dir = deps_dir.clone();

            tokio::spawn(async move {
                Self::debounce_processor(
                    storage,
                    change_queue,
                    debounce_interval,
                    tasks_dir,
                    deps_dir,
                )
                .await;
            })
        };

        // Spawn periodic blocked cache refresh task
        let cache_refresh_handle = {
            let storage = self.storage.clone();
            let refresh_interval = self.config.blocked_cache_refresh_interval;

            tokio::spawn(async move {
                Self::periodic_cache_refresh(storage, refresh_interval).await;
            })
        };

        // Process events until shutdown
        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    if let Err(e) = self.handle_event(event).await {
                        error!("Error handling file event: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
            }
        }

        // Cancel background tasks
        debounce_handle.abort();
        cache_refresh_handle.abort();

        // Process any remaining queued changes
        info!("Processing remaining queued changes...");
        Self::process_pending_changes(
            &self.storage,
            &self.change_queue,
            Duration::ZERO, // Process all immediately
            &tasks_dir,
            &deps_dir,
        )
        .await?;

        // Clean up watcher
        drop(watcher);
        info!("Daemon stopped");

        Ok(())
    }

    /// Run the daemon using jj oplog watcher instead of file system watching.
    ///
    /// This is more efficient for jj repositories as it only processes actual changes
    /// detected by jj's operation log.
    async fn run_with_oplog(&mut self) -> Result<()> {
        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn periodic blocked cache refresh task
        let cache_refresh_handle = {
            let storage = self.storage.clone();
            let refresh_interval = self.config.blocked_cache_refresh_interval;

            tokio::spawn(async move {
                Self::periodic_cache_refresh(storage, refresh_interval).await;
            })
        };

        // Create oplog watcher
        let oplog_config = oplog::OpLogWatcherConfig {
            repo_path: self.watch_path.clone(),
            poll_interval: self.config.oplog_poll_interval,
            tasks_dir: "tasks".to_string(),
            deps_dir: "deps".to_string(),
            last_op_id: None,
        };

        let watcher = oplog::OpLogWatcher::new(oplog_config)
            .map_err(|e| Error::Watcher(format!("Failed to create oplog watcher: {}", e)))?;

        // Clone storage for the callback
        let storage = self.storage.clone();
        let watch_path = self.watch_path.clone();

        // Start watching in a background task
        let watch_handle = tokio::spawn(async move {
            let result = watcher
                .watch(move |entries| {
                    // Collect all affected files
                    let mut all_files = Vec::new();
                    for entry in entries {
                        info!(
                            "jj operation: {} - {}",
                            &entry.id[..12],
                            entry.description
                        );
                        all_files.extend(entry.affected_files.clone());
                    }

                    // Sync affected files
                    let storage = storage.clone();
                    let watch_path = watch_path.clone();
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            // Process each file
                            for file in &all_files {
                                let full_path = watch_path.join(file);
                                let file_str = file.to_string_lossy();

                                if file_str.starts_with("tasks/") {
                                    if let Err(e) = Daemon::process_task_change(
                                        &storage,
                                        &full_path,
                                        &watch_path.join("tasks"),
                                    )
                                    .await
                                    {
                                        error!("Error processing task change: {}", e);
                                    }
                                } else if file_str.starts_with("deps/") {
                                    if let Err(e) = Daemon::process_dep_change(
                                        &storage,
                                        &full_path,
                                        &watch_path.join("deps"),
                                    )
                                    .await
                                    {
                                        error!("Error processing dep change: {}", e);
                                    }
                                }
                            }
                        })
                    });

                    Ok(())
                })
                .await;

            if let Err(e) = result {
                error!("OpLog watcher error: {}", e);
            }
        });

        // Wait for shutdown signal
        let _ = shutdown_rx.recv().await;
        info!("Received shutdown signal");

        // Cancel background tasks
        watch_handle.abort();
        cache_refresh_handle.abort();

        info!("Daemon stopped");
        Ok(())
    }

    /// Background task that processes queued file changes with debouncing.
    async fn debounce_processor(
        storage: Arc<RwLock<Database>>,
        change_queue: Arc<Mutex<HashMap<PathBuf, Instant>>>,
        debounce_interval: Duration,
        tasks_dir: PathBuf,
        deps_dir: PathBuf,
    ) {
        let mut interval = tokio::time::interval(debounce_interval);
        loop {
            interval.tick().await;

            if let Err(e) = Self::process_pending_changes(
                &storage,
                &change_queue,
                debounce_interval,
                &tasks_dir,
                &deps_dir,
            )
            .await
            {
                error!("Error processing pending changes: {}", e);
            }
        }
    }

    /// Process changes that have been queued for longer than debounce_interval.
    async fn process_pending_changes(
        storage: &Arc<RwLock<Database>>,
        change_queue: &Arc<Mutex<HashMap<PathBuf, Instant>>>,
        debounce_interval: Duration,
        tasks_dir: &Path,
        deps_dir: &Path,
    ) -> Result<()> {
        let mut queue = change_queue.lock().await;
        let now = Instant::now();
        let mut paths_to_process = Vec::new();

        // Find paths that are ready to process
        for (path, queued_at) in queue.iter() {
            if now.duration_since(*queued_at) >= debounce_interval {
                paths_to_process.push(path.clone());
            }
        }

        // Remove from queue and process
        for path in paths_to_process {
            queue.remove(&path);
            drop(queue); // Release lock while processing

            debug!("Processing queued change: {}", path.display());

            // Determine if this is a task or dep file
            let path_str = path.to_string_lossy();
            let is_task_file = path_str.contains("/tasks/") || path_str.contains("\\tasks\\");
            let is_dep_file = path_str.contains("/deps/") || path_str.contains("\\deps\\");

            if is_task_file {
                if let Err(e) = Self::process_task_change(storage, &path, tasks_dir).await {
                    error!("Error processing task change {}: {}", path.display(), e);
                }
            } else if is_dep_file {
                if let Err(e) = Self::process_dep_change(storage, &path, deps_dir).await {
                    error!("Error processing dep change {}: {}", path.display(), e);
                }
            }

            queue = change_queue.lock().await; // Re-acquire lock
        }

        Ok(())
    }

    /// Process a task file change (create/modify/delete).
    async fn process_task_change(
        storage: &Arc<RwLock<Database>>,
        path: &Path,
        _tasks_dir: &Path,
    ) -> Result<()> {
        // Check if file exists (modify/create) or was deleted
        if path.exists() {
            // Read and sync task
            info!("Task file changed: {}", path.display());
            match bd_storage::task_io::read_task_file(path).await {
                Ok(task) => {
                    {
                        let db = storage.read().await;
                        db.upsert_task(&task)
                            .await
                            .map_err(|e| Error::Database(e.to_string()))?;
                    }
                    info!("Synced task to database: {}", task.id);

                    // Refresh blocked cache after task changes
                    let mut db = storage.write().await;
                    if let Err(e) = db.refresh_blocked_cache().await {
                        error!("Failed to refresh blocked cache: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to read task file {}: {}", path.display(), e);
                }
            }
        } else {
            // File was deleted
            if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                info!("Task file deleted: {} (id: {})", path.display(), filename);

                {
                    let db = storage.read().await;
                    db.delete_task(filename)
                        .await
                        .map_err(|e| Error::Database(e.to_string()))?;
                }
                info!("Deleted task from database: {}", filename);

                // Refresh blocked cache after task deletion
                let mut db = storage.write().await;
                if let Err(e) = db.refresh_blocked_cache().await {
                    error!("Failed to refresh blocked cache: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Process a dependency file change (create/modify/delete).
    async fn process_dep_change(
        storage: &Arc<RwLock<Database>>,
        path: &Path,
        _deps_dir: &Path,
    ) -> Result<()> {
        // Check if file exists (modify/create) or was deleted
        if path.exists() {
            // Read and sync dep
            info!("Dep file changed: {}", path.display());
            match bd_storage::dep_io::read_dep_file(path).await {
                Ok(dep) => {
                    {
                        let db = storage.read().await;
                        db.upsert_dep(&dep)
                            .await
                            .map_err(|e| Error::Database(e.to_string()))?;
                    }
                    info!(
                        "Synced dep to database: {} --{}--> {}",
                        dep.from, dep.dep_type, dep.to
                    );

                    // Refresh blocked cache after dependency changes
                    let mut db = storage.write().await;
                    if let Err(e) = db.refresh_blocked_cache().await {
                        error!("Failed to refresh blocked cache: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to read dep file {}: {}", path.display(), e);
                }
            }
        } else {
            // File was deleted
            if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                let parts: Vec<&str> = filename.split("--").collect();
                if parts.len() == 3 {
                    let from = parts[0];
                    let dep_type = parts[1];
                    let to = parts[2];

                    info!(
                        "Dep file deleted: {} ({}--{}-->{})",
                        path.display(),
                        from,
                        dep_type,
                        to
                    );

                    {
                        let db = storage.read().await;
                        db.delete_dep(from, to, dep_type)
                            .await
                            .map_err(|e| Error::Database(e.to_string()))?;
                    }
                    info!("Deleted dep from database: {}--{}-->{}", from, dep_type, to);

                    // Refresh blocked cache after dependency deletion
                    let mut db = storage.write().await;
                    if let Err(e) = db.refresh_blocked_cache().await {
                        error!("Failed to refresh blocked cache: {}", e);
                    }
                } else {
                    warn!("Invalid dep filename format: {}", filename);
                }
            }
        }

        Ok(())
    }

    /// Background task that periodically refreshes the blocked cache.
    async fn periodic_cache_refresh(storage: Arc<RwLock<Database>>, refresh_interval: Duration) {
        let mut interval = tokio::time::interval(refresh_interval);
        loop {
            interval.tick().await;

            debug!("Periodic blocked cache refresh");
            let mut db = storage.write().await;
            if let Err(e) = db.refresh_blocked_cache().await {
                error!("Failed to refresh blocked cache: {}", e);
            }
        }
    }

    /// Handle a file system event by queuing it for debounced processing.
    async fn handle_event(&self, event: Event) -> Result<()> {
        debug!("Handling file event: {:?}", event);

        // Filter for relevant event kinds
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                // Process each path in the event
                for path in event.paths {
                    // Only process .json files
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }

                    // Determine if this is a task or dep file
                    let path_str = path.to_string_lossy();
                    let is_task_file =
                        path_str.contains("/tasks/") || path_str.contains("\\tasks\\");
                    let is_dep_file = path_str.contains("/deps/") || path_str.contains("\\deps\\");

                    if !is_task_file && !is_dep_file {
                        continue;
                    }

                    // Queue the change for debounced processing
                    self.queue_change(path).await;
                }
            }
            _ => {
                // Ignore other event types (access, metadata changes, etc.)
            }
        }

        Ok(())
    }

    /// Add a file to the change queue with current timestamp.
    async fn queue_change(&self, path: PathBuf) {
        let mut queue = self.change_queue.lock().await;
        debug!("Queuing change: {}", path.display());
        queue.insert(path, Instant::now());
    }

    /// Stop the daemon gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping daemon");

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bd_core::TaskFile;
    use chrono::Utc;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn create_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::open(&db_path).await.unwrap();
        db.init_schema().await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_daemon_creation() {
        let (db, _temp_dir) = create_test_db().await;
        let watch_path = TempDir::new().unwrap();

        let daemon = Daemon::new(Arc::new(RwLock::new(db)), watch_path.path());
        assert_eq!(daemon.watch_path, watch_path.path());
    }

    #[tokio::test]
    async fn test_daemon_with_custom_config() {
        let (db, _temp_dir) = create_test_db().await;
        let watch_path = TempDir::new().unwrap();

        let config = DaemonConfig {
            debounce_interval: Duration::from_millis(200),
            blocked_cache_refresh_interval: Duration::from_secs(10),
            use_oplog_watcher: false,
            oplog_poll_interval: Duration::from_millis(100),
        };

        let daemon = Daemon::new_with_config(Arc::new(RwLock::new(db)), watch_path.path(), config.clone());
        assert_eq!(daemon.config.debounce_interval, config.debounce_interval);
        assert_eq!(
            daemon.config.blocked_cache_refresh_interval,
            config.blocked_cache_refresh_interval
        );
    }

    #[tokio::test]
    async fn test_daemon_config_with_oplog_enabled() {
        let (db, _temp_dir) = create_test_db().await;
        let watch_path = TempDir::new().unwrap();

        let config = DaemonConfig {
            debounce_interval: Duration::from_millis(100),
            blocked_cache_refresh_interval: Duration::from_secs(5),
            use_oplog_watcher: true,
            oplog_poll_interval: Duration::from_millis(50),
        };

        let daemon = Daemon::new_with_config(Arc::new(RwLock::new(db)), watch_path.path(), config.clone());
        assert_eq!(daemon.config.use_oplog_watcher, true);
        assert_eq!(daemon.config.oplog_poll_interval, Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_process_task_change() {
        let (db, _db_temp) = create_test_db().await;
        let watch_dir = TempDir::new().unwrap();
        let tasks_dir = watch_dir.path().join("tasks");
        tokio::fs::create_dir_all(&tasks_dir).await.unwrap();

        let storage = Arc::new(RwLock::new(db));

        // Create a test task file
        let task = TaskFile {
            id: "test-123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        let task_path = tasks_dir.join("test-123.json");
        bd_storage::task_io::write_task_file(&tasks_dir, &task)
            .await
            .unwrap();

        // Process the change
        Daemon::process_task_change(&storage, &task_path, &tasks_dir)
            .await
            .unwrap();

        // Verify task was synced to database
        let db = storage.read().await;
        let retrieved = db.get_task_by_id("test-123").await.unwrap();
        assert_eq!(retrieved.id, "test-123");
        assert_eq!(retrieved.title, "Test Task");
    }

    #[tokio::test]
    async fn test_process_task_deletion() {
        let (db, _db_temp) = create_test_db().await;
        let watch_dir = TempDir::new().unwrap();
        let tasks_dir = watch_dir.path().join("tasks");
        tokio::fs::create_dir_all(&tasks_dir).await.unwrap();

        let storage = Arc::new(RwLock::new(db));

        // Create a task in the database
        let task = TaskFile {
            id: "test-delete".to_string(),
            title: "Test Task".to_string(),
            description: None,
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        {
            let db = storage.read().await;
            db.upsert_task(&task).await.unwrap();
        }

        // Verify it exists
        {
            let db = storage.read().await;
            assert!(db.get_task_by_id("test-delete").await.is_ok());
        }

        // Simulate file deletion (path doesn't exist)
        let task_path = tasks_dir.join("test-delete.json");
        Daemon::process_task_change(&storage, &task_path, &tasks_dir)
            .await
            .unwrap();

        // Verify task was deleted from database
        let db = storage.read().await;
        assert!(db.get_task_by_id("test-delete").await.is_err());
    }

    #[tokio::test]
    async fn test_change_queue_batching() {
        let (db, _db_temp) = create_test_db().await;
        let watch_dir = TempDir::new().unwrap();
        let tasks_dir = watch_dir.path().join("tasks");
        tokio::fs::create_dir_all(&tasks_dir).await.unwrap();

        let storage = Arc::new(RwLock::new(db));
        let change_queue = Arc::new(Mutex::new(HashMap::new()));

        // Create a test task file
        let task = TaskFile {
            id: "test-batch".to_string(),
            title: "Test Batch".to_string(),
            description: None,
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        let task_path = tasks_dir.join("test-batch.json");
        bd_storage::task_io::write_task_file(&tasks_dir, &task)
            .await
            .unwrap();

        // Queue the change
        {
            let mut queue = change_queue.lock().await;
            queue.insert(task_path.clone(), Instant::now());
        }

        // Immediately try to process - should not process (debounce not elapsed)
        Daemon::process_pending_changes(
            &storage,
            &change_queue,
            Duration::from_millis(100),
            &tasks_dir,
            &watch_dir.path().join("deps"),
        )
        .await
        .unwrap();

        // Queue should still have the item
        {
            let queue = change_queue.lock().await;
            assert_eq!(queue.len(), 1);
        }

        // Wait for debounce interval
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Now process - should succeed
        Daemon::process_pending_changes(
            &storage,
            &change_queue,
            Duration::from_millis(100),
            &tasks_dir,
            &watch_dir.path().join("deps"),
        )
        .await
        .unwrap();

        // Queue should be empty
        {
            let queue = change_queue.lock().await;
            assert_eq!(queue.len(), 0);
        }

        // Verify task was synced
        let db = storage.read().await;
        assert!(db.get_task_by_id("test-batch").await.is_ok());
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = DaemonConfig::default();
        assert_eq!(config.debounce_interval, Duration::from_millis(100));
        assert_eq!(
            config.blocked_cache_refresh_interval,
            Duration::from_secs(5)
        );
    }

    #[tokio::test]
    async fn test_full_sync_on_startup() {
        let (db, _db_temp) = create_test_db().await;
        let watch_dir = TempDir::new().unwrap();
        let tasks_dir = watch_dir.path().join("tasks");
        let deps_dir = watch_dir.path().join("deps");
        tokio::fs::create_dir_all(&tasks_dir).await.unwrap();
        tokio::fs::create_dir_all(&deps_dir).await.unwrap();

        // Create some test task files
        let task1 = TaskFile {
            id: "startup-1".to_string(),
            title: "Startup Task 1".to_string(),
            description: None,
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        let task2 = TaskFile {
            id: "startup-2".to_string(),
            title: "Startup Task 2".to_string(),
            description: None,
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        bd_storage::task_io::write_task_file(&tasks_dir, &task1)
            .await
            .unwrap();
        bd_storage::task_io::write_task_file(&tasks_dir, &task2)
            .await
            .unwrap();

        let storage = Arc::new(RwLock::new(db));

        // Create daemon (this will trigger full sync on run())
        let mut daemon = Daemon::new(storage.clone(), watch_dir.path());

        // Start daemon in background and immediately stop it
        let daemon_handle = tokio::spawn(async move {
            daemon.run().await.unwrap();
        });

        // Give it time to do full sync
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify tasks were synced during startup
        {
            let db = storage.read().await;
            assert!(db.get_task_by_id("startup-1").await.is_ok());
            assert!(db.get_task_by_id("startup-2").await.is_ok());
        }

        // Stop daemon
        daemon_handle.abort();
    }
}
