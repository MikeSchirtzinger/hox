//! Visualization state types
//!
//! Graph-oriented types that translate from DashboardState to what the frontend needs.

use hox_dashboard::{
    AgentStatus, DagState, DashboardState, JjOpType, JjOplogEntry, PhaseProgress, PhaseStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Node types in the visualization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    #[default]
    Agent,
    Phase,
    Task,
    /// JJ change (commit node in DAG view)
    Change,
    /// File being modified (force-positioned by simulation)
    File,
    /// Merge point (change with multiple parents)
    Merge,
    /// Session root / oldest common ancestor
    Root,
}

/// Link types between nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    #[default]
    /// Agent is working on a phase
    WorkingOn,
    /// Dependency relationship
    Dependency,
    /// Message/communication between agents
    Message,
    /// Parent → child edge in the JJ DAG
    DagParent,
    /// Agent node → their branch tip change
    AgentBranch,
    /// Change → file it modifies
    FileTouch,
    /// Branch tip → merge target
    MergeEdge,
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
    /// Fixed X position (structural nodes; None = force-directed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fx: Option<f64>,
    /// Fixed Y position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fy: Option<f64>,
    /// Fixed Z position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fz: Option<f64>,
    /// Lane index (used for Y positioning in DAG layout)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<i32>,
    /// Bookmarks pointing at this node's change
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<String>,
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
    /// Current view mode: "orchestration" | "dag"
    pub view_mode: String,
}

impl Default for VizState {
    fn default() -> Self {
        Self {
            session: Default::default(),
            metrics: Default::default(),
            nodes: Vec::new(),
            links: Vec::new(),
            phases: Vec::new(),
            oplog: Vec::new(),
            view_mode: "orchestration".to_string(),
        }
    }
}

/// Delta update (sent between full syncs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizDelta {
    pub changed_nodes: Vec<VizNode>,
    pub removed_node_ids: Vec<String>,
    pub changed_links: Vec<VizLink>,
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

/// Translate DashboardState into VizState.
///
/// Builds orchestration nodes + optional DAG nodes into a single VizState.
/// The frontend uses `view_mode` to decide which set to render.
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
            fx: None,
            fy: None,
            fz: None,
            lane: None,
            bookmarks: Vec::new(),
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
            fx: None,
            fy: None,
            fz: None,
            lane: None,
            bookmarks: Vec::new(),
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

    // If DAG data is available, append DAG nodes and links
    if let Some(ref dag) = dashboard.dag {
        let (dag_nodes, dag_links) = build_dag_nodes_and_links(dag, dashboard);
        nodes.extend(dag_nodes);
        links.extend(dag_links);
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
        view_mode: "orchestration".to_string(),
    }
}

#[allow(dead_code)]
/// Translate a DashboardState that has DAG data into a DAG-focused VizState.
///
/// Used by `translate_dag()` which is the dedicated DAG view path.
/// When `dashboard.dag` is None, returns a minimal empty VizState.
pub fn translate_dag(dashboard: &DashboardState) -> VizState {
    let dag = match &dashboard.dag {
        Some(d) => d,
        None => {
            return VizState {
                view_mode: "dag".to_string(),
                ..Default::default()
            }
        }
    };

    let (nodes, links) = build_dag_nodes_and_links(dag, dashboard);

    let oplog: Vec<VizOplogEntry> = dashboard.oplog.iter().map(translate_oplog_entry).collect();
    let phases: Vec<VizPhase> = dashboard.phases.iter().map(translate_phase).collect();
    let metrics = VizMetrics {
        total_tool_calls: dashboard.global_metrics.total_tool_calls,
        total_failures: dashboard.global_metrics.total_failures,
        success_rate: dashboard.global_metrics.success_rate(),
        active_agents: dashboard.global_metrics.active_agents,
        completed_agents: dashboard.global_metrics.completed_agents,
        total_time_ms: dashboard.global_metrics.total_time_ms,
    };
    let session = VizSession {
        id: dashboard.session.id.clone(),
        bookmark: dashboard.session.bookmark.clone(),
        started_at: dashboard.session.started_at.map(|t| t.to_rfc3339()),
        uptime_ms: 0,
    };

    VizState {
        session,
        metrics,
        nodes,
        links,
        phases,
        oplog,
        view_mode: "dag".to_string(),
    }
}

