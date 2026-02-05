//! Metrics storage (feature-flagged)

use hox_core::{ChangeId, Result};
use std::path::PathBuf;
use tokio::fs;
use tracing::debug;

use crate::collector::AgentMetrics;

/// Storage mode for metrics
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    /// Store in JJ change metadata
    JjNative,
    /// Store in append-only file
    AppendFile(PathBuf),
    /// Store in Turso database (requires feature)
    #[cfg(feature = "turso")]
    Turso(String),
}

/// Metrics storage abstraction
pub struct MetricsStorage {
    mode: StorageMode,
}

impl MetricsStorage {
    pub fn new(mode: StorageMode) -> Self {
        Self { mode }
    }

    /// Create JJ-native storage
    pub fn jj_native() -> Self {
        Self::new(StorageMode::JjNative)
    }

    /// Create append-file storage
    pub fn append_file(path: impl Into<PathBuf>) -> Self {
        Self::new(StorageMode::AppendFile(path.into()))
    }

    /// Store metrics for an agent
    pub async fn store(&self, metrics: &AgentMetrics) -> Result<()> {
        match &self.mode {
            StorageMode::JjNative => self.store_jj_native(metrics).await,
            StorageMode::AppendFile(path) => self.store_append_file(path, metrics).await,
            #[cfg(feature = "turso")]
            StorageMode::Turso(connection) => self.store_turso(connection, metrics).await,
        }
    }

    /// Load metrics for a change
    pub async fn load(&self, change_id: &ChangeId) -> Result<Option<AgentMetrics>> {
        match &self.mode {
            StorageMode::JjNative => self.load_jj_native(change_id).await,
            StorageMode::AppendFile(path) => self.load_append_file(path, change_id).await,
            #[cfg(feature = "turso")]
            StorageMode::Turso(connection) => self.load_turso(connection, change_id).await,
        }
    }

    /// Load all metrics (for aggregation)
    pub async fn load_all(&self) -> Result<Vec<AgentMetrics>> {
        match &self.mode {
            StorageMode::JjNative => {
                // JJ-native requires iterating changes
                // This is expensive - prefer external storage for aggregation
                Ok(Vec::new())
            }
            StorageMode::AppendFile(path) => self.load_all_from_file(path).await,
            #[cfg(feature = "turso")]
            StorageMode::Turso(connection) => self.load_all_turso(connection).await,
        }
    }

    // JJ-native storage implementation
    async fn store_jj_native(&self, metrics: &AgentMetrics) -> Result<()> {
        // TODO: Store as metadata on the change using hox-jj
        // For now, this is a placeholder
        debug!(
            "Would store metrics for {} on change {}",
            metrics.agent_id, metrics.change_id
        );
        Ok(())
    }

    async fn load_jj_native(&self, _change_id: &ChangeId) -> Result<Option<AgentMetrics>> {
        // TODO: Load from change metadata
        Ok(None)
    }

    // Append-file storage implementation
    async fn store_append_file(&self, path: &PathBuf, metrics: &AgentMetrics) -> Result<()> {
        let line = serde_json::to_string(metrics)?;

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Append to file
        use tokio::io::AsyncWriteExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;

        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;

        debug!("Stored metrics for {} to {:?}", metrics.agent_id, path);
        Ok(())
    }

    async fn load_append_file(
        &self,
        path: &PathBuf,
        change_id: &ChangeId,
    ) -> Result<Option<AgentMetrics>> {
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path).await?;

        for line in content.lines() {
            if line.is_empty() {
                continue;
            }

            let metrics: AgentMetrics = serde_json::from_str(line)?;
            if &metrics.change_id == change_id {
                return Ok(Some(metrics));
            }
        }

        Ok(None)
    }

    async fn load_all_from_file(&self, path: &PathBuf) -> Result<Vec<AgentMetrics>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path).await?;
        let mut all_metrics = Vec::new();

        for line in content.lines() {
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<AgentMetrics>(line) {
                Ok(metrics) => all_metrics.push(metrics),
                Err(e) => {
                    debug!("Failed to parse metrics line: {}", e);
                }
            }
        }

        Ok(all_metrics)
    }

    // Turso storage implementation (feature-gated)
    #[cfg(feature = "turso")]
    async fn store_turso(&self, connection: &str, metrics: &AgentMetrics) -> Result<()> {
        // TODO: Implement Turso storage
        info!("Turso storage not yet implemented");
        Ok(())
    }

    #[cfg(feature = "turso")]
    async fn load_turso(
        &self,
        connection: &str,
        change_id: &ChangeId,
    ) -> Result<Option<AgentMetrics>> {
        // TODO: Implement Turso loading
        Ok(None)
    }

    #[cfg(feature = "turso")]
    async fn load_all_turso(&self, connection: &str) -> Result<Vec<AgentMetrics>> {
        // TODO: Implement Turso loading all
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_append_file_storage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("metrics.jsonl");

        let storage = MetricsStorage::append_file(&path);

        let metrics = AgentMetrics::new("agent-1", "change-1");

        storage.store(&metrics).await.unwrap();

        let loaded = storage.load(&"change-1".to_string()).await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_load_all() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("metrics.jsonl");

        let storage = MetricsStorage::append_file(&path);

        storage
            .store(&AgentMetrics::new("agent-1", "change-1"))
            .await
            .unwrap();
        storage
            .store(&AgentMetrics::new("agent-2", "change-2"))
            .await
            .unwrap();

        let all = storage.load_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }
}
