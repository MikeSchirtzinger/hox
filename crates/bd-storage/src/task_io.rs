//! TaskFile I/O operations for reading and writing task JSON files.
//!
//! This module provides async file operations for managing task files in the
//! tasks/ directory, compatible with the jj-turso file-based storage architecture.

use bd_core::{Result, TaskFile};
use std::path::Path;
use tokio::fs;
use tracing::{debug, warn};

/// Read and parse a single task file from disk.
///
/// # Arguments
/// * `path` - Full path to the task JSON file
///
/// # Returns
/// * `Ok(TaskFile)` - Successfully parsed and validated task
/// * `Err(Error)` - File I/O error, JSON parse error, or validation error
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use bd_storage::task_io::read_task_file;
///
/// #[tokio::main]
/// async fn main() -> bd_core::Result<()> {
///     let task = read_task_file(Path::new("tasks/task-123.json")).await?;
///     println!("Loaded task: {}", task.title);
///     Ok(())
/// }
/// ```
pub async fn read_task_file(path: &Path) -> Result<TaskFile> {
    debug!("Reading task file: {}", path.display());

    let data = fs::read(path).await?;
    let task: TaskFile = serde_json::from_slice(&data)?;

    // Validate before returning
    task.validate()?;

    Ok(task)
}

/// Write a TaskFile to disk as pretty-printed JSON.
///
/// The file is written to `{tasks_dir}/{id}.json` with validation.
/// Creates the tasks directory if it doesn't exist.
///
/// # Arguments
/// * `tasks_dir` - Directory to write task files (e.g., "tasks/")
/// * `task` - TaskFile to write
///
/// # Returns
/// * `Ok(())` - Successfully written
/// * `Err(Error)` - Validation error, directory creation error, or write error
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use bd_storage::task_io::write_task_file;
/// use bd_core::TaskFile;
/// use chrono::Utc;
///
/// #[tokio::main]
/// async fn main() -> bd_core::Result<()> {
///     let task = TaskFile {
///         id: "task-123".to_string(),
///         title: "Example task".to_string(),
///         description: None,
///         task_type: "task".to_string(),
///         status: "open".to_string(),
///         priority: 2,
///         assigned_agent: None,
///         tags: vec![],
///         created_at: Utc::now(),
///         updated_at: Utc::now(),
///         due_at: None,
///         defer_until: None,
///     };
///
///     write_task_file(Path::new("tasks"), &task).await?;
///     Ok(())
/// }
/// ```
pub async fn write_task_file(tasks_dir: &Path, task: &TaskFile) -> Result<()> {
    // Validate before writing
    task.validate()?;

    // Ensure tasks directory exists
    fs::create_dir_all(tasks_dir).await?;

    // Serialize to pretty JSON
    let data = serde_json::to_vec_pretty(task)?;

    // Write to file
    let path = tasks_dir.join(task.filename());
    debug!("Writing task file: {}", path.display());
    fs::write(&path, data).await?;

    Ok(())
}

/// Read all task files from a directory.
///
/// Reads all `.json` files in the tasks directory, parsing and validating each.
/// Invalid files are skipped with a warning (logged via tracing).
///
/// # Arguments
/// * `tasks_dir` - Directory containing task JSON files
///
/// # Returns
/// * `Ok(Vec<TaskFile>)` - All valid tasks found
/// * `Err(Error)` - Directory read error (returns empty vec if dir doesn't exist)
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use bd_storage::task_io::read_all_task_files;
///
/// #[tokio::main]
/// async fn main() -> bd_core::Result<()> {
///     let tasks = read_all_task_files(Path::new("tasks")).await?;
///     println!("Found {} tasks", tasks.len());
///     Ok(())
/// }
/// ```
pub async fn read_all_task_files(tasks_dir: &Path) -> Result<Vec<TaskFile>> {
    debug!("Reading all task files from: {}", tasks_dir.display());

    // Handle non-existent directory gracefully
    let mut entries = match fs::read_dir(tasks_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("Tasks directory does not exist, returning empty list");
            return Ok(Vec::new());
        }
        Err(e) => return Err(e.into()),
    };

    let mut tasks = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // Skip directories
        if !path.is_file() {
            continue;
        }

        // Skip non-JSON files
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        // Try to read and parse the file
        match read_task_file(&path).await {
            Ok(task) => tasks.push(task),
            Err(e) => {
                // Log warning but continue processing other files
                warn!(
                    "Skipping invalid task file {}: {}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    e
                );
            }
        }
    }

    debug!("Successfully read {} task files", tasks.len());
    Ok(tasks)
}

