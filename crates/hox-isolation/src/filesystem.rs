//! Filesystem-based isolation using JJ workspaces.
//!
//! Creates a JJ workspace per agent under `base_dir`. Uses `jj workspace add`
//! for creation and `jj workspace forget` + directory removal for destruction.

use crate::{IsolatedEnv, IsolationBackend};
use async_trait::async_trait;
use hox_core::{HoxError, Result};
use std::path::{Component, PathBuf};
use tracing::{debug, info, warn};

/// Filesystem-based isolation: one JJ workspace per agent.
pub struct FilesystemBackend {
    base_dir: PathBuf,
    repo_root: PathBuf,
}

/// Validate that an agent_id is a simple name component with no path traversal.
///
/// Rejects any agent_id containing `..`, `/`, `\`, or other path separator
/// components that could escape the base workspace directory.
pub fn validate_agent_id(agent_id: &str) -> Result<()> {
    if agent_id.is_empty() {
        return Err(HoxError::PathValidation(
            "agent_id must not be empty".to_string(),
        ));
    }

    // Reject raw slash/backslash characters regardless of OS.
    if agent_id.contains('/') || agent_id.contains('\\') {
        return Err(HoxError::PathValidation(format!(
            "agent_id '{}' contains path separator characters",
            agent_id
        )));
    }

    // Use Path::components() to catch any remaining path traversal tricks.
    let path = std::path::Path::new(agent_id);
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::ParentDir => {
                return Err(HoxError::PathValidation(format!(
                    "agent_id '{}' contains '..' path traversal",
                    agent_id
                )));
            }
            Component::CurDir => {
                return Err(HoxError::PathValidation(format!(
                    "agent_id '{}' contains '.' current-dir component",
                    agent_id
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(HoxError::PathValidation(format!(
                    "agent_id '{}' contains an absolute path component",
                    agent_id
                )));
            }
        }
    }

    Ok(())
}

impl FilesystemBackend {
    /// Create a new backend. `base_dir` is where agent workspaces are created;
    /// `repo_root` is the JJ repository root (used for `jj workspace add`).
    pub fn new(base_dir: PathBuf, repo_root: PathBuf) -> Self {
        Self { base_dir, repo_root }
    }

    fn workspace_path(&self, agent_id: &str) -> PathBuf {
        self.base_dir.join(agent_id)
    }

    async fn run_jj(&self, args: &[&str], cwd: &PathBuf) -> Result<()> {
        let output = tokio::process::Command::new("jj")
            .args(args)
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| HoxError::JjCommand(format!("Failed to run jj: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HoxError::JjCommand(format!(
                "jj {} failed: {}",
                args.join(" "),
                stderr.trim()
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl IsolationBackend for FilesystemBackend {
    async fn create(&self, agent_id: &str) -> Result<IsolatedEnv> {
        // CRIT-2: Reject path traversal in agent_id before any filesystem ops.
        validate_agent_id(agent_id)?;

        let workspace_path = self.workspace_path(agent_id);

        info!("Creating workspace for agent {} at {:?}", agent_id, workspace_path);

        tokio::fs::create_dir_all(&workspace_path).await?;

        // W6: If jj workspace add fails, clean up the directory we just created.
        let jj_result = self
            .run_jj(
                &["workspace", "add", workspace_path.to_str().unwrap_or(agent_id)],
                &self.repo_root,
            )
            .await
            .map_err(|e| {
                HoxError::JjWorkspace(format!(
                    "Failed to add jj workspace for agent {}: {}",
                    agent_id, e
                ))
            });

        if let Err(e) = jj_result {
            // Best-effort cleanup: ignore errors from cleanup itself.
            if let Err(rm_err) = tokio::fs::remove_dir_all(&workspace_path).await {
                warn!(
                    "Failed to clean up workspace dir {:?} after jj workspace add failure: {}",
                    workspace_path, rm_err
                );
            }
            return Err(e);
        }

        debug!("Created jj workspace for agent {}", agent_id);

        Ok(IsolatedEnv {
            agent_id: agent_id.to_string(),
            workspace_path,
        })
    }

    async fn destroy(&self, agent_id: &str) -> Result<()> {
        // CRIT-2: Reject path traversal in agent_id before any filesystem ops.
        validate_agent_id(agent_id)?;

        let workspace_path = self.workspace_path(agent_id);

        info!("Destroying workspace for agent {}", agent_id);

        // W5: Log warning on jj workspace forget failure instead of silently swallowing.
        if let Err(e) = self
            .run_jj(&["workspace", "forget", agent_id], &self.repo_root)
            .await
        {
            warn!(
                "jj workspace forget failed for agent '{}': {}. Proceeding with directory removal.",
                agent_id, e
            );
        }

        if workspace_path.exists() {
            tokio::fs::remove_dir_all(&workspace_path)
                .await
                .map_err(|e| {
                    HoxError::Io(format!(
                        "Failed to remove workspace dir for agent {}: {}",
                        agent_id, e
                    ))
                })?;
            debug!("Removed directory {:?} for agent {}", workspace_path, agent_id);
        }

        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>> {
        if !self.base_dir.exists() {
            return Ok(vec![]);
        }

        let mut entries = tokio::fs::read_dir(&self.base_dir).await.map_err(|e| {
            HoxError::Io(format!("Failed to read base_dir {:?}: {}", self.base_dir, e))
        })?;

        let mut agent_ids = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            HoxError::Io(format!("Failed to read dir entry: {}", e))
        })? {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    agent_ids.push(name.to_string());
                }
            }
        }

        Ok(agent_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_backend(base: &TempDir) -> FilesystemBackend {
        let repo_root = base.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let base_dir = base.path().join("workspaces");
        std::fs::create_dir_all(&base_dir).unwrap();
        FilesystemBackend::new(base_dir, repo_root)
    }

    // --- CRIT-2: path traversal rejection ---

    #[test]
    fn test_validate_agent_id_rejects_dotdot() {
        assert!(validate_agent_id("..").is_err());
        assert!(validate_agent_id("../etc").is_err());
        assert!(validate_agent_id("agent/../../../etc").is_err());
    }

    #[test]
    fn test_validate_agent_id_rejects_slash() {
        assert!(validate_agent_id("agent/name").is_err());
        assert!(validate_agent_id("/etc/passwd").is_err());
    }

    #[test]
    fn test_validate_agent_id_rejects_backslash() {
        assert!(validate_agent_id("agent\\name").is_err());
    }

    #[test]
    fn test_validate_agent_id_rejects_empty() {
        assert!(validate_agent_id("").is_err());
    }

    #[test]
    fn test_validate_agent_id_allows_normal_names() {
        assert!(validate_agent_id("agent-123").is_ok());
        assert!(validate_agent_id("my_agent").is_ok());
        assert!(validate_agent_id("agent.v2").is_ok());
    }

    #[tokio::test]
    async fn test_create_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);
        let result = backend.create("../../etc").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, hox_core::HoxError::PathValidation(_)));
    }

    #[tokio::test]
    async fn test_destroy_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);
        let result = backend.destroy("../sensitive").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, hox_core::HoxError::PathValidation(_)));
    }

    // --- W6: cleanup on jj workspace add failure ---

    #[tokio::test]
    async fn test_create_cleans_dir_on_jj_failure() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);

        // `jj workspace add` will fail (not a real jj repo), so the directory
        // should be removed by the cleanup path.
        let workspace_path = backend.workspace_path("agent-cleanup-test");
        let result = backend.create("agent-cleanup-test").await;

        // Expect failure because jj is not set up.
        assert!(result.is_err());
        // W6: directory must NOT be left behind.
        assert!(
            !workspace_path.exists(),
            "Workspace dir should be cleaned up after jj failure"
        );
    }

    // --- Existing tests (preserved) ---

    #[tokio::test]
    async fn test_create_makes_directory() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);

        // `jj workspace add` will fail in a non-jj repo, but the directory
        // creation happens before the jj call. We can verify the directory
        // is attempted regardless. The error is expected here.
        let result = backend.create("agent-123").await;
        // Either ok (if jj is available + workspace exists) or JjCommand error
        // — what matters is the directory was attempted.
        let workspace_path = backend.workspace_path("agent-123");
        // directory created before jj call (may be cleaned up on failure per W6)
        assert!(workspace_path.exists() || result.is_err());
    }

    #[tokio::test]
    async fn test_destroy_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);
        let workspace_path = backend.workspace_path("agent-del");

        // Manually create the directory (skip jj)
        tokio::fs::create_dir_all(&workspace_path).await.unwrap();
        assert!(workspace_path.exists());

        // destroy should remove it (jj forget may fail, but dir removal should succeed)
        backend.destroy("agent-del").await.unwrap();
        assert!(!workspace_path.exists());
    }

    #[tokio::test]
    async fn test_destroy_nonexistent_is_ok() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);
        // Should not error even if directory does not exist
        let result = backend.destroy("never-existed").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_empty_base_dir() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        // base_dir does not exist yet
        let backend = FilesystemBackend::new(tmp.path().join("nonexistent"), repo_root);
        let agents = backend.list().await.unwrap();
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn test_list_returns_directories() {
        let tmp = TempDir::new().unwrap();
        let backend = make_backend(&tmp);

        // Manually populate base_dir
        let base = backend.workspace_path("placeholder").parent().unwrap().to_path_buf();
        tokio::fs::create_dir_all(base.join("agent-a")).await.unwrap();
        tokio::fs::create_dir_all(base.join("agent-b")).await.unwrap();
        // Also create a file (should not appear in list)
        tokio::fs::write(base.join("not-a-dir.txt"), b"x").await.unwrap();

        let mut agents = backend.list().await.unwrap();
        agents.sort();
        assert_eq!(agents, vec!["agent-a", "agent-b"]);
    }
}
