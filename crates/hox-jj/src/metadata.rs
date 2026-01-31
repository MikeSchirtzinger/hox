//! Hox metadata management for JJ changes
//!
//! This module provides helpers for reading and writing Hox metadata
//! on JJ changes. Until jj-dev is complete, metadata is stored
//! in structured description text.

use hox_core::{ChangeId, HoxMetadata, MessageType, Priority, Result, TaskStatus};
use regex::Regex;
use std::sync::LazyLock;

use crate::command::JjExecutor;

/// Regex patterns for parsing metadata from descriptions
static PRIORITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Priority:\s*(\w+)").unwrap());
static STATUS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Status:\s*(\w+)").unwrap());
static AGENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Agent:\s*(.+)").unwrap());
static ORCHESTRATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Orchestrator:\s*(.+)").unwrap());
static MSG_TO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Msg-To:\s*(.+)").unwrap());
static MSG_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Msg-Type:\s*(\w+)").unwrap());

/// Manager for Hox metadata operations
pub struct MetadataManager<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> MetadataManager<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Parse Hox metadata from a change description
    pub fn parse_description(description: &str) -> HoxMetadata {
        let mut metadata = HoxMetadata::new();

        for line in description.lines() {
            let line = line.trim();

            if let Some(caps) = PRIORITY_RE.captures(line) {
                if let Ok(p) = caps[1].parse::<Priority>() {
                    metadata.priority = Some(p);
                }
            }

            if let Some(caps) = STATUS_RE.captures(line) {
                if let Ok(s) = caps[1].parse::<TaskStatus>() {
                    metadata.status = Some(s);
                }
            }

            if let Some(caps) = AGENT_RE.captures(line) {
                metadata.agent = Some(caps[1].trim().to_string());
            }

            if let Some(caps) = ORCHESTRATOR_RE.captures(line) {
                metadata.orchestrator = Some(caps[1].trim().to_string());
            }

            if let Some(caps) = MSG_TO_RE.captures(line) {
                metadata.msg_to = Some(caps[1].trim().to_string());
            }

            if let Some(caps) = MSG_TYPE_RE.captures(line) {
                if let Ok(t) = caps[1].parse::<MessageType>() {
                    metadata.msg_type = Some(t);
                }
            }
        }

        metadata
    }

    /// Format Hox metadata as description lines
    pub fn format_metadata(metadata: &HoxMetadata) -> String {
        let mut lines = Vec::new();

        if let Some(priority) = &metadata.priority {
            lines.push(format!("Priority: {}", priority));
        }

        if let Some(status) = &metadata.status {
            lines.push(format!("Status: {}", status));
        }

        if let Some(agent) = &metadata.agent {
            lines.push(format!("Agent: {}", agent));
        }

        if let Some(orchestrator) = &metadata.orchestrator {
            lines.push(format!("Orchestrator: {}", orchestrator));
        }

        if let Some(msg_to) = &metadata.msg_to {
            lines.push(format!("Msg-To: {}", msg_to));
        }

        if let Some(msg_type) = &metadata.msg_type {
            lines.push(format!("Msg-Type: {}", msg_type));
        }

        lines.join("\n")
    }

    /// Read metadata from a change
    pub async fn read(&self, change_id: &ChangeId) -> Result<HoxMetadata> {
        let output = self
            .executor
            .exec(&["log", "-r", change_id, "-T", "description", "--no-graph"])
            .await?;

        Ok(Self::parse_description(&output.stdout))
    }

    /// Set metadata on a change using jj describe
    ///
    /// Note: This updates the change description to include metadata.
    /// When jj-dev is complete, this will use --set-priority etc.
    pub async fn set(&self, change_id: &ChangeId, metadata: &HoxMetadata) -> Result<()> {
        // First read existing description
        let output = self
            .executor
            .exec(&["log", "-r", change_id, "-T", "description", "--no-graph"])
            .await?;

        let existing = output.stdout.trim();

        // Remove existing metadata lines
        let cleaned: Vec<&str> = existing
            .lines()
            .filter(|line| {
                let line = line.trim();
                !PRIORITY_RE.is_match(line)
                    && !STATUS_RE.is_match(line)
                    && !AGENT_RE.is_match(line)
                    && !ORCHESTRATOR_RE.is_match(line)
                    && !MSG_TO_RE.is_match(line)
                    && !MSG_TYPE_RE.is_match(line)
            })
            .collect();

        // Build new description
        let metadata_str = Self::format_metadata(metadata);
        let new_description = if cleaned.is_empty() {
            metadata_str
        } else {
            format!("{}\n\n{}", cleaned.join("\n"), metadata_str)
        };

        // Update the change
        self.executor
            .exec(&["describe", "-r", change_id, "-m", &new_description])
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_description() {
        let desc = r#"
Task: Implement user API
Priority: high
Status: in_progress
Agent: agent-42
Orchestrator: O-A-1
Msg-To: O-A-2
Msg-Type: mutation

## Progress
- Started implementation
"#;

        let metadata = MetadataManager::<crate::command::MockJjExecutor>::parse_description(desc);

        assert_eq!(metadata.priority, Some(Priority::High));
        assert_eq!(metadata.status, Some(TaskStatus::InProgress));
        assert_eq!(metadata.agent, Some("agent-42".to_string()));
        assert_eq!(metadata.orchestrator, Some("O-A-1".to_string()));
        assert_eq!(metadata.msg_to, Some("O-A-2".to_string()));
        assert_eq!(metadata.msg_type, Some(MessageType::Mutation));
    }

    #[test]
    fn test_format_metadata() {
        let metadata = HoxMetadata::new()
            .with_priority(Priority::Critical)
            .with_status(TaskStatus::Open)
            .with_orchestrator("O-A-1");

        let formatted = MetadataManager::<crate::command::MockJjExecutor>::format_metadata(&metadata);

        assert!(formatted.contains("Priority: critical"));
        assert!(formatted.contains("Status: open"));
        assert!(formatted.contains("Orchestrator: O-A-1"));
    }
}
