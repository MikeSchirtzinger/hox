//! Hox Planning - PRD-driven project initialization
//!
//! This crate provides Product Requirements Document (PRD) structures and
//! decomposition logic to convert PRDs into executable Hox phases and tasks.

pub mod decomposer;
pub mod prd;
pub mod templates;

pub use decomposer::{DecompositionSummary, PrdDecomposer, TaskDescription};
pub use prd::ProjectRequirementsDocument;
pub use templates::{cli_tool_prd, example_prd, minimal_prd};
