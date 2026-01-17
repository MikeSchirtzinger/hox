//! Agent handoff context generation for jj-based task orchestration.
//!
//! This module provides HandoffGenerator for creating structured handoff context
//! that enables seamless agent transitions with full task context preservation.

use crate::types::{AgentHandoff, ChangeEntry, HandoffContext, HandoffSummary, Priority, Task, TaskMetadata, TaskStatus};
use anyhow::{Context as AnyhowContext, Result};
use chrono::Utc;
use std::process::Command;
use std::str::FromStr;
use tracing::{debug, warn};

/// HandoffGenerator creates structured handoff context for agent transitions.
pub struct HandoffGenerator {
    repo_root: String,
}

impl HandoffGenerator {
    /// Create a new HandoffGenerator for the given repository.
    pub fn new(repo_root: impl Into<String>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Generate handoff context from a task's current state.
    /// This would typically be called by a summarization model (e.g., Haiku).
    pub async fn generate_handoff(
        &self,
        change_id: &str,
        summary: HandoffSummary,
    ) -> Result<()> {
        let handoff = HandoffContext {
            current_focus: summary.current_focus,
            progress: summary.progress,
            next_steps: summary.next_steps,
            blockers: if summary.blockers.is_empty() { None } else { Some(summary.blockers) },
            open_questions: if summary.open_questions.is_empty() { None } else { Some(summary.open_questions) },
            files_touched: if summary.files_touched.is_empty() { None } else { Some(summary.files_touched) },
            updated_at: Utc::now(),
        };

        self.update_handoff(change_id, &handoff).await
    }

    /// Load handoff context for a task.
    pub async fn load_handoff(&self, change_id: &str) -> Result<HandoffContext> {
        let desc = self.exec_jj(&[
            "log",
            "-r",
            change_id,
            "-n",
            "1",
            "--no-graph",
            "-T",
            "description",
        ])
        .await
        .context("failed to get description")?;

        let task = self.parse_description(&desc)?;

        task.context
            .ok_or_else(|| anyhow::anyhow!("no handoff context found for change {}", change_id))
    }

    /// Get cumulative diff for a change (for new agent context).
    pub async fn get_diff(&self, change_id: &str) -> Result<String> {
        let output = self
            .exec_jj(&["diff", "-r", &format!("root()..{}", change_id)])
            .await
            .context("failed to get diff")?;

        Ok(output)
    }

    /// Get change history for context.
    pub async fn get_change_log(&self, change_id: &str) -> Result<Vec<ChangeEntry>> {
        let output = self
            .exec_jj(&[
                "log",
                "-r",
                &format!("ancestors({})", change_id),
                "--no-graph",
                "-T",
                r#"change_id ++ "|" ++ description.first_line() ++ "\n""#,
            ])
            .await
            .context("failed to get change log")?;

        let mut entries = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() < 2 {
                continue;
            }

            entries.push(ChangeEntry {
                change_id: parts[0].trim().to_string(),
                description: parts[1].trim().to_string(),
            });
        }

        Ok(entries)
    }

    /// Prepare complete handoff context for a new agent taking over a task.
    pub async fn prepare_handoff(&self, change_id: &str) -> Result<AgentHandoff> {
        // Get task description
        let desc = self
            .exec_jj(&[
                "log",
                "-r",
                change_id,
                "-n",
                "1",
                "--no-graph",
                "-T",
                "description",
            ])
            .await
            .context("failed to get description")?;

        let mut task = self.parse_description(&desc)?;
        task.change_id = change_id.to_string();

        // Get diff
        let diff = self.get_diff(change_id).await.unwrap_or_else(|e| {
            warn!("failed to get diff: {}", e);
            "(failed to get diff)".to_string()
        });

        // Get history
        let history = self.get_change_log(change_id).await.unwrap_or_else(|e| {
            warn!("failed to get change log: {}", e);
            Vec::new()
        });

        // Get metadata (placeholder - would load from .tasks/metadata.jsonl)
        let metadata = self.load_metadata(change_id).await.ok();

        Ok(AgentHandoff {
            context: task.context.clone(),
            task,
            diff,
            history,
            metadata,
        })
    }

    /// Update handoff context for a task.
    async fn update_handoff(&self, change_id: &str, handoff: &HandoffContext) -> Result<()> {
        // Get current description
        let desc = self
            .exec_jj(&[
                "log",
                "-r",
                change_id,
                "-n",
                "1",
                "--no-graph",
                "-T",
                "description",
            ])
            .await
            .context("failed to get description")?;

        // Parse and update
        let mut task = self.parse_description(&desc)?;
        task.context = Some(handoff.clone());

        // Update the change description
        self.exec_jj(&[
            "describe",
            "-r",
            change_id,
            "-m",
            &task.format_description(),
        ])
        .await?;

        Ok(())
    }

