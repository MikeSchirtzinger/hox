//! Core types for jj-native task and agent orchestration.
//!
//! Instead of maintaining a separate dependency graph in SQLite, this module
//! uses jj's native change DAG as the task graph. Tasks are changes, dependencies
//! are ancestry, and assignments are bookmarks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Priority levels for tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Critical priority (0)
    Critical = 0,
    /// High priority (1)
    High = 1,
    /// Medium priority (2)
    Medium = 2,
    /// Low priority (3)
    Low = 3,
    /// Backlog priority (4)
    Backlog = 4,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Medium
    }
}

impl Priority {
    /// Convert to i32 for legacy compatibility.
    pub fn as_i32(&self) -> i32 {
        *self as i32
    }

    /// Parse from i32 for legacy compatibility.
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Priority::Critical),
            1 => Some(Priority::High),
            2 => Some(Priority::Medium),
            3 => Some(Priority::Low),
            4 => Some(Priority::Backlog),
            _ => None,
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "critical" | "0" => Ok(Priority::Critical),
            "high" | "1" => Ok(Priority::High),
            "medium" | "2" => Ok(Priority::Medium),
            "low" | "3" => Ok(Priority::Low),
            "backlog" | "4" => Ok(Priority::Backlog),
            _ => Err(anyhow::anyhow!("invalid priority: {}", s)),
        }
    }
}

/// Task status values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is pending and not yet started
    Pending,
    /// Task is currently being worked on
    InProgress,
    /// Task is blocked by dependencies or issues
    Blocked,
    /// Task has been completed
    Completed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

impl TaskStatus {
    /// Convert to string for display.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Completed => "completed",
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(TaskStatus::Pending),
            "in_progress" | "inprogress" => Ok(TaskStatus::InProgress),
            "blocked" => Ok(TaskStatus::Blocked),
            "completed" => Ok(TaskStatus::Completed),
            _ => Err(anyhow::anyhow!("invalid task status: {}", s)),
        }
    }
}

/// Task represents a work item tracked as a jj change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// The jj change ID (e.g., "xyzabc12")
    pub change_id: String,

    /// Task title
    pub title: String,

    /// Full task description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Priority level
    pub priority: Priority,

    /// Current status
    pub status: TaskStatus,

    /// Assigned agent ID (None if unassigned)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Labels for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,

    /// Due date for time-sensitive tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,

    /// Handoff context for agent continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HandoffContext>,

    /// The jj bookmark name for this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

