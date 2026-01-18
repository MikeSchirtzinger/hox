//! JJ Operation Log watcher for detecting changes

use hox_core::Result;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::command::JjExecutor;

/// Events emitted by the oplog watcher
#[derive(Debug, Clone)]
pub enum OpLogEvent {
    /// A new operation was detected
    NewOperation {
        operation_id: String,
        description: String,
    },
    /// Watcher started
    Started,
    /// Watcher stopped
    Stopped,
    /// Error occurred
    Error(String),
}

/// Configuration for the oplog watcher
#[derive(Debug, Clone)]
pub struct OpLogWatcherConfig {
    /// Polling interval
    pub poll_interval: Duration,
    /// Number of recent operations to check
    pub check_count: usize,
}

impl Default for OpLogWatcherConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(500),
            check_count: 10,
        }
    }
}

/// Watches the JJ operation log for changes
///
/// This is more efficient than file system watching for JJ repos
/// because operations are the true source of change events.
pub struct OpLogWatcher<E: JjExecutor> {
    executor: E,
    config: OpLogWatcherConfig,
    last_operation_id: Option<String>,
}

impl<E: JjExecutor + 'static> OpLogWatcher<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            config: OpLogWatcherConfig::default(),
            last_operation_id: None,
        }
    }

    pub fn with_config(mut self, config: OpLogWatcherConfig) -> Self {
        self.config = config;
        self
    }

    /// Get the repository root
    pub fn repo_root(&self) -> &PathBuf {
        self.executor.repo_root()
    }

    /// Get the current operation ID
    async fn current_operation(&self) -> Result<Option<(String, String)>> {
        let output = self
            .executor
            .exec(&[
                "op",
                "log",
                "-n",
                "1",
                "-T",
                "operation_id ++ \"\\t\" ++ description",
                "--no-graph",
            ])
            .await?;

        if !output.success || output.stdout.trim().is_empty() {
            return Ok(None);
        }

        let line = output.stdout.lines().next().unwrap_or("");
        let parts: Vec<&str> = line.splitn(2, '\t').collect();

        if parts.len() >= 2 {
            Ok(Some((parts[0].to_string(), parts[1].to_string())))
        } else if !parts.is_empty() {
            Ok(Some((parts[0].to_string(), String::new())))
        } else {
            Ok(None)
        }
    }

    /// Start watching and return a receiver for events
    pub async fn watch(mut self) -> Result<mpsc::Receiver<OpLogEvent>> {
        let (tx, rx) = mpsc::channel(100);

        // Get initial operation ID
        if let Some((id, _)) = self.current_operation().await? {
            self.last_operation_id = Some(id);
        }

        let _ = tx.send(OpLogEvent::Started).await;
        info!("OpLog watcher started for {}", self.repo_root().display());

        tokio::spawn(async move {
            let mut poll_interval = interval(self.config.poll_interval);

            loop {
                poll_interval.tick().await;

                match self.current_operation().await {
                    Ok(Some((id, desc))) => {
                        if self.last_operation_id.as_ref() != Some(&id) {
                            debug!("New operation detected: {}", id);

                            if tx
                                .send(OpLogEvent::NewOperation {
                                    operation_id: id.clone(),
                                    description: desc,
                                })
                                .await
                                .is_err()
                            {
                                // Receiver dropped
                                break;
                            }

                            self.last_operation_id = Some(id);
                        }
                    }
                    Ok(None) => {
                        debug!("No operations found");
                    }
                    Err(e) => {
                        warn!("Error checking oplog: {}", e);
                        let _ = tx.send(OpLogEvent::Error(e.to_string())).await;
                    }
                }
            }

            let _ = tx.send(OpLogEvent::Stopped).await;
            info!("OpLog watcher stopped");
        });

        Ok(rx)
    }

    /// Check for new operations once (non-blocking)
    pub async fn check_once(&mut self) -> Result<Option<OpLogEvent>> {
        match self.current_operation().await? {
            Some((id, desc)) => {
                if self.last_operation_id.as_ref() != Some(&id) {
                    self.last_operation_id = Some(id.clone());
                    Ok(Some(OpLogEvent::NewOperation {
                        operation_id: id,
                        description: desc,
                    }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_current_operation() {
        let executor = MockJjExecutor::new().with_response(
            "op log -n 1 -T operation_id ++ \"\\t\" ++ description --no-graph",
            JjOutput {
                stdout: "abc123\ttest operation".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let watcher = OpLogWatcher::new(executor);
        let result = watcher.current_operation().await.unwrap();

        assert_eq!(
            result,
            Some(("abc123".to_string(), "test operation".to_string()))
        );
    }
}
