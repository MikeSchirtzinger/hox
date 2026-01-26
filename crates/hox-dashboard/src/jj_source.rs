//! JJ oplog data source
//!
//! Parses JJ operation log and builds dashboard state from live data.

use crate::{
    AgentNode, AgentStatus, DashboardConfig, DashboardState, DashboardError, GlobalMetrics,
    JjOpType, JjOplogEntry, OrchestrationSession, PhaseProgress, PhaseStatus, Result,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::process::Command;

/// JJ data source for live oplog and state tracking
pub struct JjDataSource {
    config: DashboardConfig,
}

impl JjDataSource {
    /// Create a new JJ data source with configuration
    pub fn new(config: DashboardConfig) -> Self {
        Self { config }
    }

    /// Fetch current dashboard state from JJ and metrics
    pub async fn fetch_state(&self) -> Result<DashboardState> {
        // Fetch all data concurrently
        let oplog = fetch_oplog(self.config.max_oplog_entries).await?;
        let bookmark = self.current_bookmark().await;

        // Extract agent information from oplog
        let agents = extract_agents_from_oplog(&oplog);

        // Build session info
        let session = OrchestrationSession {
            id: bookmark.clone().unwrap_or_else(|| "unknown".to_string()),
            bookmark,
            started_at: oplog.last().map(|e| e.timestamp),
            total_phases: infer_total_phases(&agents),
            current_phase: infer_current_phase(&agents),
        };

        // Calculate global metrics
        let global_metrics = calculate_global_metrics(&agents, &oplog);

        // Build phase progress
        let phases = build_phase_progress(&agents);

        Ok(DashboardState {
            session,
            global_metrics,
            agents,
            oplog,
            phases,
            last_updated: Some(Utc::now()),
        })
    }

    /// Get current JJ bookmark
    pub async fn current_bookmark(&self) -> Option<String> {
        let output = Command::new("jj")
            .args(&["bookmark", "list", "--all"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        // Parse first bookmark (format: "bookmark-name: change-id")
        stdout
            .lines()
            .next()?
            .split(':')
            .next()
            .map(|s| s.trim().to_string())
    }
}

/// Fetch recent JJ operation log entries
pub async fn fetch_oplog(limit: usize) -> Result<Vec<JjOplogEntry>> {
    let output = Command::new("jj")
        .args(&[
            "op",
            "log",
            "--no-graph",
            "-n",
            &limit.to_string(),
            "-T",
            r#"id ++ "|" ++ time.start().format("%Y-%m-%d %H:%M:%S") ++ "|" ++ description ++ "\n""#,
        ])
        .output()
        .map_err(|e| DashboardError::JjOplog(format!("Failed to execute jj: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DashboardError::JjOplog(format!(
            "jj op log failed: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| DashboardError::JjOplog(format!("Invalid UTF-8 in oplog: {}", e)))?;
    let mut entries = Vec::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match parse_oplog_line(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                eprintln!("Warning: Failed to parse oplog line: {} (error: {})", line, e);
                continue;
            }
        }
    }

    // Reverse to get chronological order (newest last)
    entries.reverse();

    Ok(entries)
}

/// Parse a single oplog line
fn parse_oplog_line(line: &str) -> Result<JjOplogEntry> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 3 {
        return Err(DashboardError::JjOplog(format!(
            "Invalid oplog line format: {}",
            line
        )));
    }

    let id = parts[0].trim().to_string();
    let timestamp_str = parts[1].trim();
    let description = parts[2].trim().to_string();

    // Parse timestamp (format: "YYYY-MM-DD HH:MM:SS")
    let timestamp = DateTime::parse_from_str(
        &format!("{} +0000", timestamp_str),
        "%Y-%m-%d %H:%M:%S %z",
    )
    .map_err(|e| {
        DashboardError::JjOplog(format!("Failed to parse timestamp '{}': {}", timestamp_str, e))
    })?
    .with_timezone(&Utc);

    // Extract agent ID if present
    let agent_id = extract_agent_id(&description);

    // Infer operation type
    let op_type = JjOpType::from_description(&description);

    // Extract any tags/metadata from description
    let tags = extract_tags(&description);

    Ok(JjOplogEntry {
        id,
        timestamp,
        description,
        agent_id,
        op_type,
        tags,
    })
}

/// Extract agent ID from operation description
pub fn extract_agent_id(description: &str) -> Option<String> {
    // Common patterns:
    // - "agent-abc123"
    // - "[agent:xyz]"
    // - "Agent xyz:"
    // - "O-A-1/agent-name"

    // Try pattern: agent-{id}
    if let Some(idx) = description.find("agent-") {
        let rest = &description[idx + 6..];
        if let Some(end) = rest.find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_') {
            return Some(format!("agent-{}", &rest[..end]));
        } else {
            return Some(format!("agent-{}", rest));
        }
    }

    // Try pattern: [agent:id]
    if let Some(start) = description.find("[agent:") {
        if let Some(end) = description[start..].find(']') {
            let agent_id = &description[start + 7..start + end];
            return Some(agent_id.to_string());
        }
    }

    // Try pattern: O-X-Y/agent-name
    if description.contains('/') && description.contains("O-") {
        if let Some(slash_idx) = description.find('/') {
            let before = &description[..slash_idx];
            if before.starts_with("O-") {
                let after_start = slash_idx + 1;
                if let Some(space_idx) = description[after_start..].find(' ') {
                    return Some(description[after_start..after_start + space_idx].to_string());
                } else {
                    return Some(description[after_start..].to_string());
                }
            }
        }
    }

    None
}

/// Extract tags/metadata from description
fn extract_tags(description: &str) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    // Look for [key:value] patterns
    for cap in description.match_indices('[') {
        let start = cap.0;
        if let Some(end) = description[start..].find(']') {
            let tag_content = &description[start + 1..start + end];
            if let Some(colon) = tag_content.find(':') {
                let key = tag_content[..colon].trim().to_string();
                let value = tag_content[colon + 1..].trim().to_string();
                tags.insert(key, value);
            }
        }
    }

    tags
}

/// Extract agent nodes from oplog entries
fn extract_agents_from_oplog(oplog: &[JjOplogEntry]) -> Vec<AgentNode> {
    let mut agents_map: HashMap<String, AgentNode> = HashMap::new();

    for entry in oplog {
        if let Some(agent_id) = &entry.agent_id {
            let agent = agents_map.entry(agent_id.clone()).or_insert_with(|| {
                let phase = entry.tags.get("phase")
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(1);

                AgentNode::new(agent_id.clone(), agent_id.clone(), phase)
            });

            // Update agent based on operation type
            match entry.op_type {
                JjOpType::New => {
                    agent.status = AgentStatus::Running;
                }
                JjOpType::Describe => {
                    // Description updates indicate progress
                    agent.tool_calls += 1;
                }
                JjOpType::Commit | JjOpType::Squash => {
                    // Commits often indicate completion
                    if entry.description.contains("complete") || entry.description.contains("done") {
                        agent.status = AgentStatus::Completed;
                        agent.progress = 1.0;
                    }
                }
                _ => {}
            }

            // Extract change ID if present
            if agent.change_id.is_none() {
                if let Some(change_id) = extract_change_id(&entry.description) {
                    agent.change_id = Some(change_id);
                }
            }

            // Extract task description if present
            if agent.task.is_empty() {
                if let Some(task) = entry.tags.get("task") {
                    agent.task = task.clone();
                }
            }
        }
    }

    // Estimate progress for running agents
    for (agent_id, agent) in agents_map.iter_mut() {
        if agent.status == AgentStatus::Running {
            let agent_ops: Vec<_> = oplog.iter()
                .filter(|e| e.agent_id.as_ref() == Some(agent_id))
                .collect();
            agent.progress = estimate_agent_progress(agent, &agent_ops);
        }
    }

    let mut agents: Vec<_> = agents_map.into_values().collect();
    agents.sort_by_key(|a| (a.phase, a.id.clone()));
    agents
}

/// Extract change ID from description
fn extract_change_id(description: &str) -> Option<String> {
    // Look for patterns like "abc123" (7-char hex-ish IDs)
    for word in description.split_whitespace() {
        if word.len() == 7 && word.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(word.to_string());
        }
    }
    None
}

