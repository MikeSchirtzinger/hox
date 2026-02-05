//! Hox metadata management for JJ changes
//!
//! This module provides helpers for reading and writing Hox metadata
//! on JJ changes using JJ trailers. Trailers are key-value pairs at the
//! end of commit descriptions in the format `Key: value`.

use hox_core::{ChangeId, HoxMetadata, MessageType, Priority, Result, TaskStatus};

use crate::command::JjExecutor;

/// Standard Hox trailer keys (without prefix)
pub mod trailers {
    /// Agent identifier trailer key
    pub const AGENT: &str = "Agent";
    /// Status trailer key
    pub const STATUS: &str = "Status";
    /// Priority trailer key
    pub const PRIORITY: &str = "Priority";
    /// Orchestrator identifier trailer key
    pub const ORCHESTRATOR: &str = "Orchestrator";
    /// Message recipient trailer key
    pub const MSG_TO: &str = "Msg-To";
    /// Message type trailer key
    pub const MSG_TYPE: &str = "Msg-Type";
    /// Phase number trailer key
    pub const PHASE: &str = "Phase";
    /// Task description trailer key
    pub const TASK: &str = "Task";
    /// Change ID trailer key
    pub const CHANGE: &str = "Change";

    // Legacy prefixed trailer keys (for backward compatibility)
    pub const LEGACY_AGENT: &str = "Hox-Agent";
    pub const LEGACY_STATUS: &str = "Hox-Status";
    pub const LEGACY_PRIORITY: &str = "Hox-Priority";
    pub const LEGACY_ORCHESTRATOR: &str = "Hox-Orchestrator";
    pub const LEGACY_MSG_TO: &str = "Hox-Msg-To";
    pub const LEGACY_MSG_TYPE: &str = "Hox-Msg-Type";
    pub const LEGACY_PHASE: &str = "Hox-Phase";
    pub const LEGACY_TASK: &str = "Hox-Task";
    pub const LEGACY_CHANGE: &str = "Hox-Change";
}

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

        let lines: Vec<&str> = description.lines().collect();

        // Determine trailer sections:
        // Git trailers appear at the end after a blank line separator, BUT
        // we also need to support trailers at the beginning (common in our codebase).
        // To avoid parsing body text like "The Agent: foo is responsible...",
        // we only parse from:
        // 1. The beginning up to the first non-trailer line or blank line
        // 2. After the last blank line to the end

        // Find first non-empty line
        let first_non_empty = lines.iter().position(|line| !line.trim().is_empty());

        // Find blank lines that could separate sections
        let blank_positions: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.trim().is_empty())
            .map(|(i, _)| i)
            .collect();

        let mut trailer_ranges: Vec<std::ops::Range<usize>> = Vec::new();

        // Check for trailers at the beginning (before first blank line that appears AFTER content starts)
        if let Some(first_content) = first_non_empty {
            // Find first blank line that comes after the first content line
            let first_blank_after_content = blank_positions
                .iter()
                .find(|&&pos| pos > first_content);

            if let Some(&first_blank) = first_blank_after_content {
                trailer_ranges.push(first_content..first_blank);
            } else {
                // No blank lines after content, parse entire content
                trailer_ranges.push(first_content..lines.len());
            }
        }

        // Check for trailers at the end (after last blank line)
        // Only add if different from the beginning range
        if let Some(&last_blank) = blank_positions.last() {
            if last_blank + 1 < lines.len() {
                let end_range = (last_blank + 1)..lines.len();
                // Avoid duplicate ranges
                if trailer_ranges.is_empty() || trailer_ranges[0] != end_range {
                    trailer_ranges.push(end_range);
                }
            }
        }

        for range in trailer_ranges {
            for line in &lines[range] {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Split on first colon to separate key from value
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();

                    match key {
                        trailers::PRIORITY | trailers::LEGACY_PRIORITY => {
                            if let Ok(p) = value.parse::<Priority>() {
                                metadata.priority = Some(p);
                            }
                        }
                        trailers::STATUS | trailers::LEGACY_STATUS => {
                            if let Ok(s) = value.parse::<TaskStatus>() {
                                metadata.status = Some(s);
                            }
                        }
                        trailers::AGENT | trailers::LEGACY_AGENT => {
                            metadata.agent = Some(value.to_string());
                        }
                        trailers::ORCHESTRATOR | trailers::LEGACY_ORCHESTRATOR => {
                            metadata.orchestrator = Some(value.to_string());
                        }
                        trailers::MSG_TO | trailers::LEGACY_MSG_TO => {
                            metadata.msg_to = Some(value.to_string());
                        }
                        trailers::MSG_TYPE | trailers::LEGACY_MSG_TYPE => {
                            if let Ok(t) = value.parse::<MessageType>() {
                                metadata.msg_type = Some(t);
                            }
                        }
                        _ => {} // Ignore other trailers
                    }
                }
            }
        }

        metadata
    }

    /// Format Hox metadata as description lines
    pub fn format_metadata(metadata: &HoxMetadata) -> String {
        let mut lines = Vec::new();

        if let Some(priority) = &metadata.priority {
            lines.push(format!("{}: {}", trailers::PRIORITY, priority));
        }

        if let Some(status) = &metadata.status {
            lines.push(format!("{}: {}", trailers::STATUS, status));
        }

        if let Some(agent) = &metadata.agent {
            lines.push(format!("{}: {}", trailers::AGENT, agent));
        }

        if let Some(orchestrator) = &metadata.orchestrator {
            lines.push(format!("{}: {}", trailers::ORCHESTRATOR, orchestrator));
        }

        if let Some(msg_to) = &metadata.msg_to {
            lines.push(format!("{}: {}", trailers::MSG_TO, msg_to));
        }

        if let Some(msg_type) = &metadata.msg_type {
            lines.push(format!("{}: {}", trailers::MSG_TYPE, msg_type));
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
    /// Note: This updates the change description to include metadata trailers.
    pub async fn set(&self, change_id: &ChangeId, metadata: &HoxMetadata) -> Result<()> {
        // First read existing description
        let output = self
            .executor
            .exec(&["log", "-r", change_id, "-T", "description", "--no-graph"])
            .await?;

        let existing = output.stdout.trim();

        // Remove existing metadata trailer lines
        let cleaned: Vec<&str> = existing
            .lines()
            .filter(|line| {
                let line = line.trim();
                // Check if line starts with any of our trailer keys followed by ':'
                !line.starts_with(&format!("{}:", trailers::PRIORITY))
                    && !line.starts_with(&format!("{}:", trailers::STATUS))
                    && !line.starts_with(&format!("{}:", trailers::AGENT))
                    && !line.starts_with(&format!("{}:", trailers::ORCHESTRATOR))
                    && !line.starts_with(&format!("{}:", trailers::MSG_TO))
                    && !line.starts_with(&format!("{}:", trailers::MSG_TYPE))
                    // Also strip legacy format
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_PRIORITY))
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_STATUS))
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_AGENT))
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_ORCHESTRATOR))
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_MSG_TO))
                    && !line.starts_with(&format!("{}:", trailers::LEGACY_MSG_TYPE))
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

    #[test]
    fn test_parse_legacy_trailers() {
        let desc = r#"Task: Implement user API

Hox-Priority: high
Hox-Status: in_progress
Hox-Agent: agent-42
Hox-Orchestrator: O-A-1
"#;

        let metadata = MetadataManager::<crate::command::MockJjExecutor>::parse_description(desc);

        assert_eq!(metadata.priority, Some(Priority::High));
        assert_eq!(metadata.status, Some(TaskStatus::InProgress));
        assert_eq!(metadata.agent, Some("agent-42".to_string()));
        assert_eq!(metadata.orchestrator, Some("O-A-1".to_string()));
    }

    #[test]
    fn test_body_text_not_parsed_as_trailer() {
        let desc = r#"Implement Agent: foo handler for bar

The Agent: field should not be parsed from body text.

Agent: agent-42
Status: open
"#;

        let metadata = MetadataManager::<crate::command::MockJjExecutor>::parse_description(desc);

        // Should parse trailer section Agent, not body text Agent
        assert_eq!(metadata.agent, Some("agent-42".to_string()));
        assert_eq!(metadata.status, Some(TaskStatus::Open));
    }
}
