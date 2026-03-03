//! Authentication for Anthropic API
//!
//! Supports two authentication methods:
//! 1. Claude Code OAuth token (CLAUDE_CODE_OAUTH_TOKEN) - zero API cost with subscription
//! 2. Anthropic API key (ANTHROPIC_API_KEY) - standard API access

use hox_core::{HoxError, Result};
use std::env;

/// Get authentication token for Anthropic API
///
/// Priority:
/// 1. CLAUDE_CODE_OAUTH_TOKEN (subscription, zero API cost)
/// 2. ANTHROPIC_API_KEY (standard API access)
pub fn get_auth_token() -> Result<String> {
    // Check for Claude Code OAuth token first (subscription access)
    if let Ok(oauth_token) = env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        tracing::info!("Using Claude Code OAuth token (subscription)");
        return Ok(oauth_token);
    }

    // Fall back to standard API key
    if let Ok(api_key) = env::var("ANTHROPIC_API_KEY") {
        tracing::info!("Using ANTHROPIC_API_KEY");
        return Ok(api_key);
    }

    Err(HoxError::Auth(
        "No authentication found. Set either:\n\
         - CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-... (for subscription access)\n\
         - ANTHROPIC_API_KEY=sk-ant-api03-...       (for API access)"
            .to_string(),
    ))
}

/// Get authentication token for OpenAI-compatible APIs.
///
/// Priority:
/// 1. `OPENAI_API_KEY`   — standard OpenAI or any compatible endpoint
/// 2. `OPENROUTER_API_KEY` — OpenRouter aggregator
///
/// Returns `Err` only when **neither** variable is set.  Ollama and other
/// unauthenticated endpoints do not need a key — callers may handle the
/// `Err` by defaulting to an empty string.
pub fn get_openai_auth_token() -> Result<String> {
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        tracing::info!("Using OPENAI_API_KEY");
        return Ok(key);
    }

    if let Ok(key) = env::var("OPENROUTER_API_KEY") {
        tracing::info!("Using OPENROUTER_API_KEY");
        return Ok(key);
    }

    Err(HoxError::Auth(
        "No OpenAI auth found. Set either:\n\
         - OPENAI_API_KEY=sk-...       (for OpenAI or compatible APIs)\n\
         - OPENROUTER_API_KEY=sk-or-... (for OpenRouter)"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to prevent concurrent env var modifications
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env_vars<F, R>(vars: &[(&str, Option<&str>)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_LOCK.lock().unwrap();

        // Save original values
        let originals: Vec<_> = vars.iter().map(|(k, _)| (*k, env::var(k).ok())).collect();

        // Set test values
        for (key, value) in vars {
            match value {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }

        let result = f();

        // Restore original values
        for (key, original) in originals {
            match original {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }

        result
    }

    #[test]
    fn test_oauth_token_priority() {
        with_env_vars(
            &[
                ("CLAUDE_CODE_OAUTH_TOKEN", Some("test-oauth")),
                ("ANTHROPIC_API_KEY", Some("test-api-key")),
            ],
            || {
                let token = get_auth_token().unwrap();
                assert_eq!(token, "test-oauth");
            },
        );
    }

    #[test]
    fn test_api_key_fallback() {
        with_env_vars(
            &[
                ("CLAUDE_CODE_OAUTH_TOKEN", None),
                ("ANTHROPIC_API_KEY", Some("test-api-key")),
            ],
            || {
                let token = get_auth_token().unwrap();
                assert_eq!(token, "test-api-key");
            },
        );
    }

    #[test]
    fn test_no_auth() {
        with_env_vars(
            &[
                ("CLAUDE_CODE_OAUTH_TOKEN", None),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let result = get_auth_token();
                assert!(result.is_err());
            },
        );
    }

    #[test]
    fn test_openai_key_priority() {
        with_env_vars(
            &[
                ("OPENAI_API_KEY", Some("sk-openai-key")),
                ("OPENROUTER_API_KEY", Some("sk-or-key")),
            ],
            || {
                let token = super::get_openai_auth_token().unwrap();
                assert_eq!(token, "sk-openai-key");
            },
        );
    }

    #[test]
    fn test_openrouter_fallback() {
        with_env_vars(
            &[
                ("OPENAI_API_KEY", None),
                ("OPENROUTER_API_KEY", Some("sk-or-key")),
            ],
            || {
                let token = super::get_openai_auth_token().unwrap();
                assert_eq!(token, "sk-or-key");
            },
        );
    }

    #[test]
    fn test_no_openai_auth_returns_err() {
        with_env_vars(
            &[
                ("OPENAI_API_KEY", None),
                ("OPENROUTER_API_KEY", None),
            ],
            || {
                let result = super::get_openai_auth_token();
                assert!(result.is_err());
            },
        );
    }
}
