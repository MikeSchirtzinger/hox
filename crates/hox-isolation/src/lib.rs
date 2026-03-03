//! Isolation backend abstraction for hox-orchestrator.
//!
//! Provides the `IsolationBackend` trait and implementations for creating
//! isolated agent environments. Currently supports filesystem-based isolation
//! using JJ workspaces, with safety rule enforcement via `GuardedBackend`.

pub mod filesystem;
pub mod guarded;
pub mod safety;

use async_trait::async_trait;
use hox_core::Result;
use std::path::PathBuf;

pub use filesystem::FilesystemBackend;
pub use guarded::GuardedBackend;
pub use safety::{CompiledSafetyRules, SafetyRulesConfig};

/// An isolated environment for an agent.
#[derive(Debug, Clone)]
pub struct IsolatedEnv {
    pub agent_id: String,
    pub workspace_path: PathBuf,
}

/// Pluggable isolation backend trait.
#[async_trait]
pub trait IsolationBackend: Send + Sync {
    /// Create an isolated environment for an agent.
    async fn create(&self, agent_id: &str) -> Result<IsolatedEnv>;
    /// Destroy an agent's isolated environment.
    async fn destroy(&self, agent_id: &str) -> Result<()>;
    /// List all active isolated environments.
    async fn list(&self) -> Result<Vec<String>>;
}
