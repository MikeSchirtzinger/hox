//! Core orchestrator implementation

use hox_agent::LoopConfig;
use hox_core::{
    AgentId, ChangeId, HoxError, HoxMetadata, MessageType, OrchestratorId, Phase, Result, Task,
    TaskStatus,
};
use hox_jj::{JjCommand, JjExecutor, MetadataManager, OpLogEvent, OpLogWatcher, RevsetQueries};

use crate::loop_engine::LoopEngine;
use crate::workspace::WorkspaceManager as WM;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::communication::MessageRouter;
use crate::phases::{PhaseManager, PhaseStatus};
use crate::workspace::WorkspaceManager;

/// Configuration for an orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Orchestrator identifier
    pub id: OrchestratorId,
    /// Repository root path
    pub repo_root: PathBuf,
    /// Parent orchestrator (if any)
    pub parent: Option<OrchestratorId>,
    /// Maximum parallel agents
    pub max_agents: usize,
}

impl OrchestratorConfig {
    pub fn new(id: OrchestratorId, repo_root: impl Into<PathBuf>) -> Self {
        Self {
            id,
            repo_root: repo_root.into(),
            parent: None,
            max_agents: 4,
        }
    }

    pub fn with_parent(mut self, parent: OrchestratorId) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn with_max_agents(mut self, max: usize) -> Self {
        self.max_agents = max;
        self
    }
}

/// State of an orchestrator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorState {
    /// Initial state
    Initialized,
    /// Planning phases
    Planning,
    /// Executing phases
    Running,
    /// Waiting for agents
    Waiting,
    /// Integrating results
    Integrating,
    /// Validating results
    Validating,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed(String),
}

/// The main orchestrator struct
pub struct Orchestrator<E: JjExecutor> {
    config: OrchestratorConfig,
    state: OrchestratorState,
    executor: E,
    phases: PhaseManager,
    workspace_manager: WorkspaceManager<E>,
    message_router: MessageRouter,
    agents: HashMap<String, AgentId>,
    change_id: Option<ChangeId>,
}

impl Orchestrator<JjCommand> {
    /// Create a new orchestrator with auto-detected JJ repository
    pub async fn new(config: OrchestratorConfig) -> Result<Self> {
        let executor = JjCommand::new(&config.repo_root);
        Self::with_executor(config, executor).await
    }
}

