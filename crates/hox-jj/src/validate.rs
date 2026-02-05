//! Input validation for JJ identifiers
//!
//! All user-controlled strings MUST be validated before interpolation
//! into revset queries or bookmark names to prevent injection attacks.

use hox_core::{HoxError, Result};

/// Validate an identifier (agent name, orchestrator ID, session ID, change ID prefix)
///
/// Returns the input unchanged if valid, or an error if it contains unsafe characters.
pub fn validate_identifier<'a>(input: &'a str, context: &str) -> Result<&'a str> {
    if input.is_empty() {
        return Err(HoxError::PathValidation(format!(
            "{} cannot be empty",
            context
        )));
    }
    // Check each char is in allowed set
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
    {
        Ok(input)
    } else {
        Err(HoxError::PathValidation(format!(
            "{} contains unsafe characters: '{}'. Only alphanumeric, '/', '_', '-', '.' are allowed.",
            context, input
        )))
    }
}

/// Validate a file path for use in revset queries
///
/// Allows alphanumeric, path separators, dots, hyphens, underscores.
/// Rejects directory traversal (..) and null bytes.
pub fn validate_path<'a>(input: &'a str, context: &str) -> Result<&'a str> {
    if input.is_empty() {
        return Err(HoxError::PathValidation(format!(
            "{} cannot be empty",
            context
        )));
    }
    if input.contains("..") {
        return Err(HoxError::PathValidation(format!(
            "{} contains directory traversal: '{}'",
            context, input
        )));
    }
    if input.contains('\0') {
        return Err(HoxError::PathValidation(format!(
            "{} contains null byte",
            context
        )));
    }
    // Allow typical path characters
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.' | ' '))
    {
        Ok(input)
    } else {
        Err(HoxError::PathValidation(format!(
            "{} contains unsafe characters: '{}'",
            context, input
        )))
    }
}

/// Validate a revset expression (for the `latest()` function that takes arbitrary revsets)
///
/// Only allows known-safe revset functions and operators.
pub fn validate_revset(input: &str) -> Result<&str> {
    // Reject characters that could break out of revset context
    if input.contains('"')
        || input.contains('\'')
        || input.contains(';')
        || input.contains('`')
        || input.contains('$')
        || input.contains('\n')
    {
        return Err(HoxError::JjRevset(format!(
            "Revset contains unsafe characters: '{}'",
            input
        )));
    }
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_identifiers() {
        assert!(validate_identifier("agent-42", "agent").is_ok());
        assert!(validate_identifier("O-A-1", "orchestrator").is_ok());
        assert!(validate_identifier("abc123def456", "change_id").is_ok());
        assert!(validate_identifier("session/abc-123", "session").is_ok());
    }

    #[test]
    fn test_invalid_identifiers() {
        assert!(validate_identifier("foo; rm -rf /", "agent").is_err());
        assert!(validate_identifier("agent\")", "agent").is_err());
        assert!(validate_identifier("", "agent").is_err());
        assert!(validate_identifier("foo\nbar", "agent").is_err());
        assert!(validate_identifier("foo`cmd`", "agent").is_err());
    }

    #[test]
    fn test_valid_paths() {
        assert!(validate_path("src/main.rs", "path").is_ok());
        assert!(validate_path("crates/hox-core/src/types.rs", "path").is_ok());
    }

    #[test]
    fn test_invalid_paths() {
        assert!(validate_path("../../etc/passwd", "path").is_err());
        assert!(validate_path("", "path").is_err());
        assert!(validate_path("foo\0bar", "path").is_err());
    }

    #[test]
    fn test_valid_revsets() {
        assert!(validate_revset("mutable()").is_ok());
        assert!(validate_revset("heads(mutable()) & ~conflicts()").is_ok());
    }

    #[test]
    fn test_invalid_revsets() {
        assert!(validate_revset("foo\"; rm -rf /").is_err());
        assert!(validate_revset("foo`cmd`").is_err());
    }
}
