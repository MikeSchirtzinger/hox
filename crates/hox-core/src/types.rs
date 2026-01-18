//! Core type definitions for Hox orchestration

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task priority levels (matches jj fork enhancement)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Critical = 0,
    High = 1,
    #[default]
    Medium = 2,
    Low = 3,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "critical" | "0" => Ok(Self::Critical),
            "high" | "1" => Ok(Self::High),
            "medium" | "2" => Ok(Self::Medium),
            "low" | "3" => Ok(Self::Low),
            _ => Err(format!("Invalid priority: {}", s)),
        }
    }
}

/// Task status (matches jj fork enhancement)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Open,
    InProgress,
    Blocked,
    Review,
    Done,
    Abandoned,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Blocked => write!(f, "blocked"),
            Self::Review => write!(f, "review"),
            Self::Done => write!(f, "done"),
            Self::Abandoned => write!(f, "abandoned"),
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "in_progress" | "inprogress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "review" => Ok(Self::Review),
            "done" => Ok(Self::Done),
            "abandoned" => Ok(Self::Abandoned),
            _ => Err(format!("Invalid status: {}", s)),
        }
    }
}

/// Message types for inter-agent communication
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Structural decision from orchestrator - agents MUST conform
    Mutation,
    /// Informational message - agents MAY read
    Info,
    /// Request for alignment decision - orchestrator SHOULD respond
    AlignRequest,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mutation => write!(f, "mutation"),
            Self::Info => write!(f, "info"),
            Self::AlignRequest => write!(f, "align_request"),
        }
    }
}

impl std::str::FromStr for MessageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mutation" => Ok(Self::Mutation),
            "info" => Ok(Self::Info),
            "align_request" | "alignrequest" | "align-request" => Ok(Self::AlignRequest),
            _ => Err(format!("Invalid message type: {}", s)),
        }
    }
}

/// Orchestrator identifier with hierarchical naming
///
/// Format: O-{level}-{number} (e.g., O-A-1, O-B-2)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrchestratorId {
    /// Level in the hierarchy (A, B, C, ...)
    pub level: char,
    /// Number within the level
    pub number: u32,
}

impl OrchestratorId {
    pub fn new(level: char, number: u32) -> Self {
        Self { level, number }
    }

    /// Create root orchestrator (O-A-1)
    pub fn root() -> Self {
        Self::new('A', 1)
    }

    /// Create a child orchestrator at the next level
    pub fn child(&self, number: u32) -> Self {
        let next_level = (self.level as u8 + 1) as char;
        Self::new(next_level, number)
    }

    /// Check if this orchestrator is an ancestor of another
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        self.level < other.level
    }

    /// Generate wildcard pattern for all orchestrators at this level
    pub fn level_wildcard(&self) -> String {
        format!("O-{}*", self.level)
    }
}

impl std::fmt::Display for OrchestratorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "O-{}-{}", self.level, self.number)
    }
}

impl std::str::FromStr for OrchestratorId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 || parts[0] != "O" {
            return Err(format!("Invalid orchestrator ID format: {}", s));
        }

        let level = parts[1]
            .chars()
            .next()
            .ok_or_else(|| format!("Invalid level in: {}", s))?;

        let number: u32 = parts[2]
            .parse()
            .map_err(|_| format!("Invalid number in: {}", s))?;

        Ok(Self::new(level, number))
    }
}

/// Agent identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId {
    /// The orchestrator this agent belongs to
    pub orchestrator: OrchestratorId,
    /// Unique agent identifier
    pub id: Uuid,
    /// Optional human-readable name
    pub name: Option<String>,
}

impl AgentId {
    pub fn new(orchestrator: OrchestratorId) -> Self {
        Self {
            orchestrator,
            id: Uuid::new_v4(),
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "{}/{}", self.orchestrator, name)
        } else {
            write!(f, "{}/agent-{}", self.orchestrator, &self.id.to_string()[..8])
        }
    }
}

/// JJ Change ID (hex string)
pub type ChangeId = String;

/// JJ Commit ID (hex string)
pub type CommitId = String;

/// Hox metadata that can be attached to JJ changes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HoxMetadata {
    pub priority: Option<Priority>,
    pub status: Option<TaskStatus>,
    pub agent: Option<String>,
    pub orchestrator: Option<String>,
    pub msg_to: Option<String>,
    pub msg_type: Option<MessageType>,
}

