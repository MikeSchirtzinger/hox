//! Task management using jj changes as the task graph.
//!
//! This module provides jj-native task orchestration where:
//! - Tasks are jj changes
//! - Dependencies are ancestry in the change DAG
//! - Assignments are bookmarks
//! - Metadata is stored in .tasks/metadata.jsonl

use crate::types::{HandoffContext, Priority, Task, TaskMetadata, TaskStatus};
use bd_core::{HoxError, Result};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::{debug, info, instrument};

/// ParseDescription extracts task info from a structured description.
#[instrument(skip(desc), fields(desc_len = desc.len()))]
pub fn parse_description(desc: &str) -> Result<Task> {
    debug!("Parsing task description");

    let mut task = Task::new("", "");
    task.context = Some(HandoffContext::new(""));

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
            if let Ok(p) = priority_str.parse::<i32>() {
                task.priority = Priority::from_i32(p).unwrap_or(Priority::Medium);
            }
            continue;
        }
        if let Some(status) = trimmed.strip_prefix("Status: ") {
            task.status = TaskStatus::from_str(status).unwrap_or(TaskStatus::Open);
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
    #[instrument(skip(self))]
    pub fn ensure_dir(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| HoxError::Io(e))?;
        }
        Ok(())
    }

    /// Load all metadata from the JSONL file.
    #[instrument(skip(self))]
    pub fn load(&self) -> Result<HashMap<String, TaskMetadata>> {
        debug!("Loading metadata from JSONL");
        let mut result = HashMap::new();

        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(result);
            }
            Err(e) => return Err(HoxError::Io(e)),
        };

        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line.map_err(|e| HoxError::Io(e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Skip malformed lines
            if let Ok(meta) = serde_json::from_str::<TaskMetadata>(trimmed) {
                result.insert(meta.change_id.clone(), meta);
            }
        }

        info!(count = result.len(), "Loaded metadata entries");
        Ok(result)
    }

    /// Save a single metadata entry (appends to JSONL).
    #[instrument(skip(self, meta), fields(change_id = %meta.change_id))]
    pub fn save(&self, meta: &TaskMetadata) -> Result<()> {
        debug!("Saving metadata entry");
        self.ensure_dir()?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| HoxError::Io(e))?;

        let data = serde_json::to_string(meta)
            .map_err(|e| HoxError::Json(e))?;
        writeln!(file, "{}", data)
            .map_err(|e| HoxError::Io(e))?;

        Ok(())
    }

    /// Compact rewrites the JSONL file, deduplicating by change_id (last wins).
    #[instrument(skip(self))]
    pub fn compact(&self) -> Result<()> {
        info!("Compacting metadata file");
        let all = self.load()?;
        self.ensure_dir()?;

        // Write to temp file, then rename
        let tmp_path = self.path.with_extension("jsonl.tmp");
        let mut file = File::create(&tmp_path)
            .map_err(|e| HoxError::Io(e))?;

        for meta in all.values() {
            let data = serde_json::to_string(meta)
                .map_err(|e| HoxError::Json(e))?;
            writeln!(file, "{}", data)
                .map_err(|e| HoxError::Io(e))?;
        }

        drop(file);
        fs::rename(&tmp_path, &self.path)
            .map_err(|e| HoxError::Io(e))?;

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
    #[instrument(skip(self, task), fields(title = %task.title))]
    pub async fn create_task(&self, task: &mut Task) -> Result<()> {
        info!("Creating new task");

        // Create a new change
        let desc = task.format_description();
        self.jj.exec(&["new", "-m", &desc])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to create change: {}", e)))?;

        // Get the new change ID
        let output = self.jj.exec(&["log", "-r", "@", "-n", "1", "--no-graph", "-T", "change_id"])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to get change ID: {}", e)))?;

        task.change_id = String::from_utf8_lossy(&output).trim().to_string();

        // Create bookmark for the task
        let bookmark_name = format!("task-{}", &task.change_id[..8.min(task.change_id.len())]);
        task.bookmark = Some(bookmark_name.clone());

        self.jj.exec(&["bookmark", "create", &bookmark_name])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to create bookmark: {}", e)))?;

        // Save metadata
        let mut meta = TaskMetadata::new(&task.change_id);
        meta.priority = task.priority;
        meta.labels = task.labels.clone();
        meta.due_date = task.due_date;
        meta.agent = task.agent.clone();

        self.metadata.save(&meta)?;

        info!(change_id = %task.change_id, "Task created successfully");
        Ok(())
    }

    /// Get a task by change ID.
    #[instrument(skip(self), fields(change_id))]
    pub async fn get_task(&self, change_id: &str) -> Result<Task> {
        debug!("Retrieving task");

        // Get the change description
        let output = self.jj.exec(&["log", "-r", change_id, "-n", "1", "--no-graph", "-T", "description"])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to get description: {}", e)))?;

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
    #[instrument(skip(self, task), fields(change_id = %task.change_id))]
    pub async fn update_task(&self, task: &Task) -> Result<()> {
        info!("Updating task");

        let desc = task.format_description();
        self.jj.exec(&["describe", "-r", &task.change_id, "-m", &desc])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to update description: {}", e)))?;

        // Update metadata
        let mut meta = TaskMetadata::new(&task.change_id);
        meta.priority = task.priority;
        meta.labels = task.labels.clone();
        meta.due_date = task.due_date;
        meta.agent = task.agent.clone();
        meta.touch();

        self.metadata.save(&meta)?;

        Ok(())
    }

    /// Update the handoff context for a task.
    #[instrument(skip(self, handoff), fields(change_id))]
    pub async fn update_handoff(&self, change_id: &str, handoff: &HandoffContext) -> Result<()> {
        debug!("Updating handoff context");

        // Get current task
        let mut task = self.get_task(change_id).await?;

        // Update context
        task.context = Some(handoff.clone());

        // Save changes
        self.update_task(&task).await
    }

    /// List all tasks matching a revset pattern.
    #[instrument(skip(self), fields(revset))]
    pub async fn list_tasks(&self, revset: &str) -> Result<Vec<Task>> {
        debug!("Listing tasks");

        let output = self.jj.exec(&["log", "-r", revset, "--no-graph", "-T", "change_id ++ \"\\n\""])
            .await
            .map_err(|e| HoxError::JjError(format!("Failed to query tasks: {}", e)))?;

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

        info!(count = tasks.len(), "Listed tasks");
        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_and_parse_description() {
        let mut task = Task::new("abc123", "Test Task");
        task.priority = Priority::High;
        task.status = TaskStatus::InProgress;
        task.agent = Some("agent-1".to_string());

        let mut ctx = HandoffContext::new("Working on feature X");
        ctx.add_progress("Completed step 1");
        ctx.add_next_step("Start step 2");
        ctx.add_blocker("Waiting for review");
        ctx.add_file("src/main.rs");
        task.context = Some(ctx);

        let desc = task.format_description();
        let parsed = parse_description(&desc).unwrap();

        assert_eq!(parsed.title, task.title);
        assert_eq!(parsed.priority, task.priority);
        assert_eq!(parsed.status, task.status);
        assert_eq!(parsed.agent, task.agent);

        let parsed_ctx = parsed.context.unwrap();
        assert_eq!(parsed_ctx.current_focus, "Working on feature X");
        assert_eq!(parsed_ctx.progress.len(), 1);
        assert_eq!(parsed_ctx.next_steps.len(), 1);
    }
}
