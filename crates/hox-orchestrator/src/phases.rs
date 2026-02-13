//! Phase management for orchestrated execution

use hox_core::{ChangeId, HoxError, Phase, Result, TaskStatus};
use std::collections::HashMap;

/// Status of a phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

/// Manages phases in an orchestration run
pub struct PhaseManager {
    phases: Vec<Phase>,
    current_phase_idx: usize,
    phase_status: HashMap<u32, PhaseStatus>,
}

impl PhaseManager {
    pub fn new() -> Self {
        Self {
            phases: Vec::new(),
            current_phase_idx: 0,
            phase_status: HashMap::new(),
        }
    }

    /// Add a phase
    pub fn add_phase(&mut self, phase: Phase) {
        self.phase_status.insert(phase.number, PhaseStatus::Pending);
        self.phases.push(phase);
        self.phases.sort_by_key(|p| p.number);
    }

    /// Get current phase
    pub fn current_phase(&self) -> Option<&Phase> {
        self.phases.get(self.current_phase_idx)
    }

    /// Get phase by number
    pub fn get_phase(&self, number: u32) -> Option<&Phase> {
        self.phases.iter().find(|p| p.number == number)
    }

    /// Get phase status
    pub fn phase_status(&self, number: u32) -> Option<&PhaseStatus> {
        self.phase_status.get(&number)
    }

    /// Set phase status
    pub fn set_phase_status(&mut self, number: u32, status: PhaseStatus) {
        self.phase_status.insert(number, status);
    }

    /// Mark current phase as in progress
    pub fn start_current_phase(&mut self) -> Result<()> {
        if let Some(phase) = self.current_phase() {
            self.phase_status
                .insert(phase.number, PhaseStatus::InProgress);
            Ok(())
        } else {
            Err(HoxError::Phase("No current phase".to_string()))
        }
    }

    /// Mark current phase as completed and advance
    pub fn complete_current_phase(&mut self) -> Result<()> {
        if let Some(phase) = self.current_phase() {
            self.phase_status
                .insert(phase.number, PhaseStatus::Completed);
            self.current_phase_idx += 1;
            Ok(())
        } else {
            Err(HoxError::Phase("No current phase".to_string()))
        }
    }

    /// Advance to next phase (if current is completed)
    pub fn advance(&mut self) -> Result<()> {
        if let Some(phase) = self.current_phase() {
            if self.phase_status.get(&phase.number) == Some(&PhaseStatus::Completed) {
                self.current_phase_idx += 1;
                Ok(())
            } else {
                Err(HoxError::Phase(format!(
                    "Phase {} not completed",
                    phase.number
                )))
            }
        } else {
            Err(HoxError::Phase("No current phase".to_string()))
        }
    }

    /// Check if all phases are completed
    pub fn all_completed(&self) -> bool {
        self.phases
            .iter()
            .all(|p| self.phase_status.get(&p.number) == Some(&PhaseStatus::Completed))
    }

    /// Add a task to a phase
    pub fn add_task_to_phase(&mut self, phase_number: u32, change_id: ChangeId) -> Result<()> {
        if let Some(phase) = self.phases.iter_mut().find(|p| p.number == phase_number) {
            phase.tasks.push(change_id);
            Ok(())
        } else {
            Err(HoxError::Phase(format!("Phase {} not found", phase_number)))
        }
    }

    /// Get all phases
    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    /// Check if current phase can auto-advance
    ///
    /// Returns true if ALL tasks in the current phase have status Done.
    /// Returns false if no tasks exist in the phase (no auto-advance on empty phase).
    pub fn check_auto_advance(&self, tasks: &[(String, TaskStatus)]) -> bool {
        // Empty task list - no auto-advance
        if tasks.is_empty() {
            return false;
        }

        // All tasks must be Done
        let all_done = tasks.iter().all(|(_, status)| *status == TaskStatus::Done);

        if all_done {
            if let Some(phase) = self.current_phase() {
                tracing::info!(
                    phase_number = phase.number,
                    phase_name = %phase.name,
                    task_count = tasks.len(),
                    "Phase detected as complete - all tasks done"
                );
            }
        }

        all_done
    }

