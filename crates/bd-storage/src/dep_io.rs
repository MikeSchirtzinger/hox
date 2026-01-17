//! Dependency file I/O operations.
//!
//! This module provides async functions for reading, writing, and managing
//! dependency files stored in deps/*.json format.
//!
//! Filename convention: {from}--{type}--{to}.json

use bd_core::{DepFile, Error, Result};
use std::path::Path;
use tokio::fs;
use tracing::{debug, warn};

/// Read and parse a single dependency file.
///
/// Returns an error if the file cannot be read or parsed, or if validation fails.
pub async fn read_dep_file(path: &Path) -> Result<DepFile> {
    debug!("Reading dep file: {}", path.display());

    let data = fs::read_to_string(path)
        .await
        .map_err(|e| Error::Io(e))?;

    let dep: DepFile = serde_json::from_str(&data)
        .map_err(|e| Error::Json(e))?;

    // Validate the parsed dependency
    dep.validate()?;

    Ok(dep)
}

/// Write a dependency file with validation.
///
/// The file will be written to: {deps_dir}/{from}--{type}--{to}.json
///
/// Returns an error if validation fails or the file cannot be written.
pub async fn write_dep_file(deps_dir: &Path, dep: &DepFile) -> Result<()> {
    // Validate before writing
    dep.validate()?;

    let filename = dep.to_filename();
    let path = deps_dir.join(&filename);

    debug!("Writing dep file: {}", path.display());

    // Serialize with pretty formatting
    let data = serde_json::to_string_pretty(dep)
        .map_err(|e| Error::Json(e))?;

    // Ensure the deps directory exists
    fs::create_dir_all(deps_dir)
        .await
        .map_err(|e| Error::Io(e))?;

    // Write the file
    fs::write(&path, data)
        .await
        .map_err(|e| Error::Io(e))?;

    Ok(())
}

/// Read all dependency files from a directory.
///
/// Invalid files are skipped with a warning, not an error.
/// Returns an empty vector if the directory doesn't exist.
pub async fn read_all_dep_files(deps_dir: &Path) -> Result<Vec<DepFile>> {
    debug!("Reading all dep files from: {}", deps_dir.display());

    // If directory doesn't exist, return empty vector
    if !deps_dir.exists() {
        debug!("Deps directory does not exist: {}", deps_dir.display());
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(deps_dir)
        .await
        .map_err(|e| Error::Io(e))?;

    let mut deps = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| Error::Io(e))? {
        let path = entry.path();

        // Skip directories and non-JSON files
        if !path.is_file() {
            continue;
        }

        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy(),
            None => continue,
        };

        if !filename.ends_with(".json") {
            continue;
        }

        // Try to read the file, but skip invalid ones
        match read_dep_file(&path).await {
            Ok(dep) => deps.push(dep),
            Err(e) => {
                warn!("Skipping invalid dep file {}: {}", filename, e);
                continue;
            }
        }
    }

    debug!("Read {} dependency files", deps.len());
    Ok(deps)
}

