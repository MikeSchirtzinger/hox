//! Hox Planning - PRD-driven project initialization
//!
//! This crate provides Product Requirements Document (PRD) structures and
//! decomposition logic to convert PRDs into executable Hox phases and tasks.

pub mod prd;
pub mod decomposer;
pub mod templates;

pub use prd::ProjectRequirementsDocument;
pub use decomposer::{PrdDecomposer, TaskDescription, DecompositionSummary};
pub use templates::{example_prd, minimal_prd, cli_tool_prd};