    /// Attempt to auto-advance to next phase
    ///
    /// Checks if current phase is complete using check_auto_advance.
    /// If complete, advances to next phase and returns the new PhaseStatus.
    /// Returns None if not ready to advance.
    pub fn maybe_advance(&mut self, tasks: &[(String, TaskStatus)]) -> Option<PhaseStatus> {
        if !self.check_auto_advance(tasks) {
            return None;
        }

        // Get current phase info before advancing
        let current_phase_num = self.current_phase()?.number;
        let current_phase_name = self.current_phase()?.name.clone();

        // Mark current phase as completed and advance
        if self.complete_current_phase().is_err() {
            return None;
        }

        tracing::info!(
            from_phase = current_phase_num,
            from_name = %current_phase_name,
            to_phase = self.current_phase_idx,
            "Auto-advanced to next phase"
        );

        // Return the new phase status
        if self.current_phase().is_some() {
            Some(PhaseStatus::InProgress)
        } else {
            // No more phases - we've completed all phases
            Some(PhaseStatus::Completed)
        }
    }

    /// Create a standard phase structure for feature development
    pub fn standard_feature_phases(description: &str) -> Self {
        let mut manager = Self::new();

        // Phase 0: Contracts (blocking)
        manager.add_phase(Phase::contracts(format!(
            "Define contracts for: {}",
            description
        )));

        // Phase 1: Implementation (parallel)
        manager.add_phase(Phase {
            number: 1,
            name: "implementation".to_string(),
            description: format!("Implement: {}", description),
            blocking: false,
            tasks: Vec::new(),
        });

        // Phase 2: Integration
        manager.add_phase(Phase::integration(2, "Integrate parallel work"));

        // Phase 3: Validation
        manager.add_phase(Phase::validation(3, "Validate implementation"));

        manager
    }
}

impl Default for PhaseManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_management() {
        let mut manager = PhaseManager::new();

        manager.add_phase(Phase::contracts("Test contracts"));
        manager.add_phase(Phase {
            number: 1,
            name: "impl".to_string(),
            description: "Implementation".to_string(),
            blocking: false,
            tasks: Vec::new(),
        });

        assert_eq!(manager.current_phase().unwrap().number, 0);

        manager.start_current_phase().unwrap();
        assert_eq!(manager.phase_status(0), Some(&PhaseStatus::InProgress));

