//! PRD validator for `hox plan validate`.
//!
//! Parses a PRD description and applies structural checks and quality warnings.
//!
//! ## Errors (blocking):
//! 1. Missing `hox:prd:` sentinel
//! 2. Empty vision section
//! 3. No success criteria
//! 4. Empty scope-in list
//! 5. No functional requirements (`REQ-*`)
//! 6. No decomposition hints
//!
//! ## Warnings (non-blocking):
//! - Success criteria without measurable terms
//! - Decomposition hints without estimated files
//! - Requirements not following `REQ-NNN` / `NFR-NNN` pattern

use crate::hox_prd::{Prd, RequirementKind};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A blocking structural error found during PRD validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MissingSentinel,
    ParseError(String),
    EmptyVision,
    NoSuccessCriteria,
    EmptyScopeIn,
    NoFunctionalRequirements,
    NoDecompositionHints,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::MissingSentinel => {
                f.write_str("missing sentinel: description must start with 'hox:prd:'")
            }
            ValidationError::ParseError(msg) => write!(f, "parse error: {}", msg),
            ValidationError::EmptyVision => {
                f.write_str("vision section is empty — describe what problem this solves")
            }
            ValidationError::NoSuccessCriteria => {
                f.write_str("no success criteria — add at least one measurable criterion")
            }
            ValidationError::EmptyScopeIn => {
                f.write_str("scope-in is empty — list at least one in-scope item")
            }
            ValidationError::NoFunctionalRequirements => {
                f.write_str("no functional requirements (REQ-*) defined")
            }
            ValidationError::NoDecompositionHints => {
                f.write_str("no decomposition hints — add at least one work slice")
            }
        }
    }
}

/// A non-blocking quality warning found during PRD validation.
#[derive(Debug, Clone)]
pub enum ValidationWarning {
    /// A success criterion that contains no measurable terms.
    UnmeasurableSuccessCriteria(String),
    /// A decomposition hint that lists no estimated files.
    HintsWithoutFiles(String),
    /// A requirement ID that does not follow `REQ-NNN` / `NFR-NNN`.
    NonStandardRequirementId(String),
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationWarning::UnmeasurableSuccessCriteria(c) => write!(
                f,
                "criterion '{}' may not be measurable — add numbers, percentages, or time bounds",
                truncate(c, 60)
            ),
            ValidationWarning::HintsWithoutFiles(name) => write!(
                f,
                "hint '{}' has no estimated files — adding file patterns helps the agent assign work",
                truncate(name, 40)
            ),
            ValidationWarning::NonStandardRequirementId(id) => write!(
                f,
                "requirement ID '{}' does not follow REQ-NNN / NFR-NNN pattern",
                truncate(id, 30)
            ),
        }
    }
}

/// The outcome of validating a PRD.
///
/// `is_valid()` returns `true` when there are no [`ValidationError`]s.
/// Warnings may still be present.
#[derive(Debug)]
pub struct ValidationResult {
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    /// Returns `true` when no blocking errors were found.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Format a multi-line report for stdout/stderr.
    pub fn format_report(&self) -> String {
        let mut out = String::new();

        if !self.errors.is_empty() {
            out.push_str("Errors:\n");
            for e in &self.errors {
                out.push_str(&format!("  [ERROR] {}\n", e));
            }
        }

        if !self.warnings.is_empty() {
            out.push_str("Warnings:\n");
            for w in &self.warnings {
                out.push_str(&format!("  [WARN] {}\n", w));
            }
        }

        if self.errors.is_empty() && self.warnings.is_empty() {
            out.push_str("No issues found.\n");
        }

        let error_count = self.errors.len();
        let warning_count = self.warnings.len();
        let summary = if self.is_valid() && warning_count == 0 {
            "PRD is valid — all required fields present.".to_owned()
        } else if self.is_valid() {
            format!(
                "PRD is valid with {} warning{}.",
                warning_count,
                if warning_count == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "PRD is invalid: {} error{}, {} warning{}.",
                error_count,
                if error_count == 1 { "" } else { "s" },
                warning_count,
                if warning_count == 1 { "" } else { "s" }
            )
        };
        out.push_str(&format!("\n{}\n", summary));

        out
    }
}

// ---------------------------------------------------------------------------
// Validator functions
// ---------------------------------------------------------------------------

