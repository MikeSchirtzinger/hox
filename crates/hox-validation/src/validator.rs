//! Validator agent implementation

use hox_core::{ChangeId, Result, ScoringWeights};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for a validator
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// Validator identifier
    pub id: String,
    /// Checks to perform
    pub checks: Vec<ValidationCheck>,
    /// Scoring weights
    pub weights: ScoringWeights,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            id: format!("validator-{}", &uuid::Uuid::new_v4().to_string()[..8]),
            checks: vec![
                ValidationCheck::Compilation,
                ValidationCheck::Tests,
                ValidationCheck::MutationCompliance,
            ],
            weights: ScoringWeights::default(),
        }
    }
}

/// Types of validation checks
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationCheck {
    /// Code compiles without errors
    Compilation,
    /// Tests pass
    Tests,
    /// Work complies with mutation decisions
    MutationCompliance,
    /// Contracts are adhered to
    ContractAdherence,
    /// Code quality metrics
    CodeQuality,
    /// Custom check with name
    Custom(String),
}

/// Artifact metadata for validation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationArtifactRef {
    /// Relative path from .hox/artifacts/
    pub path: PathBuf,
    /// Artifact type description
    pub artifact_type: String,
    /// Human-readable description
    pub description: String,
}

/// Result of a single validation check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: ValidationCheck,
    pub passed: bool,
    pub score: f32,
    pub details: String,
    /// Validation artifacts (screenshots, logs, etc.)
    #[serde(default)]
    pub artifacts: Vec<ValidationArtifactRef>,
}

/// Result of validation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationResult {
    Pass,
    Fail,
    Partial,
}

/// Full validation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Validator ID
    pub validator_id: String,
    /// Change being validated
    pub change_id: ChangeId,
    /// Overall result
    pub result: ValidationResult,
    /// Overall score (0.0 - 1.0)
    pub score: f32,
    /// Individual check results
    pub checks: Vec<CheckResult>,
    /// Quality score
    pub quality: f32,
    /// Completeness score
    pub completeness: f32,
    /// Time score
    pub time: f32,
    /// Efficiency score
    pub efficiency: f32,
    /// Additional notes
    pub notes: Vec<String>,
}

impl ValidationReport {
    pub fn new(validator_id: impl Into<String>, change_id: impl Into<String>) -> Self {
        Self {
            validator_id: validator_id.into(),
            change_id: change_id.into(),
            result: ValidationResult::Partial,
            score: 0.0,
            checks: Vec::new(),
            quality: 0.0,
            completeness: 0.0,
            time: 0.0,
            efficiency: 0.0,
            notes: Vec::new(),
        }
    }

    pub fn add_check(&mut self, result: CheckResult) {
        self.checks.push(result);
    }

    pub fn calculate_score(&mut self, weights: &ScoringWeights) {
        self.score = weights.calculate(self.quality, self.completeness, self.time, self.efficiency);

        // Determine overall result
        let all_passed = self.checks.iter().all(|c| c.passed);
        let any_passed = self.checks.iter().any(|c| c.passed);

        self.result = if all_passed {
            ValidationResult::Pass
        } else if any_passed {
            ValidationResult::Partial
        } else {
            ValidationResult::Fail
        };
    }
}

/// Validator agent
pub struct Validator {
    config: ValidatorConfig,
}

impl Validator {
    pub fn new(config: ValidatorConfig) -> Self {
        Self { config }
    }

    /// Get validator ID
    pub fn id(&self) -> &str {
        &self.config.id
    }

    /// Validate a change
    pub async fn validate(&self, change_id: &ChangeId) -> Result<ValidationReport> {
        let mut report = ValidationReport::new(&self.config.id, change_id);

        for check in &self.config.checks {
            let result = self.run_check(check, change_id).await?;
            report.add_check(result);
        }

        // Calculate individual scores
        report.quality = self.calculate_quality_score(&report);
        report.completeness = self.calculate_completeness_score(&report);
        report.time = 1.0; // TODO: Calculate from telemetry
        report.efficiency = self.calculate_efficiency_score(&report);

        report.calculate_score(&self.config.weights);

        Ok(report)
    }

