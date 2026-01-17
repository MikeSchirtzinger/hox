//! Core data structures for the bd issue tracker.
//!
//! This module defines the main types used throughout the jj-beads system,
//! including Issues, Dependencies, Comments, and related enums.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Issue represents a trackable work item.
/// Fields are organized into logical groups for maintainability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Issue {
    // ===== Core Identification =====
    pub id: String,
    #[serde(skip)]
    pub content_hash: String, // Internal: SHA256 of canonical content - NOT exported to JSONL

    // ===== Issue Content =====
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    // ===== Status & Workflow =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    pub priority: i32, // No skip_serializing_if: 0 is valid (P0/critical)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<IssueType>,

    // ===== Assignment =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<i32>,

    // ===== Timestamps =====
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_by_session: Option<String>,

    // ===== Time-Based Scheduling (GH#820) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_until: Option<DateTime<Utc>>,

    // ===== External Integration =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<String>,

    // ===== Compaction Metadata =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted_at_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_size: Option<i32>,

    // ===== Internal Routing (not exported to JSONL) =====
    #[serde(skip)]
    pub source_repo: String,
    #[serde(skip)]
    pub id_prefix: String,

    // ===== Relational Data (populated for export/import) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<Dependency>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<Comment>>,

    // ===== Tombstone Fields (soft-delete support) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_type: Option<String>,

    // ===== Messaging Fields (inter-agent communication) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,

    // ===== Context Markers =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,

    // ===== Bonding Fields (compound molecule lineage) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonded_from: Option<Vec<BondRef>>,

    // ===== HOP Fields (entity tracking for CV chains) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<EntityRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validations: Option<Vec<Validation>>,

    // ===== Gate Fields (async coordination primitives) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>, // Duration as nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiters: Option<Vec<String>>,

    // ===== Slot Fields (exclusive access primitives) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,

    // ===== Source Tracing Fields (formula cooking origin) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_formula: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<String>,

    // ===== Agent Identity Fields (agent-as-bead support) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_bead: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_bead: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<AgentState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig: Option<String>,

    // ===== Molecule Type Fields (swarm coordination) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mol_type: Option<MolType>,

    // ===== Event Fields (operational state changes) =====
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

impl Issue {
    /// Tombstone TTL constants
    pub const DEFAULT_TOMBSTONE_TTL: Duration = Duration::days(30);
    pub const MIN_TOMBSTONE_TTL: Duration = Duration::days(7);
    pub const CLOCK_SKEW_GRACE: Duration = Duration::hours(1);