impl HoxMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn with_status(mut self, status: TaskStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    pub fn with_orchestrator(mut self, orchestrator: impl Into<String>) -> Self {
        self.orchestrator = Some(orchestrator.into());
        self
    }

    pub fn with_message(mut self, to: impl Into<String>, msg_type: MessageType) -> Self {
        self.msg_to = Some(to.into());
        self.msg_type = Some(msg_type);
        self
    }
}

/// A task in the Hox system (corresponds to a JJ change)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// JJ change ID (primary identifier)
    pub change_id: ChangeId,
    /// JJ commit ID (current commit)
    pub commit_id: Option<CommitId>,
    /// Task description
    pub description: String,
    /// Hox metadata
    pub metadata: HoxMetadata,
    /// Parent change IDs (dependencies)
    pub parents: Vec<ChangeId>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl Task {
    pub fn new(change_id: impl Into<String>, description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            change_id: change_id.into(),
            commit_id: None,
            description: description.into(),
            metadata: HoxMetadata::new(),
            parents: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_metadata(mut self, metadata: HoxMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_parents(mut self, parents: Vec<ChangeId>) -> Self {
        self.parents = parents;
        self
    }
}

/// Handoff context for agent state preservation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandoffContext {
    /// What the agent is currently focused on
    pub current_focus: String,
    /// Progress made so far
    pub progress: Vec<String>,
    /// Planned next steps
    pub next_steps: Vec<String>,
    /// Current blockers
    pub blockers: Vec<String>,
    /// Files touched during work
    pub files_touched: Vec<String>,
    /// Key decisions made
    pub decisions: Vec<String>,
}

/// Agent telemetry data
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentTelemetry {
    /// Total tool calls made
    pub tool_calls: u32,
    /// Failed tool calls
    pub failed_calls: u32,
    /// Time spent in milliseconds
    pub time_ms: u64,
    /// Alignment requests made
    pub align_requests: u32,
    /// Mutation conflicts encountered
    pub mutation_conflicts: u32,
}

/// Scoring weights for self-evolution evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    /// Quality weight (default 0.35)
    pub quality: f32,
    /// Completeness weight (default 0.30)
    pub completeness: f32,
    /// Time weight (default 0.20)
    pub time: f32,
    /// Efficiency weight (default 0.15)
    pub efficiency: f32,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            quality: 0.35,
            completeness: 0.30,
            time: 0.20,
            efficiency: 0.15,
        }
    }
}

impl ScoringWeights {
    /// Calculate weighted score from individual scores (0.0 - 1.0 each)
    pub fn calculate(&self, quality: f32, completeness: f32, time: f32, efficiency: f32) -> f32 {
        self.quality * quality
            + self.completeness * completeness
            + self.time * time
            + self.efficiency * efficiency
    }
}

/// Phase in orchestrated execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Phase number (0 = contracts, N+1 = integration, N+2 = validation)
    pub number: u32,
    /// Phase name
    pub name: String,
    /// Description of what this phase accomplishes
    pub description: String,
    /// Whether this phase blocks subsequent phases
    pub blocking: bool,
    /// Tasks in this phase
    pub tasks: Vec<ChangeId>,
}

impl Phase {
    /// Create a contracts phase (Phase 0)
    pub fn contracts(description: impl Into<String>) -> Self {
        Self {
            number: 0,
            name: "contracts".to_string(),
            description: description.into(),
            blocking: true,
            tasks: Vec::new(),
        }
    }

    /// Create an integration phase
    pub fn integration(number: u32, description: impl Into<String>) -> Self {
        Self {
            number,
            name: "integration".to_string(),
            description: description.into(),
            blocking: true,
            tasks: Vec::new(),
        }
    }

    /// Create a validation phase
    pub fn validation(number: u32, description: impl Into<String>) -> Self {
        Self {
            number,
            name: "validation".to_string(),
            description: description.into(),
            blocking: true,
            tasks: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_id_parsing() {
        let id: OrchestratorId = "O-A-1".parse().unwrap();
        assert_eq!(id.level, 'A');
        assert_eq!(id.number, 1);
        assert_eq!(id.to_string(), "O-A-1");
    }

    #[test]
    fn test_orchestrator_child() {
        let parent = OrchestratorId::root();
        let child = parent.child(1);
        assert_eq!(child.level, 'B');
        assert_eq!(child.number, 1);
        assert_eq!(child.to_string(), "O-B-1");
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Medium);
        assert!(Priority::Medium < Priority::Low);
    }

    #[test]
    fn test_scoring_weights() {
        let weights = ScoringWeights::default();
        // All perfect scores should give 1.0
        let score = weights.calculate(1.0, 1.0, 1.0, 1.0);
        assert!((score - 1.0).abs() < 0.001);
    }
}