        manager.complete_current_phase().unwrap();
        assert_eq!(manager.current_phase().unwrap().number, 1);
    }

    #[test]
    fn test_standard_phases() {
        let manager = PhaseManager::standard_feature_phases("User authentication");

        assert_eq!(manager.phases().len(), 4);
        assert_eq!(manager.phases()[0].name, "contracts");
        assert_eq!(manager.phases()[1].name, "implementation");
        assert_eq!(manager.phases()[2].name, "integration");
        assert_eq!(manager.phases()[3].name, "validation");
    }

    #[test]
    fn test_check_auto_advance_all_done() {
        let manager = PhaseManager::new();

        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
            ("task-2".to_string(), TaskStatus::Done),
            ("task-3".to_string(), TaskStatus::Done),
        ];

        assert!(manager.check_auto_advance(&tasks));
    }

    #[test]
    fn test_check_auto_advance_not_all_done() {
        let manager = PhaseManager::new();

        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
            ("task-2".to_string(), TaskStatus::InProgress),
            ("task-3".to_string(), TaskStatus::Done),
        ];

        assert!(!manager.check_auto_advance(&tasks));
    }

    #[test]
    fn test_check_auto_advance_empty_list() {
        let manager = PhaseManager::new();
        let tasks = vec![];

        // No auto-advance on empty task list
        assert!(!manager.check_auto_advance(&tasks));
    }

    #[test]
    fn test_check_auto_advance_mixed_statuses() {
        let manager = PhaseManager::new();

        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
            ("task-2".to_string(), TaskStatus::Blocked),
            ("task-3".to_string(), TaskStatus::Review),
            ("task-4".to_string(), TaskStatus::Open),
        ];

        assert!(!manager.check_auto_advance(&tasks));
    }

    #[test]
    fn test_maybe_advance_success() {
        let mut manager = PhaseManager::new();

        // Add two phases
        manager.add_phase(Phase::contracts("Phase 0"));
        manager.add_phase(Phase {
            number: 1,
            name: "impl".to_string(),
            description: "Implementation".to_string(),
            blocking: false,
            tasks: Vec::new(),
        });

        // Start phase 0
        manager.start_current_phase().unwrap();
        assert_eq!(manager.current_phase().unwrap().number, 0);

        // All tasks done - should advance
        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
            ("task-2".to_string(), TaskStatus::Done),
        ];

        let result = manager.maybe_advance(&tasks);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PhaseStatus::InProgress);

        // Should now be on phase 1
        assert_eq!(manager.current_phase().unwrap().number, 1);
    }

    #[test]
    fn test_maybe_advance_not_ready() {
        let mut manager = PhaseManager::new();

        manager.add_phase(Phase::contracts("Phase 0"));
        manager.start_current_phase().unwrap();

        // Tasks not all done - should not advance
        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
            ("task-2".to_string(), TaskStatus::InProgress),
        ];

        let result = manager.maybe_advance(&tasks);
        assert!(result.is_none());

        // Should still be on phase 0
        assert_eq!(manager.current_phase().unwrap().number, 0);
    }

    #[test]
    fn test_maybe_advance_empty_tasks() {
        let mut manager = PhaseManager::new();

        manager.add_phase(Phase::contracts("Phase 0"));
        manager.start_current_phase().unwrap();

        // Empty task list - should not advance
        let tasks = vec![];

        let result = manager.maybe_advance(&tasks);
        assert!(result.is_none());

        // Should still be on phase 0
        assert_eq!(manager.current_phase().unwrap().number, 0);
    }

    #[test]
    fn test_maybe_advance_last_phase() {
        let mut manager = PhaseManager::new();

        // Add single phase
        manager.add_phase(Phase::contracts("Phase 0"));
        manager.start_current_phase().unwrap();

        // All tasks done
        let tasks = vec![
            ("task-1".to_string(), TaskStatus::Done),
        ];

        let result = manager.maybe_advance(&tasks);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PhaseStatus::Completed);

        // No more phases
        assert!(manager.current_phase().is_none());
    }

    #[test]
    fn test_maybe_advance_multiple_phases() {
        let mut manager = PhaseManager::new();

        // Add three phases
        manager.add_phase(Phase::contracts("Phase 0"));
        manager.add_phase(Phase {
            number: 1,
            name: "impl".to_string(),
            description: "Implementation".to_string(),
            blocking: false,
            tasks: Vec::new(),
        });
        manager.add_phase(Phase::integration(2, "Integration"));

        manager.start_current_phase().unwrap();

        let done_tasks = vec![("task-1".to_string(), TaskStatus::Done)];

        // Advance from phase 0 to phase 1
        assert!(manager.maybe_advance(&done_tasks).is_some());
        assert_eq!(manager.current_phase().unwrap().number, 1);

        // Advance from phase 1 to phase 2
        assert!(manager.maybe_advance(&done_tasks).is_some());
        assert_eq!(manager.current_phase().unwrap().number, 2);

        // Advance from phase 2 to completion
        let result = manager.maybe_advance(&done_tasks);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PhaseStatus::Completed);
        assert!(manager.current_phase().is_none());
    }
}
