//! Product Requirements Document (PRD) data structures

use hox_core::Priority;
use serde::{Deserialize, Serialize};

/// A complete Product Requirements Document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRequirementsDocument {
    pub project_name: String,
    pub version: String,
    pub status: String, // "draft", "approved", "in_progress"
    pub last_updated: String,
    pub goals: GoalsSection,
    pub requirements: RequirementsSection,
    pub epics: Vec<Epic>,
}

/// Project goals and background
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalsSection {
    pub goals: Vec<String>,
    pub background: String,
    pub timeline_estimate: Option<String>,
}

/// Functional and non-functional requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementsSection {
    pub functional: Vec<Requirement>,
    pub non_functional: Vec<Requirement>,
}

/// A single requirement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String, // "FR1", "NFR2"
    pub statement: String,
    pub description: Option<String>,
}

/// Epic-level work breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub id: String,
    pub name: String,
    pub description: String,
    pub priority: Priority,
    pub stories: Vec<Story>,
}

/// User story within an epic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Story {
    pub id: String,
    pub title: String,
    pub as_a: String,
    pub i_want: String,
    pub so_that: String,
    pub acceptance_criteria: Vec<String>,
}

impl ProjectRequirementsDocument {
    /// Create a new empty PRD
    pub fn new(project_name: impl Into<String>) -> Self {
        Self {
            project_name: project_name.into(),
            version: "1.0".to_string(),
            status: "draft".to_string(),
            last_updated: chrono::Utc::now().to_rfc3339(),
            goals: GoalsSection {
                goals: Vec::new(),
                background: String::new(),
                timeline_estimate: None,
            },
            requirements: RequirementsSection {
                functional: Vec::new(),
                non_functional: Vec::new(),
            },
            epics: Vec::new(),
        }
    }

    /// Update the last_updated timestamp
    pub fn touch(&mut self) {
        self.last_updated = chrono::Utc::now().to_rfc3339();
    }

    /// Mark as approved
    pub fn approve(&mut self) {
        self.status = "approved".to_string();
        self.touch();
    }

    /// Mark as in progress
    pub fn start(&mut self) {
        self.status = "in_progress".to_string();
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_prd() {
        let prd = ProjectRequirementsDocument::new("Test Project");
        assert_eq!(prd.project_name, "Test Project");
        assert_eq!(prd.version, "1.0");
        assert_eq!(prd.status, "draft");
        assert!(prd.epics.is_empty());
    }

    #[test]
    fn test_prd_lifecycle() {
        let mut prd = ProjectRequirementsDocument::new("Test");
        assert_eq!(prd.status, "draft");

        prd.approve();
        assert_eq!(prd.status, "approved");

        prd.start();
        assert_eq!(prd.status, "in_progress");
    }
}
