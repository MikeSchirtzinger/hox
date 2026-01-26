//! Main application state and logic
//!
//! The `App` struct holds the dashboard state and handles refresh cycles.

use crate::{DashboardConfig, DashboardState, JjDataSource, Result};
use std::time::{Duration, Instant};

/// Main application state
pub struct App {
    /// Current dashboard state (agents, metrics, oplog)
    pub state: DashboardState,
    /// Configuration (refresh rate, limits, etc.)
    pub config: DashboardConfig,
    /// Data source for fetching JJ oplog and state
    pub data_source: JjDataSource,
    /// Signal to exit the application
    pub should_quit: bool,
    /// Last time state was refreshed
    pub last_refresh: Instant,
    /// Current tab selection
    pub selected_tab: TabSelection,
}

/// Tab selection for multi-panel views
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSelection {
    /// Overview: graph + metrics + agents table
    Overview,
    /// Agents: detailed agents table view
    Agents,
    /// Oplog: full JJ operation log
    Oplog,
}

impl App {
    /// Create a new application with given configuration
    pub fn new(config: DashboardConfig) -> Self {
        let data_source = JjDataSource::new(config.clone());
        Self {
            state: DashboardState::default(),
            config,
            data_source,
            should_quit: false,
            last_refresh: Instant::now(),
            selected_tab: TabSelection::Overview,
        }
    }

    /// Refresh dashboard state from JJ data source
    pub async fn refresh(&mut self) -> Result<()> {
        self.state = self.data_source.fetch_state().await?;
        self.last_refresh = Instant::now();
        Ok(())
    }

    /// Check if refresh interval has elapsed
    pub fn should_refresh(&self) -> bool {
        let elapsed = self.last_refresh.elapsed();
        elapsed >= Duration::from_millis(self.config.refresh_ms)
    }

    /// Move to next tab
    pub fn next_tab(&mut self) {
        self.selected_tab = match self.selected_tab {
            TabSelection::Overview => TabSelection::Agents,
            TabSelection::Agents => TabSelection::Oplog,
            TabSelection::Oplog => TabSelection::Overview,
        };
    }

    /// Move to previous tab
    pub fn prev_tab(&mut self) {
        self.selected_tab = match self.selected_tab {
            TabSelection::Oplog => TabSelection::Agents,
            TabSelection::Agents => TabSelection::Overview,
            TabSelection::Overview => TabSelection::Oplog,
        };
    }

    /// Get current tab name for display
    pub fn current_tab_name(&self) -> &str {
        match self.selected_tab {
            TabSelection::Overview => "Overview",
            TabSelection::Agents => "Agents",
            TabSelection::Oplog => "Oplog",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let config = DashboardConfig::default();
        let app = App::new(config);
        assert_eq!(app.selected_tab, TabSelection::Overview);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_tab_cycling() {
        let config = DashboardConfig::default();
        let mut app = App::new(config);

        assert_eq!(app.selected_tab, TabSelection::Overview);
        app.next_tab();
        assert_eq!(app.selected_tab, TabSelection::Agents);
        app.next_tab();
        assert_eq!(app.selected_tab, TabSelection::Oplog);
        app.next_tab();
        assert_eq!(app.selected_tab, TabSelection::Overview);

        app.prev_tab();
        assert_eq!(app.selected_tab, TabSelection::Oplog);
    }

    #[test]
    fn test_should_refresh() {
        let mut config = DashboardConfig::default();
        config.refresh_ms = 100; // 100ms refresh
        let app = App::new(config);

        // Just created, should not refresh yet
        assert!(!app.should_refresh());

        // Wait a bit and check again
        std::thread::sleep(Duration::from_millis(110));
        assert!(app.should_refresh());
    }
}