impl Task {
    /// Format task description for jj change description.
    pub fn format_description(&self) -> String {
        let agent = self.agent.as_deref().unwrap_or("unassigned");

        let context_str = self.context.as_ref()
            .map(|c| c.current_focus.as_str())
            .unwrap_or("");

        let progress_str = self.context.as_ref()
            .and_then(|c| {
                if c.progress.is_empty() {
                    None
                } else {
                    Some(c.progress.iter()
                        .map(|p| format!("- [x] {}", p))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let next_steps_str = self.context.as_ref()
            .and_then(|c| {
                if c.next_steps.is_empty() {
                    None
                } else {
                    Some(c.next_steps.iter()
                        .map(|s| format!("- [ ] {}", s))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let blockers_str = self.context.as_ref()
            .and_then(|c| c.blockers.as_ref())
            .and_then(|blockers| {
                if blockers.is_empty() {
                    None
                } else {
                    Some(blockers.iter()
                        .map(|b| format!("- {}", b))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let questions_str = self.context.as_ref()
            .and_then(|c| c.open_questions.as_ref())
            .and_then(|questions| {
                if questions.is_empty() {
                    None
                } else {
                    Some(questions.iter()
                        .map(|q| format!("- {}", q))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let files_str = self.context.as_ref()
            .and_then(|c| c.files_touched.as_ref())
            .and_then(|files| {
                if files.is_empty() {
                    None
                } else {
                    Some(files.join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        format!(
            "Task: {}\nPriority: {}\nStatus: {}\nAgent: {}\n\n## Context\n{}\n\n## Progress\n{}\n\n## Next Steps\n{}\n\n## Blockers\n{}\n\n## Open Questions\n{}\n\n## Files Touched\n{}",
            self.title,
            self.priority.as_i32(),
            self.status.as_str(),
            agent,
            context_str,
            progress_str,
            next_steps_str,
            blockers_str,
            questions_str,
            files_str
        )
    }
}

/// HandoffContext captures agent state for seamless handoffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffContext {
    /// What the agent was working on
    pub current_focus: String,

    /// List of completed items
    #[serde(default)]
    pub progress: Vec<String>,

    /// Immediate next actions
    #[serde(default)]
    pub next_steps: Vec<String>,

    /// Blocking issues (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockers: Option<Vec<String>>,

    /// Unresolved decisions (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_questions: Option<Vec<String>>,

    /// Recently modified files (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_touched: Option<Vec<String>>,

    /// When this context was last updated
    pub updated_at: DateTime<Utc>,
}

/// HandoffSummary is the input for generating handoff context.
/// This would be produced by a summarization model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffSummary {
    pub current_focus: String,
    pub progress: Vec<String>,
    pub next_steps: Vec<String>,
    pub blockers: Vec<String>,
    pub open_questions: Vec<String>,
    pub files_touched: Vec<String>,
}

/// TaskMetadata is stored in .tasks/metadata.jsonl for non-DAG data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMetadata {
    /// The jj change ID
    pub change_id: String,

    /// Priority level
    pub priority: Priority,

    /// Labels for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,

    /// Due date for time-sensitive tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,

    /// Assigned agent ID (None if unassigned)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// When the task was created
    pub created_at: DateTime<Utc>,

    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
}

/// ChangeEntry represents a single change in the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    pub change_id: String,
    pub description: String,
}

/// AgentHandoff contains everything a new agent needs to continue work.
#[derive(Debug, Clone)]
pub struct AgentHandoff {
    /// Task is the task being handed off
    pub task: Task,

    /// Context is the structured handoff context
    pub context: Option<HandoffContext>,

    /// Diff is the cumulative code changes
    pub diff: String,

    /// History is the change log
    pub history: Vec<ChangeEntry>,

    /// Metadata is the task metadata
    pub metadata: Option<TaskMetadata>,
}

impl AgentHandoff {
    /// Format the handoff as a prompt for the new agent.
    pub fn format_for_agent(&self) -> String {
        let mut output = String::new();

        output.push_str("# Agent Handoff Context\n\n");

        // Task info
        output.push_str("## Task\n");
        output.push_str(&format!("**Title:** {}\n", self.task.title));
        output.push_str(&format!("**Priority:** {:?}\n", self.task.priority));
        output.push_str(&format!("**Status:** {:?}\n", self.task.status));
        if let Some(ref agent) = self.task.agent {
            output.push_str(&format!("**Previous Agent:** {}\n", agent));
        }
        output.push('\n');

        // Context
        if let Some(ref context) = self.context {
            output.push_str("## Where We Left Off\n");
            if !context.current_focus.is_empty() {
                output.push_str(&format!("{}\n\n", context.current_focus));
            }

            if !context.progress.is_empty() {
                output.push_str("### Completed\n");
                for p in &context.progress {
                    output.push_str(&format!("- [x] {}\n", p));
                }
                output.push('\n');
            }

            if !context.next_steps.is_empty() {
                output.push_str("### Next Steps\n");
                for s in &context.next_steps {
                    output.push_str(&format!("- [ ] {}\n", s));
                }
                output.push('\n');
            }

            if let Some(ref blockers) = context.blockers {
                if !blockers.is_empty() {
                    output.push_str("### Blockers\n");
                    for b in blockers {
                        output.push_str(&format!("- {}\n", b));
                    }
                    output.push('\n');
                }
            }

            if let Some(ref questions) = context.open_questions {
                if !questions.is_empty() {
                    output.push_str("### Open Questions\n");
                    for q in questions {
                        output.push_str(&format!("- {}\n", q));
                    }
                    output.push('\n');
                }
            }

            if let Some(ref files) = context.files_touched {
                if !files.is_empty() {
                    output.push_str("### Files Modified\n");
                    for f in files {
                        output.push_str(&format!("- {}\n", f));
                    }
                    output.push('\n');
                }
            }
        }

        // History summary
        if !self.history.is_empty() {
            output.push_str("## Change History\n");
            let max_history = 10.min(self.history.len());
            for entry in self.history.iter().take(max_history) {
                let short_id = if entry.change_id.len() >= 8 {
                    &entry.change_id[..8]
                } else {
                    &entry.change_id
                };
                output.push_str(&format!("- `{}`: {}\n", short_id, entry.description));
            }
            if self.history.len() > 10 {
                output.push_str(&format!("- ... and {} more changes\n", self.history.len() - 10));
            }
            output.push('\n');
        }

        // Diff summary (truncated for prompt)
        if !self.diff.is_empty() {
            output.push_str("## Code Changes\n");
            let lines: Vec<&str> = self.diff.lines().collect();
            if lines.len() > 100 {
                output.push_str("```diff\n");
                output.push_str(&lines[..100].join("\n"));
                output.push_str(&format!("\n... ({} more lines)\n", lines.len() - 100));
                output.push_str("```\n");
            } else {
                output.push_str("```diff\n");
                output.push_str(&self.diff);
                output.push_str("\n```\n");
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Medium);
        assert!(Priority::Medium < Priority::Low);
        assert!(Priority::Low < Priority::Backlog);
    }

    #[test]
    fn test_priority_serialization() {
        let priority = Priority::High;
        let json = serde_json::to_string(&priority).unwrap();
        assert_eq!(json, "\"high\"");

        let deserialized: Priority = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Priority::High);
    }

    #[test]
    fn test_task_status_serialization() {
        let status = TaskStatus::InProgress;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"in_progress\"");

        let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, TaskStatus::InProgress);
    }

    #[test]
    fn test_task_serialization() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            priority: Priority::High,
            status: TaskStatus::Pending,
            agent: None,
            labels: Some(vec!["test".to_string()]),
            due_date: None,
            context: None,
            bookmark: Some("task/test".to_string()),
        };

        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, task);
    }

    #[test]
    fn test_format_description() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            priority: Priority::High,
            status: TaskStatus::InProgress,
            agent: Some("agent-001".to_string()),
            labels: None,
            due_date: None,
            context: Some(HandoffContext {
                current_focus: "Working on tests".to_string(),
                progress: vec!["Completed setup".to_string()],
                next_steps: vec!["Add more tests".to_string()],
                blockers: None,
                open_questions: None,
                files_touched: Some(vec!["src/lib.rs".to_string()]),
                updated_at: Utc::now(),
            }),
            bookmark: None,
        };

        let desc = task.format_description();
        assert!(desc.contains("Test Task"));
        assert!(desc.contains("agent-001"));
        assert!(desc.contains("Working on tests"));
        assert!(desc.contains("Completed setup"));
    }
}
