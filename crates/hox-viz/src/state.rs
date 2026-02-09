//! Visualization state types
//!
//! Graph-oriented types that translate from DashboardState to what the frontend needs.

use hox_dashboard::{
    AgentStatus, DashboardState, JjOpType, JjOplogEntry, PhaseProgress, PhaseStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Node types in the visualization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Agent,
    Phase,
    Task,
}

/// Link types between nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    /// Agent is working on a phase
    WorkingOn,
    /// Dependency relationship
    Dependency,
    /// Message/communication between agents
    Message,
}

/// A node in the visualization graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizNode {
    pub id: String,
    pub label: String,
    pub node_type: NodeType,
    pub status: String,
    pub progress: f32,
    pub phase: Option<usize>,
    pub color: String,
    pub glow_intensity: f32,
    pub details: serde_json::Value,
}

/// A link between nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizLink {
    pub source: String,
    pub target: String,
    pub link_type: LinkType,
    pub particles: u32,
    pub particle_speed: f64,
    pub color: String,
    pub width: f32,
}

/// Metrics summary for the HUD
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VizMetrics {
    pub total_tool_calls: u32,
    pub total_failures: u32,
    pub success_rate: f32,
    pub active_agents: usize,
    pub completed_agents: usize,
    pub total_time_ms: u64,
}

/// Phase info for the HUD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizPhase {
    pub number: usize,
    pub name: String,
    pub status: String,
    pub progress: f32,
    pub agent_count: usize,
}

/// Oplog entry for the feed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizOplogEntry {
    pub id: String,
    pub timestamp: String,
    pub description: String,
    pub agent_id: Option<String>,
    pub op_type: String,
}

/// Session info
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VizSession {
    pub id: String,
    pub bookmark: Option<String>,
    pub started_at: Option<String>,
    pub uptime_ms: u64,
}

/// Full visualization state (sent on connect and periodic resync)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizState {
    pub session: VizSession,
    pub metrics: VizMetrics,
    pub nodes: Vec<VizNode>,
    pub links: Vec<VizLink>,
    pub phases: Vec<VizPhase>,
    pub oplog: Vec<VizOplogEntry>,
}

/// Delta update (sent between full syncs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizDelta {
    pub changed_nodes: Vec<VizNode>,
    pub new_oplog: Vec<VizOplogEntry>,
    pub metrics: VizMetrics,
    pub changed_phases: Vec<VizPhase>,
}

/// Convert AgentStatus to CSS color hex
fn status_color(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "#00ffff",
        AgentStatus::Completed => "#ff00ff",
        AgentStatus::Blocked => "#ffff00",
        AgentStatus::Failed => "#ff0044",
        AgentStatus::Pending => "#444444",
    }
}

/// Convert AgentStatus to glow intensity
fn status_glow(status: &AgentStatus) -> f32 {
    match status {
        AgentStatus::Running => 0.8,
        AgentStatus::Completed => 0.5,
        AgentStatus::Failed => 1.0,
        AgentStatus::Blocked => 0.6,
        AgentStatus::Pending => 0.1,
    }
}

/// Convert PhaseStatus to string
fn phase_status_str(status: &PhaseStatus) -> &'static str {
    match status {
        PhaseStatus::Pending => "pending",
        PhaseStatus::Active => "active",
        PhaseStatus::Completed => "completed",
        PhaseStatus::Failed => "failed",
    }
}

/// Convert PhaseStatus to color
fn phase_color(status: &PhaseStatus) -> &'static str {
    match status {
        PhaseStatus::Active => "#00ffff",
        PhaseStatus::Completed => "#ff00ff",
        PhaseStatus::Failed => "#ff0044",
        PhaseStatus::Pending => "#333333",
    }
}

/// Convert JjOpType to string
fn op_type_str(op_type: &JjOpType) -> &'static str {
    match op_type {
        JjOpType::New => "new",
        JjOpType::Describe => "describe",
        JjOpType::Squash => "squash",
        JjOpType::Bookmark => "bookmark",
        JjOpType::Commit => "commit",
        JjOpType::Rebase => "rebase",
        JjOpType::Workspace => "workspace",
        JjOpType::Other => "other",
    }
}

/// Convert AgentStatus to string
fn agent_status_str(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "running",
        AgentStatus::Completed => "completed",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Failed => "failed",
        AgentStatus::Pending => "pending",
    }
}

