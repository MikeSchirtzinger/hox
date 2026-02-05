//! PRD decomposition into phases and tasks

use crate::prd::ProjectRequirementsDocument;
use hox_core::{Phase, Priority, TaskStatus};

/// Decomposes a PRD into executable phases and task descriptions
pub struct PrdDecomposer;

impl PrdDecomposer {
    /// Convert PRD to phases and task descriptions
    ///
    /// Returns a tuple of (phases, tasks) where:
    /// - phases: Ordered execution phases (Phase 0 = contracts, final phases = integration/validation)
    /// - tasks: Task descriptions that can be converted to JJ changes
    pub fn decompose(prd: &ProjectRequirementsDocument) -> (Vec<Phase>, Vec<TaskDescription>) {
        let mut phases = Vec::new();
        let mut tasks = Vec::new();

        // Phase 0: Planning/Contracts
        phases.push(Phase::contracts(format!(
            "Define contracts and interfaces for: {}",
            prd.project_name
        )));

        // One phase per epic
        for (idx, epic) in prd.epics.iter().enumerate() {
            let phase = Phase {
                number: (idx + 1) as u32,
                name: format!("epic-{}", epic.id.to_lowercase().replace(' ', "-")),
                description: epic.description.clone(),
                blocking: false,
                tasks: Vec::new(),
            };
            phases.push(phase);

            // One task per story
            for story in &epic.stories {
                tasks.push(TaskDescription {
                    id: format!("{}-{}", epic.id, story.id),
                    title: story.title.clone(),
                    description: format!(
                        "As a {}\nI want {}\nSo that {}\n\n## Acceptance Criteria\n{}",
                        story.as_a,
                        story.i_want,
                        story.so_that,
                        story
                            .acceptance_criteria
                            .iter()
                            .map(|c| format!("- [ ] {}", c))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    priority: epic.priority,
                    phase: (idx + 1) as u32,
                    status: TaskStatus::Open,
                });
            }
        }

        // Final phases
        let integration_phase = prd.epics.len() + 1;
        let validation_phase = prd.epics.len() + 2;

        phases.push(Phase::integration(
            integration_phase as u32,
            "Integrate all epic work",
        ));

        phases.push(Phase::validation(
            validation_phase as u32,
            "Validate against requirements",
        ));

        (phases, tasks)
    }

    /// Generate a task summary for reporting
    pub fn summarize(prd: &ProjectRequirementsDocument) -> DecompositionSummary {
        let (phases, tasks) = Self::decompose(prd);

        let total_stories: usize = prd.epics.iter().map(|e| e.stories.len()).sum();

        DecompositionSummary {
            project_name: prd.project_name.clone(),
            total_epics: prd.epics.len(),
            total_stories,
            total_phases: phases.len(),
            total_tasks: tasks.len(),
            phases: phases
                .iter()
                .map(|p| PhaseInfo {
                    number: p.number,
                    name: p.name.clone(),
                    description: p.description.clone(),
                })
                .collect(),
        }
    }
}

/// A task description ready to be converted into a JJ change
#[derive(Debug, Clone)]
pub struct TaskDescription {
    pub id: String,
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub phase: u32,
    pub status: TaskStatus,
}

impl TaskDescription {
    /// Format as a JJ change description with structured metadata
    pub fn to_change_description(&self) -> String {
        format!(
            "Task: {}\nPriority: {}\nStatus: {}\nPhase: {}\n\n{}",
            self.title, self.priority as u32, self.status, self.phase, self.description
        )
    }
}

/// Summary of decomposition results
#[derive(Debug, Clone)]
pub struct DecompositionSummary {
    pub project_name: String,
    pub total_epics: usize,
    pub total_stories: usize,
    pub total_phases: usize,
    pub total_tasks: usize,
    pub phases: Vec<PhaseInfo>,
}

/// Phase information for summary
#[derive(Debug, Clone)]
pub struct PhaseInfo {
    pub number: u32,
    pub name: String,
    pub description: String,
}

impl std::fmt::Display for DecompositionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Project: {}", self.project_name)?;
        writeln!(f, "  Epics: {}", self.total_epics)?;
        writeln!(f, "  Stories: {}", self.total_stories)?;
        writeln!(f, "  Phases: {}", self.total_phases)?;
        writeln!(f, "  Tasks: {}", self.total_tasks)?;
        writeln!(f)?;
        writeln!(f, "Phase Breakdown:")?;
        for phase in &self.phases {
            writeln!(
                f,
                "  Phase {}: {} - {}",
                phase.number, phase.name, phase.description
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::example_prd;

    #[test]
    fn test_decompose_example_prd() {
        let prd = example_prd();
        let (phases, tasks) = PrdDecomposer::decompose(&prd);

        // Should have: contracts + 2 epics + integration + validation = 5 phases
        assert_eq!(phases.len(), 5);
        assert_eq!(phases[0].name, "contracts");
        assert_eq!(phases[phases.len() - 2].name, "integration");
        assert_eq!(phases[phases.len() - 1].name, "validation");

        // Should have tasks matching stories
        assert!(!tasks.is_empty());
        for task in &tasks {
            assert!(!task.title.is_empty());
            assert!(!task.description.is_empty());
        }
    }

    #[test]
    fn test_summarize() {
        let prd = example_prd();
        let summary = PrdDecomposer::summarize(&prd);

        assert_eq!(summary.project_name, "Example Project");
        assert_eq!(summary.total_epics, 2);
        assert_eq!(summary.total_phases, 5);
    }
}
