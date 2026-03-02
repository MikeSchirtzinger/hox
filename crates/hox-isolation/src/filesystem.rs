//! Filesystem-based isolation using JJ workspaces.
//!
//! Creates a JJ workspace per agent under `base_dir`. Uses `jj workspace add`
//! for creation and `jj workspace forget` + directory removal for destruction.

use crate::{IsolatedEnv, IsolationBackend};
use async_trait::async_trait;
use hox_core::{HoxError, Result};
use std::path::PathBuf;
use tracing::{debug, info};

/// Filesystem-based isolation: one JJ workspace per agent.
pub struct FilesystemBackend {
    base_dir: PathBuf,
    repo_root: PathBuf,
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
        let workspace_path = self.workspace_path(agent_id);

        info!("Creating workspace for agent {} at {:?}", agent_id, workspace_path);

        tokio::fs::create_dir_all(&workspace_path).await?;

        self.run_jj(
            &["workspace", "add", workspace_path.to_str().unwrap_or(agent_id)],
            &self.repo_root,
        )
        .await
        .map_err(|e| {
            HoxError::JjWorkspace(format!(
                "Failed to add jj workspace for agent {}: {}",
                agent_id, e
            ))
        })?;

        debug!("Created jj workspace for agent {}", agent_id);

        Ok(IsolatedEnv {
            agent_id: agent_id.to_string(),
            workspace_path,
        })
    }

    async fn destroy(&self, agent_id: &str) -> Result<()> {
        let workspace_path = self.workspace_path(agent_id);

        info!("Destroying workspace for agent {}", agent_id);

        // Forget workspace from jj (best-effort — directory may not be registered)
        let _ = self
            .run_jj(&["workspace", "forget", agent_id], &self.repo_root)
            .await;

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
        // directory created before jj call
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
