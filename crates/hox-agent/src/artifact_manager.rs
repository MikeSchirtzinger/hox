//! Artifact Manager - Capture and store validation artifacts
//!
//! Supports capturing screenshots, accessibility trees, and other validation artifacts
//! that prove UI changes and provide visual context for review.

use chrono::{DateTime, Utc};
use hox_core::{HoxError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

/// Types of validation artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    /// Browser screenshot
    Screenshot,
    /// Accessibility tree snapshot
    AccessibilityTree,
    /// Performance metrics log
    PerformanceLog,
    /// Custom artifact with type name
    Custom(String),
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactType::Screenshot => write!(f, "screenshot"),
            ArtifactType::AccessibilityTree => write!(f, "accessibility_tree"),
            ArtifactType::PerformanceLog => write!(f, "performance_log"),
            ArtifactType::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl ArtifactType {
    /// Get file extension for this artifact type
    pub fn extension(&self) -> &str {
        match self {
            ArtifactType::Screenshot => "png",
            ArtifactType::AccessibilityTree => "json",
            ArtifactType::PerformanceLog => "json",
            ArtifactType::Custom(_) => "bin",
        }
    }

    /// Get MIME type for this artifact
    pub fn mime_type(&self) -> &str {
        match self {
            ArtifactType::Screenshot => "image/png",
            ArtifactType::AccessibilityTree => "application/json",
            ArtifactType::PerformanceLog => "application/json",
            ArtifactType::Custom(_) => "application/octet-stream",
        }
    }
}

/// Metadata for a stored validation artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationArtifact {
    /// Type of artifact
    pub artifact_type: ArtifactType,
    /// Relative path from .hox/artifacts/
    pub path: PathBuf,
    /// MIME type
    pub mime_type: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// When created
    pub created_at: DateTime<Utc>,
    /// Human-readable description
    pub description: String,
}

impl ValidationArtifact {
    /// Get absolute path given a base directory
    pub fn absolute_path(&self, base_dir: &PathBuf) -> PathBuf {
        base_dir.join(&self.path)
    }
}

/// Manages artifact storage and retrieval
pub struct ArtifactManager {
    /// Base directory: .hox/artifacts
    base_dir: PathBuf,
}

impl ArtifactManager {
    /// Create new artifact manager
    ///
    /// # Arguments
    /// * `hox_dir` - Path to .hox directory
    pub fn new(hox_dir: PathBuf) -> Self {
        Self {
            base_dir: hox_dir.join("artifacts"),
        }
    }

    /// Store an artifact for a change
    ///
    /// # Arguments
    /// * `change_id` - JJ change ID this artifact is for
    /// * `artifact_type` - Type of artifact
    /// * `data` - Binary artifact data
    /// * `description` - Human-readable description
    pub async fn store_artifact(
        &self,
        change_id: &str,
        artifact_type: ArtifactType,
        data: &[u8],
        description: &str,
    ) -> Result<ValidationArtifact> {
        // Create directory: .hox/artifacts/{change-id}/
        let change_dir = self.base_dir.join(change_id);
        fs::create_dir_all(&change_dir).await.map_err(|e| {
            HoxError::Io(format!(
                "Failed to create artifact directory {}: {}",
                change_dir.display(),
                e
            ))
        })?;

        // Generate filename: {timestamp}-{type}.{ext}
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let filename = format!("{}-{}.{}", timestamp, artifact_type, artifact_type.extension());
        let file_path = change_dir.join(&filename);

        // Write data to file
        fs::write(&file_path, data).await.map_err(|e| {
            HoxError::Io(format!("Failed to write artifact {}: {}", file_path.display(), e))
        })?;

        let size_bytes = data.len() as u64;

        // Return metadata
        let relative_path = PathBuf::from(change_id).join(&filename);
        Ok(ValidationArtifact {
            artifact_type: artifact_type.clone(),
            path: relative_path,
            mime_type: artifact_type.mime_type().to_string(),
            size_bytes,
            created_at: Utc::now(),
            description: description.to_string(),
        })
    }

