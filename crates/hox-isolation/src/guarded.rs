//! Safety-rule-enforcing wrapper around any [`IsolationBackend`].
//!
//! [`GuardedBackend`] delegates all operations to an inner backend but checks
//! path and command operations against [`CompiledSafetyRules`] before forwarding.

use crate::{IsolatedEnv, IsolationBackend};
use crate::safety::CompiledSafetyRules;
use async_trait::async_trait;
use hox_core::Result;
use std::path::Path;

/// Wraps any `IsolationBackend` with safety rule enforcement.
pub struct GuardedBackend<B: IsolationBackend> {
    inner: B,
    rules: CompiledSafetyRules,
}

impl<B: IsolationBackend> GuardedBackend<B> {
    pub fn new(inner: B, rules: CompiledSafetyRules) -> Self {
        Self { inner, rules }
    }

    /// Check a path against deny patterns. Returns `Err(ProtectedFile)` if denied.
    pub fn check_path(&self, path: &Path) -> Result<()> {
        self.rules.check_path(path)
    }

    /// Check a command string against deny patterns. Returns `Err(ProtectedFile)` if denied.
    pub fn check_command(&self, cmd: &str) -> Result<()> {
        self.rules.check_command(cmd)
    }
}

#[async_trait]
impl<B: IsolationBackend> IsolationBackend for GuardedBackend<B> {
    async fn create(&self, agent_id: &str) -> Result<IsolatedEnv> {
        // CRIT-1: Enforce safety rules before delegating to inner backend.
        // Treat the agent_id as a path component to check against deny patterns.
        self.rules.check_path(Path::new(agent_id))?;
        self.inner.create(agent_id).await
    }

    async fn destroy(&self, agent_id: &str) -> Result<()> {
        // CRIT-1: Enforce safety rules before delegating to inner backend.
        self.rules.check_path(Path::new(agent_id))?;
        self.inner.destroy(agent_id).await
    }

    async fn list(&self) -> Result<Vec<String>> {
        self.inner.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SafetyRulesConfig;
    use hox_core::HoxError;
    use std::path::Path;

    fn make_rules(deny_paths: &[&str], deny_cmds: &[&str]) -> CompiledSafetyRules {
        let config = SafetyRulesConfig {
            deny_paths: deny_paths.iter().map(|s| s.to_string()).collect(),
            deny_commands: deny_cmds.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        CompiledSafetyRules::compile(&config).expect("compile")
    }

    struct NoopBackend;

    #[async_trait]
    impl IsolationBackend for NoopBackend {
        async fn create(&self, agent_id: &str) -> Result<IsolatedEnv> {
            Ok(IsolatedEnv {
                agent_id: agent_id.to_string(),
                workspace_path: std::path::PathBuf::from("/tmp").join(agent_id),
            })
        }
        async fn destroy(&self, _agent_id: &str) -> Result<()> {
            Ok(())
        }
        async fn list(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_denied_path_returns_protected_file() {
        let rules = make_rules(&["**/.env"], &[]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        let err = backend.check_path(Path::new(".env")).unwrap_err();
        assert!(matches!(err, HoxError::ProtectedFile(_)));
    }

    #[test]
    fn test_allowed_path_passes() {
        let rules = make_rules(&["**/.env"], &[]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        assert!(backend.check_path(Path::new("src/main.rs")).is_ok());
    }

    #[test]
    fn test_denied_command_returns_protected_file() {
        let rules = make_rules(&[], &["rm\\s+-rf\\s+/"]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        let err = backend.check_command("rm -rf /").unwrap_err();
        assert!(matches!(err, HoxError::ProtectedFile(_)));
    }

    #[test]
    fn test_allowed_command_passes() {
        let rules = make_rules(&[], &["rm\\s+-rf\\s+/"]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        assert!(backend.check_command("cargo build").is_ok());
    }

    #[test]
    fn test_empty_rules_allow_everything() {
        let rules = CompiledSafetyRules::empty();
        let backend = GuardedBackend::new(NoopBackend, rules);
        assert!(backend.check_path(Path::new(".env")).is_ok());
        assert!(backend.check_command("rm -rf /").is_ok());
    }

    // --- CRIT-1: GuardedBackend actually enforces rules in create/destroy ---

    #[tokio::test]
    async fn test_create_blocked_by_safety_rules() {
        // Agent id matching a deny_paths pattern should be rejected.
        let rules = make_rules(&[".env"], &[]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        let err = backend.create(".env").await.unwrap_err();
        assert!(matches!(err, HoxError::ProtectedFile(_)));
    }

    #[tokio::test]
    async fn test_destroy_blocked_by_safety_rules() {
        let rules = make_rules(&[".env"], &[]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        let err = backend.destroy(".env").await.unwrap_err();
        assert!(matches!(err, HoxError::ProtectedFile(_)));
    }

    #[tokio::test]
    async fn test_create_allowed_by_safety_rules() {
        let rules = make_rules(&[".env"], &[]);
        let backend = GuardedBackend::new(NoopBackend, rules);
        let env = backend.create("my-agent").await.unwrap();
        assert_eq!(env.agent_id, "my-agent");
    }

    #[tokio::test]
    async fn test_create_delegates_to_inner() {
        let rules = CompiledSafetyRules::empty();
        let backend = GuardedBackend::new(NoopBackend, rules);
        let env = backend.create("my-agent").await.unwrap();
        assert_eq!(env.agent_id, "my-agent");
    }

    #[tokio::test]
    async fn test_destroy_delegates_to_inner() {
        let rules = CompiledSafetyRules::empty();
        let backend = GuardedBackend::new(NoopBackend, rules);
        assert!(backend.destroy("my-agent").await.is_ok());
    }

    #[tokio::test]
    async fn test_list_delegates_to_inner() {
        let rules = CompiledSafetyRules::empty();
        let backend = GuardedBackend::new(NoopBackend, rules);
        assert_eq!(backend.list().await.unwrap(), Vec::<String>::new());
    }
}
