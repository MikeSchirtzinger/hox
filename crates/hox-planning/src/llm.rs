//! Unified LLM client abstraction for the hox-planning crate.
//!
//! Both the importer and decomposition modules use this single trait so callers
//! can pass the same client to either subsystem.
//!
//! ## Signature
//!
//! The primary method is `complete(system, user)` — system may be an empty
//! string for callers that only have a single prompt.  A blanket
//! `complete_simple` helper is provided for single-prompt use cases.

use async_trait::async_trait;
use hox_core::Result;

/// Abstraction over a single-turn LLM completion for testability.
///
/// Implementations must be `Send + Sync` so they can be used across `.await`
/// points in async contexts.
///
/// # Method contract
///
/// - `complete(system, user)` — `system` may be empty.
/// - `complete_simple(prompt)` — calls `complete("", prompt)` by default.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Run a single completion with an explicit system prompt and user message.
    async fn complete(&self, system: &str, user: &str) -> Result<String>;

    /// Convenience wrapper: run a completion with only a user prompt.
    async fn complete_simple(&self, prompt: &str) -> Result<String> {
        self.complete("", prompt).await
    }
}

// ---------------------------------------------------------------------------
// MockLlmClient (for tests and examples across the crate)
// ---------------------------------------------------------------------------

/// A mock [`LlmClient`] that returns a fixed response string regardless of input.
pub struct MockLlmClient {
    pub response: String,
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Ok(self.response.clone())
    }
}
