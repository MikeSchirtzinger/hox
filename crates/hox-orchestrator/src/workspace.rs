//! Workspace management for agent isolation

use hox_core::{HoxError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

use hox_jj::{JjCommand, JjExecutor};

/// Information about a workspace
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub name: String,
    pub path: PathBuf,
    pub active: bool,
}

/// Manages JJ workspaces for agent isolation
pub struct WorkspaceManager<E: JjExecutor> {
    executor: E,
    workspaces: HashMap<String, WorkspaceInfo>,
}

impl<E: JjExecutor> WorkspaceManager<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            workspaces: HashMap::new(),
        }
    }

    /// Create a new workspace for an agent
    pub async fn create_workspace(&mut self, name: &str) -> Result<PathBuf> {
        let workspace_path = self
            .executor
            .repo_root()
            .join(format!(".hox-workspaces/{}", name));

        info!("Creating workspace {} at {:?}", name, workspace_path);

        // Create workspace directory
        std::fs::create_dir_all(&workspace_path).map_err(|e| {
            HoxError::JjWorkspace(format!("Failed to create workspace directory: {}", e))
        })?;

        // Create JJ workspace
        let output = self
            .executor
            .exec(&[
                "workspace",
                "add",
                "--name",
                name,
                workspace_path.to_str().ok_or_else(|| {
                    HoxError::JjWorkspace("workspace path contains non-UTF-8 characters".into())
                })?,
            ])
            .await?;

        if !output.success && !output.stderr.contains("already exists") {
            return Err(HoxError::JjWorkspace(format!(
                "Failed to create workspace: {}",
                output.stderr
            )));
        }

        let info = WorkspaceInfo {
            name: name.to_string(),
            path: workspace_path.clone(),
            active: true,
        };

        self.workspaces.insert(name.to_string(), info);
        Ok(workspace_path)
    }

    /// Remove a workspace
    pub async fn remove_workspace(&mut self, name: &str) -> Result<()> {
        info!("Removing workspace {}", name);

        let output = self.executor.exec(&["workspace", "forget", name]).await?;

        if !output.success {
            debug!("Workspace forget warning: {}", output.stderr);
        }

        // Remove from tracking
        if let Some(info) = self.workspaces.remove(name) {
            // Optionally clean up directory
            if info.path.exists() {
                std::fs::remove_dir_all(&info.path).ok();
            }
        }

        Ok(())
    }

    /// List all workspaces
    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        let output = self.executor.exec(&["workspace", "list"]).await?;

        if !output.success {
            return Err(HoxError::JjWorkspace(output.stderr));
        }

        // Parse workspace list output
        // Format: "name: /path/to/workspace"
        let mut workspaces = Vec::new();

        for line in output.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() >= 2 {
                let name = parts[0].trim().to_string();
                let path = PathBuf::from(parts[1].trim());

                workspaces.push(WorkspaceInfo {
                    name,
                    path,
                    active: true,
                });
            }
        }

        Ok(workspaces)
    }

    /// Get workspace info
    pub fn get_workspace(&self, name: &str) -> Option<&WorkspaceInfo> {
        self.workspaces.get(name)
    }

    /// Switch to a workspace, returning a workspace-scoped executor.
    ///
    /// The returned `JjCommand` is rooted at the workspace directory so all
    /// subsequent jj operations (new, edit, describe …) run inside that
    /// workspace rather than the main repo working copy.
    pub async fn switch_to(&self, name: &str) -> Result<JjCommand> {
        if let Some(info) = self.workspaces.get(name) {
            debug!("Switching to workspace {} at {:?}", name, info.path);
            Ok(JjCommand::new(&info.path))
        } else {
            Err(HoxError::JjWorkspace(format!(
                "Workspace {} not found",
                name
            )))
        }
    }

    /// Clean up all workspaces created by this manager
    pub async fn cleanup_all(&mut self) -> Result<()> {
        let names: Vec<String> = self.workspaces.keys().cloned().collect();

        for name in names {
            self.remove_workspace(&name).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests for WorkspaceManager require a real JJ repository.
    // The JjExecutor trait mocking is available within hox-jj crate tests.
    // For unit tests here, we test the non-async logic.

    #[test]
    fn test_workspace_info() {
        let info = WorkspaceInfo {
            name: "test-agent".to_string(),
            path: PathBuf::from("/tmp/test-workspace"),
            active: true,
        };

        assert_eq!(info.name, "test-agent");
        assert!(info.active);
    }
}