    /// Run a single validation check
    async fn run_check(
        &self,
        check: &ValidationCheck,
        change_id: &ChangeId,
    ) -> Result<CheckResult> {
        match check {
            ValidationCheck::Compilation => self.check_compilation(change_id).await,
            ValidationCheck::Tests => self.check_tests(change_id).await,
            ValidationCheck::MutationCompliance => self.check_mutation_compliance(change_id).await,
            ValidationCheck::ContractAdherence => self.check_contracts(change_id).await,
            ValidationCheck::CodeQuality => self.check_code_quality(change_id).await,
            ValidationCheck::Custom(name) => self.check_custom(name, change_id).await,
        }
    }

    async fn check_compilation(&self, _change_id: &ChangeId) -> Result<CheckResult> {
        // TODO: Actually run cargo check
        Ok(CheckResult {
            check: ValidationCheck::Compilation,
            passed: true,
            score: 1.0,
            details: "Compilation check placeholder".to_string(),
            artifacts: Vec::new(),
        })
    }

    async fn check_tests(&self, _change_id: &ChangeId) -> Result<CheckResult> {
        // TODO: Actually run cargo test
        Ok(CheckResult {
            check: ValidationCheck::Tests,
            passed: true,
            score: 1.0,
            details: "Tests check placeholder".to_string(),
            artifacts: Vec::new(),
        })
    }

    async fn check_mutation_compliance(&self, _change_id: &ChangeId) -> Result<CheckResult> {
        // TODO: Check that work complies with mutation decisions
        Ok(CheckResult {
            check: ValidationCheck::MutationCompliance,
            passed: true,
            score: 1.0,
            details: "Mutation compliance check placeholder".to_string(),
            artifacts: Vec::new(),
        })
    }

    async fn check_contracts(&self, _change_id: &ChangeId) -> Result<CheckResult> {
        // TODO: Check contract adherence
        Ok(CheckResult {
            check: ValidationCheck::ContractAdherence,
            passed: true,
            score: 1.0,
            details: "Contract adherence check placeholder".to_string(),
            artifacts: Vec::new(),
        })
    }

    async fn check_code_quality(&self, _change_id: &ChangeId) -> Result<CheckResult> {
        // TODO: Run clippy and other quality checks
        Ok(CheckResult {
            check: ValidationCheck::CodeQuality,
            passed: true,
            score: 1.0,
            details: "Code quality check placeholder".to_string(),
            artifacts: Vec::new(),
        })
    }

    async fn check_custom(&self, name: &str, _change_id: &ChangeId) -> Result<CheckResult> {
        Ok(CheckResult {
            check: ValidationCheck::Custom(name.to_string()),
            passed: true,
            score: 1.0,
            details: format!("Custom check '{}' placeholder", name),
            artifacts: Vec::new(),
        })
    }

    fn calculate_quality_score(&self, report: &ValidationReport) -> f32 {
        let quality_checks: Vec<&CheckResult> = report
            .checks
            .iter()
            .filter(|c| {
                matches!(
                    c.check,
                    ValidationCheck::Compilation
                        | ValidationCheck::Tests
                        | ValidationCheck::CodeQuality
                )
            })
            .collect();

        if quality_checks.is_empty() {
            return 1.0;
        }

        quality_checks.iter().map(|c| c.score).sum::<f32>() / quality_checks.len() as f32
    }

    fn calculate_completeness_score(&self, report: &ValidationReport) -> f32 {
        let passed = report.checks.iter().filter(|c| c.passed).count();
        let total = report.checks.len();

        if total == 0 {
            return 1.0;
        }

        passed as f32 / total as f32
    }

    fn calculate_efficiency_score(&self, _report: &ValidationReport) -> f32 {
        // TODO: Calculate from telemetry (tool calls, failures, etc.)
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validator_basic() {
        let config = ValidatorConfig::default();
        let validator = Validator::new(config);

        let report = validator
            .validate(&"test-change-id".to_string())
            .await
            .unwrap();

        assert_eq!(report.result, ValidationResult::Pass);
        assert!(report.score > 0.0);
    }

    #[test]
    fn test_validation_report_scoring() {
        let mut report = ValidationReport::new("validator-1", "change-1");
        report.quality = 0.8;
        report.completeness = 1.0;
        report.time = 0.9;
        report.efficiency = 0.7;

        let weights = ScoringWeights::default();
        report.calculate_score(&weights);

        // Score should be: 0.35*0.8 + 0.30*1.0 + 0.20*0.9 + 0.15*0.7 = 0.865
        assert!((report.score - 0.865).abs() < 0.001);
    }
}