    /// List all artifacts for a change
    pub async fn list_artifacts(&self, change_id: &str) -> Result<Vec<ValidationArtifact>> {
        let change_dir = self.base_dir.join(change_id);

        // If directory doesn't exist, return empty list
        if !change_dir.exists() {
            return Ok(Vec::new());
        }

        let mut artifacts = Vec::new();
        let mut entries = fs::read_dir(&change_dir).await.map_err(|e| {
            HoxError::Io(format!(
                "Failed to read artifact directory {}: {}",
                change_dir.display(),
                e
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            HoxError::Io(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            if path.is_file() {
                // Parse metadata from file
                let metadata = fs::metadata(&path).await.map_err(|e| {
                    HoxError::Io(format!("Failed to read file metadata: {}", e))
                })?;

                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                // Infer artifact type from extension
                let artifact_type = if file_name.contains("screenshot") {
                    ArtifactType::Screenshot
                } else if file_name.contains("accessibility") {
                    ArtifactType::AccessibilityTree
                } else if file_name.contains("performance") {
                    ArtifactType::PerformanceLog
                } else {
                    ArtifactType::Custom(file_name.to_string())
                };

                let relative_path = PathBuf::from(change_id).join(file_name);

                artifacts.push(ValidationArtifact {
                    artifact_type: artifact_type.clone(),
                    path: relative_path,
                    mime_type: artifact_type.mime_type().to_string(),
                    size_bytes: metadata.len(),
                    created_at: metadata
                        .modified()
                        .ok()
                        .and_then(|t| DateTime::from_timestamp(
                            t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64,
                            0
                        ))
                        .unwrap_or_else(Utc::now),
                    description: format!("Artifact: {}", file_name),
                });
            }
        }

        Ok(artifacts)
    }

    /// Get base directory
    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }
}

/// Capture a screenshot using Chrome DevTools Protocol
///
/// # Arguments
/// * `url` - URL to navigate to
/// * `selector` - Optional CSS selector to screenshot (full page if None)
///
/// # Returns
/// PNG screenshot data
///
/// # Requirements
/// Requires Chrome running with `--remote-debugging-port=9222`
/// Requires `screenshots` feature to be enabled
#[cfg(feature = "screenshots")]
pub async fn capture_screenshot_cdp(
    url: &str,
    selector: Option<&str>,
) -> Result<Vec<u8>> {
    use headless_chrome::{Browser, LaunchOptions};

    // Launch browser (will connect to existing if available)
    let browser = Browser::new(LaunchOptions {
        headless: true,
        ..Default::default()
    })
    .map_err(|e| HoxError::Other(format!("Failed to launch browser: {}", e)))?;

    let tab = browser
        .new_tab()
        .map_err(|e| HoxError::Other(format!("Failed to create tab: {}", e)))?;

    // Navigate to URL
    tab.navigate_to(url)
        .map_err(|e| HoxError::Other(format!("Failed to navigate to {}: {}", url, e)))?;

    tab.wait_until_navigated()
        .map_err(|e| HoxError::Other(format!("Failed to wait for navigation: {}", e)))?;

    // Capture screenshot
    let screenshot_data = if let Some(sel) = selector {
        // Screenshot specific element
        let element = tab
            .wait_for_element(sel)
            .map_err(|e| HoxError::Other(format!("Failed to find element {}: {}", sel, e)))?;

        element
            .capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png)
            .map_err(|e| HoxError::Other(format!("Failed to capture element screenshot: {}", e)))?
    } else {
        // Screenshot full page
        tab.capture_screenshot(
            headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            None,
            true,
        )
        .map_err(|e| HoxError::Other(format!("Failed to capture page screenshot: {}", e)))?
    };

    Ok(screenshot_data)
}

/// Capture a screenshot (stub when screenshots feature is disabled)
#[cfg(not(feature = "screenshots"))]
pub async fn capture_screenshot_cdp(
    _url: &str,
    _selector: Option<&str>,
) -> Result<Vec<u8>> {
    Err(HoxError::Other(
        "Screenshot capture requires 'screenshots' feature to be enabled. \
         Build with --features screenshots".to_string()
    ))
}

/// Instructions for agents on how to use artifact capture
pub fn artifact_capture_instructions() -> &'static str {
    r#"## ARTIFACT CAPTURE

To capture visual validation artifacts (screenshots, accessibility trees), use XML blocks:

```
<capture_screenshot>
<url>http://localhost:3000</url>
<name>ui-save-button</name>
<selector>.save-button</selector>
</capture_screenshot>
```

Fields:
- `url` - URL to capture (required)
- `name` - Artifact name for tracking (required)
- `selector` - CSS selector for element screenshot (optional, full page if omitted)

The screenshot will be stored in `.hox/artifacts/{change-id}/` and linked to validation results.

Example - capturing UI state after a change:

<capture_screenshot>
<url>http://localhost:3000/dashboard</url>
<name>dashboard-updated-layout</name>
<selector>#main-content</selector>
</capture_screenshot>

Note: Requires Chrome running with `--remote-debugging-port=9222` or headless browser available.
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_artifact_type_display() {
        assert_eq!(ArtifactType::Screenshot.to_string(), "screenshot");
        assert_eq!(ArtifactType::AccessibilityTree.to_string(), "accessibility_tree");
        assert_eq!(ArtifactType::Custom("test".to_string()).to_string(), "test");
    }

    #[test]
    fn test_artifact_type_extensions() {
        assert_eq!(ArtifactType::Screenshot.extension(), "png");
        assert_eq!(ArtifactType::AccessibilityTree.extension(), "json");
        assert_eq!(ArtifactType::PerformanceLog.extension(), "json");
    }

    #[test]
    fn test_artifact_type_mime_types() {
        assert_eq!(ArtifactType::Screenshot.mime_type(), "image/png");
        assert_eq!(ArtifactType::AccessibilityTree.mime_type(), "application/json");
    }

    #[tokio::test]
    async fn test_artifact_manager_store() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let manager = ArtifactManager::new(hox_dir.clone());

        let test_data = b"test screenshot data";
        let artifact = manager
            .store_artifact(
                "test-change-123",
                ArtifactType::Screenshot,
                test_data,
                "Test screenshot",
            )
            .await
            .unwrap();

        assert_eq!(artifact.size_bytes, test_data.len() as u64);
        assert_eq!(artifact.mime_type, "image/png");
        assert_eq!(artifact.description, "Test screenshot");

        // Verify file was written
        let abs_path = artifact.absolute_path(&manager.base_dir);
        assert!(abs_path.exists());
        let content = fs::read(&abs_path).await.unwrap();
        assert_eq!(content, test_data);
    }

    #[tokio::test]
    async fn test_artifact_manager_list() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let manager = ArtifactManager::new(hox_dir);

        // Store multiple artifacts
        manager
            .store_artifact(
                "test-change-456",
                ArtifactType::Screenshot,
                b"screenshot1",
                "First screenshot",
            )
            .await
            .unwrap();

        manager
            .store_artifact(
                "test-change-456",
                ArtifactType::AccessibilityTree,
                b"{\"tree\": \"data\"}",
                "Accessibility tree",
            )
            .await
            .unwrap();

        // List artifacts
        let artifacts = manager.list_artifacts("test-change-456").await.unwrap();
        assert_eq!(artifacts.len(), 2);
    }

    #[tokio::test]
    async fn test_artifact_manager_list_empty() {
        let temp_dir = TempDir::new().unwrap();
        let hox_dir = temp_dir.path().to_path_buf();
        let manager = ArtifactManager::new(hox_dir);

        // List artifacts for non-existent change
        let artifacts = manager.list_artifacts("nonexistent").await.unwrap();
        assert_eq!(artifacts.len(), 0);
    }
}
