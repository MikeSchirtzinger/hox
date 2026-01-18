//! Core types for JJ-native task orchestration.
//!
//! This module defines the unified type system for the hox orchestration system.
//! The key paradigm is:
//! - Tasks ARE jj changes (the change ID is the primary identifier)
//! - Dependencies ARE ancestry in the jj DAG
//! - Assignments ARE bookmarks
//!
//! This design eliminates the need for a separate dependency graph in SQLite.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ============================================================================
// TASK STATUS
// ============================================================================

/// Task status values for the JJ-native workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is open and ready for work
    #[default]
    Open,
    /// Task is currently being worked on
    InProgress,
    /// Task is blocked by dependencies or external factors
    Blocked,
    /// Task is awaiting review
    Review,
    /// Task has been completed
    Done,
    /// Task has been abandoned/cancelled
    Abandoned,
}

impl TaskStatus {
    /// Returns true if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Abandoned)
    }

    /// Returns true if this status indicates the task is actionable.
    pub fn is_actionable(&self) -> bool {
        matches!(self, TaskStatus::Open | TaskStatus::InProgress)
    }

    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Open => "open",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
            TaskStatus::Abandoned => "abandoned",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = crate::error::HoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(TaskStatus::Open),
            "in_progress" | "inprogress" | "wip" => Ok(TaskStatus::InProgress),
            "blocked" => Ok(TaskStatus::Blocked),
            "review" | "in_review" => Ok(TaskStatus::Review),
            "done" | "completed" | "closed" => Ok(TaskStatus::Done),
            "abandoned" | "cancelled" | "canceled" => Ok(TaskStatus::Abandoned),
            _ => Err(crate::error::HoxError::ValidationError(format!(
                "invalid task status: {}",
                s
            ))),
        }
    }
}

// ============================================================================
// PRIORITY
// ============================================================================

/// Priority levels for tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Critical priority - drop everything (P0)
    Critical = 0,
    /// High priority - do soon (P1)
    High = 1,
    /// Medium priority - normal work (P2)
    #[default]
    Medium = 2,
    /// Low priority - nice to have (P3)
    Low = 3,
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
            _ => None,
        }
    }

    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Critical => "critical",
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Priority {
    type Err = crate::error::HoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "critical" | "0" | "p0" => Ok(Priority::Critical),
            "high" | "1" | "p1" => Ok(Priority::High),
            "medium" | "2" | "p2" => Ok(Priority::Medium),
            "low" | "3" | "p3" => Ok(Priority::Low),
            _ => Err(crate::error::HoxError::ValidationError(format!(
                "invalid priority: {}",
                s
            ))),
        }
    }
}

// ============================================================================
// TASK
// ============================================================================

