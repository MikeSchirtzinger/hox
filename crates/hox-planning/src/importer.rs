//! Markdown importer: converts arbitrary markdown documents into [`Prd`] structs.
//!
//! Two code paths:
//! - **Direct parse**: if `hox:prd:v1` sentinel is found, use [`Prd::from_markdown`].
//! - **LLM extraction**: for arbitrary markdown, an [`LlmClient`] maps headings to Prd fields.
//! - **Fallback**: empty [`Prd`] with [`Prd::backfill`] defaults when both paths fail.

use hox_core::Result;

use crate::hox_prd::{Prd, PRD_SENTINEL};
pub use crate::llm::{LlmClient, MockLlmClient};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Convert arbitrary markdown into a [`Prd`].
///
/// # Resolution order
/// 1. Direct parse when `hox:prd:v1` sentinel is detected.
/// 2. LLM extraction when an `llm` client is provided.
/// 3. Empty [`Prd`] with [`Prd::backfill`] defaults.
pub async fn import_markdown(md: &str, llm: Option<&dyn LlmClient>) -> Result<Prd> {
    // 1. Direct parse
    if md.contains(PRD_SENTINEL) {
        if let Ok(prd) = Prd::from_markdown(md) {
            return Ok(prd);
        }
    }

    // 2. LLM extraction
    if let Some(client) = llm {
        match extract_via_llm(md, client).await {
            Ok(prd) => return Ok(prd),
            Err(e) => tracing::warn!("LLM extraction failed: {}", e),
        }
    }

    // 3. Fallback
    let mut prd = Prd::new("Imported document");
    prd.backfill();
    Ok(prd)
}

// ---------------------------------------------------------------------------
// LLM extraction
// ---------------------------------------------------------------------------

fn build_extraction_prompt(md: &str) -> String {
    format!(
        r#"You are converting an arbitrary markdown document into a structured PRD.

Output ONLY a valid `hox:prd:v1` document using the exact format below. Do not add any explanation or commentary.

Required format:
```
hox:prd:v1 — <title>

## Vision

<one paragraph describing the problem and why it matters>

## Actors

- <stakeholder or user>

## Success Criteria

- [ ] <measurable outcome>

## Scope

### In Scope
- <item>

### Out of Scope
- <item>

## Requirements

### Functional
- **[REQ-001]**: <description>

### Non-Functional
- **[NFR-001]**: <description>

## Technical Constraints

- <constraint>

## Decomposition Hints

1. **<Name>**: <description>.
```

Rules:
- Extract the title from the document's main heading or first sentence.
- Map sections as best you can; use "(none)" for sections with no content.
- Use at least one requirement (REQ-001) even if sparse.
- Output ONLY the hox:prd:v1 block, nothing else.

Source document:
---
{}
---"#,
        md
    )
}

async fn extract_via_llm(md: &str, client: &dyn LlmClient) -> Result<Prd> {
    let prompt = build_extraction_prompt(md);
    let response = client.complete_simple(&prompt).await?;

    // The LLM may wrap output in a code fence — strip it.
    let cleaned = strip_code_fence(&response);

    Prd::from_markdown(cleaned)
}

/// Strip a leading ` ```...``` ` code fence if present, returning the inner content.
fn strip_code_fence(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(inner) = trimmed.strip_prefix("```") {
        // Skip the optional language tag on the opening fence line
        let after_tag = inner
            .find('\n')
            .map(|i| &inner[i + 1..])
            .unwrap_or(inner);
        if let Some(body) = after_tag.strip_suffix("```") {
            return body.trim();
        }
        if let Some(pos) = after_tag.rfind("```") {
            return after_tag[..pos].trim();
        }
        return after_tag.trim();
    }
    trimmed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn native_prd_md() -> &'static str {
        r#"hox:prd:v1 — Add webhook support

## Vision

Enable external services to subscribe to events via HTTP webhooks.

## Actors

- Service Owner

## Success Criteria

- [ ] Webhook registration succeeds

## Scope

### In Scope
- Webhook REST API

### Out of Scope
- GraphQL subscriptions

## Requirements

### Functional
- **[REQ-001]**: Accept webhook URL registrations via POST /webhooks

### Non-Functional
- **[NFR-001]**: Use exponential backoff

## Technical Constraints

- Must use existing NATS infrastructure

## Decomposition Hints

1. **API Layer**: Implement registration endpoints. Files: `crates/api/`. Covers: REQ-001.
"#
    }

    fn arbitrary_md() -> &'static str {
        r#"# Project: Better Search