/// Delete a task file from disk.
///
/// Removes the task file `{tasks_dir}/{id}.json`.
/// Does not return an error if the file doesn't exist.
///
/// # Arguments
/// * `tasks_dir` - Directory containing task files
/// * `id` - Task ID to delete
///
/// # Returns
/// * `Ok(())` - Successfully deleted or file didn't exist
/// * `Err(Error)` - File system error
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use bd_storage::task_io::delete_task_file;
///
/// #[tokio::main]
/// async fn main() -> bd_core::Result<()> {
///     delete_task_file(Path::new("tasks"), "task-123").await?;
///     println!("Task deleted");
///     Ok(())
/// }
/// ```
pub async fn delete_task_file(tasks_dir: &Path, id: &str) -> Result<()> {
    let filename = format!("{}.json", id);
    let path = tasks_dir.join(filename);

    debug!("Deleting task file: {}", path.display());

    // Remove file, ignoring NotFound errors
    match fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("Task file did not exist, nothing to delete");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn create_test_task(id: &str) -> TaskFile {
        TaskFile {
            id: id.to_string(),
            title: "Test Task".to_string(),
            description: Some("Test description".to_string()),
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec!["test".to_string()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        }
    }

    #[tokio::test]
    async fn test_write_and_read_task_file() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        let task = create_test_task("test-123");

        // Write task
        write_task_file(tasks_dir, &task).await.unwrap();

        // Read it back
        let path = tasks_dir.join("test-123.json");
        let read_task = read_task_file(&path).await.unwrap();

        assert_eq!(read_task.id, task.id);
        assert_eq!(read_task.title, task.title);
        assert_eq!(read_task.status, task.status);
    }

    #[tokio::test]
    async fn test_read_all_task_files() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        // Write multiple tasks
        for i in 1..=3 {
            let task = create_test_task(&format!("test-{}", i));
            write_task_file(tasks_dir, &task).await.unwrap();
        }

        // Read all
        let tasks = read_all_task_files(tasks_dir).await.unwrap();
        assert_eq!(tasks.len(), 3);
    }

    #[tokio::test]
    async fn test_read_all_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path().join("nonexistent");

        // Should return empty vec, not error
        let tasks = read_all_task_files(&tasks_dir).await.unwrap();
        assert_eq!(tasks.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_task_file() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        let task = create_test_task("test-delete");
        write_task_file(tasks_dir, &task).await.unwrap();

        // Delete it
        delete_task_file(tasks_dir, "test-delete").await.unwrap();

        // Verify it's gone
        let path = tasks_dir.join("test-delete.json");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_task() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        // Should not error
        delete_task_file(tasks_dir, "nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_validation_on_read() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        // Write invalid JSON manually
        let invalid_json = r#"{"id": "", "title": ""}"#;
        let path = tasks_dir.join("invalid.json");
        fs::write(&path, invalid_json).await.unwrap();

        // Should fail validation
        let result = read_task_file(&path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validation_on_write() {
        let temp_dir = TempDir::new().unwrap();
        let tasks_dir = temp_dir.path();

        let mut task = create_test_task("test-invalid");
        task.priority = 10; // Invalid priority

        // Should fail validation
        let result = write_task_file(tasks_dir, &task).await;
        assert!(result.is_err());
    }
}