    /// ComputeContentHash creates a deterministic hash of the issue's content.
    /// Uses all substantive fields (excluding ID, timestamps, and compaction metadata)
    /// to ensure that identical content produces identical hashes across all clones.
    pub fn compute_content_hash(&self) -> String {
        let mut hasher = Sha256::new();

        // Core fields in stable order
        Self::hash_str(&mut hasher, &self.title);
        Self::hash_opt_str(&mut hasher, &self.description);
        Self::hash_opt_str(&mut hasher, &self.design);
        Self::hash_opt_str(&mut hasher, &self.acceptance_criteria);
        Self::hash_opt_str(&mut hasher, &self.notes);
        if let Some(ref status) = self.status {
            Self::hash_str(&mut hasher, &status.to_string());
        } else {
            Self::hash_str(&mut hasher, "");
        }
        Self::hash_int(&mut hasher, self.priority);
        if let Some(ref issue_type) = self.issue_type {
            Self::hash_str(&mut hasher, &issue_type.to_string());
        } else {
            Self::hash_str(&mut hasher, "");
        }
        Self::hash_opt_str(&mut hasher, &self.assignee);
        Self::hash_opt_str(&mut hasher, &self.created_by);

        // Optional fields
        Self::hash_opt_str(&mut hasher, &self.external_ref);
        Self::hash_flag(&mut hasher, self.pinned.unwrap_or(false), "pinned");
        Self::hash_flag(&mut hasher, self.is_template.unwrap_or(false), "template");

        // Bonded molecules
        if let Some(ref bonded_from) = self.bonded_from {
            for br in bonded_from {
                Self::hash_str(&mut hasher, &br.source_id);
                Self::hash_str(&mut hasher, &br.bond_type);
                Self::hash_str(&mut hasher, &br.bond_point.clone().unwrap_or_default());
            }
        }

        // HOP entity tracking
        if let Some(ref creator) = self.creator {
            Self::hash_entity_ref(&mut hasher, creator);
        }

        // HOP validations
        if let Some(ref validations) = self.validations {
            for v in validations {
                if let Some(ref validator) = v.validator {
                    Self::hash_entity_ref(&mut hasher, validator);
                }
                Self::hash_str(&mut hasher, &v.outcome);
                Self::hash_str(&mut hasher, &v.timestamp.to_rfc3339());
                if let Some(score) = v.score {
                    Self::hash_str(&mut hasher, &format!("{}", score));
                }
                hasher.update(&[0]);
            }
        }

        // Gate fields for async coordination
        Self::hash_opt_str(&mut hasher, &self.await_type);
        Self::hash_opt_str(&mut hasher, &self.await_id);
        if let Some(timeout) = self.timeout {
            Self::hash_str(&mut hasher, &format!("{}", timeout));
        }
        hasher.update(&[0]);
        if let Some(ref waiters) = self.waiters {
            for waiter in waiters {
                Self::hash_str(&mut hasher, waiter);
            }
        }

        // Slot fields for exclusive access
        Self::hash_opt_str(&mut hasher, &self.holder);

        // Agent identity fields
        Self::hash_opt_str(&mut hasher, &self.hook_bead);
        Self::hash_opt_str(&mut hasher, &self.role_bead);
        if let Some(ref agent_state) = self.agent_state {
            Self::hash_str(&mut hasher, &agent_state.to_string());
        } else {
            Self::hash_str(&mut hasher, "");
        }
        Self::hash_opt_str(&mut hasher, &self.role_type);
        Self::hash_opt_str(&mut hasher, &self.rig);

        // Molecule type
        if let Some(ref mol_type) = self.mol_type {
            Self::hash_str(&mut hasher, &mol_type.to_string());
        } else {
            Self::hash_str(&mut hasher, "");
        }

        // Event fields
        Self::hash_opt_str(&mut hasher, &self.event_kind);
        Self::hash_opt_str(&mut hasher, &self.actor);
        Self::hash_opt_str(&mut hasher, &self.target);
        Self::hash_opt_str(&mut hasher, &self.payload);

        format!("{:x}", hasher.finalize())
    }

    // Hash helper methods
    fn hash_str(hasher: &mut Sha256, s: &str) {
        hasher.update(s.as_bytes());
        hasher.update(&[0]);
    }

    fn hash_opt_str(hasher: &mut Sha256, opt: &Option<String>) {
        if let Some(s) = opt {
            hasher.update(s.as_bytes());
        }
        hasher.update(&[0]);
    }

    fn hash_int(hasher: &mut Sha256, n: i32) {
        hasher.update(format!("{}", n).as_bytes());
        hasher.update(&[0]);
    }

    fn hash_flag(hasher: &mut Sha256, b: bool, label: &str) {
        if b {
            hasher.update(label.as_bytes());
        }
        hasher.update(&[0]);
    }

    fn hash_entity_ref(hasher: &mut Sha256, e: &EntityRef) {
        Self::hash_opt_str(hasher, &e.name);
        Self::hash_opt_str(hasher, &e.platform);
        Self::hash_opt_str(hasher, &e.org);
        Self::hash_opt_str(hasher, &e.id);
    }

    /// IsTombstone returns true if the issue has been soft-deleted
    pub fn is_tombstone(&self) -> bool {
        matches!(self.status, Some(Status::Tombstone))
    }

    /// IsExpired returns true if the tombstone has exceeded its TTL.
    /// Non-tombstone issues always return false.
    pub fn is_expired(&self, ttl: Option<Duration>) -> bool {
        // Non-tombstones never expire
        if !self.is_tombstone() {
            return false;
        }

        // Tombstones without DeletedAt are not expired
        let deleted_at = match self.deleted_at {
            Some(dt) => dt,
            None => return false,
        };

        // Negative TTL means "immediately expired"
        let ttl = match ttl {
            Some(d) if d < Duration::zero() => return true,
            Some(d) => d,
            None => Self::DEFAULT_TOMBSTONE_TTL,
        };

        // Only add clock skew grace period for normal TTLs (> 1 hour)
        let effective_ttl = if ttl > Self::CLOCK_SKEW_GRACE {
            ttl + Self::CLOCK_SKEW_GRACE
        } else {
            ttl
        };

        // Check if the tombstone has exceeded its TTL
        let expiration_time = deleted_at + effective_ttl;
        Utc::now() > expiration_time
    }