/// Delete a dependency file.
///
/// The filename is constructed from: {from}--{dep_type}--{to}.json
///
/// Returns Ok(()) even if the file doesn't exist (idempotent).
pub async fn delete_dep_file(deps_dir: &Path, from: &str, dep_type: &str, to: &str) -> Result<()> {
    let filename = format!("{}--{}--{}.json", from, dep_type, to);
    let path = deps_dir.join(&filename);

    debug!("Deleting dep file: {}", path.display());

    match fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Already deleted, no error
            debug!("Dep file already deleted: {}", path.display());
            Ok(())
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Find all dependencies involving a specific task ID.
///
/// Returns dependencies where the task is either 'from' or 'to'.
/// Invalid files are skipped with a warning.
pub async fn find_deps_for_task(deps_dir: &Path, task_id: &str) -> Result<Vec<DepFile>> {
    debug!("Finding deps for task: {}", task_id);

    // If directory doesn't exist, return empty vector
    if !deps_dir.exists() {
        debug!("Deps directory does not exist: {}", deps_dir.display());
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(deps_dir)
        .await
        .map_err(|e| Error::Io(e))?;

    let mut deps = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|e| Error::Io(e))? {
        let path = entry.path();

        // Skip directories and non-JSON files
        if !path.is_file() {
            continue;
        }

        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        if !filename.ends_with(".json") {
            continue;
        }

        // Parse filename to check if it involves this task
        match parse_dep_filename(&filename) {
            Ok((from, _dep_type, to)) => {
                // Include if task is either from or to
                if from == task_id || to == task_id {
                    match read_dep_file(&path).await {
                        Ok(dep) => deps.push(dep),
                        Err(e) => {
                            warn!("Skipping invalid dep file {}: {}", filename, e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Skipping file with invalid filename {}: {}", filename, e);
                continue;
            }
        }
    }

    debug!("Found {} dependencies for task {}", deps.len(), task_id);
    Ok(deps)
}

/// Parse a dependency filename into its components.
///
/// Expected format: {from}--{type}--{to}.json
///
/// Returns (from, dep_type, to)
fn parse_dep_filename(filename: &str) -> Result<(String, String, String)> {
    // Remove .json extension
    let name = filename.strip_suffix(".json")
        .ok_or_else(|| Error::Parse(format!("filename must end with .json: {}", filename)))?;

    // Split on --
    let parts: Vec<&str> = name.split("--").collect();

    if parts.len() != 3 {
        return Err(Error::Parse(format!(
            "invalid filename format: expected {{from}}--{{type}}--{{to}}.json, got {}",
            filename
        )));
    }

    let from = parts[0];
    let dep_type = parts[1];
    let to = parts[2];

    if from.is_empty() || dep_type.is_empty() || to.is_empty() {
        return Err(Error::Parse(format!(
            "invalid filename: from, type, and to cannot be empty: {}",
            filename
        )));
    }

    Ok((from.to_string(), dep_type.to_string(), to.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    #[test]
    fn test_parse_dep_filename() {
        // Valid filename
        let result = parse_dep_filename("task1--blocks--task2.json");
        assert!(result.is_ok());
        let (from, dep_type, to) = result.unwrap();
        assert_eq!(from, "task1");
        assert_eq!(dep_type, "blocks");
        assert_eq!(to, "task2");

        // Missing extension
        let result = parse_dep_filename("task1--blocks--task2");
        assert!(result.is_err());

        // Invalid format (too few parts)
        let result = parse_dep_filename("task1--blocks.json");
        assert!(result.is_err());

        // Invalid format (too many parts)
        let result = parse_dep_filename("task1--blocks--task2--extra.json");
        assert!(result.is_err());

        // Empty component
        let result = parse_dep_filename("task1----task2.json");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_write_and_read_dep_file() {
        let temp_dir = TempDir::new().unwrap();
        let deps_dir = temp_dir.path();

        let dep = DepFile {
            from: "task1".to_string(),
            to: "task2".to_string(),
            dep_type: "blocks".to_string(),
            created_at: Utc::now(),
        };

        // Write the file
        write_dep_file(deps_dir, &dep).await.unwrap();

        // Verify file exists
        let path = deps_dir.join("task1--blocks--task2.json");
        assert!(path.exists());

        // Read it back
        let read_dep = read_dep_file(&path).await.unwrap();
        assert_eq!(read_dep.from, dep.from);
        assert_eq!(read_dep.to, dep.to);
        assert_eq!(read_dep.dep_type, dep.dep_type);
    }

    #[tokio::test]
    async fn test_read_all_dep_files() {
        let temp_dir = TempDir::new().unwrap();
        let deps_dir = temp_dir.path();

        // Create multiple dep files
        let dep1 = DepFile {
            from: "task1".to_string(),
            to: "task2".to_string(),
            dep_type: "blocks".to_string(),
            created_at: Utc::now(),
        };

        let dep2 = DepFile {
            from: "task2".to_string(),
            to: "task3".to_string(),
            dep_type: "depends_on".to_string(),
            created_at: Utc::now(),
        };

        write_dep_file(deps_dir, &dep1).await.unwrap();
        write_dep_file(deps_dir, &dep2).await.unwrap();

        // Read all
        let deps = read_all_dep_files(deps_dir).await.unwrap();
        assert_eq!(deps.len(), 2);
    }

    #[tokio::test]
    async fn test_find_deps_for_task() {
        let temp_dir = TempDir::new().unwrap();
        let deps_dir = temp_dir.path();

        // Create deps involving task2
        let dep1 = DepFile {
            from: "task1".to_string(),
            to: "task2".to_string(),
            dep_type: "blocks".to_string(),
            created_at: Utc::now(),
        };

        let dep2 = DepFile {
            from: "task2".to_string(),
            to: "task3".to_string(),
            dep_type: "depends_on".to_string(),
            created_at: Utc::now(),
        };

        let dep3 = DepFile {
            from: "task4".to_string(),
            to: "task5".to_string(),
            dep_type: "blocks".to_string(),
            created_at: Utc::now(),
        };

        write_dep_file(deps_dir, &dep1).await.unwrap();
        write_dep_file(deps_dir, &dep2).await.unwrap();
        write_dep_file(deps_dir, &dep3).await.unwrap();

        // Find deps for task2
        let deps = find_deps_for_task(deps_dir, "task2").await.unwrap();
        assert_eq!(deps.len(), 2); // task2 is in dep1 (to) and dep2 (from)

        // Find deps for task5
        let deps = find_deps_for_task(deps_dir, "task5").await.unwrap();
        assert_eq!(deps.len(), 1); // task5 is only in dep3 (to)

        // Find deps for non-existent task
        let deps = find_deps_for_task(deps_dir, "task999").await.unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_dep_file() {
        let temp_dir = TempDir::new().unwrap();
        let deps_dir = temp_dir.path();

        let dep = DepFile {
            from: "task1".to_string(),
            to: "task2".to_string(),
            dep_type: "blocks".to_string(),
            created_at: Utc::now(),
        };

        // Write the file
        write_dep_file(deps_dir, &dep).await.unwrap();

        let path = deps_dir.join("task1--blocks--task2.json");
        assert!(path.exists());

        // Delete it
        delete_dep_file(deps_dir, "task1", "blocks", "task2").await.unwrap();
        assert!(!path.exists());

        // Delete again (should be idempotent)
        delete_dep_file(deps_dir, "task1", "blocks", "task2").await.unwrap();
    }

    #[tokio::test]
    async fn test_read_all_from_nonexistent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let deps_dir = temp_dir.path().join("nonexistent");

        // Should return empty vector, not error
        let deps = read_all_dep_files(&deps_dir).await.unwrap();
        assert_eq!(deps.len(), 0);
    }
}
