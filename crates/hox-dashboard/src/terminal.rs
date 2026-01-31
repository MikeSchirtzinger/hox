//! Terminal setup and teardown utilities
//!
//! Handles entering/exiting raw mode and alternate screen.

use crate::Result;
use hox_core::HoxError;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io::{self, Stdout};

/// Terminal type for the dashboard
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal for TUI rendering
pub fn init() -> Result<Tui> {
    // Enter raw mode to capture key events
    enable_raw_mode().map_err(|e| {
        HoxError::Dashboard(format!("Failed to enable raw mode: {}", e))
    })?;

    // Enter alternate screen to preserve terminal content
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| {
        HoxError::Dashboard(format!("Failed to enter alternate screen: {}", e))
    })?;

    // Create terminal with crossterm backend
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).map_err(|e| {
        HoxError::Dashboard(format!("Failed to create terminal: {}", e))
    })?;

    Ok(terminal)
}

/// Restore the terminal to its original state
pub fn restore() -> Result<()> {
    // Leave alternate screen
    execute!(io::stdout(), LeaveAlternateScreen).map_err(|e| {
        HoxError::Dashboard(format!("Failed to leave alternate screen: {}", e))
    })?;

    // Disable raw mode
    disable_raw_mode().map_err(|e| {
        HoxError::Dashboard(format!("Failed to disable raw mode: {}", e))
    })?;

    Ok(())
}

/// RAII guard for terminal state
///
/// Automatically restores terminal on drop, useful for panic handling.
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best effort restore - ignore errors in destructor
        let _ = restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests are disabled by default since they affect terminal state
    // Run with: cargo test --features terminal-tests

    #[test]
    #[ignore]
    fn test_init_restore() {
        let terminal = init().expect("Failed to init terminal");
        assert!(terminal.size().is_ok());
        restore().expect("Failed to restore terminal");
    }

    #[test]
    fn test_terminal_guard_creates() {
        let _guard = TerminalGuard::new();
        // Guard drops here, restoring terminal
    }
}