    /// Validate checks if the issue has valid field values
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_with_custom(None, None)
    }

    /// ValidateWithCustom checks if the issue has valid field values,
    /// allowing custom statuses and types in addition to built-in ones.
    pub fn validate_with_custom(
        &self,
        custom_statuses: Option<&[String]>,
        custom_types: Option<&[String]>,
    ) -> Result<(), ValidationError> {
        if self.title.is_empty() {
            return Err(ValidationError::TitleRequired);
        }
        if self.title.len() > 500 {
            return Err(ValidationError::TitleTooLong(self.title.len()));
        }
        if !(0..=4).contains(&self.priority) {
            return Err(ValidationError::InvalidPriority(self.priority));
        }
        if let Some(ref status) = self.status {
            if !status.is_valid_with_custom(custom_statuses) {
                return Err(ValidationError::InvalidStatus(status.to_string()));
            }
        }
        if let Some(ref issue_type) = self.issue_type {
            if !issue_type.is_valid_with_custom(custom_types) {
                return Err(ValidationError::InvalidIssueType(issue_type.to_string()));
            }
        }
        if let Some(est) = self.estimated_minutes {
            if est < 0 {
                return Err(ValidationError::NegativeEstimate);
            }
        }
        // Enforce closed_at invariant
        if self.status == Some(Status::Closed) && self.closed_at.is_none() {
            return Err(ValidationError::ClosedWithoutTimestamp);
        }
        if self.status != Some(Status::Closed)
            && self.status != Some(Status::Tombstone)
            && self.closed_at.is_some()
        {
            return Err(ValidationError::NonClosedWithTimestamp);
        }
        // Enforce tombstone invariants
        if self.status == Some(Status::Tombstone) && self.deleted_at.is_none() {
            return Err(ValidationError::TombstoneWithoutDeletedAt);
        }
        if self.status != Some(Status::Tombstone) && self.deleted_at.is_some() {
            return Err(ValidationError::NonTombstoneWithDeletedAt);
        }
        // Validate agent state if set
        if let Some(ref agent_state) = self.agent_state {
            if !agent_state.is_valid() {
                return Err(ValidationError::InvalidAgentState(agent_state.to_string()));
            }
        }
        Ok(())
    }

    /// SetDefaults applies default values for fields omitted during JSONL import.
    pub fn set_defaults(&mut self) {
        if self.status.is_none() {
            self.status = Some(Status::Open);
        }
        if self.issue_type.is_none() {
            self.issue_type = Some(IssueType::Task);
        }
    }

    /// IsCompound returns true if this issue is a compound (bonded from multiple sources).
    pub fn is_compound(&self) -> bool {
        self.bonded_from
            .as_ref()
            .map(|b| !b.is_empty())
            .unwrap_or(false)
    }

    /// GetConstituents returns the BondRefs for this compound's constituent protos.
    pub fn get_constituents(&self) -> Option<&[BondRef]> {
        self.bonded_from.as_deref()
    }
}

/// Validation errors for Issue
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("title is required")]
    TitleRequired,
    #[error("title must be 500 characters or less (got {0})")]
    TitleTooLong(usize),
    #[error("priority must be between 0 and 4 (got {0})")]
    InvalidPriority(i32),
    #[error("invalid status: {0}")]
    InvalidStatus(String),
    #[error("invalid issue type: {0}")]
    InvalidIssueType(String),
    #[error("estimated_minutes cannot be negative")]
    NegativeEstimate,
    #[error("closed issues must have closed_at timestamp")]
    ClosedWithoutTimestamp,
    #[error("non-closed issues cannot have closed_at timestamp")]
    NonClosedWithTimestamp,
    #[error("tombstone issues must have deleted_at timestamp")]
    TombstoneWithoutDeletedAt,
    #[error("non-tombstone issues cannot have deleted_at timestamp")]
    NonTombstoneWithDeletedAt,
    #[error("invalid agent state: {0}")]
    InvalidAgentState(String),
}

