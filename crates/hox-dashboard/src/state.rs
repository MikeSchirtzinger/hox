//! Shared dashboard state types
//!
//! These types define the contracts between data sources and UI widgets.
//! All parallel implementation agents MUST use these types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Overall dashboard state - the main data container
#[derive(Debug, Clone, Default)]
pub struct DashboardState {
    /// Current orchestration session info
    pub session: OrchestrationSession,
    /// Global metrics summary
    pub global_metrics: GlobalMetrics,
    /// Active agents with their progress
    pub agents: Vec<AgentNode>,
    /// Recent JJ operations
    pub oplog: Vec<JjOplogEntry>,
    /// Phase progress for the visual graph
    pub phases: Vec<PhaseProgress>,
    /// Last update timestamp
    pub last_updated: Option<DateTime<Utc>>,
}

/// Orchestration session metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestrationSession {
    /// Session identifier
    pub id: String,
    /// Current JJ bookmark
    pub bookmark: Option<String>,
    /// Session start time
    pub started_at: Option<DateTime<Utc>>,
    /// Total planned phases
    pub total_phases: usize,
    /// Current phase number
    pub current_phase: usize,
}

/// Global metrics for the header panel
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalMetrics {
    /// Total tool calls across all agents
    pub total_tool_calls: u32,
    /// Failed tool calls
    pub total_failures: u32,
    /// Total time in milliseconds
    pub total_time_ms: u64,
    /// Number of active agents
    pub active_agents: usize,
    /// Number of completed agents
    pub completed_agents: usize,
}

impl GlobalMetrics {
    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f32 {
        if self.total_tool_calls == 0 {
            return 100.0;
        }
        let successful = self.total_tool_calls.saturating_sub(self.total_failures);
        (successful as f32 / self.total_tool_calls as f32) * 100.0
    }

    /// Format total time as human-readable duration
    pub fn formatted_duration(&self) -> String {
        let seconds = self.total_time_ms / 1000;
        let minutes = seconds / 60;
        let remaining_seconds = seconds % 60;
        if minutes > 0 {
            format!("{}m {}s", minutes, remaining_seconds)
        } else {
            format!("{}s", seconds)
        }
    }
}

/// Agent node for the visual graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    /// Agent identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Which phase this agent belongs to
    pub phase: usize,
    /// Current status
    pub status: AgentStatus,
    /// Estimated progress (0.0 - 1.0)
    pub progress: f32,
    /// Change ID being worked on
    pub change_id: Option<String>,
    /// Tool calls made
    pub tool_calls: u32,
    /// Success rate (0.0 - 1.0)
    pub success_rate: f32,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Task description
    pub task: String,
}

impl AgentNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, phase: usize) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            phase,
            status: AgentStatus::Pending,
            progress: 0.0,
            change_id: None,
            tool_calls: 0,
            success_rate: 1.0,
            duration_ms: 0,
            task: String::new(),
        }
    }

    /// Progress bar characters for TUI display
    pub fn progress_bar(&self, width: usize) -> String {
        let filled = (self.progress * width as f32) as usize;
        let empty = width.saturating_sub(filled);
        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }

    /// Progress as percentage string
    pub fn progress_percent(&self) -> String {
        format!("{:.0}%", self.progress * 100.0)
    }
}

/// Agent execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// Not yet started
    Pending,
    /// Currently executing
    Running,
    /// Successfully completed
    Completed,
    /// Failed with error
    Failed,
    /// Blocked by dependencies
    Blocked,
}

impl AgentStatus {
    /// Status indicator character for TUI
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Pending => "○",
            Self::Running => "◉",
            Self::Completed => "✓",
            Self::Failed => "✗",
            Self::Blocked => "◌",
        }
    }

    /// Status color (for ratatui styling)
    pub fn color_name(&self) -> &'static str {
        match self {
            Self::Pending => "gray",
            Self::Running => "yellow",
            Self::Completed => "green",
            Self::Failed => "red",
            Self::Blocked => "magenta",
        }
    }
}

/// Parsed JJ operation log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JjOplogEntry {
    /// Operation ID
    pub id: String,
    /// Operation timestamp
    pub timestamp: DateTime<Utc>,
    /// Operation description
    pub description: String,
    /// Which agent performed this (if known)
    pub agent_id: Option<String>,
    /// Operation type (inferred)
    pub op_type: JjOpType,
    /// Tags/metadata
    pub tags: HashMap<String, String>,
}

