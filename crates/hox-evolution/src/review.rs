//! Review gates for pattern approval

use hox_core::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::patterns::Pattern;

/// Result of a review
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewResult {
    /// Pattern approved
    Approved,
    /// Pattern rejected with reason
    Rejected(String),
    /// Needs human review
    NeedsHumanReview(String),
}

/// Review gate for pattern approval
pub struct ReviewGate {
    /// Whether human approval is required for all patterns
    require_human: bool,
    /// Minimum success rate to auto-approve
    min_success_rate: f32,
    /// Minimum usage count to consider for auto-approval
    min_usage_count: u32,
}

impl ReviewGate {
    pub fn new() -> Self {
        Self {
            require_human: true,
            min_success_rate: 0.8,
            min_usage_count: 3,
        }
    }

    /// Allow auto-approval for patterns meeting criteria
    pub fn with_auto_approve(mut self, min_success_rate: f32, min_usage_count: u32) -> Self {
        self.require_human = false;
        self.min_success_rate = min_success_rate;
        self.min_usage_count = min_usage_count;
        self
    }

    /// Review a pattern for approval
    pub fn review(&self, pattern: &Pattern) -> ReviewResult {
        // Run automated checks
        let automated_result = self.automated_review(pattern);

        if let ReviewResult::Rejected(reason) = automated_result {
            info!("Pattern {} rejected: {}", pattern.name, reason);
            return ReviewResult::Rejected(reason);
        }

        // Check if human review is required
        if self.require_human {
            return ReviewResult::NeedsHumanReview(format!(
                "Pattern '{}' passed automated review but requires human approval",
                pattern.name
            ));
        }

        // Check auto-approval criteria
        if pattern.usage_count >= self.min_usage_count
            && pattern.success_rate >= self.min_success_rate
        {
            info!(
                "Pattern {} auto-approved (success: {:.0}%, usage: {})",
                pattern.name,
                pattern.success_rate * 100.0,
                pattern.usage_count
            );
            return ReviewResult::Approved;
        }

        ReviewResult::NeedsHumanReview(format!(
            "Pattern '{}' doesn't meet auto-approval criteria (need {}% success, {} uses; have {:.0}%, {})",
            pattern.name,
            (self.min_success_rate * 100.0) as u32,
            self.min_usage_count,
            pattern.success_rate * 100.0,
            pattern.usage_count
        ))
    }

    /// Run automated review checks
    fn automated_review(&self, pattern: &Pattern) -> ReviewResult {
        // Check pattern has required fields
        if pattern.name.is_empty() {
            return ReviewResult::Rejected("Pattern name is empty".to_string());
        }

        if pattern.content.is_empty() {
            return ReviewResult::Rejected("Pattern content is empty".to_string());
        }

        if pattern.description.is_empty() {
            return ReviewResult::Rejected("Pattern description is empty".to_string());
        }

        if pattern.when.is_empty() {
            return ReviewResult::Rejected("Pattern 'when' trigger is empty".to_string());
        }

        // Check content isn't too short
        if pattern.content.len() < 20 {
            return ReviewResult::Rejected("Pattern content too short (min 20 chars)".to_string());
        }

        // Check for potentially harmful patterns
        let content_lower = pattern.content.to_lowercase();
        let harmful_keywords = ["ignore", "skip", "bypass", "workaround", "hack"];

        for keyword in harmful_keywords {
            if content_lower.contains(keyword) {
                return ReviewResult::NeedsHumanReview(format!(
                    "Pattern contains potentially harmful keyword: '{}'",
                    keyword
                ));
            }
        }

        ReviewResult::Approved
    }

    /// Human approval (called externally after human review)
    pub fn human_approve(&self, pattern: &mut Pattern) -> Result<()> {
        pattern.approve();
        info!("Pattern {} approved by human", pattern.name);
        Ok(())
    }

    /// Human rejection (called externally after human review)
    pub fn human_reject(&self, pattern: &Pattern, reason: &str) -> ReviewResult {
        info!("Pattern {} rejected by human: {}", pattern.name, reason);
        ReviewResult::Rejected(format!("Human rejected: {}", reason))
    }
}

impl Default for ReviewGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::PatternCategory;

    #[test]
    fn test_automated_review_pass() {
        let gate = ReviewGate::new();
        let pattern = Pattern::new(
            "Test Pattern",
            PatternCategory::Decomposition,
            "A valid test pattern description",
        )
        .with_when("When testing the review gate")
        .with_content("This is valid pattern content that explains what to do in detail.");

        let result = gate.automated_review(&pattern);
        assert_eq!(result, ReviewResult::Approved);
    }

    #[test]
    fn test_automated_review_reject_empty() {
        let gate = ReviewGate::new();
        let pattern = Pattern::new("", PatternCategory::Decomposition, "Description");

        let result = gate.automated_review(&pattern);
        assert!(matches!(result, ReviewResult::Rejected(_)));
    }

    #[test]
    fn test_review_needs_human() {
        let gate = ReviewGate::new();
        let pattern = Pattern::new(
            "Test Pattern",
            PatternCategory::Decomposition,
            "A valid test pattern",
        )
        .with_when("When testing")
        .with_content("Valid content for the pattern that is long enough.");

        let result = gate.review(&pattern);
        assert!(matches!(result, ReviewResult::NeedsHumanReview(_)));
    }

    #[test]
    fn test_auto_approve() {
        let gate = ReviewGate::new().with_auto_approve(0.7, 2);

        let mut pattern = Pattern::new(
            "Test Pattern",
            PatternCategory::Decomposition,
            "A valid test pattern",
        )
        .with_when("When testing")
        .with_content("Valid content for the pattern that is long enough.");

        // Simulate successful usage
        pattern.usage_count = 5;
        pattern.success_rate = 0.9;

        let result = gate.review(&pattern);
        assert_eq!(result, ReviewResult::Approved);
    }
}