/// Status represents the current state of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    InProgress,
    Blocked,
    Deferred,
    Closed,
    Tombstone,
    Pinned,
    Hooked,
}

impl Status {
    pub fn is_valid(&self) -> bool {
        true // All enum variants are valid
    }

    pub fn is_valid_with_custom(&self, custom_statuses: Option<&[String]>) -> bool {
        // Enum variants are always valid, custom status validation would be for string values
        let _ = custom_statuses;
        true
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Open => write!(f, "open"),
            Status::InProgress => write!(f, "in_progress"),
            Status::Blocked => write!(f, "blocked"),
            Status::Deferred => write!(f, "deferred"),
            Status::Closed => write!(f, "closed"),
            Status::Tombstone => write!(f, "tombstone"),
            Status::Pinned => write!(f, "pinned"),
            Status::Hooked => write!(f, "hooked"),
        }
    }
}

/// IssueType categorizes the kind of work
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueType {
    Bug,
    Feature,
    Task,
    Epic,
    Chore,
    Message,
    MergeRequest,
    Molecule,
    Gate,
    Event,
}

impl IssueType {
    pub fn is_valid(&self) -> bool {
        true // All enum variants are valid
    }

    pub fn is_valid_with_custom(&self, custom_types: Option<&[String]>) -> bool {
        let _ = custom_types;
        true
    }

    /// RequiredSections returns the recommended sections for this issue type.
    pub fn required_sections(&self) -> Vec<RequiredSection> {
        match self {
            IssueType::Bug => vec![
                RequiredSection {
                    heading: "## Steps to Reproduce".to_string(),
                    hint: "Describe how to reproduce the bug".to_string(),
                },
                RequiredSection {
                    heading: "## Acceptance Criteria".to_string(),
                    hint: "Define criteria to verify the fix".to_string(),
                },
            ],
            IssueType::Task | IssueType::Feature => vec![RequiredSection {
                heading: "## Acceptance Criteria".to_string(),
                hint: "Define criteria to verify completion".to_string(),
            }],
            IssueType::Epic => vec![RequiredSection {
                heading: "## Success Criteria".to_string(),
                hint: "Define high-level success criteria".to_string(),
            }],
            _ => vec![],
        }
    }
}

impl fmt::Display for IssueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IssueType::Bug => write!(f, "bug"),
            IssueType::Feature => write!(f, "feature"),
            IssueType::Task => write!(f, "task"),
            IssueType::Epic => write!(f, "epic"),
            IssueType::Chore => write!(f, "chore"),
            IssueType::Message => write!(f, "message"),
            IssueType::MergeRequest => write!(f, "merge-request"),
            IssueType::Molecule => write!(f, "molecule"),
            IssueType::Gate => write!(f, "gate"),
            IssueType::Event => write!(f, "event"),
        }
    }
}

/// RequiredSection describes a recommended section for an issue type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredSection {
    pub heading: String,
    pub hint: String,
}

/// AgentState represents the self-reported state of an agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Spawning,
    Running,
    Working,
    Stuck,
    Done,
    Stopped,
    Dead,
}

impl AgentState {
    pub fn is_valid(&self) -> bool {
        true // All enum variants are valid
    }
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentState::Idle => write!(f, "idle"),
            AgentState::Spawning => write!(f, "spawning"),
            AgentState::Running => write!(f, "running"),
            AgentState::Working => write!(f, "working"),
            AgentState::Stuck => write!(f, "stuck"),
            AgentState::Done => write!(f, "done"),
            AgentState::Stopped => write!(f, "stopped"),
            AgentState::Dead => write!(f, "dead"),
        }
    }
}

/// MolType categorizes the molecule type for swarm coordination
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MolType {
    Swarm,
    Patrol,
    Work,
}

impl MolType {
    pub fn is_valid(&self) -> bool {
        true
    }
}

impl fmt::Display for MolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MolType::Swarm => write!(f, "swarm"),
            MolType::Patrol => write!(f, "patrol"),
            MolType::Work => write!(f, "work"),
        }
    }
}

