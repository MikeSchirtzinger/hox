use thiserror::Error;

/// Errors that can occur in the dashboard
#[derive(Debug, Error)]
pub enum DashboardError {
    /// Terminal initialization failed
    #[error("Failed to initialize terminal: {0}")]
    TerminalInit(String),

    /// JJ oplog reading failed
    #[error("Failed to read JJ oplog: {0}")]
    JjOplog(String),

    /// Metrics unavailable
    #[error("Metrics unavailable: {0}")]
    Metrics(String),

    /// Rendering failed
    #[error("Render error: {0}")]
    Render(String),

    /// Event handling error
    #[error("Event handling error: {0}")]
    Event(String),

    /// IO errors propagated from std::io
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for dashboard operations
pub type Result<T> = std::result::Result<T, DashboardError>;