/// Validate a [`Prd`] struct.
pub fn validate_prd(prd: &Prd) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if prd.vision.trim().is_empty() {
        errors.push(ValidationError::EmptyVision);
    }

    if prd.success_criteria.is_empty() {
        errors.push(ValidationError::NoSuccessCriteria);
    }

    if prd.scope_in.is_empty() {
        errors.push(ValidationError::EmptyScopeIn);
    }

    if !prd
        .requirements
        .iter()
        .any(|r| r.kind == RequirementKind::Functional)
    {
        errors.push(ValidationError::NoFunctionalRequirements);
    }

    if prd.decomposition_hints.is_empty() {
        errors.push(ValidationError::NoDecompositionHints);
    }

    // Warnings
    for criterion in &prd.success_criteria {
        if !has_measurable_terms(criterion) {
            warnings.push(ValidationWarning::UnmeasurableSuccessCriteria(
                criterion.clone(),
            ));
        }
    }

    for hint in &prd.decomposition_hints {
        if hint.estimated_files.is_empty() {
            warnings.push(ValidationWarning::HintsWithoutFiles(hint.name.clone()));
        }
    }

    for req in &prd.requirements {
        if !is_structured_id(&req.id) {
            warnings.push(ValidationWarning::NonStandardRequirementId(req.id.clone()));
        }
    }

    ValidationResult { errors, warnings }
}

/// Parse a markdown string as a PRD then validate it.
pub fn validate_markdown(md: &str) -> ValidationResult {
    if !Prd::is_prd(md) {
        return ValidationResult {
            errors: vec![ValidationError::MissingSentinel],
            warnings: Vec::new(),
        };
    }

    match Prd::from_markdown(md) {
        Ok(prd) => validate_prd(&prd),
        Err(e) => ValidationResult {
            errors: vec![ValidationError::ParseError(e.to_string())],
            warnings: Vec::new(),
        },
    }
}

// ---------------------------------------------------------------------------
// Quality heuristics
// ---------------------------------------------------------------------------

/// A criterion is considered measurable if it contains a digit or a quantitative keyword.
fn has_measurable_terms(text: &str) -> bool {
    if text.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let keywords = [
        "all ",
        "every ",
        "zero ",
        "none ",
        "always ",
        "never ",
        "each ",
        "complete",
        "success",
        "fail",
        "pass",
        "no error",
        "without error",
        "correct",
        "accurate",
    ];
    let lower = text.to_lowercase();
    keywords.iter().any(|kw| lower.contains(kw))
}

