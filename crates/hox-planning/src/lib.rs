//! Hox Planning - PRD-driven project initialization
//!
//! This crate provides Product Requirements Document (PRD) structures and
//! decomposition logic to convert PRDs into executable Hox phases and tasks.

pub mod decomposer;
pub mod decomposition;
pub mod hox_prd;
pub mod importer;
pub mod prd;
pub mod templates;
pub mod validator;
pub mod writer;

pub use decomposer::{DecompositionSummary, PrdDecomposer, TaskDescription};
pub use importer::{import_markdown, LlmClient, MockLlmClient};
pub use hox_prd::{DecompositionHint, Prd, Requirement, RequirementKind, PRD_SENTINEL};
pub use prd::ProjectRequirementsDocument;
pub use templates::{cli_tool_prd, example_prd, minimal_prd};
pub use validator::{validate_markdown, validate_prd, ValidationError, ValidationResult, ValidationWarning};
pub use writer::{write_to_file, write_to_jj, JjExecutorLike, JjOut};
