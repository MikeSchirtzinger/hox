//! Interactive 7-step PRD discovery using [`LlmClient`] to synthesize responses.
//!
//! [`run_discovery`] takes pre-collected `(step_name, user_response)` pairs and
//! calls the LLM once to synthesize them into a complete [`Prd`].
//!
//! ## Discovery steps (in order)
//!
//! | # | Name | PRD field(s) |
//! |---|------|--------------|
//! | 1 | vision | `vision` |
//! | 2 | actors | `actors` |
//! | 3 | success_criteria | `success_criteria` |
//! | 4 | scope | `scope_in`, `scope_out` |
//! | 5 | requirements | `requirements` |
//! | 6 | constraints | `constraints` |
//! | 7 | decomposition | `decomposition_hints` |

use crate::hox_prd::{Prd, PRD_SENTINEL};
use crate::importer::LlmClient;
use hox_core::Result;

// ---------------------------------------------------------------------------
// Discovery step definitions
// ---------------------------------------------------------------------------

/// A discovery step with a name and a guiding prompt shown to the user.
pub struct DiscoveryStep {
    pub name: &'static str,
    pub prompt: &'static str,
}

/// The seven sequential discovery steps.
pub const DISCOVERY_STEPS: &[DiscoveryStep] = &[
    DiscoveryStep {
        name: "vision",
        prompt: "What problem does this solve? What's the vision?",
    },
    DiscoveryStep {
        name: "actors",
        prompt: "Who are the actors/stakeholders?",
    },
    DiscoveryStep {
        name: "success_criteria",
        prompt: "What measurable outcomes define success?",
    },
    DiscoveryStep {
        name: "scope",
        prompt: "What's in scope? What's explicitly out of scope?",
    },
    DiscoveryStep {
        name: "requirements",
        prompt: "List functional (REQ-*) and non-functional (NFR-*) requirements.",
    },
    DiscoveryStep {
        name: "constraints",
        prompt: "What technical constraints exist?",
    },
    DiscoveryStep {
        name: "decomposition",
        prompt: "How should this be decomposed? Suggest slices and shared contracts.",
    },
];

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run interactive discovery: synthesize `(step_name, user_response)` pairs into a [`Prd`].
///
/// Builds a single prompt incorporating all responses and asks the LLM to emit
/// a valid `hox:prd:v1` document. Falls back to a backfilled empty [`Prd`]
/// when the LLM call or parse fails.
///
/// # Arguments
///
/// * `title` - Short title for the PRD (e.g. the feature name).
/// * `responses` - One entry per completed discovery step as `(step_name, user_response)`.
/// * `llm` - LLM client used to synthesize the responses.
pub async fn run_discovery(
    title: &str,
    responses: &[(String, String)],
    llm: &dyn LlmClient,
) -> Result<Prd> {
    let prompt = build_synthesis_prompt(title, responses);
    match llm.complete_simple(&prompt).await {
        Ok(raw) => {
            let cleaned = strip_code_fence(&raw);
            match Prd::from_markdown(cleaned) {
                Ok(prd) => Ok(prd),
                Err(e) => {
                    tracing::warn!("discovery synthesis parse failed: {}", e);
                    let mut prd = Prd::new(title);
                    prd.backfill();
                    Ok(prd)
                }
            }
        }
        Err(e) => {
            tracing::warn!("discovery LLM call failed: {}", e);
            let mut prd = Prd::new(title);
            prd.backfill();
            Ok(prd)
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

fn build_synthesis_prompt(title: &str, responses: &[(String, String)]) -> String {
    let mut parts = Vec::new();

    parts.push(format!(
        "You are synthesizing structured discovery responses into a PRD.\n\
         Title: {title}\n\n\
         The user has answered the following discovery questions:\n"
    ));

    for (step, response) in responses {
        // Find the matching step prompt for context
        let prompt = DISCOVERY_STEPS
            .iter()
            .find(|s| s.name == step.as_str())
            .map(|s| s.prompt)
            .unwrap_or(step.as_str());
        parts.push(format!("**{step}** ({prompt})\n{response}\n"));
    }

    parts.push(format!(
        r#"
Output ONLY a valid `{PRD_SENTINEL}` document using this exact format:

```
{PRD_SENTINEL} — {title}

## Vision

<synthesized vision from the user's response>

## Actors

- <actor>

## Success Criteria

- [ ] <measurable criterion>

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
- Synthesize and clean up the user's raw responses into coherent PRD content.
- Extract requirements from the requirements step using REQ-NNN / NFR-NNN identifiers.
- Map scope "in" and "out" from the scope step.
- Build decomposition hints from the decomposition step.
- Use "(none)" for any section with no content.
- Output ONLY the hox:prd:v1 block, nothing else.
"#
    ));

    parts.join("\n")
}

// ---------------------------------------------------------------------------
// Code fence stripping (mirrored from importer for internal use)
// ---------------------------------------------------------------------------

fn strip_code_fence(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(inner) = trimmed.strip_prefix("```") {
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
    use crate::importer::MockLlmClient;

    fn sample_prd_md(title: &str) -> String {
        format!(
            r#"hox:prd:v1 — {title}

## Vision

Enable external services to subscribe to events.

## Actors

- Service Owner

## Success Criteria

- [ ] Webhook registration succeeds within 200ms

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
        )
    }

    fn sample_responses() -> Vec<(String, String)> {
        vec![
            (
                "vision".to_owned(),
                "Enable external services to receive event callbacks via HTTP webhooks.".to_owned(),
            ),
            (
                "actors".to_owned(),
                "Service owners who register webhooks and external services that receive them."
                    .to_owned(),
            ),
            (
                "success_criteria".to_owned(),
                "Registration succeeds within 200ms, delivery retries 3 times.".to_owned(),
            ),
            (
                "scope".to_owned(),
                "In: Webhook REST API. Out: GraphQL subscriptions.".to_owned(),
            ),
            (
                "requirements".to_owned(),
                "REQ-001: Accept webhook registrations. NFR-001: Exponential backoff.".to_owned(),
            ),
            (
                "constraints".to_owned(),
                "Must use existing NATS infrastructure.".to_owned(),
            ),
            (
                "decomposition".to_owned(),
                "API Layer: registration endpoints. Delivery Engine: fanout and retry.".to_owned(),
            ),
        ]
    }

    #[tokio::test]
    async fn test_discovery_with_valid_llm_response() {
        let prd_md = sample_prd_md("Add webhook support");
        let client = MockLlmClient { response: prd_md };

        let responses = sample_responses();
        let prd = run_discovery("Add webhook support", &responses, &client)
            .await
            .expect("should succeed");

        assert_eq!(prd.title, "Add webhook support");
        assert!(!prd.vision.is_empty());
        assert!(!prd.requirements.is_empty());
    }

    #[tokio::test]
    async fn test_discovery_with_code_fence_response() {
        let prd_md = sample_prd_md("Fenced PRD");
        let fenced = format!("```\n{}\n```", prd_md);
        let client = MockLlmClient { response: fenced };

        let responses = sample_responses();
        let prd = run_discovery("Fenced PRD", &responses, &client)
            .await
            .expect("should succeed");

        assert_eq!(prd.title, "Fenced PRD");
    }

    #[tokio::test]
    async fn test_discovery_fallback_on_garbage_response() {
        let client = MockLlmClient {
            response: "not a prd at all".to_owned(),
        };

        let responses = sample_responses();
        let prd = run_discovery("Fallback Test", &responses, &client)
            .await
            .expect("should succeed with fallback");

        // Fallback uses title and backfill
        assert_eq!(prd.title, "Fallback Test");
        assert!(!prd.requirements.is_empty()); // backfill adds a placeholder
    }

    #[tokio::test]
    async fn test_discovery_steps_count() {
        assert_eq!(DISCOVERY_STEPS.len(), 7, "must have exactly 7 discovery steps");
    }

    #[tokio::test]
    async fn test_discovery_step_names() {
        let names: Vec<&str> = DISCOVERY_STEPS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            &[
                "vision",
                "actors",
                "success_criteria",
                "scope",
                "requirements",
                "constraints",
                "decomposition"
            ]
        );
    }

    #[tokio::test]
    async fn test_discovery_empty_responses() {
        let prd_md = sample_prd_md("Empty Responses");
        let client = MockLlmClient { response: prd_md };

        let prd = run_discovery("Empty Responses", &[], &client)
            .await
            .expect("should succeed with empty responses");

        assert_eq!(prd.title, "Empty Responses");
    }
}