/// Calculate estimated progress based on recent operations
pub fn estimate_agent_progress(agent: &AgentNode, recent_ops: &[&JjOplogEntry]) -> f32 {
    if recent_ops.is_empty() {
        return 0.0;
    }

    // Heuristic: count operation types and assign weights
    let mut score = 0.0;

    for op in recent_ops {
        match op.op_type {
            JjOpType::New => score += 0.1,           // Starting
            JjOpType::Describe => score += 0.15,     // Making progress
            JjOpType::Commit => score += 0.3,        // Significant progress
            JjOpType::Squash => score += 0.2,        // Cleanup/organization
            JjOpType::Rebase => score += 0.1,        // Maintenance
            _ => score += 0.05,                       // Other activity
        }
    }

    // Normalize by number of operations and cap at 0.95
    let normalized = (score / recent_ops.len() as f32).min(0.95);

    // Factor in current agent state
    match agent.status {
        AgentStatus::Completed => 1.0,
        AgentStatus::Failed => 0.0,
        _ => normalized,
    }
}

/// Calculate global metrics from agents and oplog
fn calculate_global_metrics(agents: &[AgentNode], _oplog: &[JjOplogEntry]) -> GlobalMetrics {
    let mut total_tool_calls = 0;
    let mut total_failures = 0;
    let mut total_time_ms = 0;

    for agent in agents {
        total_tool_calls += agent.tool_calls;
        if agent.status == AgentStatus::Failed {
            total_failures += 1;
        }
        total_time_ms += agent.duration_ms;
    }

    let active_agents = agents.iter().filter(|a| a.status == AgentStatus::Running).count();
    let completed_agents = agents.iter().filter(|a| a.status == AgentStatus::Completed).count();

    GlobalMetrics {
        total_tool_calls,
        total_failures,
        total_time_ms,
        active_agents,
        completed_agents,
    }
}

