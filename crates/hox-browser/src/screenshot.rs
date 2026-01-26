//! Screenshot capture using Chrome DevTools Protocol

use crate::browser::BrowserSession;
use crate::error::{BrowserError, Result};
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use hox_agent::{ArtifactManager, ArtifactType, ValidationArtifact};
use tracing::{debug, info};

/// Screenshot capture options
#[derive(Debug, Clone)]
pub struct ScreenshotOptions {
    /// CSS selector for element screenshot (None for full page)
    pub selector: Option<String>,
    /// Capture full page (scrolls and stitches if needed)
    pub full_page: bool,
    /// Image quality for JPEG (1-100, ignored for PNG)
    pub quality: Option<u8>,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self {
            selector: None,
            full_page: true,
            quality: None,
        }
    }
}

impl ScreenshotOptions {
    /// Create options for full-page screenshot
    pub fn full_page() -> Self {
        Self {
            selector: None,
            full_page: true,
            quality: None,
        }
    }

    /// Create options for element screenshot
    pub fn element(selector: &str) -> Self {
        Self {
            selector: Some(selector.to_string()),
            full_page: false,
            quality: None,
        }
    }
}

/// Capture a screenshot and store it as an artifact
///
/// # Arguments
/// * `session` - Active browser session
/// * `artifact_manager` - Artifact manager for storage
/// * `change_id` - JJ change ID for artifact storage
/// * `name` - Descriptive name for the screenshot
/// * `options` - Screenshot capture options
///
/// # Returns
/// Metadata for the stored artifact
///
/// # Example
/// ```no_run
/// use hox_browser::browser::BrowserSession;
/// use hox_browser::screenshot::{capture_screenshot, ScreenshotOptions};
/// use hox_agent::artifact_manager::ArtifactManager;
/// use std::path::PathBuf;
///
/// #[tokio::main]
/// async fn main() {
///     let session = BrowserSession::launch().await.unwrap();
///     session.navigate("https://example.com").await.unwrap();
///
///     let manager = ArtifactManager::new(PathBuf::from(".hox"));
///     let artifact = capture_screenshot(
///         &session,
///         &manager,
///         "change-123",
///         "homepage",
///         ScreenshotOptions::full_page()
///     ).await.unwrap();
///
///     println!("Screenshot saved: {:?}", artifact.path);
/// }
/// ```
pub async fn capture_screenshot(
    session: &BrowserSession,
    artifact_manager: &ArtifactManager,
    change_id: &str,
    name: &str,
    options: ScreenshotOptions,
) -> Result<ValidationArtifact> {
    info!(
        "Capturing screenshot '{}' for change {}",
        name, change_id
    );

    // Capture screenshot data
    let screenshot_data = if let Some(ref selector) = options.selector {
        debug!("Capturing element screenshot: {}", selector);
        capture_element_screenshot(session, selector).await?
    } else {
        debug!("Capturing full page screenshot");
        capture_full_page_screenshot(session, options.full_page).await?
    };

    // Store artifact
    let description = if let Some(ref selector) = options.selector {
        format!("Screenshot of element '{}' ({})", selector, name)
    } else {
        format!("Full page screenshot ({})", name)
    };

    let artifact: ValidationArtifact = artifact_manager
        .store_artifact(change_id, ArtifactType::Screenshot, &screenshot_data, &description)
        .await
        .map_err(|e| BrowserError::ScreenshotFailed(format!("Failed to store artifact: {}", e)))?;

    info!(
        "Screenshot stored: {} ({} bytes)",
        artifact.path.display(),
        artifact.size_bytes
    );

    Ok(artifact)
}

/// Capture full page screenshot
async fn capture_full_page_screenshot(session: &BrowserSession, full_page: bool) -> Result<Vec<u8>> {
    let tab = session.tab();

    let screenshot_data = tab
        .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, full_page)
        .map_err(|e| BrowserError::ScreenshotFailed(format!("CDP capture failed: {}", e)))?;

    Ok(screenshot_data)
}

/// Capture screenshot of a specific element
async fn capture_element_screenshot(session: &BrowserSession, selector: &str) -> Result<Vec<u8>> {
    let tab = session.tab();

    // Wait for element to be available
    let element = tab
        .wait_for_element(selector)
        .map_err(|_e| BrowserError::ElementNotFound {
            selector: selector.to_string(),
        })?;

    let screenshot_data = element
        .capture_screenshot(CaptureScreenshotFormatOption::Png)
        .map_err(|e| BrowserError::ScreenshotFailed(format!("Element capture failed: {}", e)))?;

    Ok(screenshot_data)
}

/// Convenience function for full-page screenshot
///
/// # Arguments
/// * `session` - Active browser session
/// * `artifact_manager` - Artifact manager for storage
/// * `change_id` - JJ change ID for artifact storage
/// * `name` - Descriptive name for the screenshot
pub async fn capture_full_page(
    session: &BrowserSession,
    artifact_manager: &ArtifactManager,
    change_id: &str,
    name: &str,
) -> Result<ValidationArtifact> {
    capture_screenshot(
        session,
        artifact_manager,
        change_id,
        name,
        ScreenshotOptions::full_page(),
    )
    .await
}

/// Convenience function for element screenshot
///
/// # Arguments
/// * `session` - Active browser session
/// * `artifact_manager` - Artifact manager for storage
/// * `change_id` - JJ change ID for artifact storage
/// * `name` - Descriptive name for the screenshot
/// * `selector` - CSS selector for the element
pub async fn capture_element(
    session: &BrowserSession,
    artifact_manager: &ArtifactManager,
    change_id: &str,
    name: &str,
    selector: &str,
) -> Result<ValidationArtifact> {
    capture_screenshot(
        session,
        artifact_manager,
        change_id,
        name,
        ScreenshotOptions::element(selector),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screenshot_options_default() {
        let options = ScreenshotOptions::default();
        assert!(options.selector.is_none());
        assert!(options.full_page);
        assert!(options.quality.is_none());
    }

    #[test]
    fn test_screenshot_options_full_page() {
        let options = ScreenshotOptions::full_page();
        assert!(options.selector.is_none());
        assert!(options.full_page);
    }

    #[test]
    fn test_screenshot_options_element() {
        let options = ScreenshotOptions::element("#main");
        assert_eq!(options.selector.as_deref(), Some("#main"));
        assert!(!options.full_page);
    }
}
