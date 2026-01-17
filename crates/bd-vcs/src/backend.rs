//! VCS backend trait definition.

use bd_core::Result;
use std::path::{Path, PathBuf};

/// Trait defining the interface that all VCS backends must implement.
pub trait VcsBackend: Send + Sync + std::fmt::Debug {
    /// Get the current commit/change ID (HEAD in git, @ in jj).
    fn current_commit(&self) -> Result<String>;

    /// Get the list of files that changed since the given commit.
    fn changed_files(&self, since: &str) -> Result<Vec<PathBuf>>;

    /// Find files matching a glob pattern in the repository.
    fn find_files(&self, pattern: &str) -> Result<Vec<PathBuf>>;

    /// Check if a file is tracked by the VCS.
    fn is_tracked(&self, path: &Path) -> Result<bool>;

    /// Get the repository root path.
    fn root_path(&self) -> &Path;
}
