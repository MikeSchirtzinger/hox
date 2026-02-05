//! File Executor - Parses and executes file operations from agent output
//!
//! Agents output file operations as XML blocks:
//! - `<write_to_file><path>...</path><content>...</content></write_to_file>`
//!
//! This module parses these blocks and executes them safely.

use hox_core::{HoxError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Protected file patterns that should never be overwritten
const PROTECTED_FILES: &[&str] = &[".git", ".env", "Cargo.lock", ".secrets", ".gitignore"];

/// A file operation parsed from agent output
#[derive(Debug, Clone)]
pub enum FileOperation {
    /// Write content to a file
    WriteToFile { path: String, content: String },
    /// Capture a screenshot
    CaptureScreenshot {
        url: String,
        name: String,
        selector: Option<String>,
    },
}

/// Result of executing file operations from agent output
#[derive(Debug, Default)]
pub struct ExecutionResult {
    /// Files that were created
    pub files_created: Vec<String>,
    /// Files that were modified
    pub files_modified: Vec<String>,
    /// Screenshots captured
    pub screenshots_captured: Vec<String>,
    /// Errors encountered during execution
    pub errors: Vec<String>,
}

impl ExecutionResult {
    /// Generate a summary string
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.files_created.is_empty() {
            parts.push(format!("{} created", self.files_created.len()));
        }
        if !self.files_modified.is_empty() {
            parts.push(format!("{} modified", self.files_modified.len()));
        }
        if !self.screenshots_captured.is_empty() {
            parts.push(format!("{} screenshots", self.screenshots_captured.len()));
        }
        if !self.errors.is_empty() {
            parts.push(format!("{} errors", self.errors.len()));
        }

        if parts.is_empty() {
            "no file operations".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Check if any changes were made
    pub fn has_changes(&self) -> bool {
        !self.files_created.is_empty()
            || !self.files_modified.is_empty()
            || !self.screenshots_captured.is_empty()
    }

    /// Check if there were any errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Parse and execute all file operations in agent output
pub fn execute_file_operations(output: &str) -> ExecutionResult {
    let mut result = ExecutionResult::default();

    for op in parse_operations(output) {
        match op {
            FileOperation::WriteToFile { path, content } => match execute_write(&path, &content) {
                Ok(created) => {
                    if created {
                        result.files_created.push(path);
                    } else {
                        result.files_modified.push(path);
                    }
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Failed to write {}: {}", path, e));
                }
            },
            FileOperation::CaptureScreenshot {
                url,
                name,
                selector,
            } => {
                // For now, just log that screenshot was requested
                // Actual capture requires async runtime and browser
                tracing::info!(
                    "Screenshot requested: {} (url={}, selector={:?})",
                    name,
                    url,
                    selector
                );
                result.screenshots_captured.push(name.clone());
                result.errors.push(format!(
                    "Screenshot capture '{}' requires async runtime - not yet implemented in sync context",
                    name
                ));
            }
        }
    }

    result
}

/// Parse all file operations from text
fn parse_operations(text: &str) -> Vec<FileOperation> {
    let mut operations = Vec::new();

    // Parse write operations
    operations.extend(parse_write_blocks(text));

    // Parse screenshot operations
    operations.extend(parse_screenshot_blocks(text));

    operations
}

/// Parse all <write_to_file> blocks from text
fn parse_write_blocks(text: &str) -> Vec<FileOperation> {
    let mut operations = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<write_to_file>") {
        let block_start = start + "<write_to_file>".len();

        if let Some(end) = remaining[block_start..].find("</write_to_file>") {
            let block_content = &remaining[block_start..block_start + end];

            if let Some(op) = parse_single_write_block(block_content) {
                operations.push(op);
            }

            remaining = &remaining[block_start + end + "</write_to_file>".len()..];
        } else {
            break;
        }
    }

    operations
}

/// Parse all <capture_screenshot> blocks from text
fn parse_screenshot_blocks(text: &str) -> Vec<FileOperation> {
    let mut operations = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<capture_screenshot>") {
        let block_start = start + "<capture_screenshot>".len();

        if let Some(end) = remaining[block_start..].find("</capture_screenshot>") {
            let block_content = &remaining[block_start..block_start + end];

            if let Some(op) = parse_single_screenshot_block(block_content) {
                operations.push(op);
            }

            remaining = &remaining[block_start + end + "</capture_screenshot>".len()..];
        } else {
            break;
        }
    }

    operations
}

/// Parse a single write block content to extract path and content
fn parse_single_write_block(block: &str) -> Option<FileOperation> {
    let path = extract_tag_content(block, "path")?;
    let content = extract_tag_content(block, "content")?;

    Some(FileOperation::WriteToFile {
        path: path.trim().to_string(),
        content,
    })
}

/// Parse a single screenshot block content
fn parse_single_screenshot_block(block: &str) -> Option<FileOperation> {
    let url = extract_tag_content(block, "url")?;
    let name = extract_tag_content(block, "name")?;
    let selector = extract_tag_content(block, "selector");

    Some(FileOperation::CaptureScreenshot {
        url: url.trim().to_string(),
        name: name.trim().to_string(),
        selector: selector.map(|s| s.trim().to_string()),
    })
}

/// Extract content between <tag> and </tag>
fn extract_tag_content(text: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);

    let start = text.find(&open_tag)?;
    let content_start = start + open_tag.len();
    let end = text[content_start..].find(&close_tag)?;

    Some(text[content_start..content_start + end].to_string())
}