/// Core task representation for JJ-native orchestration.
///
/// A Task IS a jj change - the change_id is the primary identifier.
/// Dependencies are expressed through jj ancestry, not stored separately.
/// Agent assignments use jj bookmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// JJ change ID (the primary identifier)
    pub change_id: String,

    /// Human-readable title
    pub title: String,

    /// Detailed description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Current status
    pub status: TaskStatus,

    /// Priority level
    pub priority: Priority,

    /// Assigned agent (bookmark name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// JJ bookmark for this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,

    /// Labels for categorization
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    /// Due date for time-sensitive tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,

    /// Handoff context for agent transitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HandoffContext>,

    /// When this task was created
    pub created_at: DateTime<Utc>,

    /// When this task was last updated
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Create a new task with minimal required fields.
    pub fn new(change_id: impl Into<String>, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            change_id: change_id.into(),
            title: title.into(),
            description: None,
            status: TaskStatus::default(),
            priority: Priority::default(),
            agent: None,
            bookmark: None,
            labels: Vec::new(),
            due_date: None,
            context: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Returns true if this task is assigned to an agent.
    pub fn is_assigned(&self) -> bool {
        self.agent.is_some()
    }

    /// Returns true if this task is in a terminal state.
    pub fn is_complete(&self) -> bool {
        self.status.is_terminal()
    }

    /// Update the updated_at timestamp to now.
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Format task as a structured jj change description.
    #[tracing::instrument(skip(self), fields(change_id = %self.change_id))]
    pub fn format_description(&self) -> String {
        let agent = self.agent.as_deref().unwrap_or("unassigned");

        let context_str = self
            .context
            .as_ref()
            .map(|c| c.current_focus.as_str())
            .unwrap_or("");

        let progress_str = self
            .context
            .as_ref()
            .and_then(|c| {
                if c.progress.is_empty() {
                    None
                } else {
                    Some(
                        c.progress
                            .iter()
                            .map(|p| format!("- [x] {}", p))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let next_steps_str = self
            .context
            .as_ref()
            .and_then(|c| {
                if c.next_steps.is_empty() {
                    None
                } else {
                    Some(
                        c.next_steps
                            .iter()
                            .map(|s| format!("- [ ] {}", s))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let blockers_str = self
            .context
            .as_ref()
            .and_then(|c| c.blockers.as_ref())
            .and_then(|blockers| {
                if blockers.is_empty() {
                    None
                } else {
                    Some(
                        blockers
                            .iter()
                            .map(|b| format!("- {}", b))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let files_str = self
            .context
            .as_ref()
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
            "Task: {}\nPriority: {}\nStatus: {}\nAgent: {}\n\n## Context\n{}\n\n## Progress\n{}\n\n## Next Steps\n{}\n\n## Blockers\n{}\n\n## Files Touched\n{}",
            self.title,
            self.priority.as_i32(),
            self.status.as_str(),
            agent,
            context_str,
            progress_str,
            next_steps_str,
            blockers_str,
            files_str
        )
    }

    /// Validate the task has required fields.
    pub fn validate(&self) -> crate::Result<()> {
        if self.change_id.is_empty() {
            return Err(crate::error::HoxError::ValidationError(
                "change_id is required".to_string(),
            ));
        }
        if self.title.is_empty() {
            return Err(crate::error::HoxError::ValidationError(
                "title is required".to_string(),
            ));
        }
        if self.title.len() > 500 {
            return Err(crate::error::HoxError::ValidationError(format!(
                "title must be 500 characters or less (got {})",
                self.title.len()
            )));
        }
        Ok(())
    }
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.change_id == other.change_id
    }
}

impl Eq for Task {}

impl std::hash::Hash for Task {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.change_id.hash(state);
    }
}

// ============================================================================
// HANDOFF CONTEXT
// ============================================================================

/// Handoff context captures agent state for seamless transitions.
///
/// When an agent needs to hand off a task (context switch, stuck, etc.),
/// this structure preserves the working state so another agent can continue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffContext {
    /// What the agent was working on (current focus area)
    pub current_focus: String,

    /// List of completed items
    #[serde(default)]
    pub progress: Vec<String>,

    /// Immediate next actions to take
    #[serde(default)]
    pub next_steps: Vec<String>,

    /// Blocking issues (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockers: Option<Vec<String>>,

    /// Recently modified files
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_touched: Option<Vec<String>>,

    /// When this context was last updated
    pub updated_at: DateTime<Utc>,
}

impl HandoffContext {
    /// Create a new handoff context.
    pub fn new(current_focus: impl Into<String>) -> Self {
        Self {
            current_focus: current_focus.into(),
            progress: Vec::new(),
            next_steps: Vec::new(),
            blockers: None,
            files_touched: None,
            updated_at: Utc::now(),
        }
    }

    /// Add a completed item.
    pub fn add_progress(&mut self, item: impl Into<String>) {
        self.progress.push(item.into());
        self.updated_at = Utc::now();
    }

    /// Add a next step.
    pub fn add_next_step(&mut self, step: impl Into<String>) {
        self.next_steps.push(step.into());
        self.updated_at = Utc::now();
    }

    /// Add a blocker.
    pub fn add_blocker(&mut self, blocker: impl Into<String>) {
        self.blockers
            .get_or_insert_with(Vec::new)
            .push(blocker.into());
        self.updated_at = Utc::now();
    }

    /// Add a file to the touched list.
    pub fn add_file(&mut self, file: impl Into<String>) {
        self.files_touched
            .get_or_insert_with(Vec::new)
            .push(file.into());
        self.updated_at = Utc::now();
    }
}

impl Default for HandoffContext {
    fn default() -> Self {
        Self::new("")
    }
}

// ============================================================================
// AGENT HANDOFF
// ============================================================================

/// Extended task info for agent handoffs.
///
/// Contains everything a new agent needs to continue work on a task,
/// including the diff, history, and context from the previous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHandoff {
    /// The task being handed off
    pub task: Task,

    /// Structured handoff context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HandoffContext>,

    /// Cumulative code changes (diff from root)
    pub diff: String,

    /// Parent change IDs (for understanding ancestry)
    #[serde(default)]
    pub parent_changes: Vec<String>,
}

impl AgentHandoff {
    /// Create a new agent handoff.
    pub fn new(task: Task) -> Self {
        Self {
            context: task.context.clone(),
            task,
            diff: String::new(),
            parent_changes: Vec::new(),
        }
    }

    /// Format the handoff as a prompt for the new agent.
    #[tracing::instrument(skip(self), fields(change_id = %self.task.change_id))]
    pub fn format_for_agent(&self) -> String {
        let mut output = String::new();

        output.push_str("# Agent Handoff Context\n\n");

        // Task info
        output.push_str("## Task\n");
        output.push_str(&format!("**Title:** {}\n", self.task.title));
        output.push_str(&format!("**Priority:** {}\n", self.task.priority));
        output.push_str(&format!("**Status:** {}\n", self.task.status));
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

        // Parent changes for context
        if !self.parent_changes.is_empty() {
            output.push_str("## Parent Changes\n");
            for change in &self.parent_changes {
                let short_id = if change.len() >= 8 {
                    &change[..8]
                } else {
                    change
                };
                output.push_str(&format!("- `{}`\n", short_id));
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

// ============================================================================
// CHANGE ENTRY
// ============================================================================

/// A single entry in the change log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    /// JJ change ID
    pub change_id: String,
    /// First line of the description
    pub description: String,
}

// ============================================================================
// TASK METADATA
// ============================================================================

/// Task metadata stored in .tasks/metadata.jsonl for non-DAG data.
///
/// This supplements the jj change description with structured metadata
/// that doesn't fit naturally in a commit message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMetadata {
    /// JJ change ID
    pub change_id: String,

    /// Priority level
    pub priority: Priority,

    /// Labels for categorization
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    /// Due date for time-sensitive tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,

    /// Assigned agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// When the task was created
    pub created_at: DateTime<Utc>,

    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
}

impl TaskMetadata {
    /// Create new metadata for a task.
    pub fn new(change_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            change_id: change_id.into(),
            priority: Priority::default(),
            labels: Vec::new(),
            due_date: None,
            agent: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Update the updated_at timestamp to now.
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_serialization() {
        let status = TaskStatus::InProgress;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"in_progress\"");

        let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, TaskStatus::InProgress);
    }

    #[test]
    fn test_task_status_from_str() {
        assert_eq!("open".parse::<TaskStatus>().unwrap(), TaskStatus::Open);
        assert_eq!(
            "in_progress".parse::<TaskStatus>().unwrap(),
            TaskStatus::InProgress
        );
        assert_eq!("wip".parse::<TaskStatus>().unwrap(), TaskStatus::InProgress);
        assert_eq!("done".parse::<TaskStatus>().unwrap(), TaskStatus::Done);
        assert_eq!(
            "completed".parse::<TaskStatus>().unwrap(),
            TaskStatus::Done
        );
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Medium);
        assert!(Priority::Medium < Priority::Low);
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
    fn test_priority_from_str() {
        assert_eq!("critical".parse::<Priority>().unwrap(), Priority::Critical);
        assert_eq!("p0".parse::<Priority>().unwrap(), Priority::Critical);
        assert_eq!("0".parse::<Priority>().unwrap(), Priority::Critical);
        assert_eq!("high".parse::<Priority>().unwrap(), Priority::High);
        assert_eq!("p1".parse::<Priority>().unwrap(), Priority::High);
    }

    #[test]
    fn test_task_new() {
        let task = Task::new("abc123", "Test Task");
        assert_eq!(task.change_id, "abc123");
        assert_eq!(task.title, "Test Task");
        assert_eq!(task.status, TaskStatus::Open);
        assert_eq!(task.priority, Priority::Medium);
        assert!(!task.is_assigned());
        assert!(!task.is_complete());
    }

    #[test]
    fn test_task_validation() {
        let mut task = Task::new("abc123", "Test Task");
        assert!(task.validate().is_ok());

        task.change_id = String::new();
        assert!(task.validate().is_err());

        task.change_id = "abc123".to_string();
        task.title = String::new();
        assert!(task.validate().is_err());

        task.title = "x".repeat(501);
        assert!(task.validate().is_err());
    }

    #[test]
    fn test_task_serialization() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            status: TaskStatus::InProgress,
            priority: Priority::High,
            agent: Some("agent-1".to_string()),
            bookmark: Some("task/test".to_string()),
            labels: vec!["test".to_string()],
            due_date: None,
            context: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.change_id, task.change_id);
        assert_eq!(deserialized.title, task.title);
        assert_eq!(deserialized.status, task.status);
        assert_eq!(deserialized.priority, task.priority);
    }

    #[test]
    fn test_handoff_context() {
        let mut ctx = HandoffContext::new("Working on feature X");
        ctx.add_progress("Completed step 1");
        ctx.add_next_step("Start step 2");
        ctx.add_blocker("Waiting for review");
        ctx.add_file("src/main.rs");

        assert_eq!(ctx.current_focus, "Working on feature X");
        assert_eq!(ctx.progress.len(), 1);
        assert_eq!(ctx.next_steps.len(), 1);
        assert_eq!(ctx.blockers.as_ref().unwrap().len(), 1);
        assert_eq!(ctx.files_touched.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_format_description() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            status: TaskStatus::InProgress,
            priority: Priority::High,
            agent: Some("agent-1".to_string()),
            bookmark: None,
            labels: Vec::new(),
            due_date: None,
            context: Some(HandoffContext {
                current_focus: "Working on tests".to_string(),
                progress: vec!["Completed setup".to_string()],
                next_steps: vec!["Add more tests".to_string()],
                blockers: None,
                files_touched: Some(vec!["src/lib.rs".to_string()]),
                updated_at: Utc::now(),
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let desc = task.format_description();
        assert!(desc.contains("Task: Test Task"));
        assert!(desc.contains("Priority: 1"));
        assert!(desc.contains("Status: in_progress"));
        assert!(desc.contains("Agent: agent-1"));
        assert!(desc.contains("Working on tests"));
        assert!(desc.contains("- [x] Completed setup"));
        assert!(desc.contains("- [ ] Add more tests"));
    }

    #[test]
    fn test_agent_handoff_format() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            status: TaskStatus::InProgress,
            priority: Priority::High,
            agent: Some("agent-1".to_string()),
            bookmark: None,
            labels: Vec::new(),
            due_date: None,
            context: Some(HandoffContext {
                current_focus: "Testing handoff".to_string(),
                progress: vec!["Step 1".to_string()],
                next_steps: vec!["Step 2".to_string()],
                blockers: None,
                files_touched: None,
                updated_at: Utc::now(),
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let handoff = AgentHandoff::new(task);
        let formatted = handoff.format_for_agent();

        assert!(formatted.contains("# Agent Handoff Context"));
        assert!(formatted.contains("**Title:** Test Task"));
        assert!(formatted.contains("**Priority:** high"));
        assert!(formatted.contains("Testing handoff"));
    }
}
