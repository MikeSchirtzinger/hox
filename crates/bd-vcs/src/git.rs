//! Git backend implementation using the gix crate.

use crate::backend::VcsBackend;
use bd_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::debug;

/// Git VCS backend implementation.
///
/// Uses Arc<Mutex<>> to make the repository thread-safe since gix::Repository
/// is not Sync by default (it contains RefCell internally).
#[derive(Debug)]
pub struct GitBackend {
    repo: Arc<Mutex<gix::Repository>>,
    root_path: PathBuf,
}

impl GitBackend {
    /// Open a git repository at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Use gix to discover and open the repository
        let repo = gix::discover(path)
            .map_err(|e| Error::Vcs(format!("Failed to open git repository: {}", e)))?;

        let root_path = repo.work_dir()
            .ok_or_else(|| Error::Vcs("Repository has no working directory".to_string()))?
            .to_path_buf();

        debug!("Opened git repository at: {}", root_path.display());

        Ok(Self {
            repo: Arc::new(Mutex::new(repo)),
            root_path
        })
    }

    /// Get the HEAD reference as a commit ID.
    fn head_commit_id(&self) -> Result<gix::ObjectId> {
        let repo = self.repo.lock().unwrap();
        let mut head = repo.head()
            .map_err(|e| Error::GitRef(format!("Failed to get HEAD: {}", e)))?;

        let commit = head.peel_to_commit_in_place()
            .map_err(|e| Error::GitRef(format!("Failed to peel HEAD to commit: {}", e)))?;

        Ok(commit.id)
    }

    /// Resolve a commit reference string to an ObjectId.
    fn resolve_commit(&self, rev: &str) -> Result<gix::ObjectId> {
        let repo = self.repo.lock().unwrap();
        let object = repo
            .rev_parse_single(rev.as_bytes())
            .map_err(|e| Error::InvalidCommit(format!("Failed to parse '{}': {}", rev, e)))?;

        Ok(object.detach())
    }
}

impl VcsBackend for GitBackend {
    fn current_commit(&self) -> Result<String> {
        let commit_id = self.head_commit_id()?;
        debug!("Current commit: {}", commit_id);
        Ok(commit_id.to_string())
    }

    fn changed_files(&self, since: &str) -> Result<Vec<PathBuf>> {
        debug!("Getting changed files since: {}", since);

        let repo = self.repo.lock().unwrap();

        // Parse the "since" commit
        let since_id = self.resolve_commit(since)?;
        let since_tree = repo.find_object(since_id)
            .map_err(|e| Error::InvalidCommit(format!("Failed to find commit: {}", e)))?
            .peel_to_tree()
            .map_err(|e| Error::GitTraversal(format!("Failed to peel to tree: {}", e)))?;

        // Get HEAD commit
        let head_id = self.head_commit_id()?;
        let head_tree = repo.find_object(head_id)
            .map_err(|e| Error::InvalidCommit(format!("Failed to find HEAD commit: {}", e)))?
            .peel_to_tree()
            .map_err(|e| Error::GitTraversal(format!("Failed to peel HEAD to tree: {}", e)))?;

        // Perform diff between the two trees
        let mut changed = Vec::new();

        head_tree.changes()
            .map_err(|e| Error::GitTraversal(format!("Failed to create tree diff: {}", e)))?
            .for_each_to_obtain_tree(
                &since_tree,
                |change| {
                    // Extract the path from the change
                    // change.location is a BStr representing the file path
                    let path_str = std::str::from_utf8(change.location)
                        .unwrap_or("");
                    if !path_str.is_empty() {
                        changed.push(PathBuf::from(path_str));
                    }
                    Ok::<_, std::io::Error>(Default::default())
                }
            )
            .map_err(|e| Error::GitTraversal(format!("Failed to diff trees: {}", e)))?;

        debug!("Found {} changed files", changed.len());
        Ok(changed)
    }

    fn find_files(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        debug!("Finding files matching pattern: {}", pattern);

        let repo = self.repo.lock().unwrap();

        // Get HEAD tree
        let head_id = self.head_commit_id()?;
        let head_obj = repo.find_object(head_id)
            .map_err(|e| Error::InvalidCommit(format!("Failed to find HEAD: {}", e)))?;
        let tree = head_obj.peel_to_tree()
            .map_err(|e| Error::GitTraversal(format!("Failed to peel to tree: {}", e)))?;

        // Compile glob pattern
        let glob = glob::Pattern::new(pattern)
            .map_err(|e| Error::Parse(format!("Invalid glob pattern: {}", e)))?;

        let mut matches = Vec::new();

        // Use tree recorder to traverse and collect entries
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse()
            .breadthfirst(&mut recorder)
            .map_err(|e| Error::GitTraversal(format!("Failed to traverse tree: {}", e)))?;

        // Filter the recorded entries by pattern
        for entry in recorder.records {
            let path_bytes = entry.filepath.as_slice();
            if let Ok(path_str) = std::str::from_utf8(path_bytes) {
                // Only match files (not directories)
                if entry.mode.is_blob() && glob.matches(path_str) {
                    matches.push(PathBuf::from(path_str));
                }
            }
        }

        debug!("Found {} files matching pattern", matches.len());
        Ok(matches)
    }

    fn is_tracked(&self, path: &Path) -> Result<bool> {
        debug!("Checking if tracked: {}", path.display());

        // Normalize path relative to repo root
        let relative_path = if path.is_absolute() {
            path.strip_prefix(&self.root_path)
                .map_err(|_| Error::Vcs(format!("Path {} is not in repository", path.display())))?
        } else {
            path
        };

        let repo = self.repo.lock().unwrap();

        // Get HEAD tree
        let head_id = self.head_commit_id()?;
        let head_obj = repo.find_object(head_id)
            .map_err(|e| Error::InvalidCommit(format!("Failed to find HEAD: {}", e)))?;
        let tree = head_obj.peel_to_tree()
            .map_err(|e| Error::GitTraversal(format!("Failed to peel to tree: {}", e)))?;

        // Look up the path in the tree
        let mut buf = Vec::new();
        let entry = tree.lookup_entry_by_path(relative_path, &mut buf);

        match entry {
            Ok(Some(_)) => {
                debug!("File is tracked");
                Ok(true)
            }
            Ok(None) => {
                debug!("File is not tracked");
                Ok(false)
            }
            Err(e) => {
                debug!("Error checking if file is tracked: {}", e);
                Ok(false)
            }
        }
    }

    fn root_path(&self) -> &Path {
        &self.root_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_invalid_path() {
        let result = GitBackend::open("/tmp/nonexistent-git-repo-12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_current_repo() {
        // Try to open the current repository (should work if we're in a git repo)
        let result = GitBackend::open(".");
        // This test just verifies the open logic works
        match result {
            Ok(backend) => {
                assert!(backend.root_path().exists());
                println!("Successfully opened repo at: {}", backend.root_path().display());
            }
            Err(e) => {
                println!("Not in a git repo: {}", e);
            }
        }
    }
}
