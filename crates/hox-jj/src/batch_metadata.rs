//! Batch metadata transactions for JJ changes.
//!
//! Accumulates multiple metadata updates and writes them in a single
//! `jj describe` call, reducing oplog writes from N to 1 (Tier 1)
//! or using `--metadata-only` to skip `update_op_heads` entirely (Tier 2 / W7).

use std::collections::HashMap;

use hox_core::Result;

use crate::command::JjExecutor;

/// Accumulates metadata changes and writes them in a single `jj describe` call.
///
/// # Tiers
/// - **Tier 1:** Single `jj describe` call (replaces N calls).
/// - **Tier 2 (W7):** Appends `--metadata-only` when fork features are available,
///   skipping `update_op_heads` for zero effective oplog writes.
pub struct MetadataBatch {
    /// Key-value trailer pairs queued for writing.
    updates: HashMap<String, String>,
    /// Whether the W7 `--metadata-only` flag is available on this jj binary.
    fork_features_available: bool,
}

impl MetadataBatch {
    /// Create a new empty batch (fork features disabled by default).
    pub fn new() -> Self {
        Self {
            updates: HashMap::new(),
            fork_features_available: false,
        }
    }

    /// Enable or disable Tier 2 (W7 `--metadata-only`) support.
    pub fn with_fork_features(mut self, available: bool) -> Self {
        self.fork_features_available = available;
        self
    }

    /// Queue a metadata trailer update.
    ///
    /// Keys are written as `Key: value` trailers in the change description.
    /// Calling `set` with the same key twice overwrites the earlier value.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.updates.insert(key.into(), value.into());
    }

    /// Number of queued updates.
    pub fn len(&self) -> usize {
        self.updates.len()
    }

    /// Returns `true` if no updates are queued.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Commit all queued updates in a single `jj describe` call.
    ///
    /// - Reads the current description for `change_id`.
    /// - Strips any existing trailer lines whose keys match our queued keys.
    /// - Appends the new trailers.
    /// - Writes back with a single `jj describe` (plus `--metadata-only` on W7).
    ///
    /// Returns immediately if the batch is empty (no-op).
    pub async fn commit<E: JjExecutor>(&self, executor: &E, change_id: &str) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }

        // Read the current description.
        let output = executor
            .exec(&["log", "-r", change_id, "-T", "description", "--no-graph"])
            .await?;
        let existing = output.stdout.trim();

        // Build a set of lowercased keys we will (re)write so we can strip stale lines.
        let queued_keys_lower: HashMap<String, &str> = self
            .updates
            .keys()
            .map(|k| (k.to_lowercase(), k.as_str()))
            .collect();

        // Strip existing trailer lines that match any of our queued keys.
        let cleaned: Vec<&str> = existing
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if let Some((key, _)) = trimmed.split_once(':') {
                    !queued_keys_lower.contains_key(&key.trim().to_lowercase())
                } else {
                    true
                }
            })
            .collect();

        // Build the new trailers block (stable key order for deterministic output).
        let mut trailer_keys: Vec<&String> = self.updates.keys().collect();
        trailer_keys.sort();
        let trailer_lines: Vec<String> = trailer_keys
            .iter()
            .map(|k| format!("{}: {}", k, self.updates[*k]))
            .collect();
        let trailers = trailer_lines.join("\n");

        // Combine body + trailers.
        let new_description = {
            let body = cleaned.join("\n");
            let body = body.trim();
            if body.is_empty() {
                trailers
            } else {
                format!("{}\n\n{}", body, trailers)
            }
        };

        // Build the argument list.
        let mut args = vec!["describe", "-r", change_id, "-m", &new_description];
        if self.fork_features_available {
            args.push("--metadata-only");
        }

        executor.exec(&args).await?;

        Ok(())
    }
}

