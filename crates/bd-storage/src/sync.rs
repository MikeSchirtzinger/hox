//! Sync module for coordinating file I/O with database operations.
//!
//! This module provides the `SyncManager` struct that synchronizes JSON task/dep files
//! with the libSQL database cache. It supports:
//!
//! - Full sync: Read all files from directories and upsert to database
//! - Incremental sync: Use VCS to identify changed files and sync only those
//! - Export: Write all database records back to JSON files
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────┐
//! │              File System                          │
//! │  tasks/task-*.json    deps/*--*--*.json          │
//! └───────────────┬───────────────────────────────────┘
//!                 │
//!                 ▼
//! ┌───────────────────────────────────────────────────┐
//! │           SyncManager                             │
//! │  • sync_all() - Full directory sync               │
//! │  • sync_changed() - VCS incremental sync          │
//! │  • export_all() - Database to files               │
//! └───────────────┬───────────────────────────────────┘
//!                 │
//!                 ▼
//! ┌───────────────────────────────────────────────────┐
//! │         Database (libSQL)                         │
//! │  tasks, deps, blocked_cache tables                │
//! └───────────────────────────────────────────────────┘
//! ```
//!
//! # Example Usage
//!
//! ```no_run
//! use bd_storage::{Database, sync::SyncManager};
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Initialize database
//! let db = Database::open(".beads/turso.db").await?;
//! db.init_schema().await?;
//!
//! // Create sync manager
//! let mut sync_manager = SyncManager::new(
//!     db,
//!     Path::new("tasks"),
//!     Path::new("deps")
//! );
//!
//! // Full sync from files to database
//! let stats = sync_manager.sync_all().await?;
//! println!("Synced {} tasks, {} deps", stats.tasks_synced, stats.deps_synced);
//!
//! // Export from database to files
//! let export_stats = sync_manager.export_all().await?;
//! println!("Exported {} tasks, {} deps", export_stats.tasks_exported, export_stats.deps_exported);
//! # Ok(())
//! # }
//! ```

use crate::db::Database;
use crate::dep_io::{read_dep_file, write_dep_file};
use crate::task_io::{read_task_file, write_task_file};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Statistics for sync operations
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Number of tasks successfully synced to database
    pub tasks_synced: usize,
    /// Number of tasks that failed to sync
    pub tasks_failed: usize,
    /// Number of dependencies successfully synced to database
    pub deps_synced: usize,
    /// Number of dependencies that failed to sync
    pub deps_failed: usize,
    /// Number of files deleted from database
    pub deleted: usize,
}

impl SyncStats {
    /// Returns true if any errors occurred during sync
    pub fn has_errors(&self) -> bool {
        self.tasks_failed > 0 || self.deps_failed > 0
    }

    /// Returns total number of items successfully synced
    pub fn total_synced(&self) -> usize {
        self.tasks_synced + self.deps_synced + self.deleted
    }

    /// Returns total number of items that failed
    pub fn total_failed(&self) -> usize {
        self.tasks_failed + self.deps_failed
    }
}

/// Statistics for export operations
#[derive(Debug, Clone, Default)]
pub struct ExportStats {
    /// Number of tasks successfully exported to files
    pub tasks_exported: usize,
    /// Number of tasks that failed to export
    pub tasks_failed: usize,
    /// Number of dependencies successfully exported to files
    pub deps_exported: usize,
    /// Number of dependencies that failed to export
    pub deps_failed: usize,
}

impl ExportStats {
    /// Returns true if any errors occurred during export
    pub fn has_errors(&self) -> bool {
        self.tasks_failed > 0 || self.deps_failed > 0
    }

    /// Returns total number of items successfully exported
    pub fn total_exported(&self) -> usize {
        self.tasks_exported + self.deps_exported
    }

    /// Returns total number of items that failed
    pub fn total_failed(&self) -> usize {
        self.tasks_failed + self.deps_failed
    }
}

