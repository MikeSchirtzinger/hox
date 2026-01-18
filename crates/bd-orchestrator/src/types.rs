//! Core types for jj-native task and agent orchestration.
//!
//! This module re-exports the unified types from bd-core and provides
//! additional orchestrator-specific types and compatibility helpers.

// ============================================================================
// Re-export unified types from bd-core
// ============================================================================

pub use bd_core::{
    AgentHandoff, ChangeEntry, HandoffContext, Priority, Task, TaskMetadata, TaskStatus,
};

// ============================================================================
// Orchestrator-specific types
// ============================================================================

use chrono::Utc;
use serde::{Deserialize, Serialize};

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

impl HandoffSummary {
    /// Convert HandoffSummary to HandoffContext.
    pub fn into_context(self) -> HandoffContext {
        let mut ctx = HandoffContext::new(self.current_focus);
        ctx.progress = self.progress;
        ctx.next_steps = self.next_steps;
        if !self.blockers.is_empty() {
            ctx.blockers = Some(self.blockers);
        }
        if !self.files_touched.is_empty() {
            ctx.files_touched = Some(self.files_touched);
        }
        ctx.updated_at = Utc::now();
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handoff_summary_into_context() {
        let summary = HandoffSummary {
            current_focus: "Working on feature X".to_string(),
            progress: vec!["Step 1".to_string()],
            next_steps: vec!["Step 2".to_string()],
            blockers: vec!["Blocked by Y".to_string()],
            open_questions: vec![],
            files_touched: vec!["src/lib.rs".to_string()],
        };

        let ctx = summary.into_context();
        assert_eq!(ctx.current_focus, "Working on feature X");
        assert_eq!(ctx.progress.len(), 1);
        assert_eq!(ctx.next_steps.len(), 1);
        assert_eq!(ctx.blockers.as_ref().unwrap().len(), 1);
        assert_eq!(ctx.files_touched.as_ref().unwrap().len(), 1);
    }
}