/// Translate DashboardState into VizState
pub fn translate(dashboard: &DashboardState) -> VizState {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // Create phase nodes
    for phase in &dashboard.phases {
        nodes.push(VizNode {
            id: format!("phase-{}", phase.number),
            label: phase.name.clone(),
            node_type: NodeType::Phase,
            status: phase_status_str(&phase.status).to_string(),
            progress: phase.progress,
            phase: Some(phase.number),
            color: phase_color(&phase.status).to_string(),
            glow_intensity: if phase.status == PhaseStatus::Active {
                0.6
            } else {
                0.2
            },
            details: serde_json::json!({
                "agent_count": phase.agent_ids.len(),
                "blocking": phase.blocking,
            }),
        });
    }

    // Create agent nodes and links
    for agent in &dashboard.agents {
        let color = status_color(&agent.status).to_string();

        nodes.push(VizNode {
            id: agent.id.clone(),
            label: agent.name.clone(),
            node_type: NodeType::Agent,
            status: agent_status_str(&agent.status).to_string(),
            progress: agent.progress,
            phase: Some(agent.phase),
            color: color.clone(),
            glow_intensity: status_glow(&agent.status),
            details: serde_json::json!({
                "tool_calls": agent.tool_calls,
                "success_rate": agent.success_rate,
                "duration_ms": agent.duration_ms,
                "task": agent.task,
                "change_id": agent.change_id,
            }),
        });

        // Link agent to its phase
        let (particles, speed) = match agent.status {
            AgentStatus::Running => (3, 0.01),
            AgentStatus::Completed => (1, 0.005),
            _ => (0, 0.0),
        };

        links.push(VizLink {
            source: agent.id.clone(),
            target: format!("phase-{}", agent.phase),
            link_type: LinkType::WorkingOn,
            particles,
            particle_speed: speed,
            color,
            width: if agent.status == AgentStatus::Running {
                2.0
            } else {
                1.0
            },
        });
    }

    // Create phase dependency links (sequential phases)
    let mut phase_numbers: Vec<usize> = dashboard.phases.iter().map(|p| p.number).collect();
    phase_numbers.sort();
    for window in phase_numbers.windows(2) {
        links.push(VizLink {
            source: format!("phase-{}", window[0]),
            target: format!("phase-{}", window[1]),
            link_type: LinkType::Dependency,
            particles: 0,
            particle_speed: 0.0,
            color: "#666666".to_string(),
            width: 1.0,
        });
    }

    // Translate oplog
    let oplog: Vec<VizOplogEntry> = dashboard.oplog.iter().map(translate_oplog_entry).collect();

    // Translate phases
    let phases: Vec<VizPhase> = dashboard.phases.iter().map(translate_phase).collect();

    // Translate metrics
    let metrics = VizMetrics {
        total_tool_calls: dashboard.global_metrics.total_tool_calls,
        total_failures: dashboard.global_metrics.total_failures,
        success_rate: dashboard.global_metrics.success_rate(),
        active_agents: dashboard.global_metrics.active_agents,
        completed_agents: dashboard.global_metrics.completed_agents,
        total_time_ms: dashboard.global_metrics.total_time_ms,
    };

    // Translate session
    let session = VizSession {
        id: dashboard.session.id.clone(),
        bookmark: dashboard.session.bookmark.clone(),
        started_at: dashboard.session.started_at.map(|t| t.to_rfc3339()),
        uptime_ms: dashboard
            .session
            .started_at
            .map(|t| {
                chrono::Utc::now()
                    .signed_duration_since(t)
                    .num_milliseconds()
                    .max(0) as u64
            })
            .unwrap_or(0),
    };

    VizState {
        session,
        metrics,
        nodes,
        links,
        phases,
        oplog,
    }
}

fn translate_oplog_entry(entry: &JjOplogEntry) -> VizOplogEntry {
    VizOplogEntry {
        id: entry.id.clone(),
        timestamp: entry.timestamp.format("%H:%M:%S").to_string(),
        description: entry.description.clone(),
        agent_id: entry.agent_id.clone(),
        op_type: op_type_str(&entry.op_type).to_string(),
    }
}

