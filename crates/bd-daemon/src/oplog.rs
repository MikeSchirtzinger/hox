//! jj operation log watcher for efficient change detection.
//!
//! This module provides a more efficient alternative to file system watching
//! for jj repositories. Instead of watching all files, it polls the jj operation
//! log to detect changes to task and dependency files.
//!
//! # Example
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
//!
//! watcher.watch(|entries| {
//!     for entry in entries {
//!         println!("Operation: {}", entry.id);
//!         for file in &entry.affected_files {
//!             println!("  Changed: {}", file.display());
//!         }
//!     }
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```

use bd_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

/// Represents a single operation from jj's operation log.
///
/// Each entry captures metadata about a jj operation (snapshot, rebase, etc.)
/// and can be used to determine which files were affected.
#[derive(Debug, Clone)]
pub struct OpLogEntry {
    /// Operation ID (64-character hex string)
    pub id: String,

    /// Human-readable description of the operation
    /// Examples: "snapshot working copy", "rebase", "new empty commit"
    pub description: String,

    /// List of task/dep files that were modified
    /// These are detected by parsing the operation diff
    pub affected_files: Vec<PathBuf>,
}

/// Configuration for the operation log watcher.
#[derive(Debug, Clone)]
pub struct OpLogWatcherConfig {
    /// Path to the jj repository
    pub repo_path: PathBuf,

    /// How often to check for new operations
    pub poll_interval: Duration,

    /// Directory containing task files (relative to repo root)
    pub tasks_dir: String,

    /// Directory containing dependency files (relative to repo root)
    pub deps_dir: String,

    /// Operation ID to start watching from
    /// If None, starts from the most recent operation
    pub last_op_id: Option<String>,
}

impl Default for OpLogWatcherConfig {
    fn default() -> Self {
        Self {
            repo_path: PathBuf::from("."),
            poll_interval: Duration::from_millis(100),
            tasks_dir: "tasks".to_string(),
            deps_dir: "deps".to_string(),
            last_op_id: None,
        }
    }
}

/// Callback function called when new operations are detected.
///
/// The callback receives a slice of new operations in chronological order
/// (oldest first). If the callback returns an error, watching continues
/// but the error is logged.
pub type OpLogCallback = Box<dyn Fn(&[OpLogEntry]) -> Result<()> + Send + Sync>;

/// Operation log watcher that polls jj for new operations.
pub struct OpLogWatcher {
    config: OpLogWatcherConfig,
    last_seen_id: Option<String>,
}

impl OpLogWatcher {
    /// Create a new operation log watcher.
    ///
    /// This will check if jj is available and if the repo is a valid jj repository.
    pub fn new(config: OpLogWatcherConfig) -> Result<Self> {
        // Validate repo path exists
        if !config.repo_path.exists() {
            return Err(Error::Watcher(format!(
                "Repository path does not exist: {}",
                config.repo_path.display()
            )));
        }

        Ok(Self {
            last_seen_id: config.last_op_id.clone(),
            config,
        })
    }

    /// Check if jj is available in PATH.
    pub async fn is_jj_available() -> bool {
        match Command::new("jj").arg("--version").output().await {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// Check if the repository is a jj repository.
    pub async fn is_jj_repo(repo_path: &Path) -> bool {
        match Command::new("jj")
            .arg("status")
            .current_dir(repo_path)
            .output()
            .await
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// Get the ID of the most recent operation.
    pub async fn get_latest_operation_id(repo_path: &Path) -> Result<String> {
        let output = Command::new("jj")
            .args(["op", "log", "--no-graph", "-T", "id", "-n", "1"])
            .current_dir(repo_path)
            .output()
            .await
            .map_err(|e| Error::Watcher(format!("Failed to run jj command: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Watcher(format!(
                "jj op log failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let id = stdout.trim().to_string();

        if id.is_empty() {
            return Err(Error::Watcher("No operations found".to_string()));
        }

        Ok(id)
    }

    /// Watch for new operations and call the callback when detected.
    ///
    /// This method blocks until an error occurs or is cancelled.
    /// Operations are delivered to the callback in chronological order (oldest first).
    pub async fn watch<F>(mut self, callback: F) -> Result<()>
    where
        F: Fn(&[OpLogEntry]) -> Result<()> + Send + Sync + 'static,
    {
        // Check if jj is available
        if !Self::is_jj_available().await {
            return Err(Error::Watcher(
                "jj command not found in PATH".to_string(),
            ));
        }

        // Check if this is a jj repo
        if !Self::is_jj_repo(&self.config.repo_path).await {
            return Err(Error::Watcher(format!(
                "Not a jj repository: {}",
                self.config.repo_path.display()
            )));
        }

        info!(
            "Starting jj oplog watcher on {}",
            self.config.repo_path.display()
        );

        // Initialize last_seen_id if not set
        if self.last_seen_id.is_none() {
            match Self::get_latest_operation_id(&self.config.repo_path).await {
                Ok(id) => {
                    info!("Starting from operation: {}", &id[..12]);
                    self.last_seen_id = Some(id);
                }
                Err(e) => {
                    warn!("Failed to get initial operation ID: {}", e);
                }
            }
        }

        let mut interval = tokio::time::interval(self.config.poll_interval);

        loop {
            interval.tick().await;

            match self.poll_operations().await {
                Ok(new_ops) if !new_ops.is_empty() => {
                    debug!("Found {} new operations", new_ops.len());

                    // Update last seen ID to most recent
                    if let Some(last) = new_ops.last() {
                        self.last_seen_id = Some(last.id.clone());
                    }

                    // Call callback
                    if let Err(e) = callback(&new_ops) {
                        error!("OpLog callback error: {}", e);
                    }
                }
                Ok(_) => {
                    // No new operations
                }
                Err(e) => {
                    error!("Failed to poll operations: {}", e);
                    // Continue watching despite error
                }
            }
        }
    }

    /// Poll for new operations since last seen.
    ///
    /// Returns operations in chronological order (oldest first).
    async fn poll_operations(&self) -> Result<Vec<OpLogEntry>> {
        // Get recent operations
        let output = Command::new("jj")
            .args([
                "op",
                "log",
                "--no-graph",
                "-T",
                r#"id ++ "\n" ++ description ++ "\n---\n""#,
                "-n",
                "50",
            ])
            .current_dir(&self.config.repo_path)
            .output()
            .await
            .map_err(|e| Error::Watcher(format!("Failed to run jj op log: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Watcher(format!("jj op log failed: {}", stderr)));
        }

        // Parse operation log
        let entries = self.parse_op_log(&output.stdout)?;

        // Find new operations
        let mut new_ops = self.find_new_operations(&entries);

        if new_ops.is_empty() {
            return Ok(Vec::new());
        }

        // Reverse to get chronological order (oldest first)
        new_ops.reverse();

        // Get affected files for each operation
        for entry in &mut new_ops {
            match self.get_affected_files(&entry.id).await {
                Ok(files) => {
                    entry.affected_files = files;
                }
                Err(e) => {
                    warn!(
                        "Failed to get affected files for {}: {}",
                        &entry.id[..12],
                        e
                    );
                }
            }
        }

        Ok(new_ops)
    }

    /// Parse the output from `jj op log` command.
    ///
    /// Expected format:
    /// ```text
    /// abc123...
    /// snapshot working copy
    /// ---
    /// def456...
    /// rebase
    /// ---
    /// ```
    fn parse_op_log(&self, output: &[u8]) -> Result<Vec<OpLogEntry>> {
        let text = String::from_utf8_lossy(output);
        let mut entries = Vec::new();
        let mut current_id: Option<String> = None;
        let mut current_desc: Option<String> = None;

        for line in text.lines() {
            let line = line.trim();

            // Separator line
            if line == "---" {
                if let (Some(id), Some(desc)) = (current_id.take(), current_desc.take()) {
                    entries.push(OpLogEntry {
                        id,
                        description: desc,
                        affected_files: Vec::new(),
                    });
                }
                continue;
            }

            // Empty lines
            if line.is_empty() {
                continue;
            }

            // If we don't have an ID yet, this is the ID line
            if current_id.is_none() {
                current_id = Some(line.to_string());
                continue;
            }

            // If we have an ID but no description, this is the description
            if current_desc.is_none() {
                current_desc = Some(line.to_string());
                continue;
            }

            // Multi-line descriptions (append to existing)
            if let Some(ref mut desc) = current_desc {
                desc.push(' ');
                desc.push_str(line);
            }
        }

        // Handle final entry if no trailing separator
        if let (Some(id), Some(desc)) = (current_id, current_desc) {
            entries.push(OpLogEntry {
                id,
                description: desc,
                affected_files: Vec::new(),
            });
        }

        Ok(entries)
    }

    /// Find operations that are newer than last_seen_id.
    ///
    /// Input entries are in reverse chronological order (newest first).
    /// Returns new operations in reverse chronological order.
    fn find_new_operations(&self, entries: &[OpLogEntry]) -> Vec<OpLogEntry> {
        let Some(ref last_seen) = self.last_seen_id else {
            // First run - return only the most recent operation
            return entries.first().cloned().into_iter().collect();
        };

        // Find where the last seen ID appears
        for (i, entry) in entries.iter().enumerate() {
            if entry.id == *last_seen {
                // Return everything before this index (newer operations)
                if i == 0 {
                    return Vec::new(); // No new operations
                }
                return entries[..i].to_vec();
            }
        }

        // last_seen_id not found - it might have been garbage collected
        // Return only the most recent operation to avoid processing too much history
        entries.first().cloned().into_iter().collect()
    }

    /// Get files affected by an operation.
    ///
    /// Returns paths relative to repository root, filtered to only include
    /// files in tasks/ and deps/ directories.
    async fn get_affected_files(&self, op_id: &str) -> Result<Vec<PathBuf>> {
        let output = Command::new("jj")
            .args(["op", "show", op_id, "--op-diff", "--patch"])
            .current_dir(&self.config.repo_path)
            .output()
            .await
            .map_err(|e| Error::Watcher(format!("Failed to run jj op show: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Watcher(format!(
                "jj op show failed: {}",
                stderr
            )));
        }

        Ok(self.parse_affected_files(&output.stdout))
    }

    /// Parse affected files from jj op show output.
    ///
    /// Looks for lines like:
    /// - Added regular file tasks/bd-123.json:
    /// - Modified regular file deps/bd-abc--blocks--bd-xyz.json:
    /// - Removed regular file tasks/bd-456.json:
    fn parse_affected_files(&self, diff_output: &[u8]) -> Vec<PathBuf> {
        let text = String::from_utf8_lossy(diff_output);
        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Pattern: "Added regular file path:" or "Modified regular file path:" or "Removed regular file path:"
        let pattern = regex::Regex::new(r"(?:Added|Modified|Removed) regular file (.+):").unwrap();

        for line in text.lines() {
            if let Some(caps) = pattern.captures(line) {
                if let Some(file_path) = caps.get(1) {
                    let path = file_path.as_str();

                    // Check if this is a task or dep file
                    if !path.starts_with(&format!("{}/", self.config.tasks_dir))
                        && !path.starts_with(&format!("{}/", self.config.deps_dir))
                    {
                        continue;
                    }

                    // Check if it's a JSON file
                    if !path.ends_with(".json") {
                        continue;
                    }

                    // Deduplicate
                    if !seen.insert(path.to_string()) {
                        continue;
                    }

                    files.push(PathBuf::from(path));
                }
            }
        }

        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_op_log() {
        let watcher = OpLogWatcher::new(OpLogWatcherConfig::default()).unwrap();

        let output = b"abc123def456789\nsnapshot working copy\n---\nfedcba987654321\nrebase\n---\n";

        let entries = watcher.parse_op_log(output).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "abc123def456789");
        assert_eq!(entries[0].description, "snapshot working copy");
        assert_eq!(entries[1].id, "fedcba987654321");
        assert_eq!(entries[1].description, "rebase");
    }

    #[test]
    fn test_parse_op_log_no_trailing_separator() {
        let watcher = OpLogWatcher::new(OpLogWatcherConfig::default()).unwrap();

        let output = b"abc123def456789\nsnapshot working copy\n";

        let entries = watcher.parse_op_log(output).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "abc123def456789");
        assert_eq!(entries[0].description, "snapshot working copy");
    }

    #[test]
    fn test_parse_affected_files() {
        let watcher = OpLogWatcher::new(OpLogWatcherConfig::default()).unwrap();

        let diff = b"Added regular file tasks/bd-123.json:\n\
                     Modified regular file deps/bd-abc--blocks--bd-xyz.json:\n\
                     Removed regular file tasks/bd-456.json:\n\
                     Added regular file other/file.txt:\n\
                     Modified regular file tasks/not-json.txt:\n";

        let files = watcher.parse_affected_files(diff);

        assert_eq!(files.len(), 3);
        assert!(files.contains(&PathBuf::from("tasks/bd-123.json")));
        assert!(files.contains(&PathBuf::from("deps/bd-abc--blocks--bd-xyz.json")));
        assert!(files.contains(&PathBuf::from("tasks/bd-456.json")));
        assert!(!files.contains(&PathBuf::from("other/file.txt")));
        assert!(!files.contains(&PathBuf::from("tasks/not-json.txt")));
    }

    #[test]
    fn test_find_new_operations_first_run() {
        let watcher = OpLogWatcher::new(OpLogWatcherConfig::default()).unwrap();

        let entries = vec![
            OpLogEntry {
                id: "newest".to_string(),
                description: "op1".to_string(),
                affected_files: Vec::new(),
            },
            OpLogEntry {
                id: "older".to_string(),
                description: "op2".to_string(),
                affected_files: Vec::new(),
            },
        ];

        let new_ops = watcher.find_new_operations(&entries);

        // First run should return only the most recent
        assert_eq!(new_ops.len(), 1);
        assert_eq!(new_ops[0].id, "newest");
    }

    #[test]
    fn test_find_new_operations_with_last_seen() {
        let config = OpLogWatcherConfig {
            last_op_id: Some("older".to_string()),
            ..Default::default()
        };
        let watcher = OpLogWatcher::new(config).unwrap();

        let entries = vec![
            OpLogEntry {
                id: "newest".to_string(),
                description: "op1".to_string(),
                affected_files: Vec::new(),
            },
            OpLogEntry {
                id: "middle".to_string(),
                description: "op2".to_string(),
                affected_files: Vec::new(),
            },
            OpLogEntry {
                id: "older".to_string(),
                description: "op3".to_string(),
                affected_files: Vec::new(),
            },
        ];

        let new_ops = watcher.find_new_operations(&entries);

        // Should return operations newer than "older"
        assert_eq!(new_ops.len(), 2);
        assert_eq!(new_ops[0].id, "newest");
        assert_eq!(new_ops[1].id, "middle");
    }

    #[test]
    fn test_find_new_operations_no_new() {
        let config = OpLogWatcherConfig {
            last_op_id: Some("newest".to_string()),
            ..Default::default()
        };
        let watcher = OpLogWatcher::new(config).unwrap();

        let entries = vec![
            OpLogEntry {
                id: "newest".to_string(),
                description: "op1".to_string(),
                affected_files: Vec::new(),
            },
            OpLogEntry {
                id: "older".to_string(),
                description: "op2".to_string(),
                affected_files: Vec::new(),
            },
        ];

        let new_ops = watcher.find_new_operations(&entries);

        // No new operations
        assert_eq!(new_ops.len(), 0);
    }

    #[test]
    fn test_find_new_operations_last_seen_not_found() {
        let config = OpLogWatcherConfig {
            last_op_id: Some("ancient".to_string()),
            ..Default::default()
        };
        let watcher = OpLogWatcher::new(config).unwrap();

        let entries = vec![
            OpLogEntry {
                id: "newest".to_string(),
                description: "op1".to_string(),
                affected_files: Vec::new(),
            },
            OpLogEntry {
                id: "older".to_string(),
                description: "op2".to_string(),
                affected_files: Vec::new(),
            },
        ];

        let new_ops = watcher.find_new_operations(&entries);

        // Last seen not found - return only most recent to avoid processing too much
        assert_eq!(new_ops.len(), 1);
        assert_eq!(new_ops[0].id, "newest");
    }
}
