//! File schema definitions for task and dependency files.
//!
//! These types represent the on-disk format of task/*.json and deps/*.json files
//! compatible with the jj-turso architecture.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// TaskFile represents a task stored as individual JSON file in tasks/*.json.
/// This structure is CRDT-friendly with flat fields and last-write-wins semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFile {
    // ===== Core Identification =====
    pub id: String,

    // ===== Task Content =====
    pub title: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Task type: bug, feature, task, epic, chore
    #[serde(rename = "type")]
    pub task_type: String,

    /// Task status: open, in_progress, blocked, closed, etc.
    pub status: String,

    // ===== Priority & Scheduling =====
    /// Priority 0-4 (P0=critical, P4=backlog)
    pub priority: i32,

    // ===== Assignment & Ownership =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_agent: Option<String>,

    // ===== Tags & Classification =====
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    // ===== Timestamps (CRDT conflict resolution) =====
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // ===== Time-Based Scheduling =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_until: Option<DateTime<Utc>>,
}

impl TaskFile {
    /// Validate checks if the TaskFile has valid field values.
    pub fn validate(&self) -> crate::Result<()> {
        if self.id.is_empty() {
            return Err(crate::Error::SchemaValidation("id is required".to_string()));
        }
        if self.title.is_empty() {
            return Err(crate::Error::SchemaValidation(
                "title is required".to_string(),
            ));
        }
        if self.title.len() > 500 {
            return Err(crate::Error::SchemaValidation(format!(
                "title must be 500 characters or less (got {})",
                self.title.len()
            )));
        }
        if !(0..=4).contains(&self.priority) {
            return Err(crate::Error::SchemaValidation(format!(
                "priority must be between 0 and 4 (got {})",
                self.priority
            )));
        }
        if self.task_type.is_empty() {
            return Err(crate::Error::SchemaValidation("type is required".to_string()));
        }
        if self.status.is_empty() {
            return Err(crate::Error::SchemaValidation(
                "status is required".to_string(),
            ));
        }
        Ok(())
    }

    /// Returns the canonical filename for this task: {id}.json
    pub fn filename(&self) -> String {
        format!("{}.json", self.id)
    }

    /// Updates the updated_at timestamp to now
    pub fn update_timestamp(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// DepFile represents a single dependency stored in deps/*.json
/// Filename convention: {from}--{type}--{to}.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepFile {
    pub from: String,
    pub to: String,

    #[serde(rename = "type")]
    pub dep_type: String,

    pub created_at: DateTime<Utc>,
}

impl DepFile {
    /// Validate checks if the DepFile has valid field values
    pub fn validate(&self) -> crate::Result<()> {
        if self.from.is_empty() {
            return Err(crate::Error::SchemaValidation("from is required".to_string()));
        }
        if self.to.is_empty() {
            return Err(crate::Error::SchemaValidation("to is required".to_string()));
        }
        if self.dep_type.is_empty() {
            return Err(crate::Error::SchemaValidation("type is required".to_string()));
        }
        if self.dep_type.len() > 50 {
            return Err(crate::Error::SchemaValidation(format!(
                "type must be 50 characters or less (got {})",
                self.dep_type.len()
            )));
        }
        Ok(())
    }

    /// Generates the filename for this dependency: {from}--{type}--{to}.json
    pub fn to_filename(&self) -> String {
        format!("{}--{}--{}.json", self.from, self.dep_type, self.to)
    }
}
