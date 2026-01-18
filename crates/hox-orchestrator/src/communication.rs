//! Communication protocol for orchestrators and agents

use hox_core::{ChangeId, MessageType, OrchestratorId};
use std::collections::HashMap;
use tracing::debug;

/// A message in the Hox communication protocol
#[derive(Debug, Clone)]
pub struct Message {
    /// Source of the message
    pub from: String,
    /// Target of the message (can include wildcards like O-A-*)
    pub to: String,
    /// Message type
    pub msg_type: MessageType,
    /// Message content
    pub content: String,
    /// Change ID where this message is stored
    pub change_id: Option<ChangeId>,
}

impl Message {
    pub fn new(from: impl Into<String>, to: impl Into<String>, msg_type: MessageType) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            msg_type,
            content: String::new(),
            change_id: None,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_change_id(mut self, change_id: ChangeId) -> Self {
        self.change_id = Some(change_id);
        self
    }

    /// Create a mutation message from an orchestrator
    pub fn mutation(orchestrator: &OrchestratorId, content: impl Into<String>) -> Self {
        Self::new(orchestrator.to_string(), "*", MessageType::Mutation)
            .with_content(content)
    }

    /// Create an alignment request from an agent
    pub fn align_request(
        agent: impl Into<String>,
        orchestrator: &OrchestratorId,
        content: impl Into<String>,
    ) -> Self {
        Self::new(agent, orchestrator.to_string(), MessageType::AlignRequest)
            .with_content(content)
    }

    /// Create an info message
    pub fn info(
        from: impl Into<String>,
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self::new(from, to, MessageType::Info).with_content(content)
    }

    /// Check if this message matches a target pattern
    pub fn matches_target(&self, target: &str) -> bool {
        if self.to == "*" {
            return true;
        }

        if self.to == target {
            return true;
        }

        // Handle wildcards like O-A-*
        if self.to.ends_with('*') {
            let prefix = &self.to[..self.to.len() - 1];
            return target.starts_with(prefix);
        }

        false
    }
}

/// Routes messages between orchestrators and agents
pub struct MessageRouter {
    /// Pending messages indexed by target
    pending: HashMap<String, Vec<Message>>,
    /// Message history
    history: Vec<Message>,
}

impl MessageRouter {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            history: Vec::new(),
        }
    }

    /// Route a message to its target
    pub fn route(&mut self, message: Message) {
        debug!(
            "Routing message from {} to {}: {}",
            message.from, message.to, message.msg_type
        );

        // Store in pending for the target
        self.pending
            .entry(message.to.clone())
            .or_default()
            .push(message.clone());

        // Also handle wildcards - store for any matching targets
        if message.to.ends_with('*') {
            // Wildcard messages are stored with their pattern
            // Receivers will check for matches
        }

        self.history.push(message);
    }

    /// Get pending messages for a target
    pub fn get_pending(&mut self, target: &str) -> Vec<Message> {
        let mut messages = Vec::new();

        // Get exact matches
        if let Some(msgs) = self.pending.remove(target) {
            messages.extend(msgs);
        }

        // Check wildcard patterns
        let wildcard_keys: Vec<String> = self
            .pending
            .keys()
            .filter(|k| k.ends_with('*'))
            .cloned()
            .collect();

        for key in wildcard_keys {
            let prefix = &key[..key.len() - 1];
            if target.starts_with(prefix) {
                if let Some(msgs) = self.pending.remove(&key) {
                    messages.extend(msgs);
                }
            }
        }

        messages
    }

    /// Check for pending messages without consuming them
    pub fn has_pending(&self, target: &str) -> bool {
        if self.pending.contains_key(target) {
            return true;
        }

        // Check wildcards
        for key in self.pending.keys() {
            if key.ends_with('*') {
                let prefix = &key[..key.len() - 1];
                if target.starts_with(prefix) {
                    return true;
                }
            }
        }

        false
    }

    /// Get message history
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Get mutations from history
    pub fn mutations(&self) -> Vec<&Message> {
        self.history
            .iter()
            .filter(|m| m.msg_type == MessageType::Mutation)
            .collect()
    }

    /// Get alignment requests from history
    pub fn align_requests(&self) -> Vec<&Message> {
        self.history
            .iter()
            .filter(|m| m.msg_type == MessageType::AlignRequest)
            .collect()
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_wildcard_matching() {
        let orchestrator = OrchestratorId::new('A', 1);
        let mut message = Message::mutation(&orchestrator, "Use user_id field");
        message.to = "O-A-*".to_string();

        assert!(message.matches_target("O-A-1"));
        assert!(message.matches_target("O-A-2"));
        assert!(message.matches_target("O-A-99"));
        assert!(!message.matches_target("O-B-1"));
    }

    #[test]
    fn test_message_router() {
        let mut router = MessageRouter::new();

        let orchestrator = OrchestratorId::new('A', 1);
        let message = Message::mutation(&orchestrator, "Test mutation");

        router.route(message);

        assert!(!router.history().is_empty());
        assert_eq!(router.mutations().len(), 1);
    }

    #[test]
    fn test_wildcard_routing() {
        let mut router = MessageRouter::new();

        let message = Message::info("O-A-1", "O-B-*", "Broadcast to level B");
        router.route(message);

        // O-B-1 should receive the message
        let messages = router.get_pending("O-B-1");
        assert_eq!(messages.len(), 1);
    }
}
