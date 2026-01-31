//! Browser automation error types - re-exports unified HoxError from hox-core
//!
//! All browser errors use the unified HoxError type with the Browser variant:
//! - Browser(String) - for browser-specific errors (launch, navigation, CDP, screenshots, etc.)
//! - IoError(std::io::Error) - for IO errors
//!
//! Error messages should be descriptive and include context about the operation that failed.

pub use hox_core::{HoxError, Result};

// For backward compatibility, type alias to HoxError
pub type BrowserError = HoxError;