/// Core DAG layout algorithm.
///
/// Returns (nodes, links) for the DAG portion of the visualization.
///
/// Layout:
/// - Topological BFS from root assigns depth (X = depth * 40)
/// - Each unique agent gets a lane; root gets center lane (Y = lane * 50)
/// - File nodes have fx/fy/fz = None (force-positioned by sim)
/// - Structural change nodes get fixed positions
fn build_dag_nodes_and_links(
    dag: &DagState,
    dashboard: &DashboardState,
) -> (Vec<VizNode>, Vec<VizLink>) {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    if dag.changes.is_empty() {
        return (nodes, links);
    }

    // Build adjacency map: change_id -> index
    let idx_map: HashMap<&str, usize> = dag
        .changes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.change_id.as_str(), i))
        .collect();

    // Assign agent lanes (sorted by agent_id for determinism)
    let mut agent_ids: Vec<String> = dag
        .changes
        .iter()
        .filter_map(|c| c.agent_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    agent_ids.sort();

    let num_agents = agent_ids.len();
    let agent_lane: HashMap<&str, i32> = agent_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i as i32))
        .collect();

    // Root gets the center lane
    let root_lane = if num_agents == 0 {
        0
    } else {
        (num_agents as i32) / 2
    };

    // BFS topological depth assignment
    let mut depth: HashMap<&str, i32> = HashMap::new();
    let root_id = dag.root_change_id.as_deref().unwrap_or_else(|| {
        dag.changes
            .first()
            .map(|c| c.change_id.as_str())
            .unwrap_or("")
    });

    let mut queue = std::collections::VecDeque::new();
    if !root_id.is_empty() {
        depth.insert(root_id, 0);
        queue.push_back(root_id);
    }

    // Build child map for BFS (parent -> children)
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for change in &dag.changes {
        for parent in &change.parents {
            if idx_map.contains_key(parent.as_str()) {
                children
                    .entry(parent.as_str())
                    .or_default()
                    .push(change.change_id.as_str());
            }
        }
    }

    while let Some(current) = queue.pop_front() {
        let current_depth = depth[current];
        if let Some(kids) = children.get(current) {
            for &child in kids {
                if !depth.contains_key(child) {
                    depth.insert(child, current_depth + 1);
                    queue.push_back(child);
                }
            }
        }
    }

    // File nodes: track which changes we've added files for (limit to 3 most recent per agent)
    let mut agent_file_count: HashMap<String, usize> = HashMap::new();

    // Sort changes by depth for deterministic output
    let mut sorted_changes: Vec<&hox_dashboard::DagChange> = dag.changes.iter().collect();
    sorted_changes.sort_by_key(|c| {
        (
            depth.get(c.change_id.as_str()).copied().unwrap_or(0),
            &c.change_id,
        )
    });

    for change in &sorted_changes {
        let cid = change.change_id.as_str();

        // Determine node type
        let node_type = if change.parents.is_empty()
            || Some(&change.change_id) == dag.root_change_id.as_ref()
        {
            NodeType::Root
        } else if change.parents.len() > 1 {
            NodeType::Merge
        } else {
            NodeType::Change
        };

        // Assign lane
        let lane = change
            .agent_id
            .as_deref()
            .and_then(|a| agent_lane.get(a).copied())
            .unwrap_or(root_lane);

        // Compute fixed position
        let d = depth.get(cid).copied().unwrap_or(0);
        let fx = (d as f64) * 40.0;
        let fy = (lane as f64) * 50.0;
        let fz = (lane as f64) * 5.0;

        // Color: conflict = red, merge = purple, root = gold, agent running = cyan, else green
        let color = if change.has_conflict {
            "#ff0044"
        } else {
            match node_type {
                NodeType::Root => "#ffcc00",
                NodeType::Merge => "#cc44ff",
                _ => {
                    // Check if any agent in dashboard is Running and owns this change
                    let is_running = dashboard.agents.iter().any(|a| {
                        a.change_id.as_deref() == Some(cid)
                            && a.status == AgentStatus::Running
                    });
                    if is_running {
                        "#00ffff"
                    } else {
                        "#00ff88"
                    }
                }
            }
        };

        nodes.push(VizNode {
            id: format!("dag-{}", cid),
            label: if change.description.is_empty() {
                cid[..cid.len().min(8)].to_string()
            } else {
                change.description.chars().take(32).collect()
            },
            node_type: node_type.clone(),
            status: if change.has_conflict {
                "conflict".to_string()
            } else {
                "ok".to_string()
            },
            progress: 0.0,
            phase: None,
            color: color.to_string(),
            glow_intensity: if change.has_conflict { 1.0 } else { 0.4 },
            details: serde_json::json!({
                "change_id": change.change_id,
                "parents": change.parents,
                "bookmarks": change.bookmarks,
                "agent_id": change.agent_id,
                "has_conflict": change.has_conflict,
                "timestamp_ms": change.timestamp_ms,
                "file_count": change.files.len(),
            }),
            fx: Some(fx),
            fy: Some(fy),
            fz: Some(fz),
            lane: Some(lane),
            bookmarks: change.bookmarks.clone(),
        });

        // Parent → child links (DagParent)
        for parent_id in &change.parents {
            if idx_map.contains_key(parent_id.as_str()) {
                let link_color = if node_type == NodeType::Merge {
                    "#cc44ff"
                } else {
                    "#336644"
                };
                links.push(VizLink {
                    source: format!("dag-{}", parent_id),
                    target: format!("dag-{}", cid),
                    link_type: LinkType::DagParent,
                    particles: 0,
                    particle_speed: 0.0,
                    color: link_color.to_string(),
                    width: 1.5,
                });
            }
        }

        // File nodes (limited to 3 most recent per agent)
        let agent_key = change
            .agent_id
            .clone()
            .unwrap_or_else(|| "__no_agent__".to_string());
        let file_count = agent_file_count.entry(agent_key).or_insert(0);
        if *file_count < 3 && !change.files.is_empty() {
            *file_count += 1;
            for file in change.files.iter().take(5) {
                let file_node_id = format!("file-{}-{}", cid, sanitize_path(&file.path));
                let file_color = match file.change_type.as_str() {
                    "added" => "#00ff88",
                    "deleted" => "#ff0044",
                    "renamed" => "#ffcc00",
                    _ => "#88aaff",
                };

                nodes.push(VizNode {
                    id: file_node_id.clone(),
                    label: short_path(&file.path),
                    node_type: NodeType::File,
                    status: file.change_type.clone(),
                    progress: 0.0,
                    phase: None,
                    color: file_color.to_string(),
                    glow_intensity: 0.2,
                    details: serde_json::json!({
                        "path": file.path,
                        "change_type": file.change_type,
                        "insertions": file.insertions,
                        "deletions": file.deletions,
                    }),
                    // File nodes are force-positioned
                    fx: None,
                    fy: None,
                    fz: None,
                    lane: None,
                    bookmarks: Vec::new(),
                });

                links.push(VizLink {
                    source: format!("dag-{}", cid),
                    target: file_node_id,
                    link_type: LinkType::FileTouch,
                    particles: 0,
                    particle_speed: 0.0,
                    color: file_color.to_string(),
                    width: 0.5,
                });
            }
        }
    }

    // Agent → branch tip links (AgentBranch)
    // For each agent in dashboard, find their latest change and create a link
    for agent in &dashboard.agents {
        if let Some(change_id) = &agent.change_id {
            let dag_node_id = format!("dag-{}", &change_id[..change_id.len().min(12)]);
            // Only emit if this node actually exists
            let node_exists = nodes.iter().any(|n| n.id == dag_node_id);
            if node_exists {
                links.push(VizLink {
                    source: agent.id.clone(),
                    target: dag_node_id,
                    link_type: LinkType::AgentBranch,
                    particles: if agent.status == AgentStatus::Running {
                        2
                    } else {
                        0
                    },
                    particle_speed: 0.008,
                    color: status_color(&agent.status).to_string(),
                    width: 1.0,
                });
            }
        }
    }

    (nodes, links)
}

