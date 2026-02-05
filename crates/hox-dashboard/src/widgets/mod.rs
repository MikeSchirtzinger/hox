//! Dashboard widgets module
//!
//! This module provides Ratatui-based widgets for the Hox dashboard.
//! Each widget is responsible for rendering a specific aspect of the dashboard.

use ratatui::style::Color;

mod agent_graph;
mod agent_table;
mod event_log;
mod metrics_panel;

pub use agent_graph::AgentGraphWidget;
pub use agent_table::AgentTableWidget;
pub use event_log::EventLogWidget;
pub use metrics_panel::MetricsPanelWidget;

/// Convert color name string to ratatui Color.
///
/// Shared utility used by multiple widgets to convert status color names
/// (from `AgentStatus::color_name()`) to ratatui `Color` values.
pub fn status_color_from_name(color_name: &str) -> Color {
    match color_name {
        "gray" => Color::DarkGray,
        "yellow" => Color::Yellow,
        "green" => Color::Green,
        "red" => Color::Red,
        "magenta" => Color::Magenta,
        // Unknown color names default to white for safe fallback
        _ => Color::White,
    }
}