/// Validate that a path is safe to write to
pub fn validate_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);

    // Reject absolute paths
    if path.is_absolute() {
        return Err(HoxError::PathValidation(format!(
            "Absolute paths not allowed: {}",
            path.display()
        )));
    }

    // Check for path traversal
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(HoxError::PathValidation(format!(
                "Path traversal not allowed: {}",
                path.display()
            )));
        }
    }

    // Check protected files
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        for protected in PROTECTED_FILES {
            if name == *protected || path.starts_with(protected) {
                return Err(HoxError::PathValidation(format!(
                    "Cannot write to protected file: {}",
                    name
                )));
            }
        }
    }

    Ok(path.to_path_buf())
}

/// Execute a file write operation
/// Returns Ok(true) if file was created, Ok(false) if modified
fn execute_write(path: &str, content: &str) -> Result<bool> {
    let path = validate_path(path)?;
    let created = !path.exists();

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| {
                HoxError::Io(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
            tracing::info!("Created directory: {}", parent.display());
        }
    }

    // Write the file
    fs::write(&path, content)
        .map_err(|e| HoxError::Io(format!("Failed to write file {}: {}", path.display(), e)))?;

    if created {
        tracing::info!("Created file: {}", path.display());
    } else {
        tracing::info!("Modified file: {}", path.display());
    }

    Ok(created)
}

/// Instructions for agents on how to use file operations
pub fn file_operation_instructions() -> &'static str {
    r#"## FILE OPERATIONS

To create or modify files, use XML blocks in your output:

```
<write_to_file>
<path>relative/path/to/file.rs</path>
<content>
// Your file content here
fn example() {
    println!("Hello");
}
</content>
</write_to_file>
```

To capture screenshots for visual validation:

```
<capture_screenshot>
<url>http://localhost:3000</url>
<name>ui-save-button</name>
<selector>.save-button</selector>
</capture_screenshot>
```

IMPORTANT:
- Use relative paths from project root
- Parent directories are created automatically
- Include COMPLETE file content (not patches)
- You can write multiple files in one response
- Files are written AFTER your response completes
- Screenshots require browser with CDP enabled

Example - creating a new module:

<write_to_file>
<path>src/new_module.rs</path>
<content>
//! New module documentation

pub fn new_function() -> String {
    "implemented".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_function() {
        assert_eq!(new_function(), "implemented");
    }
}
</content>
</write_to_file>

Example - capturing UI state:

<capture_screenshot>
<url>http://localhost:3000/dashboard</url>
<name>dashboard-layout</name>
<selector>#main-content</selector>
</capture_screenshot>
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Shared mutex to prevent concurrent directory changes
    static TEST_DIR_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_parse_single_write_block() {
        let output = r#"
Some text before

<write_to_file>
<path>src/test.rs</path>
<content>
fn hello() {
    println!("world");
}
</content>
</write_to_file>

Some text after
"#;

        let ops = parse_write_blocks(output);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            FileOperation::WriteToFile { path, content } => {
                assert_eq!(path, "src/test.rs");
                assert!(content.contains("fn hello()"));
            }
            _ => panic!("Expected WriteToFile operation"),
        }
    }

    #[test]
    fn test_parse_multiple_write_blocks() {
        let output = r#"
<write_to_file>
<path>file1.rs</path>
<content>content1</content>
</write_to_file>

<write_to_file>
<path>file2.rs</path>
<content>content2</content>
</write_to_file>
"#;

        let ops = parse_write_blocks(output);
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            FileOperation::WriteToFile { path, .. } => assert_eq!(path, "file1.rs"),
            _ => panic!("Expected WriteToFile operation"),
        }
        match &ops[1] {
            FileOperation::WriteToFile { path, .. } => assert_eq!(path, "file2.rs"),
            _ => panic!("Expected WriteToFile operation"),
        }
    }

    #[test]
    fn test_parse_screenshot_block() {
        let output = r#"
<capture_screenshot>
<url>http://localhost:3000</url>
<name>test-screenshot</name>
<selector>.my-element</selector>
</capture_screenshot>
"#;

        let ops = parse_screenshot_blocks(output);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            FileOperation::CaptureScreenshot {
                url,
                name,
                selector,
            } => {
                assert_eq!(url, "http://localhost:3000");
                assert_eq!(name, "test-screenshot");
                assert_eq!(selector.as_deref(), Some(".my-element"));
            }
            _ => panic!("Expected CaptureScreenshot operation"),
        }
    }

    #[test]
    fn test_parse_screenshot_without_selector() {
        let output = r#"
<capture_screenshot>
<url>http://example.com</url>
<name>full-page</name>
</capture_screenshot>
"#;

        let ops = parse_screenshot_blocks(output);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            FileOperation::CaptureScreenshot {
                url,
                name,
                selector,
            } => {
                assert_eq!(url, "http://example.com");
                assert_eq!(name, "full-page");
                assert!(selector.is_none());
            }
            _ => panic!("Expected CaptureScreenshot operation"),
        }
    }

    #[test]
    fn test_execute_write_creates_file() {
        let _guard = TEST_DIR_LOCK.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let result = execute_write("test.txt", "test content");
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = created

        let file_path = temp_dir.path().join("test.txt");
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "test content");

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_execute_write_creates_parent_dirs() {
        let _guard = TEST_DIR_LOCK.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let result = execute_write("a/b/c/test.txt", "nested content");
        assert!(result.is_ok());

        let file_path = temp_dir.path().join("a/b/c/test.txt");
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "nested content");

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_execute_file_operations() {
        let _guard = TEST_DIR_LOCK.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let output = r#"
<write_to_file>
<path>file1.rs</path>
<content>content1</content>
</write_to_file>

<write_to_file>
<path>sub/file2.rs</path>
<content>content2</content>
</write_to_file>
"#;

        let result = execute_file_operations(output);

        assert_eq!(result.files_created.len(), 2);
        assert!(result.errors.is_empty());

        let file1 = temp_dir.path().join("file1.rs");
        let file2 = temp_dir.path().join("sub/file2.rs");
        assert!(file1.exists());
        assert!(file2.exists());

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_execution_result_summary() {
        let mut result = ExecutionResult::default();
        assert_eq!(result.summary(), "no file operations");

        result.files_created.push("a.rs".to_string());
        result.files_created.push("b.rs".to_string());
        assert_eq!(result.summary(), "2 created");

        result.files_modified.push("c.rs".to_string());
        assert_eq!(result.summary(), "2 created, 1 modified");
    }

    #[test]
    fn test_validate_path_absolute() {
        let result = validate_path("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_traversal() {
        let result = validate_path("../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_protected() {
        let result = validate_path(".git/config");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_valid() {
        let result = validate_path("src/main.rs");
        assert!(result.is_ok());
    }
}