fn translate_phase(phase: &PhaseProgress) -> VizPhase {
    VizPhase {
        number: phase.number,
        name: phase.name.clone(),
        status: phase_status_str(&phase.status).to_string(),
        progress: phase.progress,
        agent_count: phase.agent_ids.len(),
    }
}

/// Compute a delta between old and new state
pub fn compute_delta(old: &VizState, new: &VizState) -> VizDelta {
    // Find changed nodes
    let changed_nodes: Vec<VizNode> = new
        .nodes
        .iter()
        .filter(|new_node| {
            old.nodes
                .iter()
                .find(|old_node| old_node.id == new_node.id)
                .map(|old_node| {
                    old_node.status != new_node.status
                        || (old_node.progress - new_node.progress).abs() > 0.01
                        || old_node.glow_intensity != new_node.glow_intensity
                })
                .unwrap_or(true) // New node not in old state
        })
        .cloned()
        .collect();

    // Find new oplog entries
    let old_ids: HashSet<&str> = old.oplog.iter().map(|e| e.id.as_str()).collect();
    let new_oplog: Vec<VizOplogEntry> = new
        .oplog
        .iter()
        .filter(|e| !old_ids.contains(e.id.as_str()))
        .cloned()
        .collect();

    // Find changed phases
    let changed_phases: Vec<VizPhase> = new
        .phases
        .iter()
        .filter(|new_phase| {
            old.phases
                .iter()
                .find(|old_phase| old_phase.number == new_phase.number)
                .map(|old_phase| {
                    old_phase.status != new_phase.status
                        || (old_phase.progress - new_phase.progress).abs() > 0.01
                })
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    VizDelta {
        changed_nodes,
        new_oplog,
        metrics: new.metrics.clone(),
        changed_phases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_dashboard::{AgentNode, DashboardState};

    #[test]
    fn test_translate_empty_state() {
        let state = DashboardState::default();
        let viz = translate(&state);
        assert!(viz.nodes.is_empty());
        assert!(viz.links.is_empty());
        assert!(viz.oplog.is_empty());
    }

    #[test]
    fn test_translate_with_agents() {
        let mut state = DashboardState::default();
        let mut agent = AgentNode::new("agent-1", "Builder", 1);
        agent.status = AgentStatus::Running;
        agent.progress = 0.67;
        agent.tool_calls = 42;
        state.agents.push(agent);

        state.phases.push(hox_dashboard::PhaseProgress {
            number: 1,
            name: "Phase 1".to_string(),
            blocking: false,
            status: PhaseStatus::Active,
            progress: 0.67,
            agent_ids: vec!["agent-1".to_string()],
        });

        let viz = translate(&state);
        assert_eq!(viz.nodes.len(), 2); // 1 phase + 1 agent
        assert_eq!(viz.links.len(), 1); // agent -> phase

        let agent_node = viz.nodes.iter().find(|n| n.id == "agent-1").unwrap();
        assert_eq!(agent_node.color, "#00ffff");
        assert_eq!(agent_node.node_type, NodeType::Agent);
        assert_eq!(agent_node.status, "running");
    }

    #[test]
    fn test_status_colors() {
        assert_eq!(status_color(&AgentStatus::Running), "#00ffff");
        assert_eq!(status_color(&AgentStatus::Completed), "#ff00ff");
        assert_eq!(status_color(&AgentStatus::Failed), "#ff0044");
        assert_eq!(status_color(&AgentStatus::Blocked), "#ffff00");
        assert_eq!(status_color(&AgentStatus::Pending), "#444444");
    }

    #[test]
    fn test_compute_delta() {
        let old = VizState {
            session: VizSession::default(),
            metrics: VizMetrics::default(),
            nodes: vec![VizNode {
                id: "a1".into(),
                label: "Agent 1".into(),
                node_type: NodeType::Agent,
                status: "running".into(),
                progress: 0.5,
                phase: Some(1),
                color: "#00ffff".into(),
                glow_intensity: 0.8,
                details: serde_json::json!({}),
            }],
            links: vec![],
            phases: vec![],
            oplog: vec![],
        };

        let mut new = old.clone();
        new.nodes[0].progress = 0.8;

        let delta = compute_delta(&old, &new);
        assert_eq!(delta.changed_nodes.len(), 1);
        assert_eq!(delta.changed_nodes[0].progress, 0.8);
    }
}
