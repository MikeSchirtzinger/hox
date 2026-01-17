//! Task management using jj changes as the task graph.
//!
//! This module provides jj-native task orchestration where:
//! - Tasks are jj changes
//! - Dependencies are ancestry in the change DAG
//! - Assignments are bookmarks
//! - Metadata is stored in .tasks/metadata.jsonl

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use thiserror::Error;

// Placeholder imports for types that will be merged later
// These would typically come from bd-core or similar
pub type ChangeID = String;
pub type AgentID = String;

/// Errors that can occur during task management operations.
#[derive(Error, Debug)]
pub enum TaskError {
    #[error("Failed to execute jj command: {0}")]
    JJExecution(String),

    #[error("Failed to parse task description: {0}")]
    ParseError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Task not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, TaskError>;

/// Task represents a work item tracked as a jj change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// ChangeID is the jj change ID (e.g., "xyzabc12")
    pub change_id: ChangeID,

    /// Title is the task title
    pub title: String,

    /// Description is the full task description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Priority: 0=critical, 1=high, 2=medium, 3=low, 4=backlog
    pub priority: u8,

    /// Status: pending, in_progress, blocked, completed
    pub status: String,

    /// Agent is the assigned agent ID (empty if unassigned)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentID>,

    /// Labels for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,

    /// DueDate for time-sensitive tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,

    /// Context is the handoff context for agent continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HandoffContext>,

    /// Bookmark is the jj bookmark name for this task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

/// HandoffContext captures agent state for seamless handoffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffContext {
    /// CurrentFocus describes what the agent was working on
    pub current_focus: String,

    /// Progress lists completed items
    pub progress: Vec<String>,

    /// NextSteps lists immediate next actions
    pub next_steps: Vec<String>,

    /// Blockers lists any blocking issues
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blockers: Option<Vec<String>>,

    /// OpenQuestions lists unresolved decisions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_questions: Option<Vec<String>>,

    /// FilesTouched lists recently modified files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_touched: Option<Vec<String>>,

    /// UpdatedAt is when this context was last updated
    pub updated_at: DateTime<Utc>,
}

/// TaskMetadata is stored in .tasks/metadata.jsonl for non-DAG data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub change_id: ChangeID,
    pub priority: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}