/// Manages synchronization between file system and database
pub struct SyncManager {
    db: Database,
    tasks_dir: PathBuf,
    deps_dir: PathBuf,
}

impl SyncManager {
    /// Create a new SyncManager
    ///
    /// # Arguments
    /// * `db` - Database connection
    /// * `tasks_dir` - Path to tasks directory (e.g., "tasks/")
    /// * `deps_dir` - Path to deps directory (e.g., "deps/")
    pub fn new(db: Database, tasks_dir: &Path, deps_dir: &Path) -> Self {
        Self {
            db,
            tasks_dir: tasks_dir.to_path_buf(),
            deps_dir: deps_dir.to_path_buf(),
        }
    }

    /// Perform a full sync from files to database
    ///
    /// Reads all task files from tasks_dir and all dep files from deps_dir,
    /// validates them, and upserts to the database. Individual file failures
    /// are logged but do not stop the sync process.
    ///
    /// After syncing all files, refreshes the blocked cache to ensure
    /// dependency state is up to date.
    ///
    /// # Returns
    /// * `Ok(SyncStats)` - Statistics about the sync operation
    /// * `Err(_)` - Critical error (directory access, database failure)
    ///
    /// # Example
    /// ```no_run
    /// # use bd_storage::{Database, sync::SyncManager};
    /// # use std::path::Path;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = Database::open(".beads/turso.db").await?;
    /// # let mut sync_manager = SyncManager::new(db, Path::new("tasks"), Path::new("deps"));
    /// let stats = sync_manager.sync_all().await?;
    /// println!("Synced {} tasks and {} deps", stats.tasks_synced, stats.deps_synced);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sync_all(&mut self) -> crate::Result<SyncStats> {
        info!(
            "Starting full sync: tasks={}, deps={}",
            self.tasks_dir.display(),
            self.deps_dir.display()
        );

        let mut stats = SyncStats::default();

        // Sync all task files
        self.sync_all_tasks(&mut stats).await?;

        // Sync all dependency files
        self.sync_all_deps(&mut stats).await?;

        // Refresh blocked cache after syncing
        info!("Refreshing blocked cache...");
        self.db.refresh_blocked_cache().await?;

        info!(
            "Full sync complete: tasks={} (failed={}), deps={} (failed={})",
            stats.tasks_synced, stats.tasks_failed, stats.deps_synced, stats.deps_failed
        );

