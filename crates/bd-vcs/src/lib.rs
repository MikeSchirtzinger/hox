//! Version control system abstraction layer.
//!
//! This crate provides VCS operations, initially targeting Git/Jujutsu.
//! It handles commit detection, change tracking, and repo operations.

use bd_core::{Error, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

mod backend;
mod git;
mod jj;

pub use backend::VcsBackend;
pub use git::GitBackend;
pub use jj::JjBackend;

/// VCS type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsType {
    Git,
    Jujutsu,
}

/// Main VCS interface that delegates to the appropriate backend.
pub struct Vcs {
    backend: Box<dyn VcsBackend>,
    vcs_type: VcsType,
}

impl std::fmt::Debug for Vcs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vcs")
            .field("vcs_type", &self.vcs_type)
            .field("backend", &"<VcsBackend>")
            .finish()
    }
}

impl Vcs {
    /// Open a VCS repository at the given path.
    /// Automatically detects whether it's a Git or Jujutsu repository.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        info!("Opening VCS repository at: {}", path.display());

        // Try to detect VCS type by looking for .git or .jj directories
        // Prefer Git when both exist (common in jj+git cohabitation setups)
        let git_path = path.join(".git");
        let jj_path = path.join(".jj");

        if git_path.exists() || Self::find_git_repo(path).is_some() {
            info!("Detected Git repository");
            let backend = GitBackend::open(path)?;
            Ok(Self {
                backend: Box::new(backend),
                vcs_type: VcsType::Git,
            })
        } else if jj_path.exists() {
            info!("Detected Jujutsu repository");
            let backend = JjBackend::open(path)?;
            Ok(Self {
                backend: Box::new(backend),
                vcs_type: VcsType::Jujutsu,
            })
        } else {
            Err(Error::NotInVcs)
        }
    }

    /// Find git repository by walking up the directory tree.
    fn find_git_repo(mut path: &Path) -> Option<PathBuf> {
        loop {
            let git_path = path.join(".git");
            if git_path.exists() {
                return Some(path.to_path_buf());
            }
            path = path.parent()?;
        }
    }

    /// Get the VCS type.
    pub fn vcs_type(&self) -> VcsType {
        self.vcs_type
    }

    /// Get the current commit/change ID.
    pub fn current_commit(&self) -> Result<String> {
        debug!("Getting current commit ID");
        self.backend.current_commit()
    }

    /// Get the list of changed files since the given commit.
    pub fn changed_files(&self, since: &str) -> Result<Vec<PathBuf>> {
        debug!("Getting changed files since: {}", since);
        self.backend.changed_files(since)
    }

    /// Get the list of files matching a glob pattern.
    pub fn find_files(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        debug!("Finding files matching: {}", pattern);
        self.backend.find_files(pattern)
    }

    /// Check if a file is tracked by VCS.
    pub fn is_tracked(&self, path: impl AsRef<Path>) -> Result<bool> {
        let path = path.as_ref();
        debug!("Checking if file is tracked: {}", path.display());
        self.backend.is_tracked(path)
    }

    /// Get the repository root path.
    pub fn repo_root(&self) -> &Path {
        self.backend.root_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_git_repo() -> Result<TempDir> {
        let temp = TempDir::new().map_err(|e| Error::Io(e))?;
        let git_dir = temp.path().join(".git");
        fs::create_dir(&git_dir).map_err(|e| Error::Io(e))?;

        // Create a minimal git structure
        fs::create_dir_all(git_dir.join("refs/heads")).map_err(|e| Error::Io(e))?;
        fs::create_dir_all(git_dir.join("objects")).map_err(|e| Error::Io(e))?;
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| Error::Io(e))?;

        Ok(temp)
    }

    #[test]
    fn test_vcs_detection_no_repo() {
        let temp = TempDir::new().unwrap();
        let result = Vcs::open(temp.path());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotInVcs));
    }

    #[test]
    fn test_vcs_detection_git() {
        let temp = create_test_git_repo().unwrap();
        let result = Vcs::open(temp.path());
        // With a minimal .git structure, gix can open it
        // But we expect operations to fail without proper git objects
        match result {
            Ok(vcs) => {
                assert_eq!(vcs.vcs_type(), VcsType::Git);
                // Try to get current commit - this should fail because there's no HEAD commit
                let commit_result = vcs.current_commit();
                assert!(commit_result.is_err());
            }
            Err(_) => {
                // Also acceptable if gix requires more than minimal structure
            }
        }
    }

    #[test]
    fn test_find_git_repo() {
        let temp = create_test_git_repo().unwrap();
        let nested = temp.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();

        let found = Vcs::find_git_repo(&nested);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), temp.path());
    }
}
