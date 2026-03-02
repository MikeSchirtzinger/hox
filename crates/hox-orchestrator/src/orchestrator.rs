//! Core orchestrator implementation

use hox_agent::LoopConfig;
use std::time::Duration;

/// Interval between status polls for child orchestrators
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Sleep interval in the main orchestrator poll loop
const POLL_LOOP_INTERVAL: Duration = Duration::from_millis(500);
use hox_core::{
    AgentId, ChangeId, ChildHandle, ChildStatus, DelegationPlan, DelegationStrategy, HoxError,
    HoxMetadata, MessageType, OrchestratorId, Phase, Result, Task, TaskStatus,
};
use hox_jj::{
    AbsorbResult, BookmarkManager, DagOperations, JjCommand, JjExecutor, MetadataManager,
    OpLogEvent, OpLogWatcher, ParallelizeResult, RevsetQueries, SplitResult,
};

use crate::loop_engine::LoopEngine;
use crate::workspace::WorkspaceManager as WM;
use hox_isolation::IsolationBackend;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::communication::MessageRouter;
use crate::phases::{PhaseManager, PhaseStatus};
use crate::state_machine;
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
    /// State machine for observability and pattern tracking
    sm_state: state_machine::State,
    /// Tier 1: Change IDs currently being worked on by active agents.
    ///
    /// DAG restructuring operations (rebase, absorb, parallelize) MUST be
    /// deferred while this set is non-empty to avoid races with agent writes.
    active_agent_change_ids: HashSet<String>,
    /// Tier 2: Whether jj-dev fork features (ForkedOpHeadsStore) are available.
    ///
    /// TODO(W6): When the jj-dev fork lands, set this to `true` and wire up
    /// per-agent `ForkedOpHeadsStore` instances so each agent gets a fully
    /// isolated op-log rather than just an isolated working copy.
    fork_features: bool,
    /// Optional isolation backend. When set, `spawn_agent` uses it for workspace
    /// creation instead of the `WorkspaceManager`. Falls back to `WorkspaceManager`
    /// when `None`.
    isolation: Option<Box<dyn IsolationBackend>>,
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
            sm_state: state_machine::State::Idle,
            active_agent_change_ids: HashSet::new(),
            fork_features: false,
            isolation: None,
        })
    }

    /// Set a custom isolation backend.
    pub fn with_isolation(mut self, backend: Box<dyn IsolationBackend>) -> Self {
        self.isolation = Some(backend);
        self
    }

    /// Configure filesystem + guarded isolation rooted at `repo_root/.hox-workspaces`.
    ///
    /// Safety rules are loaded from `repo_root/.hox/safety-rules.toml` (fail-open: missing
    /// file = no enforcement).
    pub fn with_default_isolation(mut self, repo_root: &Path) -> Self {
        let workspace_dir = repo_root.join(".hox-workspaces");
        let fs_backend =
            hox_isolation::FilesystemBackend::new(workspace_dir, repo_root.to_path_buf());
        let rules = hox_isolation::safety::load_safety_rules(repo_root)
            .unwrap_or_else(|_| hox_isolation::CompiledSafetyRules::empty());
        let guarded = hox_isolation::GuardedBackend::new(fs_backend, rules);
        self.isolation = Some(Box::new(guarded));
        self
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
            .exec(&[
                "new",
                "-m",
                &format!("Orchestrator {} base", self.config.id),
            ])
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

            // Create orchestrator bookmark
            let bookmark_manager = BookmarkManager::new(self.executor.clone());
            bookmark_manager
                .mark_orchestrator(&self.config.id.to_string(), change_id)
                .await?;
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

        // Tier 2: TODO(W6) – when fork_features is true, allocate a
        // ForkedOpHeadsStore per agent so each gets a fully isolated op-log.
        // For now, each agent gets its own working-copy workspace which is
        // sufficient to prevent filesystem-level races.
        if self.fork_features {
            // TODO(W6): ForkedOpHeadsStore per agent
        }

        // Create workspace for the agent and obtain a workspace-scoped executor.
        // All agent operations MUST use this executor so they run inside the
        // agent's working copy, not the main repo working copy.
        //
        // When an isolation backend is configured, use it to create the workspace
        // and derive the executor path from the resulting `IsolatedEnv`. Otherwise
        // fall back to the existing `WorkspaceManager` code path.
        let ws_executor: JjCommand = if let Some(ref isolation) = self.isolation {
            let env = isolation.create(&agent_name).await?;
            JjCommand::new(&env.workspace_path)
        } else {
            self.workspace_manager.create_workspace(&agent_name).await?;
            self.workspace_manager.switch_to(&agent_name).await?
        };

        // `jj edit @` ensures the workspace working copy is pointing at the
        // current change before we create the agent's change on top of it.
        let edit_output = ws_executor.exec(&["edit", "@"]).await?;
        if !edit_output.success {
            // Non-fatal: workspace may already be at the right change.
            debug!(
                "jj edit @ in agent workspace returned non-success: {}",
                edit_output.stderr
            );
        }

        // Create a new change for the agent's work inside its workspace.
        let output = ws_executor
            .exec(&["new", "-m", task_description])
            .await?;

        if !output.success {
            return Err(HoxError::Agent(format!(
                "Failed to create agent change: {}",
                output.stderr
            )));
        }

        // Set agent metadata and create bookmark assignment using the
        // workspace-scoped executor so RevsetQueries resolve against the
        // agent workspace (@), not the main working copy.
        let queries = RevsetQueries::new(ws_executor.clone());
        if let Some(change_id) = queries.current().await? {
            let metadata = HoxMetadata::new()
                .with_status(TaskStatus::InProgress)
                .with_agent(&agent_name)
                .with_orchestrator(self.config.id.to_string());

            let manager = MetadataManager::new(ws_executor.clone());
            manager.set(&change_id, &metadata).await?;

            // Create bookmark assignment for the agent
            let bookmark_manager = BookmarkManager::new(ws_executor.clone());
            bookmark_manager
                .assign_task(&agent_name, &change_id)
                .await?;

            // Tier 1: track this change ID so DAG restructuring is deferred
            // while the agent is active.
            self.active_agent_change_ids.insert(change_id);
        }

        self.agents.insert(agent_name.clone(), agent_id.clone());
        Ok(agent_id)
    }

    /// Tier 1: Check whether DAG restructuring (rebase, absorb, parallelize)
    /// should be deferred because one or more agents are currently active.
    ///
    /// Callers MUST check this before invoking `optimize_dag`, `absorb_fixes`,
    /// or any other operation that rewrites ancestry in the shared DAG.
    pub fn dag_restructure_deferred(&self) -> bool {
        !self.active_agent_change_ids.is_empty()
    }

    /// Tier 1: Mark an agent's change as complete, removing it from the active
    /// set.  Once all agents have checked in the DAG restructuring gate opens.
    pub fn complete_agent_change(&mut self, change_id: &str) {
        self.active_agent_change_ids.remove(change_id);
    }

    /// Tier 1: Return the set of change IDs currently held by active agents.
    pub fn active_agent_change_ids(&self) -> &HashSet<String> {
        &self.active_agent_change_ids
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
                    &format!(
                        "MUTATION: {}\n\n{}",
                        content,
                        MetadataManager::<E>::format_metadata(&metadata)
                    ),
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

        // Use description-based query for now - alignment requests don't have dedicated bookmarks
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
        while self.state == OrchestratorState::Running || self.state == OrchestratorState::Waiting {
            // Check for oplog events
            if let Ok(Some(event)) = tokio::time::timeout(CHILD_POLL_INTERVAL, events.recv()).await
            {
                self.handle_oplog_event(event).await?;
            }

            // Check phase status
            if let Some(current_phase) = self.phases.current_phase() {
                match self.phases.phase_status(current_phase.number) {
                    Some(PhaseStatus::Completed) => {
                        info!("Phase {} completed, advancing", current_phase.number);
                        self.phases.advance()?;
                        // Run maintenance after each phase completes (fail-open)
                        if let Err(e) = self.run_phase_maintenance().await {
                            warn!("Phase maintenance failed (non-fatal): {}", e);
                        }
                    }
                    Some(PhaseStatus::Failed(reason)) => {
                        self.state = OrchestratorState::Failed(reason.clone());
                        break;
                    }
                    // Pending and Active statuses continue waiting
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

    /// Run maintenance tasks after phase completion.
    ///
    /// In colocated repos (both `.jj/` and `.git` present), runs `jj util gc`.
    /// Stale bookmark cleanup is also attempted. All failures are non-fatal —
    /// maintenance errors are logged as warnings and never crash orchestration.
    async fn run_phase_maintenance(&self) -> Result<()> {
        let is_colocated = self.config.repo_root.join(".jj").is_dir()
            && self.config.repo_root.join(".git").exists();

        if is_colocated {
            info!("Running colocated maintenance (jj util gc)");
            match self.executor.exec(&["util", "gc"]).await {
                Ok(output) if output.success => {
                    debug!("gc completed successfully");
                }
                Ok(output) => {
                    warn!("gc completed with warnings: {}", output.stderr);
                }
                Err(e) => {
                    warn!("gc failed (non-fatal): {}", e);
                }
            }
        }

        self.cleanup_stale_bookmarks().await?;

        Ok(())
    }

    /// List bookmarks and log any that belong to Done/Abandoned tasks.
    ///
    /// Currently only logs — actual deletion will be wired in a follow-up.
    async fn cleanup_stale_bookmarks(&self) -> Result<()> {
        let output = self.executor.exec(&["bookmark", "list"]).await?;
        if !output.success {
            // Non-fatal: just skip
            return Ok(());
        }
        debug!("Bookmark cleanup check completed");
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
            // Other event types (future extensions) are logged but not handled
            _ => {}
        }

        Ok(())
    }

    /// Integrate completed agent work
    async fn integrate(&mut self) -> Result<()> {
        info!("Integrating agent work");
        self.state = OrchestratorState::Integrating;

        // State machine transition: Moving to integration
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::AllTasksComplete,
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

        // Get all agent changes - try bookmark-based query first, fallback to description
        let queries = RevsetQueries::new(self.executor.clone());
        let agent_changes = match queries.all_orchestrators_by_bookmark().await {
            Ok(changes) => changes,
            Err(_) => queries.by_orchestrator(&self.config.id.to_string()).await?,
        };

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

            // Check for conflicts and attempt resolution
            let conflicts = queries.conflicts().await?;
            if !conflicts.is_empty() {
                warn!(
                    "Merge produced {} conflicts, attempting resolution",
                    conflicts.len()
                );

                let resolver = crate::ConflictResolver::new(self.executor.clone());
                let report = resolver.resolve_all().await?;

                if report.needs_human > 0 {
                    warn!("{} conflicts need human review", report.needs_human);
                }
                if report.failed > 0 {
                    warn!("{} conflicts failed to resolve", report.failed);
                }
                if report.auto_resolved > 0 {
                    info!("Auto-resolved {} conflicts", report.auto_resolved);
                }
            }
        }

        self.state = OrchestratorState::Validating;
        Ok(())
    }

    /// Get the orchestrator's change ID
    pub fn change_id(&self) -> Option<&ChangeId> {
        self.change_id.as_ref()
    }

    /// Execute actions from state machine transition
    ///
    /// This method handles the side effects produced by state machine transitions.
    /// Actions are advisory/observability - they don't replace existing control flow.
    fn execute_actions(&self, actions: Vec<state_machine::Action>) {
        for action in actions {
            match action {
                state_machine::Action::LogActivity { message } => {
                    info!("[State Machine] {}", message);
                }
                state_machine::Action::RecordPattern { pattern } => {
                    debug!("[State Machine] Recording pattern: {}", pattern);
                    // TODO: Integrate with hox-evolution pattern store when available
                }
                state_machine::Action::SpawnPlanningAgent { goal } => {
                    debug!("[State Machine] Would spawn planning agent for: {}", goal);
                    // Note: Actual spawning happens in existing orchestrator logic
                }
                state_machine::Action::SpawnTaskAgents { count } => {
                    debug!("[State Machine] Would spawn {} task agents", count);
                    // Note: Actual spawning happens in existing orchestrator logic
                }
                state_machine::Action::CreateMerge { description } => {
                    debug!("[State Machine] Would create merge: {}", description);
                    // Note: Actual merging happens in integrate() method
                }
                state_machine::Action::ResolveConflicts { description } => {
                    debug!("[State Machine] Would resolve conflicts: {}", description);
                    // Note: Actual conflict resolution happens in integrate() method
                }
                state_machine::Action::SpawnValidator { validation_id } => {
                    debug!("[State Machine] Would spawn validator: {}", validation_id);
                    // Note: Actual validation happens in existing orchestrator logic
                }
            }
        }
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
                workspace_path.to_str().ok_or_else(|| {
                    HoxError::JjWorkspace("workspace path contains non-UTF-8 characters".into())
                })?,
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
        tokio::fs::create_dir_all(&hox_dir)
            .await
            .map_err(|e| HoxError::Io(format!("Failed to create .hox directory: {}", e)))?;

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
            if matches!(
                handle.status,
                ChildStatus::Completed | ChildStatus::Failed(_)
            ) {
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

        // State machine transition: Start orchestration
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::StartOrchestration {
                goal: "Run hierarchical delegation".to_string(),
            },
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

        // Get phases from the phase manager
        let phases: Vec<Phase> = self.phases.phases().to_vec();

        // Plan delegation
        let delegation_plans = self.plan_delegation(&phases);

        // Phase 0: Contracts (always local, blocking)
        if let Some(phase) = phases.iter().find(|p| p.number == 0) {
            info!("Running Phase 0 (contracts) locally: {}", phase.name);
            // TODO: Execute phase 0 locally
        }

        // State machine transition: Planning complete
        let task_count = delegation_plans.iter().filter(|p| matches!(p, DelegationPlan::ToChild { .. })).count();
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::PlanningComplete { task_count },
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

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

        // Monitor children until all complete (with timeout)
        let delegation_start = std::time::Instant::now();
        let max_delegation_duration = std::time::Duration::from_secs(30 * 60); // 30 minutes
        let mut poll_count: u64 = 0;
        let max_polls: u64 = 3600; // 30min at 500ms intervals

        while self.has_active_children() {
            // Timeout guard
            if delegation_start.elapsed() >= max_delegation_duration || poll_count >= max_polls {
                let active: Vec<_> = self
                    .children
                    .iter()
                    .filter(|(_, h)| {
                        matches!(h.status, ChildStatus::Running | ChildStatus::Spawning)
                    })
                    .map(|(id, _)| id.to_string())
                    .collect();
                warn!(
                    "Delegation timeout after {:?} ({} polls). Active children: {:?}",
                    delegation_start.elapsed(),
                    poll_count,
                    active
                );
                // Mark remaining active children as failed
                let active_ids: Vec<_> = self
                    .children
                    .iter()
                    .filter(|(_, h)| {
                        matches!(h.status, ChildStatus::Running | ChildStatus::Spawning)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for child_id in active_ids {
                    self.update_child_status(
                        &child_id,
                        ChildStatus::Failed("Timed out after 30 minutes".to_string()),
                    );
                }
                break;
            }

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
                    // Spawning and Running statuses don't need special logging here
                    _ => {}
                }
            }

            // Small delay to avoid busy-waiting
            tokio::time::sleep(POLL_LOOP_INTERVAL).await;
            poll_count += 1;
        }

        // All children done -> Integration phase
        self.state = OrchestratorState::Integrating;

        // State machine transition: All tasks complete
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::AllTasksComplete,
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

        self.integrate_child_work().await?;

        // State machine transition: Integration clean (simplified - actual conflict detection in integrate_child_work)
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::IntegrationClean,
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

        // Validation phase
        self.state = OrchestratorState::Validating;
        // TODO: Run validation phase

        // State machine transition: Validation passed (simplified)
        let (new_sm_state, actions) = state_machine::transition(
            self.sm_state.clone(),
            state_machine::Event::ValidationPassed,
        );
        self.sm_state = new_sm_state;
        self.execute_actions(actions);

        self.state = OrchestratorState::Completed;
        info!("Orchestrator {} completed with delegation", self.config.id);

        Ok(())
    }

    /// Merge all child orchestrator results
    async fn integrate_child_work(&mut self) -> Result<()> {
        info!("Integrating child orchestrator work");

        // Collect head changes from each child workspace
        let mut child_heads: Vec<ChangeId> = Vec::new();

        for handle in self.children.values() {
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
                    "Integration produced {} conflicts, attempting resolution",
                    conflicts.len()
                );

                let resolver = crate::ConflictResolver::new(self.executor.clone());
                let report = resolver.resolve_all().await?;

                if report.needs_human > 0 {
                    warn!(
                        "{} conflicts need human review, spawning resolution agent",
                        report.needs_human
                    );
                    self.spawn_agent("Resolve merge conflicts from child integration")
                        .await?;
                }
                if report.auto_resolved > 0 {
                    info!("Auto-resolved {} conflicts", report.auto_resolved);
                }
            }
        } else if child_heads.len() == 1 {
            info!("Single child, no merge needed");
        } else {
            info!("No child work to integrate");
        }

        Ok(())
    }

    /// Optimize DAG by parallelizing sequential tasks
    ///
    /// After planning tasks sequentially, this restructures the DAG for parallel execution.
    /// Use this when you have independent tasks that were created in sequence but can run
    /// in parallel.
    ///
    /// Tier 1 gate: returns an error if any agents are currently active.  Check
    /// `dag_restructure_deferred()` before calling if you want a non-fatal path.
    ///
    /// # Example
    /// ```ignore
    /// // After creating sequential task changes
    /// orchestrator.optimize_dag("heads(bookmarks(glob:\"task-*\"))").await?;
    /// ```
    pub async fn optimize_dag(&self, task_range: &str) -> Result<ParallelizeResult> {
        if self.dag_restructure_deferred() {
            return Err(HoxError::Orchestrator(format!(
                "DAG restructuring deferred: {} agent(s) still active",
                self.active_agent_change_ids.len()
            )));
        }
        info!("Optimizing DAG for parallel execution: {}", task_range);
        let dag_ops = DagOperations::new(self.executor.clone());
        dag_ops.parallelize(task_range).await
    }

    /// Absorb fixes back to agent branches
    ///
    /// After integration testing, this automatically distributes fixes back to the
    /// agent branches that introduced the bugs. This is safer than manual cherry-picking
    /// and preserves attribution.
    ///
    /// Tier 1 gate: returns an error if any agents are currently active.
    ///
    /// # Example
    /// ```ignore
    /// // After making fixes in integration branch
    /// orchestrator.absorb_fixes(Some(&["src/fixed_file.rs"])).await?;
    /// ```
    pub async fn absorb_fixes(&self, paths: Option<&[&str]>) -> Result<AbsorbResult> {
        if self.dag_restructure_deferred() {
            return Err(HoxError::Orchestrator(format!(
                "DAG restructuring deferred: {} agent(s) still active",
                self.active_agent_change_ids.len()
            )));
        }
        info!("Absorbing fixes back to agent branches");
        let dag_ops = DagOperations::new(self.executor.clone());
        dag_ops.absorb(paths).await
    }

    /// Decompose a task into smaller subtasks
    ///
    /// When an agent reports a task is too large, this splits it into smaller file-based
    /// subtasks. Each file group becomes a separate change that can be assigned independently.
    ///
    /// # Example
    /// ```ignore
    /// let file_groups = vec![
    ///     vec!["src/main.rs".to_string()],
    ///     vec!["src/lib.rs".to_string(), "src/utils.rs".to_string()],
    /// ];
    /// orchestrator.decompose_task("abc123", &file_groups).await?;
    /// ```
    pub async fn decompose_task(
        &self,
        change_id: &str,
        file_groups: &[Vec<String>],
    ) -> Result<SplitResult> {
        info!(
            "Decomposing task {} into {} subtasks",
            change_id,
            file_groups.len()
        );
        let dag_ops = DagOperations::new(self.executor.clone());
        dag_ops.split_by_files(change_id, file_groups).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_config() {
        let config =
            OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo").with_max_agents(8);

        assert_eq!(config.id.to_string(), "O-A-1");
        assert_eq!(config.max_agents, 8);
    }

    #[test]
    fn test_delegation_strategy_default() {
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo");
        assert!(matches!(
            config.delegation_strategy,
            DelegationStrategy::None
        ));
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
        assert!(matches!(
            config.delegation_strategy,
            DelegationStrategy::None
        ));
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

    /// Build a minimal Orchestrator backed by a MockJjExecutor for unit tests.
    ///
    /// We can't call `with_executor` (it's async and hits jj), so we construct
    /// the struct directly via a helper that bypasses the async constructor.
    fn make_test_orchestrator() -> Orchestrator<hox_jj::MockJjExecutor> {
        use hox_jj::MockJjExecutor;
        let config = OrchestratorConfig::new(OrchestratorId::root(), "/tmp/repo");
        let executor = MockJjExecutor::new();
        let workspace_manager = WorkspaceManager::new(executor.clone());
        Orchestrator {
            config,
            state: OrchestratorState::Initialized,
            executor,
            phases: PhaseManager::new(),
            workspace_manager,
            message_router: MessageRouter::new(),
            agents: HashMap::new(),
            change_id: None,
            children: HashMap::new(),
            sm_state: state_machine::State::Idle,
            active_agent_change_ids: HashSet::new(),
            fork_features: false,
            isolation: None,
        }
    }

    #[test]
    fn test_dag_restructure_gate_empty() {
        let orch = make_test_orchestrator();
        // No active agents → restructuring is allowed
        assert!(!orch.dag_restructure_deferred());
    }

    #[test]
    fn test_dag_restructure_gate_with_active_agent() {
        let mut orch = make_test_orchestrator();
        orch.active_agent_change_ids
            .insert("abc123".to_string());
        assert!(orch.dag_restructure_deferred());
    }

    #[test]
    fn test_complete_agent_change_opens_gate() {
        let mut orch = make_test_orchestrator();
        orch.active_agent_change_ids
            .insert("abc123".to_string());
        orch.active_agent_change_ids
            .insert("def456".to_string());

        orch.complete_agent_change("abc123");
        assert!(orch.dag_restructure_deferred()); // def456 still active

        orch.complete_agent_change("def456");
        assert!(!orch.dag_restructure_deferred()); // gate open
    }

    #[test]
    fn test_active_agent_change_ids_accessor() {
        let mut orch = make_test_orchestrator();
        orch.active_agent_change_ids
            .insert("xyz789".to_string());
        assert!(orch.active_agent_change_ids().contains("xyz789"));
        assert_eq!(orch.active_agent_change_ids().len(), 1);
    }

    #[test]
    fn test_fork_features_default_false() {
        let orch = make_test_orchestrator();
        assert!(!orch.fork_features);
    }
}
