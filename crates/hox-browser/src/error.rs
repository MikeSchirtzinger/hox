//! Browser automation error types

use thiserror::Error;

/// Errors that can occur during browser automation
#[derive(Debug, Error)]
pub enum BrowserError {
    /// Failed to launch browser
    #[error("Failed to launch browser: {0}")]
    LaunchFailed(String),

    /// Failed to connect to existing browser
    #[error("Failed to connect to browser: {0}")]
    ConnectionFailed(String),

    /// Failed to navigate to URL
    #[error("Failed to navigate to {url}: {reason}")]
    NavigationFailed { url: String, reason: String },

    /// Failed to capture screenshot
    #[error("Failed to capture screenshot: {0}")]
    ScreenshotFailed(String),

    /// Element not found
    #[error("Element not found: {selector}")]
    ElementNotFound { selector: String },

    /// Operation timed out
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// JavaScript evaluation error
    #[error("JavaScript evaluation failed: {0}")]
    JavaScriptError(String),

    /// CDP protocol error
    #[error("Chrome DevTools Protocol error: {0}")]
    ProtocolError(String),

    /// Invalid argument or state
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(String),
}

/// Result type for browser operations
pub type Result<T> = std::result::Result<T, BrowserError>;

impl From<std::io::Error> for BrowserError {
    fn from(err: std::io::Error) -> Self {
        BrowserError::Io(err.to_string())
    }
}
