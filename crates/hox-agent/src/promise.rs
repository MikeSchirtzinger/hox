//! Completion promise parsing for Ralph-style completion signals
//!
//! This module parses `<promise>COMPLETE</promise>` markers from agent output,
//! allowing agents to signal task completion with optional reasoning and confidence.
//!
//! ## Format
//!
//! Basic completion:
//! ```xml
//! <promise>COMPLETE</promise>
//! ```
//!
//! With reasoning:
//! ```xml
//! <promise>COMPLETE</promise>
//! <completion_reasoning>
//! All tests passed, implementation complete, documentation updated.
//! Confidence: 95%
//! </completion_reasoning>
//! ```

use serde::{Deserialize, Serialize};

/// Completion promise parsed from agent output
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionPromise {
    /// Whether the agent signaled completion
    pub is_complete: bool,
    /// Optional reasoning for completion
    pub reasoning: Option<String>,
    /// Optional confidence score (0.0-1.0)
    pub confidence: Option<f32>,
    /// The raw promise block if found
    pub raw_block: Option<String>,
}

impl CompletionPromise {
    /// Parse completion promise from agent output
    ///
    /// Looks for `<promise>COMPLETE</promise>` marker and optional
    /// `<completion_reasoning>...</completion_reasoning>` block.
    ///
    /// # Examples
    ///
    /// ```
    /// use hox_agent::CompletionPromise;
    ///
    /// let output = "<promise>COMPLETE</promise>";
    /// let promise = CompletionPromise::parse(output);
    /// assert!(promise.is_complete);
    /// ```
    pub fn parse(output: &str) -> Self {
        // Look for <promise>COMPLETE</promise>
        let promise_start = output.find("<promise>");
        let promise_end = output.find("</promise>");

        let is_complete = if let (Some(start), Some(end)) = (promise_start, promise_end) {
            let content_start = start + "<promise>".len();
            if content_start < end {
                let content = output[content_start..end].trim();
                content.eq_ignore_ascii_case("COMPLETE")
            } else {
                false
            }
        } else {
            false
        };

        if !is_complete {
            return Self::default();
        }

        // Extract the raw promise block
        let raw_block = promise_start
            .zip(promise_end)
            .map(|(start, end)| output[start..end + "</promise>".len()].to_string());

        // Look for optional reasoning
        let reasoning = extract_tag_content(output, "completion_reasoning");

        // Extract confidence if present in reasoning
        let confidence = reasoning.as_ref().and_then(|r| extract_confidence(r));

        Self {
            is_complete: true,
            reasoning,
            confidence,
            raw_block,
        }
    }

    /// Check if the promise indicates completion
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Get confidence score if available
    pub fn confidence(&self) -> Option<f32> {
        self.confidence
    }
}

/// Extract content from an XML-style tag
fn extract_tag_content(text: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    let start = text.find(&start_tag)?;
    let end = text.find(&end_tag)?;

    let content_start = start + start_tag.len();
    if content_start >= end {
        return None;
    }

    Some(text[content_start..end].trim().to_string())
}

/// Extract confidence percentage from reasoning text
///
/// Looks for patterns like "Confidence: 95%" or "95% confident"
fn extract_confidence(reasoning: &str) -> Option<f32> {
    // Look for "Confidence: XX%" pattern
    if let Some(start) = reasoning.find("Confidence:") {
        let after = &reasoning[start + "Confidence:".len()..];
        return parse_percentage(after);
    }

    // Look for "XX% confident" pattern
    if let Some(pos) = reasoning.find("% confident") {
        let before = &reasoning[..pos];
        if let Some(num_start) = before.rfind(char::is_whitespace) {
            return parse_percentage(&before[num_start..]);
        }
    }

    None
}

/// Parse a percentage string to a 0.0-1.0 float
fn parse_percentage(text: &str) -> Option<f32> {
    let trimmed = text.trim();
    let num_str = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>();

    let value: f32 = num_str.parse().ok()?;

    // Convert percentage to 0.0-1.0 range
    if value > 1.0 {
        Some(value / 100.0)
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_complete() {
        let output = "<promise>COMPLETE</promise>";
        let promise = CompletionPromise::parse(output);
        assert!(promise.is_complete);
        assert!(promise.reasoning.is_none());
        assert!(promise.confidence.is_none());
    }

    #[test]
    fn test_complete_case_insensitive() {
        let output = "<promise>complete</promise>";
        let promise = CompletionPromise::parse(output);
        assert!(promise.is_complete);
    }

    #[test]
    fn test_complete_with_reasoning() {
        let output = r#"
<promise>COMPLETE</promise>
<completion_reasoning>
All tests passed, implementation complete, documentation updated.
Confidence: 95%
</completion_reasoning>
"#;
        let promise = CompletionPromise::parse(output);
        assert!(promise.is_complete);
        assert!(promise.reasoning.is_some());
        let reasoning = promise.reasoning.unwrap();
        assert!(reasoning.contains("All tests passed"));
        assert!(reasoning.contains("Confidence: 95%"));
    }

    #[test]
    fn test_confidence_extraction() {
        let reasoning = "All tests passed. Confidence: 95%";
        let confidence = extract_confidence(reasoning);
        assert_eq!(confidence, Some(0.95));
    }

    #[test]
    fn test_confidence_alternative_format() {
        let reasoning = "I am 80% confident in this solution.";
        let confidence = extract_confidence(reasoning);
        assert_eq!(confidence, Some(0.80));
    }

    #[test]
    fn test_no_promise() {
        let output = "Just some regular output without a promise.";
        let promise = CompletionPromise::parse(output);
        assert!(!promise.is_complete);
    }

    #[test]
    fn test_incomplete_promise_tag() {
        let output = "<promise>INCOMPLETE</promise>";
        let promise = CompletionPromise::parse(output);
        assert!(!promise.is_complete);
    }

    #[test]
    fn test_promise_in_context() {
        let output = r#"
I've completed all the required tasks:
- Fixed all tests
- Updated documentation
- Verified builds

<promise>COMPLETE</promise>

The implementation is ready for review.
"#;
        let promise = CompletionPromise::parse(output);
        assert!(promise.is_complete);
    }

    #[test]
    fn test_raw_block_capture() {
        let output = "<promise>COMPLETE</promise>";
        let promise = CompletionPromise::parse(output);
        assert_eq!(promise.raw_block, Some("<promise>COMPLETE</promise>".to_string()));
    }

    #[test]
    fn test_extract_tag_content() {
        let text = "<test>content here</test>";
        let content = extract_tag_content(text, "test");
        assert_eq!(content, Some("content here".to_string()));
    }

    #[test]
    fn test_extract_tag_content_multiline() {
        let text = r#"
<reasoning>
Line 1
Line 2
Line 3
</reasoning>
"#;
        let content = extract_tag_content(text, "reasoning");
        assert!(content.is_some());
        assert!(content.unwrap().contains("Line 1"));
    }

    #[test]
    fn test_parse_percentage() {
        assert_eq!(parse_percentage("95%"), Some(0.95));
        assert_eq!(parse_percentage("  80  "), Some(0.80));
        assert_eq!(parse_percentage("0.95"), Some(0.95));
        assert_eq!(parse_percentage("100"), Some(1.00));
    }
}
