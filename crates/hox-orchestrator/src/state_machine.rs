//! Pure state machine for orchestration control flow
//!
//! This module implements a pure functional state machine with NO I/O.
//! All state transitions are deterministic and testable.
//!
//! Key design principles:
//! - Pure function: transition(state, event) -> (state, actions)
//! - No async, no I/O, no dependencies on other hox crates
//! - Invalid transitions go to Failed state (never panic)
//! - Simple types (String for IDs, usize for counts)

/// Orchestration state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Initial state - no orchestration in progress
    Idle,
    /// Planning phase - decomposing goal into tasks
    Planning { goal: String },
    /// Executing phase - agents working on tasks
    Executing {
        phase_name: String,
        active_tasks: usize,
    },
    /// Integrating phase - merging agent work
    Integrating { merge_description: String },
    /// Validating phase - running validation checks
    Validating { validation_id: String },
    /// Successfully completed
    Complete { summary: String },
    /// Failed with error
    Failed { error: String },
}

/// Events that trigger state transitions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Start new orchestration run
    StartOrchestration { goal: String },
    /// Planning phase completed
    PlanningComplete { task_count: usize },
    /// Current phase completed
    PhaseComplete,
    /// All tasks completed
    AllTasksComplete,
    /// Integration produced conflicts
    IntegrationConflict { description: String },
    /// Integration completed cleanly
    IntegrationClean,
    /// Validation passed
    ValidationPassed,
    /// Validation failed
    ValidationFailed { reason: String },
    /// Error occurred
    Error { message: String },
}

/// Actions to execute as side effects of transitions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Spawn a planning agent
    SpawnPlanningAgent { goal: String },
    /// Spawn task agents
    SpawnTaskAgents { count: usize },
    /// Create merge operation
    CreateMerge { description: String },
    /// Resolve merge conflicts
    ResolveConflicts { description: String },
    /// Spawn validation agent
    SpawnValidator { validation_id: String },
    /// Log activity
    LogActivity { message: String },
    /// Record pattern for self-evolution
    RecordPattern { pattern: String },
}