impl<E: JjExecutor + Clone + 'static> Orchestrator<E> {
    /// Create a new orchestrator with a custom executor
    pub async fn with_executor(config: OrchestratorConfig, executor: E) -> Result<Self> {
        let workspace_manager = WorkspaceManager::new(executor.clone());

        Ok(Self {
            config,
            state: OrchestratorState::Initialized,
            executor,
            phases: PhaseManager::new(),
            workspace_manager,
            message_router: MessageRouter::new(),
            agents: HashMap::new(),
            change_id: None,
        })
    }

    /// Get the orchestrator ID
    pub fn id(&self) -> &OrchestratorId {
        &self.config.id
    }

    /// Get current state
    pub fn state(&self) -> &OrchestratorState {
        &self.state
    }

    /// Initialize the orchestrator's workspace and base change
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing orchestrator {}", self.config.id);

        // Create the orchestrator's base change
        let output = self
            .executor
            .exec(&["new", "-m", &format!("Orchestrator {} base", self.config.id)])
            .await?;

        if !output.success {
            return Err(HoxError::Orchestrator(format!(
                "Failed to create base change: {}",
                output.stderr
            )));
        }

        // Get the change ID
        let queries = RevsetQueries::new(self.executor.clone());
        self.change_id = queries.current().await?;

        // Set orchestrator metadata
        if let Some(change_id) = &self.change_id {
            let metadata = HoxMetadata::new()
                .with_status(TaskStatus::Open)
                .with_orchestrator(self.config.id.to_string());

            let manager = MetadataManager::new(self.executor.clone());
            manager.set(change_id, &metadata).await?;
        }

        self.state = OrchestratorState::Initialized;
        Ok(())
    }

    /// Add a phase to the orchestrator
    pub fn add_phase(&mut self, phase: Phase) {
        self.phases.add_phase(phase);
    }

    /// Spawn an agent for a task
    pub async fn spawn_agent(&mut self, task_description: &str) -> Result<AgentId> {
        if self.agents.len() >= self.config.max_agents {
            return Err(HoxError::Orchestrator(format!(
                "Maximum agents ({}) reached",
                self.config.max_agents
            )));
        }

        let agent_id = AgentId::new(self.config.id.clone());
        let agent_name = format!("agent-{}", &agent_id.id.to_string()[..8]);

        info!("Spawning agent {} for: {}", agent_name, task_description);

        // Create workspace for the agent
        self.workspace_manager
            .create_workspace(&agent_name)
            .await?;

        // Create a new change for the agent's work
        let output = self
            .executor
            .exec(&["new", "-m", task_description])
            .await?;

        if !output.success {
            return Err(HoxError::Agent(format!(
                "Failed to create agent change: {}",
                output.stderr
            )));
        }

        // Set agent metadata
        let queries = RevsetQueries::new(self.executor.clone());
        if let Some(change_id) = queries.current().await? {
            let metadata = HoxMetadata::new()
                .with_status(TaskStatus::InProgress)
                .with_agent(&agent_name)
                .with_orchestrator(self.config.id.to_string());

            let manager = MetadataManager::new(self.executor.clone());
            manager.set(&change_id, &metadata).await?;
        }

        self.agents.insert(agent_name.clone(), agent_id.clone());
        Ok(agent_id)
    }

    /// Send a mutation message to agents
    pub async fn send_mutation(&self, content: &str, targets: &str) -> Result<()> {
        info!("Sending mutation to {}: {}", targets, content);

        let metadata = HoxMetadata::new()
            .with_orchestrator(self.config.id.to_string())
            .with_message(targets, MessageType::Mutation);

        if let Some(change_id) = &self.change_id {
            // Update description with mutation content
            let output = self
                .executor
                .exec(&[
                    "describe",
                    "-r",
                    change_id,
                    "-m",
                    &format!("MUTATION: {}\n\n{}", content, MetadataManager::<E>::format_metadata(&metadata)),
                ])
                .await?;

            if !output.success {
                return Err(HoxError::MessageRouting(output.stderr));
            }
        }

        Ok(())
    }

    /// Check for alignment requests from agents
    pub async fn check_align_requests(&self) -> Result<Vec<(ChangeId, String)>> {
        let queries = RevsetQueries::new(self.executor.clone());
        let changes = queries.align_requests().await?;

        let mut requests = Vec::new();
        let manager = MetadataManager::new(self.executor.clone());

        for change_id in changes {
            let metadata = manager.read(&change_id).await?;
            if metadata.orchestrator.as_ref() == Some(&self.config.id.to_string()) {
                // Get the description for the request content
                let output = self
                    .executor
                    .exec(&["log", "-r", &change_id, "-T", "description", "--no-graph"])
                    .await?;
                requests.push((change_id, output.stdout));
            }
        }

        Ok(requests)
    }

    /// Start the orchestration loop
    pub async fn run(&mut self) -> Result<()> {
        self.state = OrchestratorState::Running;
        info!("Orchestrator {} starting run", self.config.id);

        // Start oplog watcher
        let watcher = OpLogWatcher::new(self.executor.clone());
        let mut events = watcher.watch().await?;

        // Main orchestration loop
        while self.state == OrchestratorState::Running
            || self.state == OrchestratorState::Waiting
        {
            // Check for oplog events
            if let Ok(Some(event)) = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                events.recv(),
            )
            .await
            {
                self.handle_oplog_event(event).await?;
            }

            // Check phase status
            if let Some(current_phase) = self.phases.current_phase() {
                match self.phases.phase_status(current_phase.number) {
                    Some(PhaseStatus::Completed) => {
                        info!("Phase {} completed, advancing", current_phase.number);
                        self.phases.advance()?;
                    }
                    Some(PhaseStatus::Failed(reason)) => {
                        self.state = OrchestratorState::Failed(reason.clone());
                        break;
                    }
                    _ => {}
                }
            } else {
                // No more phases
                self.state = OrchestratorState::Integrating;
                break;
            }

            // Check for alignment requests
            let requests = self.check_align_requests().await?;
            for (change_id, content) in requests {
                debug!("Processing align request from {}: {}", change_id, content);
                // TODO: Handle alignment request
            }
        }

        if self.state == OrchestratorState::Integrating {
            self.integrate().await?;
        }

        Ok(())
    }

    /// Handle an oplog event
    async fn handle_oplog_event(&mut self, event: OpLogEvent) -> Result<()> {

        match event {
            OpLogEvent::NewOperation {
                operation_id,
                description,
            } => {
                debug!("New operation: {} - {}", operation_id, description);
                // Check if this affects our agents
                // TODO: Parse operation and update state accordingly
            }
            OpLogEvent::Error(e) => {
                warn!("OpLog error: {}", e);
            }
            _ => {}
        }

        Ok(())
    }

    /// Integrate completed agent work
    async fn integrate(&mut self) -> Result<()> {
        info!("Integrating agent work");
        self.state = OrchestratorState::Integrating;

        // Get all agent changes
        let queries = RevsetQueries::new(self.executor.clone());
        let agent_changes = queries.by_orchestrator(&self.config.id.to_string()).await?;

        if agent_changes.len() > 1 {
            // Merge all agent changes
            let merge_args: Vec<&str> = std::iter::once("new")
                .chain(agent_changes.iter().map(|s| s.as_str()))
                .chain(std::iter::once("-m"))
                .chain(std::iter::once("Integration merge"))
                .collect();

            let output = self.executor.exec(&merge_args).await?;

            if !output.success {
                return Err(HoxError::MergeConflict(output.stderr));
            }

            // Check for conflicts
            let conflicts = queries.conflicts().await?;
            if !conflicts.is_empty() {
                warn!("Merge produced {} conflicts", conflicts.len());
                // TODO: Handle conflicts - spawn integration agent
            }
        }

        self.state = OrchestratorState::Validating;
        Ok(())
    }

    /// Get the orchestrator's change ID
    pub fn change_id(&self) -> Option<&ChangeId> {
        self.change_id.as_ref()
    }

    /// Get active agents
    pub fn agents(&self) -> &HashMap<String, AgentId> {
        &self.agents
    }

    /// Run a Ralph-style loop on a task
    ///
    /// This method spawns fresh agents in a loop until all backpressure checks pass
    /// or max iterations is reached. Each iteration is completely stateless - context
    /// comes from JJ metadata and backpressure signals.
    pub async fn run_loop(
        &mut self,
        task: Task,
        loop_config: Option<LoopConfig>,
    ) -> Result<hox_agent::LoopResult> {
        let config = loop_config.unwrap_or_default();
        info!(
            "Starting Ralph-style loop for task {} with model {:?}, max {} iterations",
            task.change_id, config.model, config.max_iterations
        );

        // Create workspace manager clone for the loop engine
        let workspace_manager = WM::new(self.executor.clone());

        // Create .hox directory if it doesn't exist
        let hox_dir = self.config.repo_root.join(".hox");
        tokio::fs::create_dir_all(&hox_dir).await.map_err(|e| {
            HoxError::Io(format!("Failed to create .hox directory: {}", e))
        })?;

        let mut loop_engine = LoopEngine::new(
            self.executor.clone(),
            workspace_manager,
            config,
            self.config.repo_root.clone(),
        )
        .with_activity_logging(hox_dir);

        loop_engine.run(&task).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_config() {
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo")
            .with_max_agents(8);

        assert_eq!(config.id.to_string(), "O-A-1");
        assert_eq!(config.max_agents, 8);
    }
}
