//! Visual verification helpers for UI validation

use crate::browser::BrowserSession;
use crate::error::Result;
use crate::screenshot::capture_element;
use hox_agent::{ArtifactManager, ValidationArtifact};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Result of a visual element check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualCheck {
    /// Whether the element exists in the DOM
    pub element_exists: bool,
    /// Text content of the element (if found)
    pub text_content: Option<String>,
    /// Screenshot artifact (if captured)
    pub screenshot: Option<ValidationArtifact>,
    /// Additional attributes captured
    pub attributes: Vec<ElementAttribute>,
}

/// HTML element attribute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementAttribute {
    pub name: String,
    pub value: String,
}

impl VisualCheck {
    /// Create a check result for non-existent element
    pub fn not_found() -> Self {
        Self {
            element_exists: false,
            text_content: None,
            screenshot: None,
            attributes: Vec::new(),
        }
    }

    /// Check if verification passed (element exists)
    pub fn passed(&self) -> bool {
        self.element_exists
    }

    /// Check if verification failed (element missing)
    pub fn failed(&self) -> bool {
        !self.element_exists
    }
}

/// Verify an element exists and capture its state
///
/// # Arguments
/// * `session` - Active browser session
/// * `selector` - CSS selector for the element
/// * `change_id` - JJ change ID for artifact storage
/// * `artifact_manager` - Optional artifact manager for screenshot capture
///
/// # Returns
/// Visual check result with element state and optional screenshot
///
/// # Example
/// ```no_run
/// use hox_browser::browser::BrowserSession;
/// use hox_browser::verification::verify_element;
/// use hox_agent::artifact_manager::ArtifactManager;
/// use std::path::PathBuf;
///
/// #[tokio::main]
/// async fn main() {
///     let session = BrowserSession::launch().await.unwrap();
///     session.navigate("https://example.com").await.unwrap();
///
///     let manager = ArtifactManager::new(PathBuf::from(".hox"));
///     let check = verify_element(
///         &session,
///         ".save-button",
///         "change-123",
///         Some(&manager)
///     ).await.unwrap();
///
///     assert!(check.element_exists);
///     println!("Button text: {:?}", check.text_content);
/// }
/// ```
pub async fn verify_element(
    session: &BrowserSession,
    selector: &str,
    change_id: &str,
    artifact_manager: Option<&ArtifactManager>,
) -> Result<VisualCheck> {
    info!("Verifying element: {}", selector);

    // Check if element exists
    let exists = session.element_exists(selector).await;

    if !exists {
        debug!("Element not found: {}", selector);
        return Ok(VisualCheck::not_found());
    }

    // Get text content
    let text_content = session.get_text_content(selector).await.ok();

    // Get common attributes
    let attributes = get_element_attributes(session, selector).await?;

    // Capture screenshot if artifact manager provided
    let screenshot: Option<ValidationArtifact> = if let Some(manager) = artifact_manager {
        debug!("Capturing screenshot of element: {}", selector);
        let artifact = capture_element(session, manager, change_id, selector, selector)
            .await
            .ok();
        artifact
    } else {
        None
    };

    Ok(VisualCheck {
        element_exists: true,
        text_content,
        screenshot,
        attributes,
    })
}

/// Verify multiple elements in a single check
///
/// # Arguments
/// * `session` - Active browser session
/// * `selectors` - List of CSS selectors to verify
/// * `change_id` - JJ change ID for artifact storage
/// * `artifact_manager` - Optional artifact manager for screenshot capture
pub async fn verify_elements(
    session: &BrowserSession,
    selectors: &[&str],
    change_id: &str,
    artifact_manager: Option<&ArtifactManager>,
) -> Result<Vec<(String, VisualCheck)>> {
    let mut results = Vec::new();

    for selector in selectors {
        let check = verify_element(session, selector, change_id, artifact_manager).await?;
        results.push((selector.to_string(), check));
    }

    Ok(results)
}

/// Verify text content matches expected value
///
/// # Arguments
/// * `session` - Active browser session
/// * `selector` - CSS selector for the element
/// * `expected_text` - Expected text content
pub async fn verify_text(
    session: &BrowserSession,
    selector: &str,
    expected_text: &str,
) -> Result<bool> {
    debug!(
        "Verifying text in {}: expected '{}'",
        selector, expected_text
    );

    let actual_text = session.get_text_content(selector).await?;
    let matches = actual_text.trim() == expected_text.trim();

    if matches {
        info!("Text verification passed for {}", selector);
    } else {
        info!(
            "Text verification failed for {}: expected '{}', got '{}'",
            selector, expected_text, actual_text
        );
    }

    Ok(matches)
}

/// Verify element has expected attribute value
///
/// # Arguments
/// * `session` - Active browser session
/// * `selector` - CSS selector for the element
/// * `attribute` - Attribute name to check
/// * `expected_value` - Expected attribute value
pub async fn verify_attribute(
    session: &BrowserSession,
    selector: &str,
    attribute: &str,
    expected_value: &str,
) -> Result<bool> {
    debug!(
        "Verifying attribute {}={} on {}",
        attribute, expected_value, selector
    );

    let script = format!(
        "document.querySelector('{}')?.getAttribute('{}')",
        selector, attribute
    );

    let result = session.evaluate_script(&script).await?;
    let actual_value = result.as_str().unwrap_or("");

    let matches = actual_value == expected_value;

    if matches {
        info!("Attribute verification passed for {}", selector);
    } else {
        info!(
            "Attribute verification failed for {}: expected '{}', got '{}'",
            selector, expected_value, actual_value
        );
    }

    Ok(matches)
}

/// Get common attributes from an element
async fn get_element_attributes(
    session: &BrowserSession,
    selector: &str,
) -> Result<Vec<ElementAttribute>> {
    let script = format!(
        r#"
        const el = document.querySelector('{}');
        if (!el) {{ return []; }}
        const attrs = ['id', 'class', 'type', 'name', 'value', 'href', 'src', 'aria-label'];
        attrs
            .map(name => ({{ name, value: el.getAttribute(name) }}))
            .filter(a => a.value !== null);
        "#,
        selector
    );

    let result = session.evaluate_script(&script).await?;

    let attributes: Vec<ElementAttribute> = serde_json::from_value(result).unwrap_or_default();

    Ok(attributes)
}

/// Verify page loaded successfully
///
/// Checks for common error indicators and page readiness
pub async fn verify_page_loaded(session: &BrowserSession) -> Result<bool> {
    debug!("Verifying page loaded successfully");

    // Check document ready state
    let ready_state = session
        .evaluate_script("document.readyState")
        .await?
        .as_str()
        .unwrap_or("")
        .to_string();

    if ready_state != "complete" && ready_state != "interactive" {
        return Ok(false);
    }

    // Check for common error pages
    let title = session.get_title().await?;
    let error_indicators = ["404", "Error", "Not Found", "403", "500"];

    if error_indicators.iter().any(|&e| title.contains(e)) {
        return Ok(false);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_check_not_found() {
        let check = VisualCheck::not_found();
        assert!(!check.element_exists);
        assert!(check.failed());
        assert!(!check.passed());
        assert!(check.text_content.is_none());
        assert!(check.screenshot.is_none());
    }

    #[test]
    fn test_element_attribute() {
        let attr = ElementAttribute {
            name: "id".to_string(),
            value: "main".to_string(),
        };
        assert_eq!(attr.name, "id");
        assert_eq!(attr.value, "main");
    }
}