/// Infer total number of phases from agents
fn infer_total_phases(agents: &[AgentNode]) -> usize {
    agents.iter().map(|a| a.phase).max().unwrap_or(1)
}

/// Infer current phase from agent statuses
fn infer_current_phase(agents: &[AgentNode]) -> usize {
    // Find the lowest phase number that has running or pending agents
    let active_phase = agents
        .iter()
        .filter(|a| matches!(a.status, AgentStatus::Running | AgentStatus::Pending))
        .map(|a| a.phase)
        .min();

    active_phase.unwrap_or_else(|| {
        // If no active agents, return the highest completed phase
        agents
            .iter()
            .filter(|a| a.status == AgentStatus::Completed)
            .map(|a| a.phase)
            .max()
            .unwrap_or(0)
    })
}

/// Build phase progress from agents
fn build_phase_progress(agents: &[AgentNode]) -> Vec<PhaseProgress> {
    let mut phases_map: HashMap<usize, PhaseProgress> = HashMap::new();

    for agent in agents {
        let phase = phases_map.entry(agent.phase).or_insert_with(|| {
            PhaseProgress::new(agent.phase, format!("Phase {}", agent.phase))
        });

        phase.agent_ids.push(agent.id.clone());

        // Update phase status based on agent statuses
        let phase_agents: Vec<_> = agents.iter().filter(|a| a.phase == agent.phase).collect();
        let all_completed = phase_agents.iter().all(|a| a.status == AgentStatus::Completed);
        let any_failed = phase_agents.iter().any(|a| a.status == AgentStatus::Failed);
        let any_running = phase_agents.iter().any(|a| a.status == AgentStatus::Running);

        phase.status = if any_failed {
            PhaseStatus::Failed
        } else if all_completed {
            PhaseStatus::Completed
        } else if any_running {
            PhaseStatus::Active
        } else {
            PhaseStatus::Pending
        };

        // Calculate phase progress as average of agent progress
        let total_progress: f32 = phase_agents.iter().map(|a| a.progress).sum();
        phase.progress = total_progress / phase_agents.len() as f32;
    }

    let mut phases: Vec<_> = phases_map.into_values().collect();
    phases.sort_by_key(|p| p.number);
    phases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_agent_id_patterns() {
        assert_eq!(
            extract_agent_id("Working on agent-abc123 task"),
            Some("agent-abc123".to_string())
        );
        assert_eq!(
            extract_agent_id("[agent:xyz] completed task"),
            Some("xyz".to_string())
        );
        assert_eq!(
            extract_agent_id("O-A-1/agent-foo operation"),
            Some("agent-foo".to_string())
        );
        assert_eq!(extract_agent_id("No agent here"), None);
    }

    #[test]
    fn test_extract_tags() {
        let desc = "Task [phase:1] [priority:high] completed";
        let tags = extract_tags(desc);
        assert_eq!(tags.get("phase"), Some(&"1".to_string()));
        assert_eq!(tags.get("priority"), Some(&"high".to_string()));
    }

    #[test]
    fn test_estimate_agent_progress_empty() {
        let agent = AgentNode::new("a1", "Agent 1", 1);
        let ops = vec![];
        assert_eq!(estimate_agent_progress(&agent, &ops), 0.0);
    }

    #[test]
    fn test_extract_change_id() {
        assert_eq!(
            extract_change_id("commit abc123d completed"),
            Some("abc123d".to_string())
        );
        assert_eq!(extract_change_id("no change id here"), None);
    }

    #[test]
    fn test_parse_oplog_line() {
        let line = "abc123|2025-01-25 10:30:00|describe commit [agent:test]";
        let entry = parse_oplog_line(line).expect("Failed to parse");
        assert_eq!(entry.id, "abc123");
        assert_eq!(entry.agent_id, Some("test".to_string()));
        assert_eq!(entry.op_type, JjOpType::Describe);
    }

    #[test]
    fn test_infer_total_phases() {
        let agents = vec![
            AgentNode::new("a1", "A1", 1),
            AgentNode::new("a2", "A2", 2),
            AgentNode::new("a3", "A3", 2),
        ];
        assert_eq!(infer_total_phases(&agents), 2);
    }

    #[test]
    fn test_infer_current_phase() {
        let mut agents = vec![
            AgentNode::new("a1", "A1", 1),
            AgentNode::new("a2", "A2", 2),
        ];
        agents[0].status = AgentStatus::Completed;
        agents[1].status = AgentStatus::Running;
        assert_eq!(infer_current_phase(&agents), 2);
    }
}
