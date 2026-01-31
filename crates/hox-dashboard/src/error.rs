//! Dashboard error types - re-exports unified HoxError from hox-core
//!
//! All dashboard errors use the unified HoxError type with appropriate variants:
//! - Dashboard(String) - for terminal, rendering, and event handling errors
//! - JjOplog(String) - for JJ operation log errors
//! - Metrics(String) - for metrics errors
//! - IoError(std::io::Error) - for IO errors

pub use hox_core::{HoxError, Result};

// For backward compatibility, type alias to HoxError
pub type DashboardError = HoxError;
