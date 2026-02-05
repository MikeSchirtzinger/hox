//! Byzantine fault tolerant consensus implementation

use hox_core::ChangeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::validator::{ValidationReport, ValidationResult};

/// Configuration for Byzantine consensus
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Number of faulty validators to tolerate (f)
    /// Total validators needed: 3f + 1
    pub fault_tolerance: usize,
    /// Threshold for consensus (0.0 - 1.0)
    pub threshold: f32,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            fault_tolerance: 1, // Tolerate 1 faulty validator, need 4 total
            threshold: 0.75,    // 3/4 must agree
        }
    }
}

impl ConsensusConfig {
    /// Calculate minimum validators needed
    pub fn min_validators(&self) -> usize {
        3 * self.fault_tolerance + 1
    }

    /// Calculate minimum votes needed for consensus
    pub fn min_votes(&self, total_validators: usize) -> usize {
        ((total_validators as f32) * self.threshold).ceil() as usize
    }
}

/// A vote from a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub validator_id: String,
    pub change_id: ChangeId,
    pub result: ValidationResult,
    pub score: f32,
    pub report: ValidationReport,
}

/// Result of consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusResult {
    /// Consensus reached: Pass
    Pass {
        votes_for: usize,
        votes_against: usize,
        average_score: f32,
    },
    /// Consensus reached: Fail
    Fail {
        votes_for: usize,
        votes_against: usize,
        average_score: f32,
        reasons: Vec<String>,
    },
    /// Not enough validators
    InsufficientValidators { have: usize, need: usize },
    /// No consensus reached
    NoConsensus {
        votes_for: usize,
        votes_against: usize,
        partial: usize,
    },
}

/// Byzantine consensus implementation
pub struct ByzantineConsensus {
    config: ConsensusConfig,
    votes: HashMap<ChangeId, Vec<Vote>>,
}

impl ByzantineConsensus {
    pub fn new(config: ConsensusConfig) -> Self {
        Self {
            config,
            votes: HashMap::new(),
        }
    }

    /// Add a vote from a validator
    pub fn add_vote(&mut self, vote: Vote) {
        self.votes
            .entry(vote.change_id.clone())
            .or_default()
            .push(vote);
    }

    /// Check if we have enough validators for a change
    pub fn has_enough_validators(&self, change_id: &ChangeId) -> bool {
        let count = self.votes.get(change_id).map(|v| v.len()).unwrap_or(0);
        count >= self.config.min_validators()
    }

    /// Attempt to reach consensus for a change
    pub fn reach_consensus(&self, change_id: &ChangeId) -> ConsensusResult {
        let votes = match self.votes.get(change_id) {
            Some(v) => v,
            None => {
                return ConsensusResult::InsufficientValidators {
                    have: 0,
                    need: self.config.min_validators(),
                }
            }
        };

        let total = votes.len();
        let min_needed = self.config.min_validators();

        if total < min_needed {
            return ConsensusResult::InsufficientValidators {
                have: total,
                need: min_needed,
            };
        }

        // Count votes
        let mut pass_votes = 0;
        let mut fail_votes = 0;
        let mut partial_votes = 0;
        let mut total_score = 0.0;
        let mut fail_reasons = Vec::new();

        for vote in votes {
            total_score += vote.score;
            match vote.result {
                ValidationResult::Pass => pass_votes += 1,
                ValidationResult::Fail => {
                    fail_votes += 1;
                    // Collect failure reasons from the report
                    for check in &vote.report.checks {
                        if !check.passed {
                            fail_reasons.push(format!(
                                "{}: {:?} - {}",
                                vote.validator_id, check.check, check.details
                            ));
                        }
                    }
                }
                ValidationResult::Partial => partial_votes += 1,
            }
        }

        let average_score = total_score / total as f32;
        let min_votes = self.config.min_votes(total);

        // Check for consensus
        if pass_votes >= min_votes {
            ConsensusResult::Pass {
                votes_for: pass_votes,
                votes_against: fail_votes,
                average_score,
            }
        } else if fail_votes >= min_votes {
            ConsensusResult::Fail {
                votes_for: pass_votes,
                votes_against: fail_votes,
                average_score,
                reasons: fail_reasons,
            }
        } else {
            ConsensusResult::NoConsensus {
                votes_for: pass_votes,
                votes_against: fail_votes,
                partial: partial_votes,
            }
        }
    }

    /// Get all votes for a change
    pub fn get_votes(&self, change_id: &ChangeId) -> Option<&Vec<Vote>> {
        self.votes.get(change_id)
    }

    /// Clear votes for a change
    pub fn clear_votes(&mut self, change_id: &ChangeId) {
        self.votes.remove(change_id);
    }

    /// Get the configuration
    pub fn config(&self) -> &ConsensusConfig {
        &self.config
    }
}

impl Default for ByzantineConsensus {
    fn default() -> Self {
        Self::new(ConsensusConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::ValidationReport;

    fn make_vote(
        validator_id: &str,
        change_id: &str,
        result: ValidationResult,
        score: f32,
    ) -> Vote {
        Vote {
            validator_id: validator_id.to_string(),
            change_id: change_id.to_string(),
            result: result.clone(),
            score,
            report: {
                let mut report = ValidationReport::new(validator_id, change_id);
                report.result = result;
                report.score = score;
                report
            },
        }
    }

    #[test]
    fn test_consensus_config() {
        let config = ConsensusConfig::default();
        assert_eq!(config.min_validators(), 4); // 3*1 + 1
        assert_eq!(config.min_votes(4), 3); // ceil(4 * 0.75)
    }

    #[test]
    fn test_consensus_pass() {
        let mut consensus = ByzantineConsensus::default();
        let change_id = "test-change".to_string();

        // 4 validators, 3 pass
        consensus.add_vote(make_vote("v1", &change_id, ValidationResult::Pass, 0.9));
        consensus.add_vote(make_vote("v2", &change_id, ValidationResult::Pass, 0.85));
        consensus.add_vote(make_vote("v3", &change_id, ValidationResult::Pass, 0.95));
        consensus.add_vote(make_vote("v4", &change_id, ValidationResult::Fail, 0.3));

        let result = consensus.reach_consensus(&change_id);

        match result {
            ConsensusResult::Pass { votes_for, .. } => {
                assert_eq!(votes_for, 3);
            }
            _ => panic!("Expected Pass consensus"),
        }
    }

    #[test]
    fn test_consensus_insufficient() {
        let consensus = ByzantineConsensus::default();
        let change_id = "test-change".to_string();

        let result = consensus.reach_consensus(&change_id);

        match result {
            ConsensusResult::InsufficientValidators { have, need } => {
                assert_eq!(have, 0);
                assert_eq!(need, 4);
            }
            _ => panic!("Expected InsufficientValidators"),
        }
    }
}
