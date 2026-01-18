//! Telemetry collection for agents

use chrono::{DateTime, Utc};
use hox_core::{AgentTelemetry, ChangeId, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Types of telemetry events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryEvent {
    /// Tool was called
    ToolCall {
        tool_name: String,
        success: bool,
        duration_ms: u64,
    },
    /// Status changed
    StatusChange {
        from: TaskStatus,
        to: TaskStatus,
    },
    /// Alignment requested
    AlignmentRequested {
        topic: String,
    },
    /// Mutation conflict encountered
    MutationConflict {
        mutation_source: String,
    },
    /// Custom event
    Custom {
        name: String,
        data: serde_json::Value,
    },
}

/// Collected telemetry for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// Agent identifier
    pub agent_id: String,
    /// Change ID being worked on
    pub change_id: ChangeId,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time (if completed)
    pub ended_at: Option<DateTime<Utc>>,
    /// Telemetry summary
    pub telemetry: AgentTelemetry,
    /// All events
    pub events: Vec<(DateTime<Utc>, TelemetryEvent)>,
}

impl AgentMetrics {
    pub fn new(agent_id: impl Into<String>, change_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            change_id: change_id.into(),
            started_at: Utc::now(),
            ended_at: None,
            telemetry: AgentTelemetry::default(),
            events: Vec::new(),
        }
    }

    pub fn record_event(&mut self, event: TelemetryEvent) {
        let now = Utc::now();

        // Update counters based on event type
        match &event {
            TelemetryEvent::ToolCall { success, .. } => {
                self.telemetry.tool_calls += 1;
                if !success {
                    self.telemetry.failed_calls += 1;
                }
            }
            TelemetryEvent::AlignmentRequested { .. } => {
                self.telemetry.align_requests += 1;
            }
            TelemetryEvent::MutationConflict { .. } => {
                self.telemetry.mutation_conflicts += 1;
            }
            _ => {}
        }

        self.events.push((now, event));
    }

    pub fn complete(&mut self) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.telemetry.time_ms = (now - self.started_at).num_milliseconds() as u64;
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f32 {
        if self.telemetry.tool_calls == 0 {
            return 1.0;
        }
        let successful = self.telemetry.tool_calls - self.telemetry.failed_calls;
        successful as f32 / self.telemetry.tool_calls as f32
    }
}

/// Metrics collector for the system
pub struct MetricsCollector {
    /// Metrics by agent ID
    agents: Arc<RwLock<HashMap<String, AgentMetrics>>>,
    /// Global counters
    total_tool_calls: AtomicU32,
    total_failures: AtomicU32,
    total_time_ms: AtomicU64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            total_tool_calls: AtomicU32::new(0),
            total_failures: AtomicU32::new(0),
            total_time_ms: AtomicU64::new(0),
        }
    }

    /// Start tracking an agent
    pub async fn start_agent(&self, agent_id: &str, change_id: &str) {
        let mut agents = self.agents.write().await;
        agents.insert(
            agent_id.to_string(),
            AgentMetrics::new(agent_id, change_id),
        );
        debug!("Started tracking agent {}", agent_id);
    }

    /// Record an event for an agent
    pub async fn record(&self, agent_id: &str, event: TelemetryEvent) {
        // Update global counters
        if let TelemetryEvent::ToolCall { success, .. } = &event {
            self.total_tool_calls.fetch_add(1, Ordering::Relaxed);
            if !success {
                self.total_failures.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Update agent metrics
        let mut agents = self.agents.write().await;
        if let Some(metrics) = agents.get_mut(agent_id) {
            metrics.record_event(event);
        }
    }

    /// Complete tracking for an agent
    pub async fn complete_agent(&self, agent_id: &str) -> Option<AgentMetrics> {
        let mut agents = self.agents.write().await;
        if let Some(metrics) = agents.get_mut(agent_id) {
            metrics.complete();
            self.total_time_ms
                .fetch_add(metrics.telemetry.time_ms, Ordering::Relaxed);
            return Some(metrics.clone());
        }
        None
    }

    /// Get metrics for an agent
    pub async fn get_agent_metrics(&self, agent_id: &str) -> Option<AgentMetrics> {
        let agents = self.agents.read().await;
        agents.get(agent_id).cloned()
    }

    /// Get all agent metrics
    pub async fn get_all_metrics(&self) -> HashMap<String, AgentMetrics> {
        let agents = self.agents.read().await;
        agents.clone()
    }

    /// Get global summary
    pub fn global_summary(&self) -> GlobalMetrics {
        GlobalMetrics {
            total_tool_calls: self.total_tool_calls.load(Ordering::Relaxed),
            total_failures: self.total_failures.load(Ordering::Relaxed),
            total_time_ms: self.total_time_ms.load(Ordering::Relaxed),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Global metrics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMetrics {
    pub total_tool_calls: u32,
    pub total_failures: u32,
    pub total_time_ms: u64,
}

impl GlobalMetrics {
    pub fn success_rate(&self) -> f32 {
        if self.total_tool_calls == 0 {
            return 1.0;
        }
        let successful = self.total_tool_calls - self.total_failures;
        successful as f32 / self.total_tool_calls as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collection() {
        let collector = MetricsCollector::new();

        collector.start_agent("agent-1", "change-1").await;

        collector
            .record(
                "agent-1",
                TelemetryEvent::ToolCall {
                    tool_name: "read".to_string(),
                    success: true,
                    duration_ms: 100,
                },
            )
            .await;

        collector
            .record(
                "agent-1",
                TelemetryEvent::ToolCall {
                    tool_name: "write".to_string(),
                    success: false,
                    duration_ms: 50,
                },
            )
            .await;

        let metrics = collector.complete_agent("agent-1").await.unwrap();

        assert_eq!(metrics.telemetry.tool_calls, 2);
        assert_eq!(metrics.telemetry.failed_calls, 1);
        assert_eq!(metrics.success_rate(), 0.5);
    }
}
