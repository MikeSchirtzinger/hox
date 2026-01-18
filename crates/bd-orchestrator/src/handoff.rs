//! Agent handoff context generation for jj-based task orchestration.
//!
//! This module provides HandoffGenerator for creating structured handoff context
//! that enables seamless agent transitions with full task context preservation.

use crate::types::{AgentHandoff, ChangeEntry, HandoffContext, HandoffSummary, Priority, Task, TaskStatus};
use bd_core::{HoxError, Result};
use std::process::Command;
use std::str::FromStr;
use tracing::{debug, info, instrument, warn};

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
    #[instrument(skip(self, summary), fields(change_id))]
    pub async fn generate_handoff(
        &self,
        change_id: &str,
        summary: HandoffSummary,
    ) -> Result<()> {
        info!("Generating handoff context");
        let handoff = summary.into_context();
        self.update_handoff(change_id, &handoff).await
    }

    /// Load handoff context for a task.
    #[instrument(skip(self), fields(change_id))]
    pub async fn load_handoff(&self, change_id: &str) -> Result<HandoffContext> {
        debug!("Loading handoff context");
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
        .map_err(|e| HoxError::JjError(format!("failed to get description: {}", e)))?;

        let task = self.parse_description(&desc)?;

        task.context
            .ok_or_else(|| HoxError::Parse(format!("no handoff context found for change {}", change_id)))
    }

    /// Get cumulative diff for a change (for new agent context).
    #[instrument(skip(self), fields(change_id))]
    pub async fn get_diff(&self, change_id: &str) -> Result<String> {
        debug!("Getting cumulative diff");
        let output = self
            .exec_jj(&["diff", "-r", &format!("root()..{}", change_id)])
            .await
            .map_err(|e| HoxError::JjError(format!("failed to get diff: {}", e)))?;

        Ok(output)
    }

    /// Get change history for context.
    #[instrument(skip(self), fields(change_id))]
    pub async fn get_change_log(&self, change_id: &str) -> Result<Vec<ChangeEntry>> {
        debug!("Getting change log");
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
            .map_err(|e| HoxError::JjError(format!("failed to get change log: {}", e)))?;

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
    #[instrument(skip(self), fields(change_id))]
    pub async fn prepare_handoff(&self, change_id: &str) -> Result<AgentHandoff> {
        info!("Preparing complete handoff package");

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
            .map_err(|e| HoxError::JjError(format!("failed to get description: {}", e)))?;

        let mut task = self.parse_description(&desc)?;
        task.change_id = change_id.to_string();

        // Get diff
        let diff = self.get_diff(change_id).await.unwrap_or_else(|e| {
            warn!("failed to get diff: {}", e);
            "(failed to get diff)".to_string()
        });

        // Get parent changes
        let parent_changes = self.get_change_log(change_id).await
            .map(|entries| entries.into_iter().map(|e| e.change_id).collect())
            .unwrap_or_else(|e| {
                warn!("failed to get parent changes: {}", e);
                Vec::new()
            });

        let context = task.context.clone();

        Ok(AgentHandoff {
            task,
            context,
            diff,
            parent_changes,
        })
    }

    /// Update handoff context for a task.
    #[instrument(skip(self, handoff), fields(change_id))]
    async fn update_handoff(&self, change_id: &str, handoff: &HandoffContext) -> Result<()> {
        debug!("Updating handoff context");

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
            .map_err(|e| HoxError::JjError(format!("failed to get description: {}", e)))?;

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
        .await
        .map_err(|e| HoxError::JjError(format!("failed to update description: {}", e)))?;

        Ok(())
    }

    /// Parse task from structured description.
    #[instrument(skip(self, desc))]
    fn parse_description(&self, desc: &str) -> Result<Task> {
        let mut task = Task::new("", "");
        task.context = Some(HandoffContext::new(""));

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
                task.status = TaskStatus::from_str(status).unwrap_or(TaskStatus::Open);
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

    /// Execute a jj command and return stdout.
    #[instrument(skip(self, args), fields(command = %args.join(" ")))]
    async fn exec_jj(&self, args: &[&str]) -> Result<String> {
        debug!("Executing jj command");

        let output = Command::new("jj")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| HoxError::JjError(format!("failed to execute jj: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HoxError::JjError(format!("jj command failed: {}", stderr)));
        }

        String::from_utf8(output.stdout)
            .map_err(|e| HoxError::Parse(format!("invalid UTF-8 in jj output: {}", e)))
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
        assert!(task.bookmark.is_none());

        let context = task.context.unwrap();
        assert_eq!(context.current_focus, "Working on the authentication module");
        assert_eq!(context.progress.len(), 2);
        assert_eq!(context.next_steps.len(), 2);
        assert_eq!(context.blockers.as_ref().unwrap().len(), 1);
        assert_eq!(context.files_touched.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_format_description() {
        let mut task = Task::new("abc123", "Test Task");
        task.priority = Priority::Medium;
        task.status = TaskStatus::Open;
        task.agent = Some("test-agent".to_string());

        let mut ctx = HandoffContext::new("Testing handoff");
        ctx.add_progress("Step 1");
        ctx.add_next_step("Step 2");
        task.context = Some(ctx);

        let formatted = task.format_description();
        assert!(formatted.contains("Task: Test Task"));
        assert!(formatted.contains("Priority: 2"));
        assert!(formatted.contains("Status: open"));
        assert!(formatted.contains("Agent: test-agent"));
        assert!(formatted.contains("Testing handoff"));
        assert!(formatted.contains("- [x] Step 1"));
        assert!(formatted.contains("- [ ] Step 2"));
    }
}
