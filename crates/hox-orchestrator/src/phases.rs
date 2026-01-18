//! Phase management for orchestrated execution

use hox_core::{ChangeId, HoxError, Phase, Result};
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
        self.phase_status
            .insert(phase.number, PhaseStatus::Pending);
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
        self.phases.iter().all(|p| {
            self.phase_status.get(&p.number) == Some(&PhaseStatus::Completed)
        })
    }

    /// Add a task to a phase
    pub fn add_task_to_phase(&mut self, phase_number: u32, change_id: ChangeId) -> Result<()> {
        if let Some(phase) = self.phases.iter_mut().find(|p| p.number == phase_number) {
            phase.tasks.push(change_id);
            Ok(())
        } else {
            Err(HoxError::Phase(format!(
                "Phase {} not found",
                phase_number
            )))
        }
    }

    /// Get all phases
    pub fn phases(&self) -> &[Phase] {
        &self.phases
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
        assert_eq!(
            manager.phase_status(0),
            Some(&PhaseStatus::InProgress)
        );

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
}