impl JjOplogEntry {
    /// Format for display in event log
    pub fn formatted_time(&self) -> String {
        self.timestamp.format("%H:%M:%S").to_string()
    }
}

/// JJ operation types (inferred from description)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JjOpType {
    /// New change created
    New,
    /// Change description updated
    Describe,
    /// Changes squashed
    Squash,
    /// Bookmark created/moved
    Bookmark,
    /// Commit made
    Commit,
    /// Rebase operation
    Rebase,
    /// Workspace operation
    Workspace,
    /// Unknown/other
    Other,
}

impl JjOpType {
    /// Parse operation type from description
    pub fn from_description(desc: &str) -> Self {
        let lower = desc.to_lowercase();
        if lower.contains("new empty commit") || lower.contains("create commit") {
            Self::New
        } else if lower.contains("describe") || lower.contains("description") {
            Self::Describe
        } else if lower.contains("squash") {
            Self::Squash
        } else if lower.contains("bookmark") {
            Self::Bookmark
        } else if lower.contains("commit") {
            Self::Commit
        } else if lower.contains("rebase") {
            Self::Rebase
        } else if lower.contains("workspace") {
            Self::Workspace
        } else {
            Self::Other
        }
    }

    /// Icon for TUI display
    pub fn icon(&self) -> &'static str {
        match self {
            Self::New => "+",
            Self::Describe => "✎",
            Self::Squash => "⊕",
            Self::Bookmark => "⚑",
            Self::Commit => "●",
            Self::Rebase => "↻",
            Self::Workspace => "⬡",
            Self::Other => "·",
        }
    }
}

/// Phase progress for the orchestration graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseProgress {
    /// Phase number (0 = contracts, 1+ = execution phases)
    pub number: usize,
    /// Phase name
    pub name: String,
    /// Is this a blocking phase?
    pub blocking: bool,
    /// Phase status
    pub status: PhaseStatus,
    /// Overall progress (0.0 - 1.0)
    pub progress: f32,
    /// Agent IDs in this phase
    pub agent_ids: Vec<String>,
}

impl PhaseProgress {
    pub fn new(number: usize, name: impl Into<String>) -> Self {
        Self {
            number,
            name: name.into(),
            blocking: false,
            status: PhaseStatus::Pending,
            progress: 0.0,
            agent_ids: Vec::new(),
        }
    }

    /// Progress bar for TUI display
    pub fn progress_bar(&self, width: usize) -> String {
        let filled = (self.progress * width as f32) as usize;
        let empty = width.saturating_sub(filled);
        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }
}

/// Phase execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseStatus {
    Pending,
    Active,
    Completed,
    Failed,
}

/// Dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Refresh interval in milliseconds
    pub refresh_ms: u64,
    /// Maximum oplog entries to display
    pub max_oplog_entries: usize,
    /// Show timestamps in local time
    pub local_time: bool,
    /// Path to metrics file (if using file-based source)
    pub metrics_path: Option<String>,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_ms: 500,
            max_oplog_entries: 50,
            local_time: true,
            metrics_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_metrics_success_rate() {
        let metrics = GlobalMetrics {
            total_tool_calls: 100,
            total_failures: 5,
            ..Default::default()
        };
        assert!((metrics.success_rate() - 95.0).abs() < 0.1);
    }

    #[test]
    fn test_agent_progress_bar() {
        let agent = AgentNode {
            progress: 0.5,
            ..AgentNode::new("a1", "Agent 1", 1)
        };
        let bar = agent.progress_bar(10);
        assert_eq!(bar, "█████░░░░░");
    }

    #[test]
    fn test_jj_op_type_parsing() {
        assert_eq!(
            JjOpType::from_description("new empty commit"),
            JjOpType::New
        );
        assert_eq!(
            JjOpType::from_description("describe commit abc123"),
            JjOpType::Describe
        );
        assert_eq!(
            JjOpType::from_description("create bookmark main"),
            JjOpType::Bookmark
        );
    }

    #[test]
    fn test_agent_status_indicators() {
        assert_eq!(AgentStatus::Running.indicator(), "◉");
        assert_eq!(AgentStatus::Completed.indicator(), "✓");
        assert_eq!(AgentStatus::Failed.indicator(), "✗");
    }

    #[test]
    fn test_formatted_duration() {
        let metrics = GlobalMetrics {
            total_time_ms: 125000, // 2m 5s
            ..Default::default()
        };
        assert_eq!(metrics.formatted_duration(), "2m 5s");
    }
}