/// Sanitize a file path for use in a node ID
fn sanitize_path(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Extract the last component of a path for display
fn short_path(path: &str) -> String {
    path.split('/')
        .last()
        .unwrap_or(path)
        .to_string()
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

    // Find removed node IDs
    let new_ids: HashSet<&str> = new.nodes.iter().map(|n| n.id.as_str()).collect();
    let removed_node_ids: Vec<String> = old
        .nodes
        .iter()
        .filter(|n| !new_ids.contains(n.id.as_str()))
        .map(|n| n.id.clone())
        .collect();

    // Find changed links (new ones not in old)
    let old_link_keys: HashSet<String> = old
        .links
        .iter()
        .map(|l| format!("{}->{}", l.source, l.target))
        .collect();
    let changed_links: Vec<VizLink> = new
        .links
        .iter()
        .filter(|l| !old_link_keys.contains(&format!("{}->{}", l.source, l.target)))
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
        removed_node_ids,
        changed_links,
        new_oplog,
        metrics: new.metrics.clone(),
        changed_phases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_dashboard::{AgentNode, DagChange, DagState, DashboardState};

    #[test]
    fn test_translate_empty_state() {
        let state = DashboardState::default();
        let viz = translate(&state);
        assert!(viz.nodes.is_empty());
        assert!(viz.links.is_empty());
        assert!(viz.oplog.is_empty());
        assert_eq!(viz.view_mode, "orchestration");
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
                fx: None,
                fy: None,
                fz: None,
                lane: None,
                bookmarks: Vec::new(),
            }],
            links: vec![],
            phases: vec![],
            oplog: vec![],
            view_mode: "orchestration".into(),
        };

        let mut new = old.clone();
        new.nodes[0].progress = 0.8;

        let delta = compute_delta(&old, &new);
        assert_eq!(delta.changed_nodes.len(), 1);
        assert_eq!(delta.changed_nodes[0].progress, 0.8);
        assert!(delta.removed_node_ids.is_empty());
    }

    #[test]
    fn test_compute_delta_removed_node() {
        let old = VizState {
            nodes: vec![
                VizNode {
                    id: "a1".into(),
                    label: "A1".into(),
                    node_type: NodeType::Agent,
                    status: "running".into(),
                    progress: 0.0,
                    phase: None,
                    color: "#00ffff".into(),
                    glow_intensity: 0.8,
                    details: serde_json::json!({}),
                    fx: None, fy: None, fz: None, lane: None,
                    bookmarks: Vec::new(),
                },
                VizNode {
                    id: "a2".into(),
                    label: "A2".into(),
                    node_type: NodeType::Agent,
                    status: "running".into(),
                    progress: 0.0,
                    phase: None,
                    color: "#00ffff".into(),
                    glow_intensity: 0.8,
                    details: serde_json::json!({}),
                    fx: None, fy: None, fz: None, lane: None,
                    bookmarks: Vec::new(),
                },
            ],
            links: vec![],
            phases: vec![],
            oplog: vec![],
            session: Default::default(),
            metrics: Default::default(),
            view_mode: "orchestration".into(),
        };

        let mut new = old.clone();
        new.nodes.retain(|n| n.id != "a2"); // remove a2

        let delta = compute_delta(&old, &new);
        assert_eq!(delta.removed_node_ids, vec!["a2".to_string()]);
    }

    #[test]
    fn test_translate_dag_empty() {
        let state = DashboardState::default();
        let viz = translate_dag(&state);
        assert!(viz.nodes.is_empty());
        assert!(viz.links.is_empty());
        assert_eq!(viz.view_mode, "dag");
    }

    #[test]
    fn test_translate_dag_with_changes() {
        let mut state = DashboardState::default();

        state.dag = Some(DagState {
            root_change_id: Some("aaaaaaaaaaaa".to_string()),
            changes: vec![
                DagChange {
                    change_id: "aaaaaaaaaaaa".to_string(),
                    description: "Root commit".to_string(),
                    parents: vec![],
                    bookmarks: vec!["task/root".to_string()],
                    agent_id: None,
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 1709000000000,
                },
                DagChange {
                    change_id: "bbbbbbbbbbbb".to_string(),
                    description: "Implement feature".to_string(),
                    parents: vec!["aaaaaaaaaaaa".to_string()],
                    bookmarks: vec!["agent/eng/task/feat".to_string()],
                    agent_id: Some("agent/eng".to_string()),
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 1709000001000,
                },
            ],
        });

        let viz = translate_dag(&state);
        assert_eq!(viz.view_mode, "dag");

        // Should have 2 change nodes
        let dag_nodes: Vec<_> = viz
            .nodes
            .iter()
            .filter(|n| matches!(n.node_type, NodeType::Change | NodeType::Root))
            .collect();
        assert_eq!(dag_nodes.len(), 2);

        // Root node
        let root = viz.nodes.iter().find(|n| n.id == "dag-aaaaaaaaaaaa").unwrap();
        assert_eq!(root.node_type, NodeType::Root);
        assert_eq!(root.fx, Some(0.0));

        // Child node
        let child = viz
            .nodes
            .iter()
            .find(|n| n.id == "dag-bbbbbbbbbbbb")
            .unwrap();
        assert_eq!(child.node_type, NodeType::Change);
        assert_eq!(child.fx, Some(40.0)); // depth 1

        // DagParent link should exist
        let parent_link = viz.links.iter().find(|l| {
            l.source == "dag-aaaaaaaaaaaa"
                && l.target == "dag-bbbbbbbbbbbb"
                && l.link_type == LinkType::DagParent
        });
        assert!(parent_link.is_some());
    }

    #[test]
    fn test_translate_dag_merge_node() {
        let mut state = DashboardState::default();

        state.dag = Some(DagState {
            root_change_id: Some("root000000aa".to_string()),
            changes: vec![
                DagChange {
                    change_id: "root000000aa".to_string(),
                    description: "Root".to_string(),
                    parents: vec![],
                    bookmarks: vec![],
                    agent_id: None,
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 0,
                },
                DagChange {
                    change_id: "branch1aaaaa".to_string(),
                    description: "Branch A".to_string(),
                    parents: vec!["root000000aa".to_string()],
                    bookmarks: vec![],
                    agent_id: Some("agent/a".to_string()),
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 1,
                },
                DagChange {
                    change_id: "branch2bbbbb".to_string(),
                    description: "Branch B".to_string(),
                    parents: vec!["root000000aa".to_string()],
                    bookmarks: vec![],
                    agent_id: Some("agent/b".to_string()),
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 2,
                },
                DagChange {
                    change_id: "mergenode000".to_string(),
                    description: "Merge".to_string(),
                    parents: vec![
                        "branch1aaaaa".to_string(),
                        "branch2bbbbb".to_string(),
                    ],
                    bookmarks: vec![],
                    agent_id: None,
                    files: vec![],
                    has_conflict: false,
                    timestamp_ms: 3,
                },
            ],
        });

        let viz = translate_dag(&state);
        let merge_node = viz
            .nodes
            .iter()
            .find(|n| n.id == "dag-mergenode000")
            .unwrap();
        assert_eq!(merge_node.node_type, NodeType::Merge);
        assert_eq!(merge_node.color, "#cc44ff");
    }

    #[test]
    fn test_short_path() {
        assert_eq!(short_path("crates/foo/src/lib.rs"), "lib.rs");
        assert_eq!(short_path("README.md"), "README.md");
    }

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("src/lib.rs"), "src_lib_rs");
        assert_eq!(sanitize_path("foo-bar_baz"), "foo-bar_baz");
    }
}
