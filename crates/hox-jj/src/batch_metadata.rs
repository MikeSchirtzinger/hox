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

        // Split description into (body, trailer_block) by finding the last blank-line separator.
        // Trailers are conventionally at the end, separated from the body by a blank line.
        // Only strip matching keys from the trailer block — body lines with colons are preserved.
        let (body_lines, trailer_lines_existing): (Vec<&str>, Vec<&str>) = {
            let lines: Vec<&str> = existing.lines().collect();
            // Find the last blank line index (if any)
            let last_blank = lines.iter().rposition(|l| l.trim().is_empty());
            match last_blank {
                Some(idx) => {
                    // Everything before idx is body; everything after is the trailer block
                    let body = &lines[..idx];
                    let trailers = &lines[idx + 1..];
                    (body.to_vec(), trailers.to_vec())
                }
                None => {
                    // No blank line — entire content is body; no trailer block to strip
                    (lines, vec![])
                }
            }
        };

        // Strip only the trailer lines (last paragraph) whose key matches a queued key.
        let filtered_trailers: Vec<&str> = trailer_lines_existing
            .iter()
            .filter(|line| {
                let trimmed = line.trim();
                if let Some((key, _)) = trimmed.split_once(':') {
                    !queued_keys_lower.contains_key(&key.trim().to_lowercase())
                } else {
                    true
                }
            })
            .copied()
            .collect();

        // Reconstruct the cleaned description: body + any surviving old trailers.
        let cleaned: Vec<&str> = if filtered_trailers.is_empty() {
            body_lines
        } else {
            let mut combined = body_lines;
            combined.push(""); // restore the blank separator
            combined.extend(filtered_trailers);
            combined
        };

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

    // --- CRIT-4: Body lines containing colons must not be stripped ---

    #[tokio::test]
    async fn test_body_colon_lines_not_stripped() {
        // A description where the body contains a line that looks like a trailer
        // (e.g. "Fix: auth timeout") but is actually body text.
        // The real trailer (Status) only appears after the final blank line.
        let change_id = "crit4a";
        let existing_desc = "Fix timeout issues\n\nFix: auth timeout happened during load\nSee: https://example.com/issue\n\nStatus: open";
        let log_key = format!("log -r {} -T description --no-graph", change_id);

        // Only the trailing "Status: open" should be replaced.
        // "Fix: auth timeout..." and "See: ..." are in the body (before the last blank line)
        // and must be preserved verbatim.
        let expected_msg = "Fix timeout issues\n\nFix: auth timeout happened during load\nSee: https://example.com/issue\n\nStatus: in_progress";
        let describe_key = format!("describe -r {} -m {}", change_id, expected_msg);

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
        batch.commit(&executor, change_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_body_with_colon_no_trailer_block() {
        // Description has body lines with colons but no blank-line-separated trailer block.
        // Adding a new trailer should append after a blank line without disturbing the body.
        let change_id = "crit4b";
        let existing_desc = "Fix: resolve connection pool exhaustion";
        let log_key = format!("log -r {} -T description --no-graph", change_id);

        // Body has no trailer block, so the whole thing is body.
        // New trailer appended after blank line.
        let expected_msg = "Fix: resolve connection pool exhaustion\n\nStatus: open";
        let describe_key = format!("describe -r {} -m {}", change_id, expected_msg);

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
        batch.set("Status", "open");
        batch.commit(&executor, change_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_multi_paragraph_body_colon_lines_preserved() {
        // Multi-paragraph description with colon-containing lines scattered through body.
        // Only the final paragraph (after last blank line) is the trailer zone.
        let change_id = "crit4c";
        let existing_desc = "Implement VCS abstraction\n\nNote: this is complex\nSee: design-doc.md\n\nProgress notes\nFix: edge case handled\n\nAgent: old-agent\nStatus: open";
        let log_key = format!("log -r {} -T description --no-graph", change_id);

        // The last blank line is before "Agent: old-agent\nStatus: open".
        // That block is the trailer zone — Agent and Status get stripped/replaced.
        // All body paragraphs (including lines with colons) must be preserved.
        let expected_msg = "Implement VCS abstraction\n\nNote: this is complex\nSee: design-doc.md\n\nProgress notes\nFix: edge case handled\n\nAgent: agent-99\nStatus: in_progress";
        let describe_key = format!("describe -r {} -m {}", change_id, expected_msg);

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
        batch.set("Agent", "agent-99");
        batch.commit(&executor, change_id).await.unwrap();
    }
}