impl Task {
    /// FormatDescription creates a structured description for a task change.
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
            .and_then(|b| {
                if b.is_empty() {
                    None
                } else {
                    Some(b.iter()
                        .map(|item| format!("- {}", item))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let questions_str = self.context.as_ref()
            .and_then(|c| c.open_questions.as_ref())
            .and_then(|q| {
                if q.is_empty() {
                    None
                } else {
                    Some(q.iter()
                        .map(|item| format!("- {}", item))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        let files_str = self.context.as_ref()
            .and_then(|c| c.files_touched.as_ref())
            .and_then(|f| {
                if f.is_empty() {
                    None
                } else {
                    Some(f.join("\n"))
                }
            })
            .unwrap_or_else(|| "None".to_string());

        format!(
            "Task: {}
Priority: {}
Status: {}
Agent: {}

## Context
{}

## Progress
{}

## Next Steps
{}

## Blockers
{}

## Open Questions
{}

## Files Touched
{}
",
            self.title,
            self.priority,
            self.status,
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

/// ParseDescription extracts task info from a structured description.
pub fn parse_description(desc: &str) -> Result<Task> {
    let mut task = Task {
        change_id: String::new(), // Will be set by caller
        title: String::new(),
        description: None,
        priority: 2, // Default to medium
        status: String::from("pending"),
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
    let mut current_section = String::new();

    for line in lines {
        let trimmed = line.trim();

        // Parse header fields
        if let Some(title) = trimmed.strip_prefix("Task: ") {
            task.title = title.to_string();
            continue;
        }
        if let Some(priority_str) = trimmed.strip_prefix("Priority: ") {
            task.priority = priority_str.parse().unwrap_or(2);
            continue;
        }
        if let Some(status) = trimmed.strip_prefix("Status: ") {
            task.status = status.to_string();
            continue;
        }
        if let Some(agent_str) = trimmed.strip_prefix("Agent: ") {
            if agent_str != "unassigned" {
                task.agent = Some(agent_str.to_string());
            }
            continue;
        }

        // Track sections
        if let Some(section) = trimmed.strip_prefix("## ") {
            current_section = section.to_string();
            continue;
        }

        // Skip empty lines and "None" markers
        if trimmed.is_empty() || trimmed == "None" {
            continue;
        }

        // Parse section content
        if let Some(context) = task.context.as_mut() {
            match current_section.as_str() {
                "Context" => {
                    if context.current_focus.is_empty() {
                        context.current_focus = trimmed.to_string();
                    } else {
                        context.current_focus.push('\n');
                        context.current_focus.push_str(trimmed);
                    }
                }
                "Progress" => {
                    if let Some(item) = trimmed.strip_prefix("- [x] ") {
                        context.progress.push(item.to_string());
                    }
                }
                "Next Steps" => {
                    if let Some(item) = trimmed.strip_prefix("- [ ] ") {
                        context.next_steps.push(item.to_string());
                    }
                }
                "Blockers" => {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        context.blockers
                            .get_or_insert_with(Vec::new)
                            .push(item.to_string());
                    }
                }
                "Open Questions" => {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        context.open_questions
                            .get_or_insert_with(Vec::new)
                            .push(item.to_string());
                    }
                }
                "Files Touched" => {
                    if !trimmed.is_empty() {
                        context.files_touched
                            .get_or_insert_with(Vec::new)
                            .push(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    Ok(task)
}

/// MetadataStore manages .tasks/metadata.jsonl
pub struct MetadataStore {
    #[allow(dead_code)]
    repo_root: PathBuf,
    path: PathBuf,
}

impl MetadataStore {
    /// Create a new metadata store for the given repository.
    pub fn new<P: AsRef<Path>>(repo_root: P) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let path = repo_root.join(".tasks").join("metadata.jsonl");
        Self { repo_root, path }
    }

    /// Ensure the .tasks directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Load all metadata from the JSONL file.
    pub fn load(&self) -> Result<HashMap<ChangeID, TaskMetadata>> {
        let mut result = HashMap::new();

        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(result);
            }
            Err(e) => return Err(e.into()),
        };

        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Skip malformed lines
            if let Ok(meta) = serde_json::from_str::<TaskMetadata>(trimmed) {
                result.insert(meta.change_id.clone(), meta);
            }
        }

        Ok(result)
    }

    /// Save a single metadata entry (appends to JSONL).
    pub fn save(&self, meta: &TaskMetadata) -> Result<()> {
        self.ensure_dir()?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let data = serde_json::to_string(meta)?;
        writeln!(file, "{}", data)?;

        Ok(())
    }

    /// Compact rewrites the JSONL file, deduplicating by change_id (last wins).
    pub fn compact(&self) -> Result<()> {
        let all = self.load()?;
        self.ensure_dir()?;

        // Write to temp file, then rename
        let tmp_path = self.path.with_extension("jsonl.tmp");
        let mut file = File::create(&tmp_path)?;

        for meta in all.values() {
            let data = serde_json::to_string(meta)?;
            writeln!(file, "{}", data)?;
        }

        drop(file);
        fs::rename(&tmp_path, &self.path)?;

        Ok(())
    }
}

/// JJExecutor is a trait for running jj commands.
#[async_trait::async_trait]
pub trait JJExecutor: Send + Sync {
    async fn exec(&self, args: &[&str]) -> Result<Vec<u8>>;
}

/// TaskManager orchestrates tasks using jj changes.
pub struct TaskManager<J: JJExecutor> {
    #[allow(dead_code)]
    repo_root: PathBuf,
    jj: J,
    metadata: MetadataStore,
}

impl<J: JJExecutor> TaskManager<J> {
    /// Create a new task manager for the given repository.
    pub fn new<P: AsRef<Path>>(repo_root: P, jj: J) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let metadata = MetadataStore::new(&repo_root);
        Self {
            repo_root,
            jj,
            metadata,
        }
    }

    /// Create a new task as a jj change.
    pub async fn create_task(&self, task: &mut Task) -> Result<()> {
        // Create a new change
        let desc = task.format_description();
        self.jj.exec(&["new", "-m", &desc])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to create change: {}", e)))?;

        // Get the new change ID
        let output = self.jj.exec(&["log", "-r", "@", "-n", "1", "--no-graph", "-T", "change_id"])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to get change ID: {}", e)))?;

        task.change_id = String::from_utf8_lossy(&output).trim().to_string();

        // Create bookmark for the task
        let bookmark_name = format!("task-{}", &task.change_id[..8.min(task.change_id.len())]);
        task.bookmark = Some(bookmark_name.clone());

        self.jj.exec(&["bookmark", "create", &bookmark_name])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to create bookmark: {}", e)))?;

        // Save metadata
        let meta = TaskMetadata {
            change_id: task.change_id.clone(),
            priority: task.priority,
            labels: task.labels.clone(),
            due_date: task.due_date,
            agent: task.agent.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.metadata.save(&meta)?;

        Ok(())
    }

    /// Get a task by change ID.
    pub async fn get_task(&self, change_id: &str) -> Result<Task> {
        // Get the change description
        let output = self.jj.exec(&["log", "-r", change_id, "-n", "1", "--no-graph", "-T", "description"])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to get description: {}", e)))?;

        let desc = String::from_utf8_lossy(&output);
        let mut task = parse_description(&desc)?;
        task.change_id = change_id.to_string();

        // Merge metadata
        let metadata = self.metadata.load()?;
        if let Some(meta) = metadata.get(change_id) {
            task.priority = meta.priority;
            task.labels = meta.labels.clone();
            task.due_date = meta.due_date;
            task.agent = meta.agent.clone();
        }

        Ok(task)
    }

    /// Update a task's description.
    pub async fn update_task(&self, task: &Task) -> Result<()> {
        let desc = task.format_description();
        self.jj.exec(&["describe", "-r", &task.change_id, "-m", &desc])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to update description: {}", e)))?;

        // Update metadata
        let meta = TaskMetadata {
            change_id: task.change_id.clone(),
            priority: task.priority,
            labels: task.labels.clone(),
            due_date: task.due_date,
            agent: task.agent.clone(),
            created_at: Utc::now(), // Note: ideally preserve original
            updated_at: Utc::now(),
        };

        self.metadata.save(&meta)?;

        Ok(())
    }

    /// Update the handoff context for a task.
    pub async fn update_handoff(&self, change_id: &str, handoff: &HandoffContext) -> Result<()> {
        // Get current task
        let mut task = self.get_task(change_id).await?;

        // Update context
        task.context = Some(handoff.clone());

        // Save changes
        self.update_task(&task).await
    }

    /// List all tasks matching a revset pattern.
    pub async fn list_tasks(&self, revset: &str) -> Result<Vec<Task>> {
        let output = self.jj.exec(&["log", "-r", revset, "--no-graph", "-T", "change_id ++ \"\\n\""])
            .await
            .map_err(|e| TaskError::JJExecution(format!("Failed to query tasks: {}", e)))?;

        let output_str = String::from_utf8_lossy(&output);
        let change_ids: Vec<&str> = output_str
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let metadata = self.metadata.load()?;
        let mut tasks = Vec::new();

        for change_id in change_ids {
            // Get the change description
            let desc_output = self.jj.exec(&["log", "-r", change_id, "-n", "1", "--no-graph", "-T", "description"])
                .await;

            if let Ok(desc_bytes) = desc_output {
                let desc = String::from_utf8_lossy(&desc_bytes);
                if let Ok(mut task) = parse_description(&desc) {
                    task.change_id = change_id.to_string();

                    // Merge metadata
                    if let Some(meta) = metadata.get(change_id) {
                        task.priority = meta.priority;
                        task.labels = meta.labels.clone();
                        task.due_date = meta.due_date;
                        task.agent = meta.agent.clone();
                    }

                    tasks.push(task);
                }
            }
        }

        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_and_parse_description() {
        let task = Task {
            change_id: "abc123".to_string(),
            title: "Test Task".to_string(),
            description: None,
            priority: 1,
            status: "in_progress".to_string(),
            agent: Some("agent-1".to_string()),
            labels: None,
            due_date: None,
            context: Some(HandoffContext {
                current_focus: "Working on feature X".to_string(),
                progress: vec!["Completed step 1".to_string()],
                next_steps: vec!["Start step 2".to_string()],
                blockers: Some(vec!["Waiting for review".to_string()]),
                open_questions: Some(vec!["Should we use approach A or B?".to_string()]),
                files_touched: Some(vec!["src/main.rs".to_string()]),
                updated_at: Utc::now(),
            }),
            bookmark: None,
        };

        let desc = task.format_description();
        let parsed = parse_description(&desc).unwrap();

        assert_eq!(parsed.title, task.title);
        assert_eq!(parsed.priority, task.priority);
        assert_eq!(parsed.status, task.status);
        assert_eq!(parsed.agent, task.agent);

        let ctx = parsed.context.unwrap();
        assert_eq!(ctx.current_focus, "Working on feature X");
        assert_eq!(ctx.progress.len(), 1);
        assert_eq!(ctx.next_steps.len(), 1);
    }
}