## Overview
We need a faster search system that returns results in under 100ms.

## Users
- End users searching the catalog
- Admins tuning relevance

## Goals
- Sub-100ms p99 latency
- Support fuzzy matching
"#
    }

    #[tokio::test]
    async fn test_direct_parse_native_prd() {
        let prd = import_markdown(native_prd_md(), None)
            .await
            .expect("direct parse should succeed");
        assert_eq!(prd.title, "Add webhook support");
        assert_eq!(prd.version, "v1");
        assert!(!prd.requirements.is_empty());
    }

    #[tokio::test]
    async fn test_llm_extraction_with_mock() {
        // Mock returns a valid hox:prd:v1 document
        let mock_response = native_prd_md().to_string();
        let client = MockLlmClient {
            response: mock_response,
        };

        let prd = import_markdown(arbitrary_md(), Some(&client))
            .await
            .expect("LLM extraction should succeed");
        assert_eq!(prd.title, "Add webhook support");
        assert!(!prd.requirements.is_empty());
    }

    #[tokio::test]
    async fn test_llm_extraction_with_code_fence() {
        // LLM wraps response in a code fence
        let fenced = format!("```\n{}\n```", native_prd_md());
        let client = MockLlmClient { response: fenced };

        let prd = import_markdown(arbitrary_md(), Some(&client))
            .await
            .expect("code fence stripped and parsed");
        assert_eq!(prd.title, "Add webhook support");
    }

    #[tokio::test]
    async fn test_fallback_when_no_llm() {
        let prd = import_markdown(arbitrary_md(), None)
            .await
            .expect("fallback should succeed");
        // backfill ensures non-empty title and at least one requirement
        assert!(!prd.title.is_empty());
        assert!(!prd.requirements.is_empty());
    }

    #[tokio::test]
    async fn test_fallback_when_llm_returns_garbage() {
        let client = MockLlmClient {
            response: "not a prd at all!!!".to_string(),
        };

        let prd = import_markdown(arbitrary_md(), Some(&client))
            .await
            .expect("fallback after LLM failure should succeed");
        assert!(!prd.title.is_empty());
        assert!(!prd.requirements.is_empty());
    }

    #[tokio::test]
    async fn test_malformed_sentinel_direct_parse_succeeds_with_defaults() {
        // Sentinel present but no title or sections — from_markdown still succeeds
        // (returns a Prd with empty fields). Direct parse wins; LLM is not called.
        let sparse = "hox:prd:v1 — Sparse doc\n\nNo recognized sections here.";
        let client = MockLlmClient {
            // Would return this if called, but it shouldn't be called
            response: native_prd_md().to_string(),
        };

        let prd = import_markdown(sparse, Some(&client))
            .await
            .expect("direct parse succeeds on sparse doc");
        // Title comes from the sentinel line
        assert_eq!(prd.title, "Sparse doc");
    }

    #[tokio::test]
    async fn test_no_sentinel_uses_llm() {
        // No sentinel → direct parse skipped → LLM called
        let mock_response = native_prd_md().to_string();
        let client = MockLlmClient {
            response: mock_response,
        };

        let prd = import_markdown(arbitrary_md(), Some(&client))
            .await
            .expect("LLM extraction should succeed");
        assert_eq!(prd.title, "Add webhook support");
    }

    #[tokio::test]
    async fn test_empty_markdown_fallback() {
        let prd = import_markdown("", None)
            .await
            .expect("empty doc should produce backfilled Prd");
        assert!(!prd.title.is_empty());
    }

    #[test]
    fn test_strip_code_fence_with_lang_tag() {
        let input = "```markdown\nhox:prd:v1 — Title\n```";
        assert_eq!(strip_code_fence(input), "hox:prd:v1 — Title");
    }

    #[test]
    fn test_strip_code_fence_no_fence() {
        let input = "hox:prd:v1 — Title";
        assert_eq!(strip_code_fence(input), "hox:prd:v1 — Title");
    }
}
