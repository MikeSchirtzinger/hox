//! Dashboard widgets module
//!
//! This module provides Ratatui-based widgets for the Hox dashboard.
//! Each widget is responsible for rendering a specific aspect of the dashboard.

mod agent_graph;
mod metrics_panel;
mod event_log;
mod agent_table;

pub use agent_graph::AgentGraphWidget;
pub use metrics_panel::MetricsPanelWidget;
pub use event_log::EventLogWidget;
pub use agent_table::AgentTableWidget;
