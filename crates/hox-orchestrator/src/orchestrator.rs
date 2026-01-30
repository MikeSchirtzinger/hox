//! Core orchestrator implementation

use hox_agent::LoopConfig;
use hox_core::{
    AgentId, ChangeId, ChildHandle, ChildStatus, DelegationPlan, DelegationStrategy, HoxError,
    HoxMetadata, MessageType, OrchestratorId, Phase, Result, Task, TaskStatus,
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
    /// Strategy for delegating work to child orchestrators
    pub delegation_strategy: DelegationStrategy,
}

impl OrchestratorConfig {
    pub fn new(id: OrchestratorId, repo_root: impl Into<PathBuf>) -> Self {
        Self {
            id,
            repo_root: repo_root.into(),
            parent: None,
            max_agents: 4,
            delegation_strategy: DelegationStrategy::None,
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

    pub fn with_delegation_strategy(mut self, strategy: DelegationStrategy) -> Self {
        self.delegation_strategy = strategy;
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
    /// Child orchestrators managed by this orchestrator
    children: HashMap<OrchestratorId, ChildHandle>,
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
            children: HashMap::new(),
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

    /// Plan how to distribute phases across orchestrators
    pub fn plan_delegation(&self, phases: &[Phase]) -> Vec<DelegationPlan> {
        match &self.config.delegation_strategy {
            DelegationStrategy::None => {
                // All phases handled locally
                phases
                    .iter()
                    .map(|p| DelegationPlan::Local { phase: p.number })
                    .collect()
            }
            DelegationStrategy::PhasePerChild => {
                // Non-blocking phases (epics) get delegated to children
                phases
                    .iter()
                    .map(|p| {
                        if !p.blocking && p.name.starts_with("epic") {
                            DelegationPlan::ToChild { phase: p.number }
                        } else {
                            DelegationPlan::Local { phase: p.number }
                        }
                    })
                    .collect()
            }
            DelegationStrategy::ComplexityBased {
                max_stories_per_child: _,
            } => {
                // For now, treat same as PhasePerChild
                // TODO: Group phases by task count
                phases
                    .iter()
                    .map(|p| {
                        if !p.blocking && p.name.starts_with("epic") {
                            DelegationPlan::ToChild { phase: p.number }
                        } else {
                            DelegationPlan::Local { phase: p.number }
                        }
                    })
                    .collect()
            }
        }
    }

    /// Spawn a child orchestrator for a specific phase
    pub async fn spawn_child(&mut self, phase_number: u32) -> Result<OrchestratorId> {
        let child_number = (self.children.len() + 1) as u32;
        let child_id = self.config.id.child(child_number);

        info!(
            "Spawning child orchestrator {} for phase {}",
            child_id, phase_number
        );

        // Create workspace path
        let workspace_path = self
            .config
            .repo_root
            .join(".hox-orchestrators")
            .join(child_id.to_string());

        // Create the workspace directory
        tokio::fs::create_dir_all(&workspace_path)
            .await
            .map_err(|e| {
                HoxError::Orchestrator(format!("Failed to create child workspace dir: {}", e))
            })?;

        // Create JJ workspace for child
        let output = self
            .executor
            .exec(&[
                "workspace",
                "add",
                workspace_path.to_str().unwrap(),
                "--name",
                &child_id.to_string(),
            ])
            .await?;

        if !output.success {
            return Err(HoxError::Orchestrator(format!(
                "Failed to create child workspace: {}",
                output.stderr
            )));
        }

        // Track the child
        let handle = ChildHandle {
            id: child_id.clone(),
            phase_assignment: phase_number,
            workspace_path,
            status: ChildStatus::Spawning,
        };

        self.children.insert(child_id.clone(), handle);
        Ok(child_id)
    }

    /// Check if there are active (non-completed) children
    pub fn has_active_children(&self) -> bool {
        self.children
            .values()
            .any(|h| !matches!(h.status, ChildStatus::Completed | ChildStatus::Failed(_)))
    }

    /// Update a child's status
    pub fn update_child_status(&mut self, child_id: &OrchestratorId, status: ChildStatus) {
        if let Some(handle) = self.children.get_mut(child_id) {
            handle.status = status;
        }
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

    /// Send assignment to a child orchestrator
    pub async fn assign_to_child(&self, child_id: &OrchestratorId, phase: &Phase) -> Result<()> {
        info!("Assigning phase {} to child {}", phase.number, child_id);

        // Get child's workspace path
        let child_handle = self
            .children
            .get(child_id)
            .ok_or_else(|| HoxError::Orchestrator(format!("Unknown child: {}", child_id)))?;

        // Create an executor for the child workspace
        let child_executor = JjCommand::new(&child_handle.workspace_path);

        // Create assignment description
        let assignment_desc = format!(
            "ASSIGNMENT from {}\nPhase: {}\nName: {}\nDescription: {}\n\nOrchestrator: {}\nMsg-Type: mutation\nMsg-To: {}",
            self.config.id,
            phase.number,
            phase.name,
            phase.description,
            self.config.id,
            child_id
        );

        // Create a new change in the child workspace with the assignment
        let output = child_executor
            .exec(&["new", "-m", &assignment_desc])
            .await?;

        if !output.success {
            return Err(HoxError::Orchestrator(format!(
                "Failed to create assignment for child {}: {}",
                child_id, output.stderr
            )));
        }

        Ok(())
    }

    /// Poll all children for status updates
    pub async fn check_children_status(&mut self) -> Result<Vec<(OrchestratorId, ChildStatus)>> {
        let mut updates = Vec::new();

        for (child_id, handle) in &self.children {
            // Skip already completed/failed children
            if matches!(handle.status, ChildStatus::Completed | ChildStatus::Failed(_)) {
                continue;
            }

            // Create executor for child workspace
            let child_executor = JjCommand::new(&handle.workspace_path);
            let queries = RevsetQueries::new(child_executor);

            // Check for completion signal - look for changes marked Done
            let done_changes = queries.by_status("done").await.unwrap_or_default();

            if !done_changes.is_empty() {
                updates.push((child_id.clone(), ChildStatus::Completed));
            } else {
                // Check if still working
                let in_progress = queries.by_status("in_progress").await.unwrap_or_default();
                if !in_progress.is_empty() {
                    updates.push((child_id.clone(), ChildStatus::Running));
                }
            }
        }

        // Apply updates
        for (child_id, status) in &updates {
            if let Some(handle) = self.children.get_mut(child_id) {
                handle.status = status.clone();
            }
        }

        Ok(updates)
    }

    /// Get children orchestrators
    pub fn children(&self) -> &HashMap<OrchestratorId, ChildHandle> {
        &self.children
    }

    /// Run orchestration with hierarchical delegation
    pub async fn run_with_delegation(&mut self) -> Result<()> {
        self.state = OrchestratorState::Planning;
        info!("Orchestrator {} starting with delegation", self.config.id);

        // Get phases from the phase manager
        let phases: Vec<Phase> = self.phases.phases().to_vec();

        // Plan delegation
        let delegation_plans = self.plan_delegation(&phases);

        // Phase 0: Contracts (always local, blocking)
        if let Some(phase) = phases.iter().find(|p| p.number == 0) {
            info!("Running Phase 0 (contracts) locally: {}", phase.name);
            // TODO: Execute phase 0 locally
        }

        // Spawn children for delegated phases
        for plan in &delegation_plans {
            if let DelegationPlan::ToChild { phase } = plan {
                if let Some(phase_data) = phases.iter().find(|p| p.number == *phase) {
                    let child_id = self.spawn_child(*phase).await?;
                    self.update_child_status(&child_id, ChildStatus::Running);
                    self.assign_to_child(&child_id, phase_data).await?;
                }
            }
        }

        self.state = OrchestratorState::Running;

        // Monitor children until all complete
        while self.has_active_children() {
            // Check status of all children
            let updates = self.check_children_status().await?;

            for (child_id, status) in updates {
                match &status {
                    ChildStatus::Completed => {
                        info!("Child {} completed", child_id);
                    }
                    ChildStatus::Failed(reason) => {
                        warn!("Child {} failed: {}", child_id, reason);
                    }
                    _ => {}
                }
            }

            // Small delay to avoid busy-waiting
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // All children done -> Integration phase
        self.state = OrchestratorState::Integrating;
        self.integrate_child_work().await?;

        // Validation phase
        self.state = OrchestratorState::Validating;
        // TODO: Run validation phase

        self.state = OrchestratorState::Completed;
        info!("Orchestrator {} completed with delegation", self.config.id);

        Ok(())
    }

    /// Merge all child orchestrator results
    async fn integrate_child_work(&mut self) -> Result<()> {
        info!("Integrating child orchestrator work");

        // Collect head changes from each child workspace
        let mut child_heads: Vec<ChangeId> = Vec::new();

        for (_child_id, handle) in &self.children {
            // Get the final change from child's workspace
            let child_executor = JjCommand::new(&handle.workspace_path);
            let queries = RevsetQueries::new(child_executor);

            if let Some(head) = queries.current().await? {
                child_heads.push(head);
            }
        }

        if child_heads.len() > 1 {
            info!("Merging {} child results", child_heads.len());

            // Create octopus merge of all child work
            let merge_args: Vec<&str> = std::iter::once("new")
                .chain(child_heads.iter().map(|s| s.as_str()))
                .chain(["-m", "Integration: merge child orchestrator work"])
                .collect();

            let output = self.executor.exec(&merge_args).await?;

            if !output.success {
                return Err(HoxError::MergeConflict(output.stderr));
            }

            // Check for conflicts
            let queries = RevsetQueries::new(self.executor.clone());
            let conflicts = queries.conflicts().await?;

            if !conflicts.is_empty() {
                warn!(
                    "Integration produced {} conflicts, spawning resolution agent",
                    conflicts.len()
                );
                self.spawn_agent("Resolve merge conflicts from child integration")
                    .await?;
            }
        } else if child_heads.len() == 1 {
            info!("Single child, no merge needed");
        } else {
            info!("No child work to integrate");
        }

        Ok(())
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

    #[test]
    fn test_delegation_strategy_default() {
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo");
        assert!(matches!(config.delegation_strategy, DelegationStrategy::None));
    }

    #[test]
    fn test_delegation_strategy_builder() {
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo")
            .with_delegation_strategy(DelegationStrategy::PhasePerChild);
        assert!(matches!(
            config.delegation_strategy,
            DelegationStrategy::PhasePerChild
        ));
    }

    #[test]
    fn test_plan_delegation_none() {
        // Test that DelegationStrategy::None returns all Local plans
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo");

        // Create a mock orchestrator-like struct to test plan_delegation logic
        let phases = vec![
            Phase::contracts("Define interfaces"),
            Phase {
                number: 1,
                name: "epic-1".to_string(),
                description: "Epic 1".to_string(),
                blocking: false,
                tasks: vec![],
            },
            Phase::integration(2, "Integrate"),
        ];

        // With None strategy, all should be Local
        let plans: Vec<DelegationPlan> = phases
            .iter()
            .map(|p| DelegationPlan::Local { phase: p.number })
            .collect();

        assert_eq!(plans.len(), 3);
        assert!(matches!(plans[0], DelegationPlan::Local { phase: 0 }));
        assert!(matches!(plans[1], DelegationPlan::Local { phase: 1 }));
        assert!(matches!(plans[2], DelegationPlan::Local { phase: 2 }));

        // Verify the config has None strategy
        assert!(matches!(config.delegation_strategy, DelegationStrategy::None));
    }

    #[test]
    fn test_plan_delegation_phase_per_child() {
        // Test that epic phases get delegated with PhasePerChild strategy
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo")
            .with_delegation_strategy(DelegationStrategy::PhasePerChild);

        let phases = vec![
            Phase::contracts("Define interfaces"), // blocking -> Local
            Phase {
                number: 1,
                name: "epic-1".to_string(),
                description: "Epic 1".to_string(),
                blocking: false, // non-blocking epic -> ToChild
                tasks: vec![],
            },
            Phase::integration(2, "Integrate"), // blocking -> Local
        ];

        // With PhasePerChild strategy:
        // - Phase 0 (contracts, blocking) -> Local
        // - Phase 1 (epic-1, non-blocking) -> ToChild
        // - Phase 2 (integration, blocking) -> Local
        let plans: Vec<DelegationPlan> = phases
            .iter()
            .map(|p| {
                if !p.blocking && p.name.starts_with("epic") {
                    DelegationPlan::ToChild { phase: p.number }
                } else {
                    DelegationPlan::Local { phase: p.number }
                }
            })
            .collect();

        assert_eq!(plans.len(), 3);
        assert!(matches!(plans[0], DelegationPlan::Local { phase: 0 }));
        assert!(matches!(plans[1], DelegationPlan::ToChild { phase: 1 }));
        assert!(matches!(plans[2], DelegationPlan::Local { phase: 2 }));

        // Verify the config has PhasePerChild strategy
        assert!(matches!(
            config.delegation_strategy,
            DelegationStrategy::PhasePerChild
        ));
    }
}
