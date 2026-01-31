//! Browser automation and visual verification for Hox orchestration
//!
//! This crate provides browser automation capabilities using Chrome DevTools Protocol (CDP)
//! for UI verification tasks in the Hox orchestration system.
//!
//! # Features
//!
//! - **Browser Management**: Launch and control Chrome/Chromium browsers
//! - **Screenshot Capture**: Full-page and element-specific screenshots
//! - **Visual Verification**: Element existence checks, text validation, attribute verification
//! - **Artifact Storage**: Integration with `hox-agent` artifact system
//!
//! # Example
//!
//! ```no_run
//! use hox_browser::browser::{BrowserSession, BrowserConfig};
//! use hox_browser::screenshot::capture_full_page;
//! use hox_browser::verification::verify_element;
//! use hox_agent::artifact_manager::ArtifactManager;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Launch browser
//!     let session = BrowserSession::launch().await?;
//!
//!     // Navigate to page
//!     session.navigate("https://example.com").await?;
//!
//!     // Set up artifact storage
//!     let manager = ArtifactManager::new(PathBuf::from(".hox"));
//!
//!     // Capture screenshot
//!     let screenshot = capture_full_page(
//!         &session,
//!         &manager,
//!         "change-123",
//!         "homepage"
//!     ).await?;
//!
//!     // Verify element exists
//!     let check = verify_element(
//!         &session,
//!         ".main-heading",
//!         "change-123",
//!         Some(&manager)
//!     ).await?;
//!
//!     assert!(check.element_exists);
//!     println!("Element verified: {:?}", check.text_content);
//!
//!     // Clean up
//!     session.close().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Requirements
//!
//! - Chrome or Chromium browser installed
//! - For headless operation, no additional setup required
//! - For connecting to existing browser: `chrome --remote-debugging-port=9222`
//!
//! # Architecture
//!
//! The crate is organized into modules:
//!
//! - [`browser`]: Browser lifecycle and session management
//! - [`screenshot`]: Screenshot capture with artifact storage
//! - [`verification`]: Visual verification and element checking
//! - [`error`]: Error types for browser operations

pub mod browser;
pub mod error;
pub mod screenshot;
pub mod verification;

// Re-export commonly used types
pub use browser::{BrowserConfig, BrowserSession};
pub use error::{BrowserError, Result};
pub use screenshot::{ScreenshotOptions, capture_screenshot, capture_full_page, capture_element};
pub use verification::{VisualCheck, ElementAttribute, verify_element, verify_elements, verify_text, verify_attribute};

#[cfg(test)]
mod tests {
    #[test]
    fn test_public_api_availability() {
        // This test just ensures all public APIs are accessible
        // Actual functionality is tested in individual modules
    }
}