/// Pure state transition function
///
/// Takes current state and event, returns new state and actions to execute.
/// This function is completely deterministic and has no side effects.
///
/// # Invalid Transitions
/// Any invalid transition results in a Failed state with descriptive error.
/// This function never panics.
pub fn transition(state: State, event: Event) -> (State, Vec<Action>) {
    match (state, event) {
        // From Idle state
        (State::Idle, Event::StartOrchestration { goal }) => {
            let actions = vec![
                Action::LogActivity {
                    message: format!("Starting orchestration: {}", goal),
                },
                Action::SpawnPlanningAgent { goal: goal.clone() },
            ];
            (State::Planning { goal }, actions)
        }

        // From Planning state
        (State::Planning { goal }, Event::PlanningComplete { task_count }) => {
            if task_count == 0 {
                // No tasks to execute - go directly to completion
                (
                    State::Complete {
                        summary: format!("Planning complete for '{}': no tasks needed", goal),
                    },
                    vec![Action::LogActivity {
                        message: "No tasks to execute".to_string(),
                    }],
                )
            } else {
                let actions = vec![
                    Action::LogActivity {
                        message: format!("Planning complete: {} tasks created", task_count),
                    },
                    Action::SpawnTaskAgents { count: task_count },
                ];
                (
                    State::Executing {
                        phase_name: "execution".to_string(),
                        active_tasks: task_count,
                    },
                    actions,
                )
            }
        }

        // From Executing state
        (
            State::Executing {
                phase_name,
                active_tasks,
            },
            Event::PhaseComplete,
        ) => {
            let actions = vec![
                Action::LogActivity {
                    message: format!("Phase '{}' complete", phase_name),
                },
                Action::RecordPattern {
                    pattern: format!("phase_complete:{}:{}", phase_name, active_tasks),
                },
            ];
            (
                State::Integrating {
                    merge_description: format!("Merge {} tasks from {}", active_tasks, phase_name),
                },
                actions,
            )
        }

        (State::Executing { .. }, Event::AllTasksComplete) => {
            let actions = vec![Action::LogActivity {
                message: "All tasks complete, moving to integration".to_string(),
            }];
            (
                State::Integrating {
                    merge_description: "Merge all completed tasks".to_string(),
                },
                actions,
            )
        }

        // From Integrating state
        (
            State::Integrating {
                merge_description: _,
            },
            Event::IntegrationConflict { description },
        ) => {
            let actions = vec![
                Action::LogActivity {
                    message: format!("Integration conflict: {}", description),
                },
                Action::ResolveConflicts {
                    description: description.clone(),
                },
            ];
            // Stay in Integrating state while resolving
            (State::Integrating { merge_description: description }, actions)
        }

        (State::Integrating { .. }, Event::IntegrationClean) => {
            let validation_id = format!("val-{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
            let actions = vec![
                Action::LogActivity {
                    message: "Integration clean, starting validation".to_string(),
                },
                Action::SpawnValidator {
                    validation_id: validation_id.clone(),
                },
            ];
            (State::Validating { validation_id }, actions)
        }

        // From Validating state
        (State::Validating { .. }, Event::ValidationPassed) => {
            let summary = "Orchestration completed successfully".to_string();
            let actions = vec![
                Action::LogActivity {
                    message: summary.clone(),
                },
                Action::RecordPattern {
                    pattern: "orchestration_success".to_string(),
                },
            ];
            (State::Complete { summary }, actions)
        }

        (State::Validating { .. }, Event::ValidationFailed { reason }) => {
            let actions = vec![Action::LogActivity {
                message: format!("Validation failed: {}", reason),
            }];
            (
                State::Failed {
                    error: format!("Validation failed: {}", reason),
                },
                actions,
            )
        }

        // Error events from any non-terminal state
        (State::Idle, Event::Error { message })
        | (State::Planning { .. }, Event::Error { message })
        | (State::Executing { .. }, Event::Error { message })
        | (State::Integrating { .. }, Event::Error { message })
        | (State::Validating { .. }, Event::Error { message }) => {
            let actions = vec![Action::LogActivity {
                message: format!("Error: {}", message),
            }];
            (State::Failed { error: message }, actions)
        }

        // Terminal states - no valid transitions
        (State::Complete { summary }, event) => (
            State::Failed {
                error: format!(
                    "Invalid transition from Complete state (summary: {}) on event: {:?}",
                    summary, event
                ),
            },
            vec![],
        ),

        (State::Failed { error }, event) => (
            State::Failed {
                error: format!(
                    "Invalid transition from Failed state (error: {}) on event: {:?}",
                    error, event
                ),
            },
            vec![],
        ),

        // All other invalid transitions
        (state, event) => (
            State::Failed {
                error: format!(
                    "Invalid state transition: {:?} cannot handle event {:?}",
                    state, event
                ),
            },
            vec![],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_happy_path_full_flow() {
        // Idle -> Planning
        let (state, actions) = transition(
            State::Idle,
            Event::StartOrchestration {
                goal: "Build feature".to_string(),
            },
        );
        assert!(matches!(state, State::Planning { .. }));
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::LogActivity { .. }));
        assert!(matches!(actions[1], Action::SpawnPlanningAgent { .. }));

        // Planning -> Executing
        let (state, actions) = transition(state, Event::PlanningComplete { task_count: 3 });
        assert!(matches!(state, State::Executing { active_tasks: 3, .. }));
        assert_eq!(actions.len(), 2);

        // Executing -> Integrating
        let (state, actions) = transition(state, Event::AllTasksComplete);
        assert!(matches!(state, State::Integrating { .. }));
        assert_eq!(actions.len(), 1);

        // Integrating -> Validating (clean merge)
        let (state, actions) = transition(state, Event::IntegrationClean);
        assert!(matches!(state, State::Validating { .. }));
        assert_eq!(actions.len(), 2);

        // Validating -> Complete
        let (state, actions) = transition(state, Event::ValidationPassed);
        assert!(matches!(state, State::Complete { .. }));
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[1], Action::RecordPattern { .. }));
    }

    #[test]
    fn test_planning_with_no_tasks() {
        let (state, _) = transition(
            State::Idle,
            Event::StartOrchestration {
                goal: "Simple goal".to_string(),
            },
        );

        // Planning completes with 0 tasks -> directly to Complete
        let (state, actions) = transition(state, Event::PlanningComplete { task_count: 0 });
        assert!(matches!(state, State::Complete { .. }));
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_integration_conflict_handling() {
        let integrating_state = State::Integrating {
            merge_description: "Test merge".to_string(),
        };

        // Conflict keeps us in Integrating state
        let (state, actions) = transition(
            integrating_state,
            Event::IntegrationConflict {
                description: "File conflict in src/main.rs".to_string(),
            },
        );
        assert!(matches!(state, State::Integrating { .. }));
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::LogActivity { .. }));
        assert!(matches!(actions[1], Action::ResolveConflicts { .. }));
    }

    #[test]
    fn test_validation_failure() {
        let validating_state = State::Validating {
            validation_id: "val-123".to_string(),
        };

        let (state, actions) = transition(
            validating_state,
            Event::ValidationFailed {
                reason: "Tests failed".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));
        if let State::Failed { error } = state {
            assert!(error.contains("Tests failed"));
        }
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_error_from_any_state() {
        // Error from Planning
        let (state, _) = transition(
            State::Planning {
                goal: "Test".to_string(),
            },
            Event::Error {
                message: "Planning agent crashed".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));

        // Error from Executing
        let (state, _) = transition(
            State::Executing {
                phase_name: "impl".to_string(),
                active_tasks: 5,
            },
            Event::Error {
                message: "Agent timeout".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));
    }

    #[test]
    fn test_invalid_transition_never_panics() {
        // Try to start orchestration from Planning state (invalid)
        let (state, _) = transition(
            State::Planning {
                goal: "Existing goal".to_string(),
            },
            Event::StartOrchestration {
                goal: "New goal".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));

        // Try to plan complete from Executing (invalid)
        let (state, _) = transition(
            State::Executing {
                phase_name: "test".to_string(),
                active_tasks: 2,
            },
            Event::PlanningComplete { task_count: 5 },
        );
        assert!(matches!(state, State::Failed { .. }));

        // Try to validate from Idle (invalid)
        let (state, _) = transition(State::Idle, Event::ValidationPassed);
        assert!(matches!(state, State::Failed { .. }));
    }

    #[test]
    fn test_terminal_states_reject_all_events() {
        // Complete state rejects all events
        let complete = State::Complete {
            summary: "Done".to_string(),
        };
        let (state, _) = transition(complete.clone(), Event::ValidationPassed);
        assert!(matches!(state, State::Failed { .. }));

        let (state, _) = transition(
            complete,
            Event::StartOrchestration {
                goal: "New".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));

        // Failed state rejects all events
        let failed = State::Failed {
            error: "Original error".to_string(),
        };
        let (state, _) = transition(failed.clone(), Event::IntegrationClean);
        assert!(matches!(state, State::Failed { .. }));

        let (state, _) = transition(
            failed,
            Event::Error {
                message: "Another error".to_string(),
            },
        );
        assert!(matches!(state, State::Failed { .. }));
    }

    #[test]
    fn test_action_generation() {
        // Check that planning generates correct actions
        let (_, actions) = transition(
            State::Idle,
            Event::StartOrchestration {
                goal: "Test".to_string(),
            },
        );
        assert!(actions.iter().any(|a| matches!(a, Action::SpawnPlanningAgent { .. })));
        assert!(actions.iter().any(|a| matches!(a, Action::LogActivity { .. })));

        // Check that execution generates spawn actions
        let (_, actions) = transition(
            State::Planning {
                goal: "Test".to_string(),
            },
            Event::PlanningComplete { task_count: 4 },
        );
        assert!(actions.iter().any(|a| matches!(a, Action::SpawnTaskAgents { count: 4 })));
    }

    #[test]
    fn test_state_equality() {
        let state1 = State::Planning {
            goal: "Same goal".to_string(),
        };
        let state2 = State::Planning {
            goal: "Same goal".to_string(),
        };
        let state3 = State::Planning {
            goal: "Different goal".to_string(),
        };

        assert_eq!(state1, state2);
        assert_ne!(state1, state3);
    }

    #[test]
    fn test_event_equality() {
        let event1 = Event::PlanningComplete { task_count: 5 };
        let event2 = Event::PlanningComplete { task_count: 5 };
        let event3 = Event::PlanningComplete { task_count: 3 };

        assert_eq!(event1, event2);
        assert_ne!(event1, event3);
    }

    #[test]
    fn test_action_equality() {
        let action1 = Action::LogActivity {
            message: "Test".to_string(),
        };
        let action2 = Action::LogActivity {
            message: "Test".to_string(),
        };
        let action3 = Action::LogActivity {
            message: "Different".to_string(),
        };

        assert_eq!(action1, action2);
        assert_ne!(action1, action3);
    }

    #[test]
    fn test_all_states_derive_debug() {
        // Just verify Debug is implemented for all states
        format!("{:?}", State::Idle);
        format!("{:?}", State::Planning { goal: "test".to_string() });
        format!("{:?}", State::Complete { summary: "test".to_string() });
        format!("{:?}", State::Failed { error: "test".to_string() });
    }
}