/// Dependency represents a relationship between issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub issue_id: String,
    pub depends_on_id: String,
    #[serde(rename = "type")]
    pub dep_type: DependencyType,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// DependencyType categorizes the relationship
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyType {
    // Workflow types (affect ready work calculation)
    Blocks,
    ParentChild,
    ConditionalBlocks,
    WaitsFor,

    // Association types
    Related,
    DiscoveredFrom,

    // Graph link types
    RepliesTo,
    RelatesTo,
    Duplicates,
    Supersedes,

    // Entity types (HOP foundation)
    AuthoredBy,
    AssignedTo,
    ApprovedBy,

    // Convoy tracking
    Tracks,

    // Reference types
    Until,
    CausedBy,
    Validates,
}

impl DependencyType {
    pub fn is_valid(&self) -> bool {
        true // All enum variants are valid
    }

    pub fn is_well_known(&self) -> bool {
        true // All enum variants are well-known
    }

    /// AffectsReadyWork returns true if this dependency type blocks work.
    pub fn affects_ready_work(&self) -> bool {
        matches!(
            self,
            DependencyType::Blocks
                | DependencyType::ParentChild
                | DependencyType::ConditionalBlocks
                | DependencyType::WaitsFor
        )
    }
}

impl fmt::Display for DependencyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DependencyType::Blocks => "blocks",
            DependencyType::ParentChild => "parent-child",
            DependencyType::ConditionalBlocks => "conditional-blocks",
            DependencyType::WaitsFor => "waits-for",
            DependencyType::Related => "related",
            DependencyType::DiscoveredFrom => "discovered-from",
            DependencyType::RepliesTo => "replies-to",
            DependencyType::RelatesTo => "relates-to",
            DependencyType::Duplicates => "duplicates",
            DependencyType::Supersedes => "supersedes",
            DependencyType::AuthoredBy => "authored-by",
            DependencyType::AssignedTo => "assigned-to",
            DependencyType::ApprovedBy => "approved-by",
            DependencyType::Tracks => "tracks",
            DependencyType::Until => "until",
            DependencyType::CausedBy => "caused-by",
            DependencyType::Validates => "validates",
        };
        write!(f, "{}", s)
    }
}

/// WaitsForMeta holds metadata for waits-for dependencies (fanout gates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitsForMeta {
    pub gate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawner_id: Option<String>,
}

pub const WAITS_FOR_ALL_CHILDREN: &str = "all-children";
pub const WAITS_FOR_ANY_CHILDREN: &str = "any-children";

/// FailureCloseKeywords are keywords that indicate an issue was closed due to failure.
pub const FAILURE_CLOSE_KEYWORDS: &[&str] = &[
    "failed",
    "rejected",
    "wontfix",
    "won't fix",
    "canceled",
    "cancelled",
    "abandoned",
    "blocked",
    "error",
    "timeout",
    "aborted",
];

/// IsFailureClose returns true if the close reason indicates the issue failed.
pub fn is_failure_close(close_reason: &str) -> bool {
    if close_reason.is_empty() {
        return false;
    }
    let lower = close_reason.to_lowercase();
    FAILURE_CLOSE_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(keyword))
}

/// Comment represents a comment on an issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub issue_id: String,
    pub author: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

impl fmt::Display for Comment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.created_at.format("%Y-%m-%d %H:%M:%S"),
            self.author,
            self.text
        )
    }
}

/// Event represents an audit trail entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub issue_id: String,
    pub event_type: EventType,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// EventType categorizes audit trail events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Created,
    Updated,
    StatusChanged,
    Commented,
    Closed,
    Reopened,
    DependencyAdded,
    DependencyRemoved,
    LabelAdded,
    LabelRemoved,
    Compacted,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventType::Created => write!(f, "created"),
            EventType::Updated => write!(f, "updated"),
            EventType::StatusChanged => write!(f, "status_changed"),
            EventType::Commented => write!(f, "commented"),
            EventType::Closed => write!(f, "closed"),
            EventType::Reopened => write!(f, "reopened"),
            EventType::DependencyAdded => write!(f, "dependency_added"),
            EventType::DependencyRemoved => write!(f, "dependency_removed"),
            EventType::LabelAdded => write!(f, "label_added"),
            EventType::LabelRemoved => write!(f, "label_removed"),
            EventType::Compacted => write!(f, "compacted"),
        }
    }
}

/// BondRef tracks compound molecule lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondRef {
    pub source_id: String,
    pub bond_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bond_point: Option<String>,
}