        Ok(stats)
    }

    /// Sync only files that have changed since a specific commit
    ///
    /// Uses VCS (git/jj) to identify files that have changed since the specified
    /// commit, then syncs only those files. This is much faster than a full sync
    /// for incremental updates.
    ///
    /// # Arguments
    /// * `since_commit` - Commit hash to diff against (e.g., "HEAD~1", "abc123")
    ///
    /// # Returns
    /// * `Ok(SyncStats)` - Statistics about the sync operation
    /// * `Err(_)` - VCS command failed, file access error, or database failure
    ///
    /// # Example
    /// ```no_run
    /// # use bd_storage::{Database, sync::SyncManager};
    /// # use std::path::Path;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = Database::open(".beads/turso.db").await?;
    /// # let mut sync_manager = SyncManager::new(db, Path::new("tasks"), Path::new("deps"));
    /// // Sync files changed since last commit
    /// let stats = sync_manager.sync_changed("HEAD~1").await?;
    /// println!("Synced {} changed files", stats.total_synced());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sync_changed(&mut self, since_commit: &str) -> crate::Result<SyncStats> {
        info!("Starting incremental sync since commit: {}", since_commit);

        let mut stats = SyncStats::default();

        // Get list of changed files from VCS
        let changed_files = self.get_changed_files(since_commit).await?;

        if changed_files.is_empty() {
            info!("No files changed since {}", since_commit);
            return Ok(stats);
        }

        info!("Found {} changed files", changed_files.len());

        // Process each changed file
        for (path, change_type) in changed_files {
            match change_type.as_str() {
                "D" => {
                    // File deleted - remove from database
                    if let Err(e) = self.handle_deletion(&path).await {
                        warn!("Failed to handle deletion of {}: {}", path.display(), e);
                    } else {
                        stats.deleted += 1;
                    }
                }
                _ => {
                    // File added or modified - sync to database
                    if path.starts_with(&self.tasks_dir) && path.extension() == Some(std::ffi::OsStr::new("json")) {
                        match self.sync_task_file(&path).await {
                            Ok(_) => stats.tasks_synced += 1,
                            Err(e) => {
                                warn!("Failed to sync task {}: {}", path.display(), e);
                                stats.tasks_failed += 1;
                            }
                        }
                    } else if path.starts_with(&self.deps_dir) && path.extension() == Some(std::ffi::OsStr::new("json")) {
                        match self.sync_dep_file(&path).await {
                            Ok(_) => stats.deps_synced += 1,
                            Err(e) => {
                                warn!("Failed to sync dep {}: {}", path.display(), e);
                                stats.deps_failed += 1;
                            }
                        }
                    }
                }
            }
        }

        // Refresh blocked cache if we synced anything
        if stats.total_synced() > 0 {
            info!("Refreshing blocked cache...");
            self.db.refresh_blocked_cache().await?;
        }

        info!(
            "Incremental sync complete: tasks={} (failed={}), deps={} (failed={}), deleted={}",
            stats.tasks_synced, stats.tasks_failed, stats.deps_synced, stats.deps_failed, stats.deleted
        );

        Ok(stats)
    }

    /// Export all tasks and dependencies from database to files
    ///
    /// Queries all tasks and deps from the database and writes them to
    /// {tasks_dir}/*.json and {deps_dir}/*.json files. This is useful for:
    /// - Initial setup when database exists but files don't
    /// - Recovery from file system issues
    /// - Creating backups
    ///
    /// # Returns
    /// * `Ok(ExportStats)` - Statistics about the export operation
    /// * `Err(_)` - Database query failed or file write failed
    ///
    /// # Example
    /// ```no_run
    /// # use bd_storage::{Database, sync::SyncManager};
    /// # use std::path::Path;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = Database::open(".beads/turso.db").await?;
    /// # let sync_manager = SyncManager::new(db, Path::new("tasks"), Path::new("deps"));
    /// let stats = sync_manager.export_all().await?;
    /// println!("Exported {} tasks and {} deps", stats.tasks_exported, stats.deps_exported);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn export_all(&self) -> crate::Result<ExportStats> {
        info!(
            "Starting export to tasks={}, deps={}",
            self.tasks_dir.display(),
            self.deps_dir.display()
        );

        let mut stats = ExportStats::default();

        // Ensure directories exist
        fs::create_dir_all(&self.tasks_dir).await?;
        fs::create_dir_all(&self.deps_dir).await?;

        // Export all tasks
        info!("Exporting tasks...");
        let tasks = self.db.list_all_tasks().await?;
        for task in tasks {
            match write_task_file(&self.tasks_dir, &task).await {
                Ok(_) => {
                    debug!("Exported task: {}", task.id);
                    stats.tasks_exported += 1;
                }
                Err(e) => {
                    warn!("Failed to export task {}: {}", task.id, e);
                    stats.tasks_failed += 1;
                }
            }
        }

        // Export all dependencies
        info!("Exporting dependencies...");
        let deps = self.db.list_all_deps().await?;
        for dep in deps {
            match write_dep_file(&self.deps_dir, &dep).await {
                Ok(_) => {
                    debug!("Exported dep: {} --{}--> {}", dep.from, dep.dep_type, dep.to);
                    stats.deps_exported += 1;
                }
                Err(e) => {
                    warn!("Failed to export dep {} --{}--> {}: {}", dep.from, dep.dep_type, dep.to, e);
                    stats.deps_failed += 1;
                }
            }
        }

        info!(
            "Export complete: tasks={} (failed={}), deps={} (failed={})",
            stats.tasks_exported, stats.tasks_failed, stats.deps_exported, stats.deps_failed
        );

        Ok(stats)
    }

    // ===== Private helper methods =====

    /// Sync all task files from tasks directory
    async fn sync_all_tasks(&self, stats: &mut SyncStats) -> crate::Result<()> {
        // Check if directory exists
        if !self.tasks_dir.exists() {
            info!("Tasks directory doesn't exist: {} (skipping)", self.tasks_dir.display());
            return Ok(());
        }

        let mut entries = fs::read_dir(&self.tasks_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Skip directories and non-JSON files
            if !path.is_file() {
                continue;
            }

            if path.extension() != Some(std::ffi::OsStr::new("json")) {
                continue;
            }

            // Try to sync the task
            match self.sync_task_file(&path).await {
                Ok(_) => stats.tasks_synced += 1,
                Err(e) => {
                    warn!("Failed to sync task {}: {}", path.display(), e);
                    stats.tasks_failed += 1;
                }
            }
        }

        Ok(())
    }

    /// Sync all dependency files from deps directory
    async fn sync_all_deps(&self, stats: &mut SyncStats) -> crate::Result<()> {
        // Check if directory exists
        if !self.deps_dir.exists() {
            info!("Deps directory doesn't exist: {} (skipping)", self.deps_dir.display());
            return Ok(());
        }

        let mut entries = fs::read_dir(&self.deps_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Skip directories and non-JSON files
            if !path.is_file() {
                continue;
            }

            if path.extension() != Some(std::ffi::OsStr::new("json")) {
                continue;
            }

            // Try to sync the dependency
            match self.sync_dep_file(&path).await {
                Ok(_) => stats.deps_synced += 1,
                Err(e) => {
                    warn!("Failed to sync dep {}: {}", path.display(), e);
                    stats.deps_failed += 1;
                }
            }
        }

        Ok(())
    }

    /// Sync a single task file to database
    async fn sync_task_file(&self, path: &Path) -> crate::Result<()> {
        debug!("Syncing task file: {}", path.display());

        // Read and validate task file
        let task = read_task_file(path).await.map_err(|e| {
            crate::DbError::Core(e)
        })?;

        // Upsert to database
        self.db.upsert_task(&task).await?;

        debug!("Synced task: {} ({})", task.id, task.title);
        Ok(())
    }

    /// Sync a single dependency file to database
    async fn sync_dep_file(&self, path: &Path) -> crate::Result<()> {
        debug!("Syncing dep file: {}", path.display());

        // Read and validate dependency file
        let dep = read_dep_file(path).await.map_err(|e| {
            crate::DbError::Core(e)
        })?;

        // Upsert to database
        self.db.upsert_dep(&dep).await?;

        debug!("Synced dep: {} --{}--> {}", dep.from, dep.dep_type, dep.to);
        Ok(())
    }

    /// Get list of changed files from VCS since a commit
    ///
    /// Returns list of (path, change_type) tuples where change_type is:
    /// - "A" = added
    /// - "M" = modified
    /// - "D" = deleted
    async fn get_changed_files(&self, since_commit: &str) -> crate::Result<Vec<(PathBuf, String)>> {
        // Try jj first, fall back to git
        if self.is_jj_repo().await {
            self.get_jj_changed_files(since_commit).await
        } else {
            self.get_git_changed_files(since_commit).await
        }
    }

    /// Check if current directory is a jj repository
    async fn is_jj_repo(&self) -> bool {
        Path::new(".jj").exists()
    }

    /// Get changed files using jj
    async fn get_jj_changed_files(&self, since_commit: &str) -> crate::Result<Vec<(PathBuf, String)>> {
        let output = tokio::process::Command::new("jj")
            .args(&["diff", "--stat", "-r", since_commit])
            .output()
            .await?;

        if !output.status.success() {
            return Err(crate::DbError::Other(format!(
                "jj diff command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        self.parse_diff_output(&output.stdout).await
    }

    /// Get changed files using git
    async fn get_git_changed_files(&self, since_commit: &str) -> crate::Result<Vec<(PathBuf, String)>> {
        let output = tokio::process::Command::new("git")
            .args(&["diff", "--name-status", since_commit])
            .output()
            .await?;

        if !output.status.success() {
            return Err(crate::DbError::Other(format!(
                "git diff command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        self.parse_git_diff_output(&output.stdout).await
    }

    /// Parse git diff --name-status output
    async fn parse_git_diff_output(&self, output: &[u8]) -> crate::Result<Vec<(PathBuf, String)>> {
        let text = String::from_utf8_lossy(output);
        let mut files = Vec::new();

        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let change_type = parts[0].to_string();
                let path = PathBuf::from(parts[1]);
                files.push((path, change_type));
            }
        }

        Ok(files)
    }

    /// Parse jj diff output (simplified version)
    async fn parse_diff_output(&self, output: &[u8]) -> crate::Result<Vec<(PathBuf, String)>> {
        // For now, treat all jj changes as modifications
        // A more sophisticated implementation would parse jj's diff format
        let text = String::from_utf8_lossy(output);
        let mut files = Vec::new();

        for line in text.lines() {
            if line.contains(".json") {
                // Extract filename from diff output
                if let Some(path_str) = line.split_whitespace().next() {
                    let path = PathBuf::from(path_str);
                    if path.exists() {
                        files.push((path, "M".to_string()));
                    } else {
                        files.push((path, "D".to_string()));
                    }
                }
            }
        }

        Ok(files)
    }

    /// Handle deletion of a file
    async fn handle_deletion(&self, path: &Path) -> crate::Result<()> {
        if path.starts_with(&self.tasks_dir) {
            // Extract task ID from filename (task-123.json -> task-123)
            if let Some(stem) = path.file_stem() {
                let task_id = stem.to_string_lossy();
                self.db.delete_task(&task_id).await?;
                info!("Deleted task from database: {}", task_id);
            }
        } else if path.starts_with(&self.deps_dir) {
            // Extract dep info from filename (from--type--to.json)
            if let Some(stem) = path.file_stem() {
                let stem_str = stem.to_string_lossy();
                let parts: Vec<&str> = stem_str.split("--").collect();
                if parts.len() == 3 {
                    self.db.delete_dep(parts[0], parts[2], parts[1]).await?;
                    info!("Deleted dep from database: {} --{}--> {}", parts[0], parts[1], parts[2]);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bd_core::{DepFile, TaskFile};
    use chrono::Utc;
    use tempfile::TempDir;

    async fn setup_test_env() -> (TempDir, Database, PathBuf, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let tasks_dir = temp_dir.path().join("tasks");
        let deps_dir = temp_dir.path().join("deps");

        fs::create_dir_all(&tasks_dir).await.unwrap();
        fs::create_dir_all(&deps_dir).await.unwrap();

        let db = Database::open(&db_path).await.unwrap();
        db.init_schema().await.unwrap();

        (temp_dir, db, tasks_dir, deps_dir)
    }

    fn create_test_task(id: &str) -> TaskFile {
        TaskFile {
            id: id.to_string(),
            title: format!("Test task {}", id),
            description: Some("Test description".to_string()),
            task_type: "feature".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        }
    }

    fn create_test_dep(from: &str, to: &str, dep_type: &str) -> DepFile {
        DepFile {
            from: from.to_string(),
            to: to.to_string(),
            dep_type: dep_type.to_string(),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_sync_all_empty_directories() {
        let (_temp, db, tasks_dir, deps_dir) = setup_test_env().await;
        let mut sync_manager = SyncManager::new(db, &tasks_dir, &deps_dir);

        let stats = sync_manager.sync_all().await.unwrap();

        assert_eq!(stats.tasks_synced, 0);
        assert_eq!(stats.deps_synced, 0);
        assert!(!stats.has_errors());
    }

    #[tokio::test]
    async fn test_sync_all_with_tasks() {
        let (_temp, db, tasks_dir, deps_dir) = setup_test_env().await;

        // Write test task files
        let task1 = create_test_task("task-001");
        let task2 = create_test_task("task-002");

        write_task_file(&tasks_dir, &task1).await.unwrap();
        write_task_file(&tasks_dir, &task2).await.unwrap();

        // Sync
        let mut sync_manager = SyncManager::new(db, &tasks_dir, &deps_dir);
        let stats = sync_manager.sync_all().await.unwrap();

        assert_eq!(stats.tasks_synced, 2);
        assert_eq!(stats.tasks_failed, 0);
    }

    #[tokio::test]
    async fn test_sync_all_with_deps() {
        let (_temp, db, tasks_dir, deps_dir) = setup_test_env().await;

        // Write test task files first (needed for foreign key constraints)
        let task1 = create_test_task("task-001");
        let task2 = create_test_task("task-002");
        let task3 = create_test_task("task-003");

        write_task_file(&tasks_dir, &task1).await.unwrap();
        write_task_file(&tasks_dir, &task2).await.unwrap();
        write_task_file(&tasks_dir, &task3).await.unwrap();

        // Write test dep files
        let dep1 = create_test_dep("task-001", "task-002", "blocks");
        let dep2 = create_test_dep("task-002", "task-003", "depends-on");

        write_dep_file(&deps_dir, &dep1).await.unwrap();
        write_dep_file(&deps_dir, &dep2).await.unwrap();

        // Sync
        let mut sync_manager = SyncManager::new(db, &tasks_dir, &deps_dir);
        let stats = sync_manager.sync_all().await.unwrap();

        assert_eq!(stats.tasks_synced, 3);
        assert_eq!(stats.deps_synced, 2);
        assert_eq!(stats.deps_failed, 0);
    }

    #[tokio::test]
    async fn test_export_all() {
        let (_temp, db, tasks_dir, deps_dir) = setup_test_env().await;

        // Insert test tasks first (for foreign key constraints)
        let task1 = create_test_task("task-001");
        let task2 = create_test_task("task-002");
        db.upsert_task(&task1).await.unwrap();
        db.upsert_task(&task2).await.unwrap();

        // Insert test dependency
        let dep = create_test_dep("task-001", "task-002", "blocks");
        db.upsert_dep(&dep).await.unwrap();

        // Export
        let sync_manager = SyncManager::new(db, &tasks_dir, &deps_dir);
        let stats = sync_manager.export_all().await.unwrap();

        assert_eq!(stats.tasks_exported, 2);
        assert_eq!(stats.deps_exported, 1);
        assert!(!stats.has_errors());

        // Verify files exist
        assert!(tasks_dir.join("task-001.json").exists());
        assert!(tasks_dir.join("task-002.json").exists());
        assert!(deps_dir.join("task-001--blocks--task-002.json").exists());
    }

    #[tokio::test]
    async fn test_sync_stats_helpers() {
        let stats = SyncStats {
            tasks_synced: 5,
            tasks_failed: 2,
            deps_synced: 3,
            deps_failed: 1,
            deleted: 1,
        };

        assert!(stats.has_errors());
        assert_eq!(stats.total_synced(), 9); // 5 + 3 + 1
        assert_eq!(stats.total_failed(), 3); // 2 + 1
    }

    #[tokio::test]
    async fn test_export_stats_helpers() {
        let stats = ExportStats {
            tasks_exported: 10,
            tasks_failed: 1,
            deps_exported: 5,
            deps_failed: 0,
        };

        assert!(stats.has_errors());
        assert_eq!(stats.total_exported(), 15); // 10 + 5
        assert_eq!(stats.total_failed(), 1);
    }
}