    /// Parse task from structured description.
    fn parse_description(&self, desc: &str) -> Result<Task> {
        let mut task = Task {
            change_id: String::new(),
            title: String::new(),
            description: None,
            priority: Priority::Medium,
            status: TaskStatus::Pending,
            agent: None,
            labels: None,
            due_date: None,
            context: Some(HandoffContext {
                current_focus: String::new(),
                progress: Vec::new(),
                next_steps: Vec::new(),
                blockers: None,
                open_questions: None,
                files_touched: None,
                updated_at: Utc::now(),
            }),
            bookmark: None,
        };

        let lines: Vec<&str> = desc.lines().collect();
        let mut current_section = "";

        for line in lines {
            let line = line.trim();

            // Parse header fields
            if let Some(title) = line.strip_prefix("Task: ") {
                task.title = title.to_string();
                continue;
            }
            if let Some(priority) = line.strip_prefix("Priority: ") {
                task.priority = Priority::from_str(priority).unwrap_or(Priority::Medium);
                continue;
            }
            if let Some(status) = line.strip_prefix("Status: ") {
                task.status = TaskStatus::from_str(status).unwrap_or(TaskStatus::Pending);
                continue;
            }
            if let Some(agent) = line.strip_prefix("Agent: ") {
                if agent != "unassigned" {
                    task.agent = Some(agent.to_string());
                }
                continue;
            }

            // Track sections
            if let Some(section) = line.strip_prefix("## ") {
                current_section = section;
                continue;
            }

            // Parse section content
            if line.is_empty() || line == "None" {
                continue;
            }

            let context = task.context.as_mut().unwrap();

            match current_section {
                "Context" => {
                    if context.current_focus.is_empty() {
                        context.current_focus = line.to_string();
                    } else {
                        context.current_focus.push('\n');
                        context.current_focus.push_str(line);
                    }
                }
                "Progress" => {
                    if let Some(item) = line.strip_prefix("- [x] ") {
                        context.progress.push(item.to_string());
                    }
                }
                "Next Steps" => {
                    if let Some(item) = line.strip_prefix("- [ ] ") {
                        context.next_steps.push(item.to_string());
                    }
                }
                "Blockers" => {
                    if let Some(item) = line.strip_prefix("- ") {
                        context.blockers.get_or_insert_with(Vec::new).push(item.to_string());
                    }
                }
                "Open Questions" => {
                    if let Some(item) = line.strip_prefix("- ") {
                        context.open_questions.get_or_insert_with(Vec::new).push(item.to_string());
                    }
                }
                "Files Touched" => {
                    if !line.is_empty() {
                        context.files_touched.get_or_insert_with(Vec::new).push(line.to_string());
                    }
                }
                _ => {}
            }
        }

        Ok(task)
    }

    /// Load task metadata from .tasks/metadata.jsonl (placeholder).
    async fn load_metadata(&self, _change_id: &str) -> Result<TaskMetadata> {
        // This is a placeholder - in a full implementation, this would:
        // 1. Read .tasks/metadata.jsonl
        // 2. Parse JSONL entries
        // 3. Find the entry for this change_id
        // For now, return an error indicating metadata is not available
        Err(anyhow::anyhow!("metadata loading not yet implemented"))
    }

    /// Execute a jj command and return stdout.
    async fn exec_jj(&self, args: &[&str]) -> Result<String> {
        debug!("executing jj command: jj {}", args.join(" "));

        let output = Command::new("jj")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .context("failed to execute jj command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("jj command failed: {}", stderr);
        }

        Ok(String::from_utf8(output.stdout)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_description() {
        let desc = r#"Task: Implement feature X
Priority: 1
Status: in_progress
Agent: agent-1

## Context
Working on the authentication module

## Progress
- [x] Set up database schema
- [x] Implement login endpoint

## Next Steps
- [ ] Add password hashing
- [ ] Implement logout

## Blockers
- Waiting for API key

## Open Questions
- Should we use JWT or sessions?

## Files Touched
src/auth.rs
src/db.rs
"#;

        let generator = HandoffGenerator::new(".");
        let task = generator.parse_description(desc).unwrap();

        assert_eq!(task.title, "Implement feature X");
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.agent, Some("agent-1".to_string()));

        let context = task.context.unwrap();
        assert_eq!(context.current_focus, "Working on the authentication module");
        assert_eq!(context.progress.len(), 2);
        assert_eq!(context.next_steps.len(), 2);
        assert_eq!(context.blockers.as_ref().unwrap().len(), 1);
        assert_eq!(context.open_questions.as_ref().unwrap().len(), 1);
        assert_eq!(context.files_touched.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_format_description() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            priority: Priority::Medium,
            status: TaskStatus::Pending,
            agent: Some("test-agent".to_string()),
            labels: None,
            due_date: None,
            context: Some(HandoffContext {
                current_focus: "Testing handoff".to_string(),
                progress: vec!["Step 1".to_string()],
                next_steps: vec!["Step 2".to_string()],
                blockers: None,
                open_questions: None,
                files_touched: None,
                updated_at: Utc::now(),
            }),
            bookmark: None,
        };

        let formatted = task.format_description();
        assert!(formatted.contains("Task: Test Task"));
        assert!(formatted.contains("Priority: 2"));
        assert!(formatted.contains("Status: pending"));
        assert!(formatted.contains("Agent: test-agent"));
        assert!(formatted.contains("Testing handoff"));
        assert!(formatted.contains("- [x] Step 1"));
        assert!(formatted.contains("- [ ] Step 2"));
    }
}