/// Bond type constants
pub const BOND_TYPE_SEQUENTIAL: &str = "sequential";
pub const BOND_TYPE_PARALLEL: &str = "parallel";
pub const BOND_TYPE_CONDITIONAL: &str = "conditional";
pub const BOND_TYPE_ROOT: &str = "root";

/// EntityRef is a structured reference to an entity (human, agent, or org).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl EntityRef {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.platform.is_none() && self.org.is_none() && self.id.is_none()
    }

    /// URI returns the entity as a HOP URI.
    /// Format: entity://hop/<platform>/<org>/<id>
    pub fn uri(&self) -> Option<String> {
        match (&self.platform, &self.org, &self.id) {
            (Some(platform), Some(org), Some(id)) => {
                Some(format!("entity://hop/{}/{}/{}", platform, org, id))
            }
            _ => None,
        }
    }

    /// Parse a HOP entity URI into an EntityRef
    pub fn parse_uri(uri: &str) -> Result<Self, String> {
        const PREFIX: &str = "entity://hop/";
        if !uri.starts_with(PREFIX) {
            return Err(format!("invalid entity URI: must start with \"{}\"", PREFIX));
        }

        let rest = &uri[PREFIX.len()..];
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() != 3 || parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
            return Err(format!(
                "invalid entity URI: expected entity://hop/<platform>/<org>/<id>, got \"{}\"",
                uri
            ));
        }

        Ok(EntityRef {
            name: None,
            platform: Some(parts[0].to_string()),
            org: Some(parts[1].to_string()),
            id: Some(parts[2].to_string()),
        })
    }
}

impl fmt::Display for EntityRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref name) = self.name {
            write!(f, "{}", name)
        } else if let Some(uri) = self.uri() {
            write!(f, "{}", uri)
        } else if let Some(ref id) = self.id {
            write!(f, "{}", id)
        } else {
            write!(f, "")
        }
    }
}

/// Validation records who validated/approved work completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validator: Option<EntityRef>,
    pub outcome: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

pub const VALIDATION_ACCEPTED: &str = "accepted";
pub const VALIDATION_REJECTED: &str = "rejected";
pub const VALIDATION_REVISION_REQUESTED: &str = "revision_requested";

impl Validation {
    pub fn is_valid_outcome(&self) -> bool {
        matches!(
            self.outcome.as_str(),
            VALIDATION_ACCEPTED | VALIDATION_REJECTED | VALIDATION_REVISION_REQUESTED
        )
    }
}

/// Statistics provides aggregate metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    pub total_issues: i32,
    pub open_issues: i32,
    pub in_progress_issues: i32,
    pub closed_issues: i32,
    pub blocked_issues: i32,
    pub deferred_issues: i32,
    pub ready_issues: i32,
    pub tombstone_issues: i32,
    pub pinned_issues: i32,
    pub epics_eligible_for_closure: i32,
    pub average_lead_time_hours: f64,
}

/// Label represents a tag on an issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub issue_id: String,
    pub label: String,
}

/// BlockedIssue extends Issue with blocking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedIssue {
    #[serde(flatten)]
    pub issue: Issue,
    pub blocked_by_count: i32,
    pub blocked_by: Vec<String>,
}

/// TreeNode represents a node in a dependency tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    #[serde(flatten)]
    pub issue: Issue,
    pub depth: i32,
    pub parent_id: String,
    pub truncated: bool,
}

/// MoleculeProgressStats provides efficient progress info for large molecules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoleculeProgressStats {
    pub molecule_id: String,
    pub molecule_title: String,
    pub total: i32,
    pub completed: i32,
    pub in_progress: i32,
    pub current_step_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_closed: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_closed: Option<DateTime<Utc>>,
}

/// DependencyCounts holds counts for dependencies and dependents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCounts {
    pub dependency_count: i32,
    pub dependent_count: i32,
}

/// IssueWithDependencyMetadata extends Issue with dependency relationship type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueWithDependencyMetadata {
    #[serde(flatten)]
    pub issue: Issue,
    pub dependency_type: DependencyType,
}

/// IssueWithCounts extends Issue with dependency relationship counts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueWithCounts {
    #[serde(flatten)]
    pub issue: Issue,
    pub dependency_count: i32,
    pub dependent_count: i32,
}