impl Default for MetadataBatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    // Helper: executor that returns an empty description and records the last describe call.
    fn executor_with_empty_desc(change_id: &str) -> MockJjExecutor {
        let log_key = format!("log -r {} -T description --no-graph", change_id);
        MockJjExecutor::new().with_response(
            &log_key,
            JjOutput { stdout: String::new(), stderr: String::new(), success: true },
        )
    }

    // Helper: executor that returns `desc` as the current description.
    fn executor_with_desc(change_id: &str, desc: &str) -> MockJjExecutor {
        let log_key = format!("log -r {} -T description --no-graph", change_id);
        MockJjExecutor::new().with_response(
            &log_key,
            JjOutput { stdout: desc.to_string(), stderr: String::new(), success: true },
        )
    }

    // --- Empty batch is a no-op ---

    #[tokio::test]
    async fn test_empty_batch_is_noop() {
        let executor = MockJjExecutor::new(); // no responses registered — any exec call would error
        let batch = MetadataBatch::new();
        // Should return Ok without calling any executor methods.
        assert!(batch.commit(&executor, "abc123").await.is_ok());
    }

    // --- Multiple updates batched into a single describe call ---

    #[tokio::test]
    async fn test_multiple_updates_single_describe() {
        let change_id = "xyz789";
        // We need log + describe responses.
        let log_key = format!("log -r {} -T description --no-graph", change_id);

        // Build a tracking executor.  We'll use a mock that accepts the describe
        // with the expected message.  The exact message content is verified
        // by checking the executor accepts the call without error.
        let mut batch = MetadataBatch::new();
        batch.set("Status", "in_progress");
        batch.set("Priority", "high");
        batch.set("Agent", "agent-42");

        assert_eq!(batch.len(), 3);
        assert!(!batch.is_empty());

        // Build an executor that accepts both calls.
        // Keys sort alphabetically: Agent, Priority, Status.
        let expected_msg = "Agent: agent-42\nPriority: high\nStatus: in_progress";
        let describe_key =
            format!("describe -r {} -m {}", change_id, expected_msg);

        let executor = MockJjExecutor::new()
            .with_response(
                &log_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            )
            .with_response(
                &describe_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            );

        batch.commit(&executor, change_id).await.unwrap();
    }

    // --- W7 flag appended when fork features enabled ---

    #[tokio::test]
    async fn test_w7_flag_appended_when_fork_enabled() {
        let change_id = "fork001";
        let log_key = format!("log -r {} -T description --no-graph", change_id);
        let expected_msg = "Status: open";
        let describe_key =
            format!("describe -r {} -m {} --metadata-only", change_id, expected_msg);

        let executor = MockJjExecutor::new()
            .with_response(
                &log_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            )
            .with_response(
                &describe_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            );

        let mut batch = MetadataBatch::new().with_fork_features(true);
        batch.set("Status", "open");
        batch.commit(&executor, change_id).await.unwrap();
    }

    // --- W7 flag NOT appended when fork features disabled ---

    #[tokio::test]
    async fn test_w7_flag_absent_when_fork_disabled() {
        let change_id = "nofork";
        let log_key = format!("log -r {} -T description --no-graph", change_id);
        let expected_msg = "Status: open";
        let describe_key = format!("describe -r {} -m {}", change_id, expected_msg);

        let executor = MockJjExecutor::new()
            .with_response(
                &log_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            )
            .with_response(
                &describe_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            );

        let mut batch = MetadataBatch::new().with_fork_features(false);
        batch.set("Status", "open");
        batch.commit(&executor, change_id).await.unwrap();
    }

    // --- Existing body text is preserved; stale trailers are replaced ---

    #[tokio::test]
    async fn test_existing_trailers_replaced_body_preserved() {
        let change_id = "upd001";
        let existing_desc = "Implement feature\n\nStatus: open\nPriority: low";
        let log_key = format!("log -r {} -T description --no-graph", change_id);
        // After stripping Status and Priority from existing, body = "Implement feature".
        // New trailers (sorted): Priority: high, Status: in_progress.
        let expected_msg =
            "Implement feature\n\nPriority: high\nStatus: in_progress";
        let describe_key =
            format!("describe -r {} -m {}", change_id, expected_msg);

        let executor = MockJjExecutor::new()
            .with_response(
                &log_key,
                JjOutput {
                    stdout: existing_desc.to_string(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with_response(
                &describe_key,
                JjOutput { stdout: String::new(), stderr: String::new(), success: true },
            );

        let mut batch = MetadataBatch::new();
        batch.set("Status", "in_progress");
        batch.set("Priority", "high");
        batch.commit(&executor, change_id).await.unwrap();
    }
}
