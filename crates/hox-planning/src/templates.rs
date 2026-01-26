//! Template PRDs for common project types

use crate::prd::*;
use hox_core::Priority;

/// Create an example PRD for demonstration
pub fn example_prd() -> ProjectRequirementsDocument {
    ProjectRequirementsDocument {
        project_name: "Example Project".to_string(),
        version: "1.0".to_string(),
        status: "draft".to_string(),
        last_updated: chrono::Utc::now().to_rfc3339(),
        goals: GoalsSection {
            goals: vec![
                "Implement core feature X".to_string(),
                "Ensure high test coverage".to_string(),
                "Maintain clean architecture".to_string(),
            ],
            background: "This project aims to demonstrate the Hox planning system by creating a well-structured example application.".to_string(),
            timeline_estimate: Some("2 weeks".to_string()),
        },
        requirements: RequirementsSection {
            functional: vec![
                Requirement {
                    id: "FR1".to_string(),
                    statement: "System shall support X functionality".to_string(),
                    description: Some("Detailed description of how X should work.".to_string()),
                },
                Requirement {
                    id: "FR2".to_string(),
                    statement: "System shall persist user data".to_string(),
                    description: Some("User data must be stored and retrieved reliably.".to_string()),
                },
            ],
            non_functional: vec![
                Requirement {
                    id: "NFR1".to_string(),
                    statement: "Response time < 200ms for 95th percentile".to_string(),
                    description: None,
                },
                Requirement {
                    id: "NFR2".to_string(),
                    statement: "Code coverage > 80%".to_string(),
                    description: None,
                },
            ],
        },
        epics: vec![
            Epic {
                id: "Epic-1".to_string(),
                name: "Core Feature Implementation".to_string(),
                description: "Implement the core feature with proper architecture".to_string(),
                priority: Priority::High,
                stories: vec![
                    Story {
                        id: "1.1".to_string(),
                        title: "Setup project structure".to_string(),
                        as_a: "developer".to_string(),
                        i_want: "a well-organized project structure".to_string(),
                        so_that: "I can work efficiently and maintain code quality".to_string(),
                        acceptance_criteria: vec![
                            "Directory structure follows Rust conventions".to_string(),
                            "All dependencies declared in Cargo.toml".to_string(),
                            "CI/CD pipeline configured".to_string(),
                        ],
                    },
                    Story {
                        id: "1.2".to_string(),
                        title: "Implement core data types".to_string(),
                        as_a: "developer".to_string(),
                        i_want: "well-defined core data types".to_string(),
                        so_that: "the system has a solid foundation".to_string(),
                        acceptance_criteria: vec![
                            "All core types defined with proper traits".to_string(),
                            "Serialization/deserialization working".to_string(),
                            "Comprehensive unit tests".to_string(),
                        ],
                    },
                ],
            },
            Epic {
                id: "Epic-2".to_string(),
                name: "Testing Infrastructure".to_string(),
                description: "Build comprehensive testing infrastructure".to_string(),
                priority: Priority::Medium,
                stories: vec![
                    Story {
                        id: "2.1".to_string(),
                        title: "Setup test framework".to_string(),
                        as_a: "developer".to_string(),
                        i_want: "a robust test framework".to_string(),
                        so_that: "I can verify code correctness".to_string(),
                        acceptance_criteria: vec![
                            "Unit test framework configured".to_string(),
                            "Integration test harness ready".to_string(),
                            "Test coverage reporting enabled".to_string(),
                        ],
                    },
                ],
            },
        ],
    }
}

/// Create a minimal PRD template
pub fn minimal_prd(project_name: impl Into<String>) -> ProjectRequirementsDocument {
    let mut prd = ProjectRequirementsDocument::new(project_name);
    prd.goals.background = "Fill in project background here.".to_string();
    prd
}

/// Create a CLI tool PRD template
pub fn cli_tool_prd(tool_name: impl Into<String>) -> ProjectRequirementsDocument {
    let name = tool_name.into();
    ProjectRequirementsDocument {
        project_name: name.clone(),
        version: "1.0".to_string(),
        status: "draft".to_string(),
        last_updated: chrono::Utc::now().to_rfc3339(),
        goals: GoalsSection {
            goals: vec![
                format!("Create a production-ready CLI tool: {}", name),
                "Provide excellent user experience".to_string(),
                "Ensure reliability and error handling".to_string(),
            ],
            background: format!("This CLI tool will provide command-line functionality for {}.", name),
            timeline_estimate: Some("1 week".to_string()),
        },
        requirements: RequirementsSection {
            functional: vec![
                Requirement {
                    id: "FR1".to_string(),
                    statement: "CLI shall parse command-line arguments".to_string(),
                    description: Some("Using clap for argument parsing".to_string()),
                },
                Requirement {
                    id: "FR2".to_string(),
                    statement: "CLI shall provide help documentation".to_string(),
                    description: Some("Auto-generated from clap annotations".to_string()),
                },
            ],
            non_functional: vec![
                Requirement {
                    id: "NFR1".to_string(),
                    statement: "Startup time < 100ms".to_string(),
                    description: None,
                },
                Requirement {
                    id: "NFR2".to_string(),
                    statement: "Clear error messages for user errors".to_string(),
                    description: None,
                },
            ],
        },
        epics: vec![
            Epic {
                id: "Epic-1".to_string(),
                name: "CLI Foundation".to_string(),
                description: "Build the CLI foundation with argument parsing".to_string(),
                priority: Priority::Critical,
                stories: vec![
                    Story {
                        id: "1.1".to_string(),
                        title: "Setup CLI structure".to_string(),
                        as_a: "user".to_string(),
                        i_want: "a working CLI skeleton".to_string(),
                        so_that: "the tool can be invoked from command line".to_string(),
                        acceptance_criteria: vec![
                            "CLI compiles and runs".to_string(),
                            "Help text displays correctly".to_string(),
                            "Version flag works".to_string(),
                        ],
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_prd() {
        let prd = example_prd();
        assert_eq!(prd.project_name, "Example Project");
        assert!(!prd.epics.is_empty());
        assert!(!prd.requirements.functional.is_empty());
    }

    #[test]
    fn test_minimal_prd() {
        let prd = minimal_prd("Test");
        assert_eq!(prd.project_name, "Test");
        assert_eq!(prd.status, "draft");
    }

    #[test]
    fn test_cli_tool_prd() {
        let prd = cli_tool_prd("my-tool");
        assert_eq!(prd.project_name, "my-tool");
        assert_eq!(prd.epics.len(), 1);
        assert_eq!(prd.epics[0].priority, Priority::Critical);
    }
}