/// Returns `true` if `id` matches `REQ-NNN` or `NFR-NNN` (case-insensitive).
fn is_structured_id(id: &str) -> bool {
    let upper = id.to_uppercase();
    (upper.starts_with("REQ-") || upper.starts_with("NFR-"))
        && upper.len() > 4
        && upper[4..]
            .chars()
            .next()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let end: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", end)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hox_prd::{DecompositionHint, Prd, Requirement, RequirementKind};

    fn complete_prd() -> Prd {
        Prd {
            version: "v1".to_owned(),
            title: "Test Feature".to_owned(),
            vision: "Enable users to do X efficiently.".to_owned(),
            actors: vec!["Developer".to_owned()],
            success_criteria: vec![
                "Feature completes within 200ms for 99% of requests".to_owned(),
            ],
            scope_in: vec!["Core implementation".to_owned()],
            scope_out: vec![],
            requirements: vec![Requirement {
                id: "REQ-001".to_owned(),
                description: "System must do the thing".to_owned(),
                kind: RequirementKind::Functional,
            }],
            constraints: vec![],
            decomposition_hints: vec![DecompositionHint {
                name: "Core Slice".to_owned(),
                description: "Implements the core functionality".to_owned(),
                estimated_files: vec!["crates/foo/".to_owned()],
                requirements: vec!["REQ-001".to_owned()],
            }],
        }
    }

    // --- error cases ---

    #[test]
    fn test_missing_sentinel_error() {
        let result = validate_markdown("This is not a PRD at all.");
        assert!(!result.is_valid());
        assert!(result.errors.contains(&ValidationError::MissingSentinel));
    }

    #[test]
    fn test_complete_prd_is_valid() {
        let prd = complete_prd();
        let result = validate_prd(&prd);
        assert!(
            result.is_valid(),
            "Expected valid, got: {}",
            result.format_report()
        );
    }

    #[test]
    fn test_empty_vision_is_error() {
        let mut prd = complete_prd();
        prd.vision = String::new();
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result.errors.contains(&ValidationError::EmptyVision));
    }

    #[test]
    fn test_no_success_criteria_is_error() {
        let mut prd = complete_prd();
        prd.success_criteria.clear();
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result.errors.contains(&ValidationError::NoSuccessCriteria));
    }

    #[test]
    fn test_empty_scope_in_is_error() {
        let mut prd = complete_prd();
        prd.scope_in.clear();
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result.errors.contains(&ValidationError::EmptyScopeIn));
    }

    #[test]
    fn test_no_functional_requirements_is_error() {
        let mut prd = complete_prd();
        prd.requirements = vec![Requirement {
            id: "NFR-001".to_owned(),
            description: "Must be fast".to_owned(),
            kind: RequirementKind::NonFunctional,
        }];
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result
            .errors
            .contains(&ValidationError::NoFunctionalRequirements));
    }

    #[test]
    fn test_no_decomposition_hints_is_error() {
        let mut prd = complete_prd();
        prd.decomposition_hints.clear();
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result
            .errors
            .contains(&ValidationError::NoDecompositionHints));
    }

    #[test]
    fn test_multiple_errors_accumulate() {
        let mut prd = complete_prd();
        prd.vision = String::new();
        prd.success_criteria.clear();
        prd.scope_in.clear();
        prd.requirements.clear();
        prd.decomposition_hints.clear();
        let result = validate_prd(&prd);
        assert!(!result.is_valid());
        assert!(result.errors.len() >= 5);
    }

    // --- warning cases ---

    #[test]
    fn test_vague_criterion_warns() {
        let mut prd = complete_prd();
        prd.success_criteria = vec!["The feature should work well".to_owned()];
        let result = validate_prd(&prd);
        assert!(result.is_valid());
        assert!(result.warnings.iter().any(|w| matches!(
            w,
            ValidationWarning::UnmeasurableSuccessCriteria(_)
        )));
    }

    #[test]
    fn test_measurable_criterion_no_warning() {
        let mut prd = complete_prd();
        prd.success_criteria = vec!["Response time under 100ms for all requests".to_owned()];
        let result = validate_prd(&prd);
        assert!(result.is_valid());
        assert!(!result.warnings.iter().any(|w| matches!(
            w,
            ValidationWarning::UnmeasurableSuccessCriteria(_)
        )));
    }

    #[test]
    fn test_hint_without_files_warns() {
        let mut prd = complete_prd();
        prd.decomposition_hints[0].estimated_files.clear();
        let result = validate_prd(&prd);
        assert!(result.is_valid());
        assert!(result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::HintsWithoutFiles(_))));
    }

    #[test]
    fn test_non_standard_id_warns() {
        let mut prd = complete_prd();
        prd.requirements = vec![Requirement {
            id: "auth-feature".to_owned(),
            description: "Must authenticate users".to_owned(),
            kind: RequirementKind::Functional,
        }];
        let result = validate_prd(&prd);
        assert!(result.is_valid());
        assert!(result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::NonStandardRequirementId(_))));
    }

    #[test]
    fn test_nfr_id_no_warning() {
        let mut prd = complete_prd();
        prd.requirements.push(Requirement {
            id: "NFR-001".to_owned(),
            description: "Must handle 10k concurrent users".to_owned(),
            kind: RequirementKind::NonFunctional,
        });
        let result = validate_prd(&prd);
        assert!(result.is_valid());
        // NFR-001 should not trigger the non-standard ID warning
        assert!(!result.warnings.iter().any(|w| {
            if let ValidationWarning::NonStandardRequirementId(id) = w {
                id == "NFR-001"
            } else {
                false
            }
        }));
    }

    // --- validate_markdown ---

    #[test]
    fn test_validate_markdown_valid_prd() {
        let prd = complete_prd();
        let md = prd.to_markdown();
        let result = validate_markdown(&md);
        assert!(result.is_valid(), "{}", result.format_report());
    }

    #[test]
    fn test_validate_markdown_not_prd() {
        let result = validate_markdown("Fix typo in README");
        assert!(!result.is_valid());
        assert!(result.errors.contains(&ValidationError::MissingSentinel));
    }

    // --- format_report ---

    #[test]
    fn test_format_report_no_issues() {
        let prd = complete_prd();
        let result = validate_prd(&prd);
        let report = result.format_report();
        // Either no-issues message or valid summary
        assert!(report.contains("valid") || report.contains("No issues"));
    }

    #[test]
    fn test_format_report_with_errors() {
        let result = validate_markdown("not a prd");
        let report = result.format_report();
        assert!(report.contains("ERROR"));
        assert!(report.contains("invalid"));
    }

    // --- heuristic unit tests ---

    #[test]
    fn test_measurable_terms_with_digits() {
        assert!(has_measurable_terms("99.9% uptime"));
        assert!(has_measurable_terms("within 200ms"));
    }

    #[test]
    fn test_measurable_terms_with_keywords() {
        assert!(has_measurable_terms("All requests must succeed"));
        assert!(has_measurable_terms("Zero downtime during migration"));
    }

    #[test]
    fn test_unmeasurable_terms() {
        assert!(!has_measurable_terms("The system should be fast"));
        assert!(!has_measurable_terms("Users are happy"));
    }

    #[test]
    fn test_structured_id_valid() {
        assert!(is_structured_id("REQ-001"));
        assert!(is_structured_id("req-001"));
        assert!(is_structured_id("NFR-001"));
        assert!(is_structured_id("REQ-A1"));
    }

    #[test]
    fn test_structured_id_invalid() {
        assert!(!is_structured_id("auth-feature"));
        assert!(!is_structured_id("REQ-"));
        assert!(!is_structured_id("FEAT-001"));
        assert!(!is_structured_id(""));
    }
}
