//! JJ-lib direct integration backend (feature-gated)
//!
//! This module provides direct jj-lib API integration for hot-path operations,
//! eliminating subprocess overhead. Falls back to subprocess for unsupported operations.

#[cfg(feature = "jj-lib-integration")]
use async_trait::async_trait;
#[cfg(feature = "jj-lib-integration")]
use hox_core::Result;
#[cfg(feature = "jj-lib-integration")]
use std::path::PathBuf;
#[cfg(feature = "jj-lib-integration")]
use tracing::{debug, instrument, warn};

#[cfg(feature = "jj-lib-integration")]
use crate::command::{JjExecutor, JjOutput};

/// JJ executor using jj-lib for hot-path operations
#[cfg(feature = "jj-lib-integration")]
#[derive(Clone)]
pub struct JjLibExecutor {
    repo_root: PathBuf,
    // Fallback subprocess executor for unsupported operations
    fallback: crate::command::JjCommand,
}

#[cfg(feature = "jj-lib-integration")]
impl JjLibExecutor {
    /// Create a new JjLibExecutor for the given repository
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        let repo_root = repo_root.into();
        Self {
            fallback: crate::command::JjCommand::new(repo_root.clone()),
            repo_root,
        }
    }

    /// Auto-detect repository root from current directory
    pub async fn detect() -> Result<Self> {
        let cmd = crate::command::JjCommand::detect().await?;
        Ok(Self::new(cmd.repo_root().clone()))
    }

    /// Check if this operation can be accelerated with jj-lib
    fn can_use_jj_lib(&self, args: &[&str]) -> bool {
        if args.is_empty() {
            return false;
        }

        // Hot-path operations that could benefit from jj-lib integration
        match args[0] {
            "describe" | "log" | "status" | "show" => true,
            _ => false,
        }
    }

    /// Execute operation using jj-lib API
    ///
    /// NOTE: This is a STUB implementation. The actual jj-lib API integration
    /// requires deeper knowledge of jj-lib's workspace and repository APIs.
    /// For now, we document what each operation WOULD do and fall back to subprocess.
    async fn exec_with_jj_lib(&self, args: &[&str]) -> Result<JjOutput> {
        debug!("Attempting jj-lib acceleration for: {:?}", args);

        // NOTE: Full jj-lib integration would involve:
        // 1. Opening a jj-lib Workspace: let workspace = Workspace::load(...)?
        // 2. Getting the repo handle: let repo = workspace.repo()
        // 3. Executing operations using jj-lib APIs:
        //    - describe: repo.set_commit_message(...)
        //    - log: repo.view().heads().iter().map(...)
        //    - status: workspace.working_copy().status()
        //    - show: repo.store().get_commit(...)
        //
        // Since jj-lib's API is complex and evolving, we fall back to subprocess
        // for the initial implementation. This structure allows future optimization
        // without breaking the interface.

        match args.get(0) {
            Some(&"describe") => {
                // FUTURE: Use jj_lib::repo::MutableRepo::set_commit_message
                warn!("jj-lib describe not yet implemented, using subprocess fallback");
                self.fallback.exec(args).await
            }
            Some(&"log") => {
                // FUTURE: Use jj_lib::repo::ReadonlyRepo view APIs and revsets
                warn!("jj-lib log not yet implemented, using subprocess fallback");
                self.fallback.exec(args).await
            }
            Some(&"status") => {
                // FUTURE: Use jj_lib::working_copy::WorkingCopy::status
                warn!("jj-lib status not yet implemented, using subprocess fallback");
                self.fallback.exec(args).await
            }
            Some(&"show") => {
                // FUTURE: Use jj_lib::repo::ReadonlyRepo::store().get_commit()
                warn!("jj-lib show not yet implemented, using subprocess fallback");
                self.fallback.exec(args).await
            }
            _ => {
                // Unsupported operation
                warn!(
                    "Operation not accelerated by jj-lib: {:?}, using subprocess",
                    args
                );
                self.fallback.exec(args).await
            }
        }
    }
}

#[cfg(feature = "jj-lib-integration")]
#[async_trait]
impl JjExecutor for JjLibExecutor {
    #[instrument(skip(self), fields(repo = %self.repo_root.display()))]
    async fn exec(&self, args: &[&str]) -> Result<JjOutput> {
        if self.can_use_jj_lib(args) {
            // Try jj-lib acceleration, but fall back to subprocess if needed
            self.exec_with_jj_lib(args).await
        } else {
            // Use subprocess for non-hot-path operations
            debug!("Using subprocess for operation: {:?}", args);
            self.fallback.exec(args).await
        }
    }

    fn repo_root(&self) -> &PathBuf {
        &self.repo_root
    }
}

#[cfg(all(test, feature = "jj-lib-integration"))]
mod tests {
    use super::*;

    #[test]
    fn test_can_use_jj_lib() {
        let executor = JjLibExecutor::new("/tmp/test");

        // Hot-path operations
        assert!(executor.can_use_jj_lib(&["describe"]));
        assert!(executor.can_use_jj_lib(&["log", "-r", "@"]));
        assert!(executor.can_use_jj_lib(&["status"]));
        assert!(executor.can_use_jj_lib(&["show"]));

        // Other operations should use subprocess
        assert!(!executor.can_use_jj_lib(&["commit"]));
        assert!(!executor.can_use_jj_lib(&["new"]));
        assert!(!executor.can_use_jj_lib(&["git", "push"]));
        assert!(!executor.can_use_jj_lib(&[]));
    }

    #[test]
    fn test_constructor() {
        let executor = JjLibExecutor::new("/tmp/test");
        assert_eq!(executor.repo_root(), &PathBuf::from("/tmp/test"));
    }
}
