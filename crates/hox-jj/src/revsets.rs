//! Revset query helpers for Hox orchestration

use hox_core::{ChangeId, Result};

use crate::command::{JjExecutor, JjOutput};

/// Helper for building and executing revset queries
pub struct RevsetQueries<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor> RevsetQueries<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Execute a revset query and return matching change IDs
    pub async fn query(&self, revset: &str) -> Result<Vec<ChangeId>> {
        let output = self
            .executor
            .exec(&["log", "-r", revset, "-T", "change_id ++ \"\\n\"", "--no-graph"])
            .await?;

        Ok(parse_change_ids(&output))
    }

    /// Find ready tasks (open status, no conflicts, at heads)
    ///
    /// Revset: `heads(description(glob:"Status: open")) - conflicts()`
    ///
    /// Note: When JJ fork is complete, this becomes:
    /// `heads(status(open)) - conflicts()`
    pub async fn ready_tasks(&self) -> Result<Vec<ChangeId>> {
        self.query("heads(description(glob:\"Status: open\")) - conflicts()")
            .await
    }

    /// Find tasks assigned to a specific orchestrator
    ///
    /// Note: When JJ fork is complete, this becomes:
    /// `orchestrator("O-A-1")`
    pub async fn by_orchestrator(&self, orchestrator: &str) -> Result<Vec<ChangeId>> {
        let revset = format!("description(glob:\"Orchestrator: {}\")", orchestrator);
        self.query(&revset).await
    }

    /// Find tasks assigned to a specific agent
    pub async fn by_agent(&self, agent: &str) -> Result<Vec<ChangeId>> {
        let revset = format!("description(glob:\"Agent: {}\")", agent);
        self.query(&revset).await
    }

    /// Find messages addressed to a target (supports wildcards)
    ///
    /// Note: When JJ fork is complete with glob support for msg_to:
    /// `msg_to("O-A-*")`
    pub async fn messages_to(&self, target: &str) -> Result<Vec<ChangeId>> {
        // For now, we need to handle wildcards in application code
        // JJ's glob support in description() is limited
        let revset = format!("description(glob:\"Msg-To: {}\")", target);
        self.query(&revset).await
    }

    /// Find mutation messages (structural decisions from orchestrators)
    pub async fn mutations(&self) -> Result<Vec<ChangeId>> {
        self.query("description(glob:\"Msg-Type: mutation\")")
            .await
    }

    /// Find alignment requests
    pub async fn align_requests(&self) -> Result<Vec<ChangeId>> {
        self.query("description(glob:\"Msg-Type: align_request\")")
            .await
    }

    /// Find ancestors of a change (what blocks this task)
    pub async fn ancestors(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        let revset = format!("ancestors({}) & mutable()", change_id);
        self.query(&revset).await
    }

    /// Find descendants of a change (what this task blocks)
    pub async fn descendants(&self, change_id: &ChangeId) -> Result<Vec<ChangeId>> {
        let revset = format!("descendants({})", change_id);
        self.query(&revset).await
    }

    /// Find tasks by priority
    pub async fn by_priority(&self, priority: &str) -> Result<Vec<ChangeId>> {
        let revset = format!("description(glob:\"Priority: {}\")", priority);
        self.query(&revset).await
    }

    /// Find tasks by status
    pub async fn by_status(&self, status: &str) -> Result<Vec<ChangeId>> {
        let revset = format!("description(glob:\"Status: {}\")", status);
        self.query(&revset).await
    }

    /// Find changes with conflicts
    pub async fn conflicts(&self) -> Result<Vec<ChangeId>> {
        self.query("conflicts()").await
    }

    /// Get current working copy change
    pub async fn current(&self) -> Result<Option<ChangeId>> {
        let changes = self.query("@").await?;
        Ok(changes.into_iter().next())
    }
}

/// Parse change IDs from JJ output
fn parse_change_ids(output: &JjOutput) -> Vec<ChangeId> {
    output
        .stdout
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_query_parsing() {
        let executor = MockJjExecutor::new().with_response(
            "log -r @ -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: "abc123\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let queries = RevsetQueries::new(executor);
        let result = queries.query("@").await.unwrap();

        assert_eq!(result, vec!["abc123"]);
    }
}
