//! Main run loop for the dashboard application
//!
//! Handles terminal initialization, event loop, and cleanup.

use crate::{
    app::App,
    event::{self, is_next_tab_event, is_prev_tab_event, is_quit_event, is_refresh_event, AppEvent},
    terminal, ui, DashboardConfig, Result,
};
use std::time::Duration;

/// Main entry point for running the dashboard
pub async fn run(config: DashboardConfig) -> Result<()> {
    // Initialize terminal
    let mut terminal = terminal::init()?;

    // Create terminal guard for cleanup on panic
    let _guard = terminal::TerminalGuard::new();

    // Create application state
    let mut app = App::new(config);

    // Initial refresh to populate data
    if let Err(e) = app.refresh().await {
        eprintln!("Warning: Initial refresh failed: {}", e);
        // Continue anyway - empty state is valid
    }

    // Main event loop
    loop {
        // Draw current state
        terminal.draw(|frame| ui::draw(frame, &app))?;

        // Check if auto-refresh is needed
        if app.should_refresh() {
            if let Err(e) = app.refresh().await {
                eprintln!("Warning: Auto-refresh failed: {}", e);
                // Continue - don't crash on refresh errors
            }
        }

        // Poll for events with a short timeout
        let timeout = Duration::from_millis(100);
        match event::poll_event(timeout)? {
            Some(AppEvent::Key(key)) => {
                if is_quit_event(key) {
                    break;
                } else if is_refresh_event(key) {
                    if let Err(e) = app.refresh().await {
                        eprintln!("Warning: Manual refresh failed: {}", e);
                    }
                } else if is_next_tab_event(key) {
                    app.next_tab();
                } else if is_prev_tab_event(key) {
                    app.prev_tab();
                }
            }
            Some(AppEvent::Resize(_, _)) => {
                // Terminal was resized, will redraw on next iteration
            }
            Some(AppEvent::Tick) | None => {
                // Just a tick, continue
            }
        }

        // Check application quit flag
        if app.should_quit {
            break;
        }
    }

    // Restore terminal state
    terminal::restore()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = DashboardConfig {
            refresh_ms: 1000,
            max_oplog_entries: 100,
            local_time: true,
            metrics_path: None,
        };
        assert_eq!(config.refresh_ms, 1000);
        assert_eq!(config.max_oplog_entries, 100);
    }
}
