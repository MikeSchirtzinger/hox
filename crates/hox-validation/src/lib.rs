//! # hox-validation
//!
//! Validation system with Byzantine consensus for Hox orchestration.
//!
//! This crate provides:
//! - Validator agent implementation
//! - Byzantine fault tolerant consensus (3f+1)
//! - Quality scoring and metrics

#![allow(dead_code)]

mod consensus;
mod validator;

pub use consensus::{ByzantineConsensus, ConsensusConfig, ConsensusResult, Vote};
pub use validator::{ValidationReport, ValidationResult, Validator, ValidatorConfig};