/// IssueDetails extends Issue with labels, dependencies, dependents, and comments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetails {
    #[serde(flatten)]
    pub issue: Issue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<IssueWithDependencyMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependents: Option<Vec<IssueWithDependencyMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<Comment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// EpicStatus represents an epic with its completion status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicStatus {
    pub epic: Issue,
    pub total_children: i32,
    pub closed_children: i32,
    pub eligible_for_close: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_validation() {
        let mut issue = Issue {
            id: "test-1".to_string(),
            content_hash: String::new(),
            title: "Test Issue".to_string(),
            description: None,
            design: None,
            acceptance_criteria: None,
            notes: None,
            status: Some(Status::Open),
            priority: 2,
            issue_type: Some(IssueType::Task),
            assignee: None,
            estimated_minutes: None,
            created_at: Utc::now(),
            created_by: None,
            updated_at: Utc::now(),
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            compaction_level: None,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: None,
            source_repo: String::new(),
            id_prefix: String::new(),
            labels: None,
            dependencies: None,
            comments: None,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            sender: None,
            ephemeral: None,
            pinned: None,
            is_template: None,
            bonded_from: None,
            creator: None,
            validations: None,
            await_type: None,
            await_id: None,
            timeout: None,
            waiters: None,
            holder: None,
            source_formula: None,
            source_location: None,
            hook_bead: None,
            role_bead: None,
            agent_state: None,
            last_activity: None,
            role_type: None,
            rig: None,
            mol_type: None,
            event_kind: None,
            actor: None,
            target: None,
            payload: None,
        };

        assert!(issue.validate().is_ok());

        // Test invalid priority
        issue.priority = 10;
        assert!(issue.validate().is_err());
        issue.priority = 2;

        // Test empty title
        issue.title = String::new();
        assert!(issue.validate().is_err());
    }

    #[test]
    fn test_content_hash() {
        let issue = Issue {
            id: "test-1".to_string(),
            content_hash: String::new(),
            title: "Test Issue".to_string(),
            description: Some("Description".to_string()),
            design: None,
            acceptance_criteria: None,
            notes: None,
            status: Some(Status::Open),
            priority: 2,
            issue_type: Some(IssueType::Task),
            assignee: None,
            estimated_minutes: None,
            created_at: Utc::now(),
            created_by: None,
            updated_at: Utc::now(),
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            compaction_level: None,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: None,
            source_repo: String::new(),
            id_prefix: String::new(),
            labels: None,
            dependencies: None,
            comments: None,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            sender: None,
            ephemeral: None,
            pinned: None,
            is_template: None,
            bonded_from: None,
            creator: None,
            validations: None,
            await_type: None,
            await_id: None,
            timeout: None,
            waiters: None,
            holder: None,
            source_formula: None,
            source_location: None,
            hook_bead: None,
            role_bead: None,
            agent_state: None,
            last_activity: None,
            role_type: None,
            rig: None,
            mol_type: None,
            event_kind: None,
            actor: None,
            target: None,
            payload: None,
        };

        let hash = issue.compute_content_hash();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 hex string is 64 chars
    }

    #[test]
    fn test_dependency_type_affects_ready_work() {
        assert!(DependencyType::Blocks.affects_ready_work());
        assert!(DependencyType::ParentChild.affects_ready_work());
        assert!(!DependencyType::Related.affects_ready_work());
        assert!(!DependencyType::RepliesTo.affects_ready_work());
    }

    #[test]
    fn test_entity_ref_uri() {
        let entity = EntityRef {
            name: Some("polecat/Nux".to_string()),
            platform: Some("gastown".to_string()),
            org: Some("steveyegge".to_string()),
            id: Some("polecat-nux".to_string()),
        };

        let uri = entity.uri().unwrap();
        assert_eq!(uri, "entity://hop/gastown/steveyegge/polecat-nux");

        let parsed = EntityRef::parse_uri(&uri).unwrap();
        assert_eq!(parsed.platform, Some("gastown".to_string()));
        assert_eq!(parsed.org, Some("steveyegge".to_string()));
        assert_eq!(parsed.id, Some("polecat-nux".to_string()));
    }

    #[test]
    fn test_is_failure_close() {
        assert!(is_failure_close("failed to complete"));
        assert!(is_failure_close("Rejected by reviewer"));
        assert!(is_failure_close("wontfix"));
        assert!(!is_failure_close("completed successfully"));
        assert!(!is_failure_close(""));
    }
}
