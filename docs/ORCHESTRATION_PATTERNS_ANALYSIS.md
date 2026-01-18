# Orchestration Patterns Analysis for bd-orchestrator

**Analyzed:** 2026-01-17
**Project:** ~/dev/hox (jj-beads-rs workspace)
**Focus:** Multi-agent orchestration patterns using jujutsu VCS

---

## Executive Summary

The `bd-orchestrator` crate implements a **VCS-native orchestration pattern** where the jujutsu (jj) change DAG serves as the task graph. This is a fundamentally different approach from traditional orchestration systems, leveraging version control primitives for agent coordination.

**Key Innovation:** Tasks ARE changes, dependencies ARE ancestry, assignments ARE bookmarks.

---

## 1. CURRENT PATTERNS: Existing Orchestration Implementation

### 1.1 Task Decomposition Approach

**Pattern:** JJ Change DAG as Task Graph

```rust
// Tasks are jj changes with structured descriptions
pub struct Task {
    pub change_id: String,      // jj change ID (immutable)
    pub title: String,
    pub priority: Priority,
    pub status: TaskStatus,
    pub agent: Option<String>,
    pub context: Option<HandoffContext>,  // Agent continuity
    pub bookmark: Option<String>,         // Assignment marker
}
```

**Implementation Details:**

1. **Task Creation:**
   - Creates a new jj change with `jj new -m <description>`
   - Stores structured metadata in change description
   - Creates bookmark: `task-<change_id_prefix>`
   - Appends metadata to `.tasks/metadata.jsonl`

2. **Dependency Representation:**
   - Dependencies are parent-child relationships in the change DAG
   - No separate dependency table needed
   - Query via revsets: `ancestors(task_id) & mutable()`

3. **Task Metadata Storage:**
   - **In DAG:** Title, description, handoff context (in change description)
   - **External JSONL:** Priority, labels, due_date, agent assignment
   - **Why split?** DAG is immutable history, metadata is mutable state

**Strengths:**
- ✅ Natural dependency tracking through VCS ancestry
- ✅ Complete history of task evolution
- ✅ Conflict resolution via jj's merge capabilities
- ✅ Built-in distributed collaboration (jj push/pull)

**Limitations:**
- ⚠️ No explicit parallel task grouping
- ⚠️ Metadata split between DAG and JSONL
- ⚠️ Requires understanding of jj revsets

### 1.2 Agent Handoff Mechanisms

**Pattern:** Handoff Context Embedded in Change Descriptions

```rust
pub struct HandoffContext {
    pub current_focus: String,
    pub progress: Vec<String>,
    pub next_steps: Vec<String>,
    pub blockers: Option<Vec<String>>,
    pub open_questions: Option<Vec<String>>,
    pub files_touched: Option<Vec<String>>,
    pub updated_at: DateTime<Utc>,
}
```

**Handoff Flow:**

```
Agent A (finishing):
1. Updates HandoffContext with current state
2. Commits changes to working copy
3. jj describe -m <updated_task_description>
4. Creates bookmark: agent-<next_agent>/<task_id>

Agent B (starting):
1. Queries: jj log -r "bookmarks(glob:agent-B/*)"
2. Loads HandoffContext from change description
3. Gets cumulative diff: jj diff -r root()..task_id
4. Gets history: jj log -r ancestors(task_id)
5. Receives complete AgentHandoff package
```

**HandoffGenerator API:**

```rust
impl HandoffGenerator {
    // Generate handoff from summarization model output
    pub async fn generate_handoff(&self, change_id: &str, summary: HandoffSummary) -> Result<()>

    // Load existing handoff
    pub async fn load_handoff(&self, change_id: &str) -> Result<HandoffContext>

    // Get cumulative diff for context
    pub async fn get_diff(&self, change_id: &str) -> Result<String>

    // Prepare complete handoff package
    pub async fn prepare_handoff(&self, change_id: &str) -> Result<AgentHandoff>
}
```

**Strengths:**
- ✅ Complete context preservation (code + narrative)
- ✅ Version-controlled handoff state
- ✅ Supports asynchronous agent transitions
- ✅ No central coordinator needed

**Gaps:**
- ❌ No retry mechanism if handoff fails
- ❌ No validation that new agent acknowledged handoff
- ❌ No timeout handling for stale assignments

### 1.3 State Machine / Workflow Patterns

**Pattern:** Implicit State Machine via Revsets

The system doesn't have an explicit state machine. Instead, task states are computed from:
- Change description (status field)
- Bookmark presence (assignment)
- DAG position (dependencies)
- Metadata JSONL (priority, labels)

**State Queries via Revsets:**

```rust
impl RevsetQueries<E: JjExecutor> {
    // Ready = no incomplete dependencies, no conflicts
    async fn ready_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"heads(bookmarks(glob:"task-*")) - conflicts()"#;
        self.query_change_ids(revset).await
    }

    // Blocked = has incomplete ancestors
    async fn blocked_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"task-*") & descendants(mutable())"#;
        self.query_change_ids(revset).await
    }

    // In Progress = has agent bookmark
    async fn in_progress_tasks(&self) -> Result<Vec<String>> {
        let revset = r#"bookmarks(glob:"agent-*/*")"#;
        self.query_change_ids(revset).await
    }

    // Agent's tasks
    async fn agent_tasks(&self, agent_id: &str) -> Result<Vec<String>> {
        let revset = format!(r#"bookmarks(glob:"agent-{}/*")"#, agent_id);
        self.query_change_ids(revset).await
    }
}
```

**State Transitions:**

```
pending → in_progress:  Create agent bookmark
in_progress → blocked:  Detect dependency via ancestors()
blocked → ready:        Dependencies become immutable
ready → completed:      Mark status in description + jj abandon
```

**Strengths:**
- ✅ Declarative state computation
- ✅ No state machine code to maintain
- ✅ Powerful query capabilities via revsets

**Limitations:**
- ⚠️ State computation can be expensive (multiple jj queries)
- ⚠️ No explicit transitions or guards
- ⚠️ Hard to validate state machine correctness

### 1.4 Dependency Resolution

**Pattern:** DAG Ancestry + Revset Queries

Dependencies are represented by the change DAG structure:

```
A (parent)
├── B (child - depends on A)
└── C (child - depends on A)
    └── D (grandchild - depends on C and transitively on A)
```

**Dependency Queries:**

```rust
// Get all dependencies for a task
async fn task_dependencies(&self, change_id: &str) -> Result<Vec<String>> {
    let revset = format!("ancestors({}) & mutable()", change_id);
    self.query_change_ids(&revset).await
}

// Get all dependent tasks
async fn dependent_tasks(&self, change_id: &str) -> Result<Vec<String>> {
    let revset = format!("descendants({}) - {}", change_id, change_id);
    self.query_change_ids(&revset).await
}

// Build complete dependency graph
async fn build_dependency_graph(&self) -> Result<DependencyGraph> {
    // Queries all tasks, determines status, builds edges
}
```

**Resolution Strategy:**
1. Query `heads(bookmarks(glob:"task-*"))` for leaf tasks (no dependents)
2. Filter out conflicts: `- conflicts()`
3. Check if ancestors are immutable (completed)
4. Return ready tasks for assignment

**Strengths:**
- ✅ Automatic transitive dependency tracking
- ✅ Conflict detection via jj
- ✅ Natural support for DAG structures (not just trees)

**Gaps:**
- ❌ No cycle detection (relies on jj's DAG properties)
- ❌ No priority-based dependency resolution
- ❌ Limited support for "soft" dependencies

---

## 2. PATTERN GAPS: Missing Orchestration Capabilities

### 2.1 Parallel Execution Coordination

**Current State:** No explicit parallel execution support

**Gap Analysis:**

The system can identify multiple ready tasks but lacks:

1. **Work Stealing:** No mechanism for idle agents to claim tasks
2. **Load Balancing:** No distribution strategy across agents
3. **Resource Pools:** No concept of agent capabilities/capacity
4. **Barrier Synchronization:** No way to wait for parallel tasks to complete

**Impact:**
- Agents must poll for available work
- No automatic load distribution
- Manual coordination required for parallel workflows

**Example of Missing Pattern:**

```rust
// What we SHOULD have but DON'T:
struct ParallelTaskGroup {
    tasks: Vec<ChangeID>,
    barrier: BarrierID,      // Wait for all to complete
    concurrency_limit: usize, // Max parallel tasks
}

impl TaskManager {
    // MISSING: Claim next available task atomically
    async fn claim_ready_task(&self, agent_id: &str, capabilities: &[String])
        -> Result<Option<Task>>;

    // MISSING: Release task back to pool
    async fn release_task(&self, change_id: &str, reason: ReleaseReason) -> Result<()>;
}
```

### 2.2 Failure Recovery / Retry Logic

**Current State:** No retry mechanism

**Gap Analysis:**

If an agent fails, the task remains assigned:
- Bookmark `agent-X/task-Y` stays in place
- No timeout detection
- No automatic reassignment
- No failure metadata capture

**What's Missing:**

```rust
// Needed for production resilience:
pub struct TaskAttempt {
    task_id: ChangeID,
    agent_id: AgentID,
    attempt_number: u32,
    started_at: DateTime<Utc>,
    failed_at: Option<DateTime<Utc>>,
    failure_reason: Option<String>,
    retry_strategy: RetryStrategy,
}

pub enum RetryStrategy {
    NoRetry,
    ExponentialBackoff { max_attempts: u32, base_delay: Duration },
    FixedDelay { max_attempts: u32, delay: Duration },
    ImmediateReassign,
}

impl TaskManager {
    // MISSING: Detect stale assignments
    async fn find_stale_assignments(&self, timeout: Duration) -> Result<Vec<Task>>;

    // MISSING: Retry failed task
    async fn retry_task(&self, change_id: &str, strategy: RetryStrategy) -> Result<()>;

    // MISSING: Mark task as permanently failed
    async fn mark_failed(&self, change_id: &str, reason: String) -> Result<()>;
}
```

### 2.3 Priority Queuing

**Current State:** Priority exists but isn't used in assignment

Tasks have priority levels (Critical → Backlog), but the `ready_tasks()` query doesn't order by priority:

```rust
// Current implementation
async fn ready_tasks(&self) -> Result<Vec<String>> {
    let revset = r#"heads(bookmarks(glob:"task-*")) - conflicts()"#;
    // Returns unordered!
    self.query_change_ids(revset).await
}
```

**What's Needed:**

```rust
pub struct PriorityQueue {
    // MISSING: Priority-aware task ordering
    async fn pop_highest_priority(&self, constraints: Constraints) -> Result<Option<Task>>;

    // MISSING: Priority inversion detection
    async fn detect_priority_inversions(&self) -> Result<Vec<(Task, Task)>>;
}

// Should support revset like:
// sort(ready_tasks, "-priority, due_date")
```

### 2.4 Resource Contention Handling

**Current State:** No resource modeling

Tasks don't declare resource requirements (CPU, memory, GPU, API quota, etc.).

**Missing Capabilities:**

```rust
pub struct ResourceRequirements {
    cpu_cores: Option<f32>,
    memory_gb: Option<f32>,
    gpu: Option<GpuSpec>,
    api_quota: Vec<ApiQuotaSpec>,
    exclusive_lock: Option<String>,  // e.g., "production_deploy"
}

pub struct ResourcePool {
    available: Resources,
    allocated: HashMap<ChangeID, Resources>,
}

impl TaskManager {
    // MISSING: Check if resources are available
    async fn can_assign_task(&self, task: &Task, agent: &Agent) -> Result<bool>;

    // MISSING: Reserve resources
    async fn allocate_resources(&self, task: &Task, agent: &Agent) -> Result<()>;

    // MISSING: Release resources
    async fn release_resources(&self, task: &Task) -> Result<()>;
}
```

### 2.5 Backpressure Mechanisms

**Current State:** No flow control

If tasks are created faster than agents can complete them, the queue grows unbounded.

**Missing Patterns:**

```rust
pub struct BackpressureConfig {
    max_pending_tasks: usize,
    max_tasks_per_agent: usize,
    queue_size_threshold: usize,
}

impl TaskManager {
    // MISSING: Check if system is overloaded
    async fn is_backpressure_active(&self) -> Result<bool>;

    // MISSING: Reject new task creation when overloaded
    async fn create_task_with_backpressure(&self, task: &Task)
        -> Result<Result<(), BackpressureError>>;

    // MISSING: Drain signal
    async fn wait_for_queue_drain(&self, target_size: usize) -> Result<()>;
}
```

---

## 3. AGENT-FIRST DESIGN: Multi-Agent Support Analysis

### 3.1 Autonomous Agent Operation

**Current Support:** Moderate

**What Works:**
- ✅ Agents can query for their assigned tasks
- ✅ Handoff context enables autonomous continuation
- ✅ No central coordinator required (decentralized)

**What's Missing:**
- ❌ No agent heartbeat/liveness detection
- ❌ No autonomous task claiming (requires external coordination)
- ❌ No agent-specific configuration (capabilities, quotas)

**Agent Autonomy Gap:**

```rust
// Current: Agent must know its ID and poll
let tasks = queries.agent_tasks("agent-A").await?;

// Better: Agent registers and claims work autonomously
pub struct Agent {
    id: AgentID,
    capabilities: Vec<String>,
    max_concurrent_tasks: usize,
    heartbeat_interval: Duration,
}

impl Agent {
    // MISSING: Self-service work claiming
    async fn claim_next_task(&mut self, tm: &TaskManager) -> Result<Option<Task>>;

    // MISSING: Periodic heartbeat
    async fn send_heartbeat(&self, tm: &TaskManager) -> Result<()>;

    // MISSING: Graceful shutdown
    async fn shutdown(&self, tm: &TaskManager, handoff: bool) -> Result<()>;
}
```

### 3.2 Agent-to-Agent Communication

**Current State:** Indirect communication only

Agents communicate by:
1. Updating HandoffContext in change descriptions
2. Creating/updating bookmarks
3. Leaving comments (not implemented yet)

**Gap: No Direct Messaging:**

```rust
// MISSING: Agent messaging system
pub struct AgentMessage {
    from_agent: AgentID,
    to_agent: AgentID,
    task_context: Option<ChangeID>,
    message_type: MessageType,
    payload: serde_json::Value,
    timestamp: DateTime<Utc>,
}

pub enum MessageType {
    Question,           // Ask for clarification
    BlockerNotification, // Notify of blocking issue
    HandoffRequest,     // Request task reassignment
    ProgressUpdate,     // Share progress
}

impl TaskManager {
    // MISSING: Send message between agents
    async fn send_message(&self, msg: AgentMessage) -> Result<()>;

    // MISSING: Get messages for agent
    async fn get_messages(&self, agent_id: &str, since: DateTime<Utc>)
        -> Result<Vec<AgentMessage>>;
}
```

**Current Workaround:**
- Store messages in change description's "Open Questions" or "Blockers"
- Limited structure and discoverability

### 3.3 Shared Context Management

**Current Support:** Strong (via jj)

**What Works:**
- ✅ Shared state via jj repository
- ✅ Conflict resolution via jj merge
- ✅ Distributed sync via jj push/pull
- ✅ HandoffContext provides rich shared context

**Enhancement Opportunity:**

```rust
// Good foundation, could add:
pub struct SharedContext {
    workspace: PathBuf,              // jj repo root
    session_log: Vec<SessionEntry>,  // Activity log
    global_state: HashMap<String, serde_json::Value>, // Shared KV
}

impl SharedContext {
    // MISSING: Shared state access
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>>;
    async fn set(&self, key: &str, value: serde_json::Value) -> Result<()>;

    // MISSING: Activity log
    async fn log_activity(&self, entry: SessionEntry) -> Result<()>;
    async fn get_recent_activity(&self, limit: usize) -> Result<Vec<SessionEntry>>;
}
```

### 3.4 Work Stealing / Load Balancing

**Current State:** Not implemented

**What's Missing:**

The system has no concept of:
- Agent load (current task count)
- Agent capacity (max concurrent tasks)
- Work stealing protocol
- Dynamic task redistribution

**Needed Implementation:**

```rust
pub struct LoadBalancer {
    agents: HashMap<AgentID, AgentStatus>,
    rebalance_threshold: f32,  // e.g., 0.3 = 30% imbalance triggers rebalance
}

pub struct AgentStatus {
    agent_id: AgentID,
    current_tasks: usize,
    capacity: usize,
    last_heartbeat: DateTime<Utc>,
    is_available: bool,
}

impl LoadBalancer {
    // MISSING: Detect imbalanced load
    async fn is_imbalanced(&self) -> Result<bool>;

    // MISSING: Rebalance tasks across agents
    async fn rebalance(&mut self, tm: &TaskManager) -> Result<RebalanceReport>;

    // MISSING: Work stealing
    async fn steal_task(&self, from: &AgentID, to: &AgentID) -> Result<Option<Task>>;
}
```

**Current Workaround:**
- External orchestrator must manually distribute work
- No automatic load balancing

---

## 4. JJ INTEGRATION: VCS-Native Orchestration

### 4.1 Revset-Based Task Selection

**Current Implementation:** Excellent foundation

The system leverages jj's powerful revset language:

```rust
// Examples of current revset usage:
"heads(bookmarks(glob:\"task-*\")) - conflicts()"  // Ready tasks
"bookmarks(glob:\"agent-{id}/*\")"                   // Agent's tasks
"ancestors({id}) & mutable()"                        // Dependencies
"descendants({id}) - {id}"                           // Dependents
```

**Strengths:**
- ✅ Declarative query language
- ✅ Composable filters
- ✅ Efficient DAG traversal
- ✅ Natural dependency tracking

**Enhancement Opportunities:**

```rust
// Advanced revset patterns we COULD use:
pub struct RevsetLibrary;

impl RevsetLibrary {
    // Tasks ready for agent with specific capability
    fn tasks_for_capability(capability: &str) -> String {
        format!(
            r#"
            heads(bookmarks(glob:"task-*"))
            - conflicts()
            & file("tags.json", "capability:{}")
            "#,
            capability
        )
    }

    // Critical path tasks (most dependents)
    fn critical_path_tasks() -> &'static str {
        r#"
        sort(
            heads(bookmarks(glob:"task-*")),
            "descendants_count",
            desc
        )
        "#
    }

    // Overdue tasks
    fn overdue_tasks() -> String {
        let now = Utc::now().to_rfc3339();
        format!(r#"description ~ "DueDate.*{}" "#, now)
    }
}
```

### 4.2 Oplog Watching for Triggers

**Current Implementation:** Excellent (bd-daemon/oplog.rs)

The oplog watcher is a key innovation:

```rust
pub struct OpLogWatcher {
    config: OpLogWatcherConfig,
    last_seen_id: Option<String>,
}

impl OpLogWatcher {
    // Poll for new operations
    async fn poll_operations(&self) -> Result<Vec<OpLogEntry>> {
        // Queries: jj op log --no-graph -n 50
        // Parses operation descriptions
        // Gets affected files via: jj op show <id> --op-diff
    }

    // Watch loop
    pub async fn watch<F>(mut self, callback: F) -> Result<()>
    where F: Fn(&[OpLogEntry]) -> Result<()> {
        // Polls at interval (default 100ms)
        // Delivers operations in chronological order
        // Filters to tasks/ and deps/ changes
    }
}
```

**Strengths:**
- ✅ Efficient change detection (no filesystem watching)
- ✅ Captures all jj operations (commits, rebases, merges)
- ✅ Chronological ordering
- ✅ Filtered to relevant paths
- ✅ Handles operation log garbage collection

**Integration Points:**

```rust
// Oplog can trigger:
// 1. Task index updates
// 2. Dependency graph refresh
// 3. Agent notifications
// 4. Webhook dispatch

pub async fn setup_oplog_triggers(
    watcher: OpLogWatcher,
    tm: TaskManager,
) -> Result<()> {
    watcher.watch(move |entries| {
        for entry in entries {
            match entry.description.as_str() {
                desc if desc.contains("snapshot") => {
                    // Task state might have changed
                    tm.refresh_task_cache()?;
                },
                desc if desc.contains("rebase") => {
                    // Dependencies might have changed
                    tm.rebuild_dep_graph()?;
                },
                _ => {}
            }
        }
        Ok(())
    }).await
}
```

### 4.3 Branch-Based Isolation

**Current Implementation:** Partial (bookmarks, not branches)

The system uses bookmarks for assignment:
- `task-<id>` bookmarks mark task changes
- `agent-<id>/<task>` bookmarks mark assignments

**Not Using:** jj branches for workspace isolation

**Opportunity for Enhancement:**

```rust
// Could use jj workspaces for agent isolation:
pub struct AgentWorkspace {
    workspace_id: String,
    agent_id: AgentID,
    working_copy_path: PathBuf,
}

impl AgentWorkspace {
    // Create isolated workspace for agent
    async fn create(agent_id: &str, base_repo: &Path) -> Result<Self> {
        // jj workspace add ../agent-{id}
        // Each agent gets its own working copy
        // Changes are still in the same repo
    }

    // Sync workspace with main
    async fn sync(&self) -> Result<()> {
        // jj git fetch (if using git backend)
        // jj rebase -d main
    }

    // Cleanup workspace
    async fn cleanup(self) -> Result<()> {
        // jj workspace forget
    }
}

// Benefits:
// - Agents don't interfere with each other's working copies
// - Can work offline, sync later
// - Natural isolation for concurrent work
```

### 4.4 Conflict Resolution Strategy

**Current State:** Relies on jj's built-in conflict resolution

**How Conflicts Arise:**
1. Two agents update same task concurrently
2. Task descriptions diverge
3. jj detects conflict in change description

**Current Handling:**

```rust
// Conflicting tasks are filtered out:
async fn ready_tasks(&self) -> Result<Vec<String>> {
    let revset = r#"heads(bookmarks(glob:"task-*")) - conflicts()"#;
    //                                                  ^^^^^^^^^^
    //                                                  Excludes conflicts
    self.query_change_ids(revset).await
}
```

**Gap: No Automatic Conflict Resolution**

```rust
// MISSING: Conflict resolution strategies
pub enum ConflictResolution {
    TakeNewest,           // Use most recent timestamp
    TakeHighestPriority,  // Prefer higher priority agent
    Merge,                // Merge non-conflicting fields
    Manual,               // Require human intervention
}

impl TaskManager {
    // MISSING: Detect conflicts
    async fn detect_conflicts(&self) -> Result<Vec<ConflictedTask>>;

    // MISSING: Auto-resolve conflicts
    async fn resolve_conflict(
        &self,
        task_id: &str,
        strategy: ConflictResolution
    ) -> Result<Task>;

    // MISSING: Merge handoff contexts
    async fn merge_handoff_contexts(
        &self,
        base: &HandoffContext,
        left: &HandoffContext,
        right: &HandoffContext,
    ) -> Result<HandoffContext>;
}
```

---

## 5. RECOMMENDED PATTERNS: Production-Ready Orchestration

### 5.1 Saga Pattern for Distributed Transactions

**Why Needed:** Tasks often span multiple systems (DB, API, file ops)

**Current Gap:** No compensation logic for partial failures

**Recommendation:**

```rust
/// Saga pattern for distributed task execution
pub struct Saga {
    steps: Vec<SagaStep>,
    compensations: Vec<CompensationFn>,
    state: SagaState,
}

pub struct SagaStep {
    name: String,
    action: Box<dyn Fn() -> BoxFuture<'static, Result<()>>>,
    compensation: Box<dyn Fn() -> BoxFuture<'static, Result<()>>>,
}

pub enum SagaState {
    NotStarted,
    InProgress { completed_steps: usize },
    Succeeded,
    Failed { failed_at_step: usize },
    Compensating { compensated_steps: usize },
    Compensated,
}

impl Saga {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            compensations: Vec::new(),
            state: SagaState::NotStarted,
        }
    }

    pub fn add_step(
        &mut self,
        name: String,
        action: impl Fn() -> BoxFuture<'static, Result<()>> + 'static,
        compensation: impl Fn() -> BoxFuture<'static, Result<()>> + 'static,
    ) {
        self.steps.push(SagaStep {
            name,
            action: Box::new(action),
            compensation: Box::new(compensation),
        });
    }

    pub async fn execute(&mut self) -> Result<()> {
        self.state = SagaState::InProgress { completed_steps: 0 };

        for (i, step) in self.steps.iter().enumerate() {
            match (step.action)().await {
                Ok(_) => {
                    self.state = SagaState::InProgress { completed_steps: i + 1 };
                }
                Err(e) => {
                    self.state = SagaState::Failed { failed_at_step: i };
                    self.compensate(i).await?;
                    return Err(e);
                }
            }
        }

        self.state = SagaState::Succeeded;
        Ok(())
    }

    async fn compensate(&mut self, up_to_step: usize) -> Result<()> {
        self.state = SagaState::Compensating { compensated_steps: 0 };

        // Execute compensations in reverse order
        for (i, step) in self.steps[..up_to_step].iter().enumerate().rev() {
            (step.compensation)().await?;
            self.state = SagaState::Compensating {
                compensated_steps: up_to_step - i,
            };
        }

        self.state = SagaState::Compensated;
        Ok(())
    }
}

// Integration with TaskManager
impl TaskManager {
    pub async fn execute_task_with_saga(
        &self,
        task: &Task,
    ) -> Result<()> {
        let mut saga = Saga::new();

        // Example: Multi-step deployment task
        saga.add_step(
            "backup_database".to_string(),
            || Box::pin(async { self.backup_db().await }),
            || Box::pin(async { self.restore_db().await }),
        );

        saga.add_step(
            "deploy_code".to_string(),
            || Box::pin(async { self.deploy().await }),
            || Box::pin(async { self.rollback_deploy().await }),
        );

        saga.add_step(
            "run_migrations".to_string(),
            || Box::pin(async { self.migrate().await }),
            || Box::pin(async { self.rollback_migrations().await }),
        );

        saga.execute().await
    }
}
```

**Integration with Current System:**

```rust
// Store saga state in change description
pub struct TaskWithSaga {
    task: Task,
    saga_state: Option<SagaState>,
    saga_log: Vec<SagaLogEntry>,
}

pub struct SagaLogEntry {
    step_name: String,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    status: StepStatus,
    error: Option<String>,
}
```

### 5.2 Actor Model for Agents

**Why Needed:** Agents need concurrent, isolated execution

**Current Gap:** No agent lifecycle management, no message passing

**Recommendation:**

```rust
use tokio::sync::mpsc;

/// Actor-based agent implementation
pub struct AgentActor {
    id: AgentID,
    capabilities: Vec<String>,
    max_concurrent: usize,
    current_tasks: Vec<ChangeID>,
    mailbox: mpsc::Receiver<AgentMessage>,
    tx: mpsc::Sender<AgentMessage>,
}

pub enum AgentMessage {
    AssignTask { task: Task },
    CancelTask { task_id: ChangeID },
    UpdateHandoff { task_id: ChangeID, context: HandoffContext },
    Shutdown,
    HealthCheck { respond_to: oneshot::Sender<AgentHealth> },
}

pub struct AgentHealth {
    is_alive: bool,
    current_load: usize,
    last_activity: DateTime<Utc>,
}

impl AgentActor {
    pub fn spawn(
        id: AgentID,
        capabilities: Vec<String>,
        max_concurrent: usize,
    ) -> mpsc::Sender<AgentMessage> {
        let (tx, rx) = mpsc::channel(100);

        let mut actor = Self {
            id,
            capabilities,
            max_concurrent,
            current_tasks: Vec::new(),
            mailbox: rx,
            tx: tx.clone(),
        };

        tokio::spawn(async move {
            actor.run().await;
        });

        tx
    }

    async fn run(&mut self) {
        while let Some(msg) = self.mailbox.recv().await {
            match msg {
                AgentMessage::AssignTask { task } => {
                    if self.current_tasks.len() < self.max_concurrent {
                        self.execute_task(task).await;
                    } else {
                        // Send back to queue
                        self.reject_task(task).await;
                    }
                }

                AgentMessage::CancelTask { task_id } => {
                    self.current_tasks.retain(|id| id != &task_id);
                }

                AgentMessage::UpdateHandoff { task_id, context } => {
                    self.update_handoff(task_id, context).await;
                }

                AgentMessage::Shutdown => {
                    self.shutdown().await;
                    break;
                }

                AgentMessage::HealthCheck { respond_to } => {
                    let health = AgentHealth {
                        is_alive: true,
                        current_load: self.current_tasks.len(),
                        last_activity: Utc::now(),
                    };
                    let _ = respond_to.send(health);
                }
            }
        }
    }

    async fn execute_task(&mut self, task: Task) {
        let task_id = task.change_id.clone();
        self.current_tasks.push(task_id.clone());

        // Spawn task execution
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match Self::run_task(task).await {
                Ok(_) => {
                    // Task completed successfully
                }
                Err(e) => {
                    // Task failed, send message to retry
                    tracing::error!("Task {} failed: {}", task_id, e);
                }
            }

            // Remove from current tasks
            let _ = tx.send(AgentMessage::CancelTask { task_id }).await;
        });
    }

    async fn run_task(task: Task) -> Result<()> {
        // Actual task execution logic
        todo!()
    }
}

// Supervisor for agent actors
pub struct AgentSupervisor {
    agents: HashMap<AgentID, mpsc::Sender<AgentMessage>>,
}

impl AgentSupervisor {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn spawn_agent(
        &mut self,
        id: AgentID,
        capabilities: Vec<String>,
        max_concurrent: usize,
    ) {
        let tx = AgentActor::spawn(id.clone(), capabilities, max_concurrent);
        self.agents.insert(id, tx);
    }

    pub async fn assign_task(&self, agent_id: &AgentID, task: Task) -> Result<()> {
        let tx = self.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        tx.send(AgentMessage::AssignTask { task }).await
            .map_err(|e| anyhow::anyhow!("Failed to send task: {}", e))
    }

    pub async fn check_health(&self, agent_id: &AgentID) -> Result<AgentHealth> {
        let tx = self.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let (response_tx, response_rx) = oneshot::channel();
        tx.send(AgentMessage::HealthCheck { respond_to: response_tx }).await?;

        response_rx.await
            .map_err(|e| anyhow::anyhow!("Health check failed: {}", e))
    }

    pub async fn shutdown_all(&mut self) -> Result<()> {
        for (_, tx) in self.agents.drain() {
            let _ = tx.send(AgentMessage::Shutdown).await;
        }
        Ok(())
    }
}
```

### 5.3 Event Sourcing for State

**Why Needed:** Complete audit trail, time-travel debugging, replay

**Current State:** Partial (jj provides history, but not full event log)

**Recommendation:**

```rust
/// Event sourcing for task state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskEvent {
    Created {
        task_id: ChangeID,
        task: Task,
        timestamp: DateTime<Utc>,
        created_by: AgentID,
    },

    Assigned {
        task_id: ChangeID,
        agent_id: AgentID,
        timestamp: DateTime<Utc>,
    },

    Started {
        task_id: ChangeID,
        agent_id: AgentID,
        timestamp: DateTime<Utc>,
    },

    ProgressUpdated {
        task_id: ChangeID,
        handoff: HandoffContext,
        timestamp: DateTime<Utc>,
    },

    Blocked {
        task_id: ChangeID,
        blocker_ids: Vec<ChangeID>,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    Unblocked {
        task_id: ChangeID,
        timestamp: DateTime<Utc>,
    },

    Completed {
        task_id: ChangeID,
        agent_id: AgentID,
        timestamp: DateTime<Utc>,
    },

    Failed {
        task_id: ChangeID,
        agent_id: AgentID,
        error: String,
        timestamp: DateTime<Utc>,
    },

    Reassigned {
        task_id: ChangeID,
        from_agent: AgentID,
        to_agent: AgentID,
        reason: String,
        timestamp: DateTime<Utc>,
    },
}

/// Event store backed by JSONL
pub struct EventStore {
    path: PathBuf,
}

impl EventStore {
    pub async fn append(&self, event: TaskEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        let json = serde_json::to_string(&event)?;
        file.write_all(format!("{}\n", json).as_bytes()).await?;

        Ok(())
    }

    pub async fn read_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let file = File::open(&self.path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut events = Vec::new();

        while let Some(line) = lines.next_line().await? {
            let event: TaskEvent = serde_json::from_str(&line)?;

            // Filter to specific task
            if event.task_id() == task_id {
                events.push(event);
            }
        }

        Ok(events)
    }

    pub async fn replay_to_state(&self, task_id: &str) -> Result<Task> {
        let events = self.read_events(task_id).await?;

        let mut task = None;

        for event in events {
            match event {
                TaskEvent::Created { task: t, .. } => {
                    task = Some(t);
                }
                TaskEvent::Assigned { agent_id, .. } => {
                    if let Some(ref mut task) = task {
                        task.agent = Some(agent_id);
                        task.status = TaskStatus::InProgress;
                    }
                }
                TaskEvent::ProgressUpdated { handoff, .. } => {
                    if let Some(ref mut task) = task {
                        task.context = Some(handoff);
                    }
                }
                TaskEvent::Completed { .. } => {
                    if let Some(ref mut task) = task {
                        task.status = TaskStatus::Completed;
                    }
                }
                // ... handle other events
                _ => {}
            }
        }

        task.ok_or_else(|| anyhow::anyhow!("No creation event found"))
    }
}

impl TaskEvent {
    fn task_id(&self) -> &str {
        match self {
            TaskEvent::Created { task_id, .. }
            | TaskEvent::Assigned { task_id, .. }
            | TaskEvent::Started { task_id, .. }
            | TaskEvent::ProgressUpdated { task_id, .. }
            | TaskEvent::Blocked { task_id, .. }
            | TaskEvent::Unblocked { task_id, .. }
            | TaskEvent::Completed { task_id, .. }
            | TaskEvent::Failed { task_id, .. }
            | TaskEvent::Reassigned { task_id, .. } => task_id,
        }
    }
}
```

**Integration with JJ:**

```rust
// Store event log in .tasks/events.jsonl
// On each jj operation, append events
// Use events for:
// 1. Audit trail
// 2. Time-travel queries
// 3. Analytics
// 4. Debugging
```

### 5.4 CQRS for Read/Write Separation

**Why Needed:** Optimize reads (queries) separately from writes (commands)

**Current Gap:** Queries and writes both go through TaskManager

**Recommendation:**

```rust
/// Command side: Handles writes
pub struct CommandHandler {
    tm: TaskManager,
    event_store: EventStore,
}

impl CommandHandler {
    pub async fn create_task(&self, cmd: CreateTaskCommand) -> Result<ChangeID> {
        // Validate command
        cmd.validate()?;

        // Execute command
        let mut task = Task::from(cmd);
        self.tm.create_task(&mut task).await?;

        // Publish event
        let event = TaskEvent::Created {
            task_id: task.change_id.clone(),
            task: task.clone(),
            timestamp: Utc::now(),
            created_by: cmd.created_by,
        };
        self.event_store.append(event).await?;

        Ok(task.change_id)
    }

    pub async fn assign_task(&self, cmd: AssignTaskCommand) -> Result<()> {
        // Execute
        // ... (similar pattern)

        // Publish event
        let event = TaskEvent::Assigned {
            task_id: cmd.task_id.clone(),
            agent_id: cmd.agent_id.clone(),
            timestamp: Utc::now(),
        };
        self.event_store.append(event).await?;

        Ok(())
    }
}

/// Query side: Handles reads (denormalized views)
pub struct QueryHandler {
    // Denormalized read models
    ready_tasks_cache: RwLock<Vec<Task>>,
    agent_workload_cache: RwLock<HashMap<AgentID, AgentWorkload>>,
    dependency_graph_cache: RwLock<DependencyGraph>,
}

impl QueryHandler {
    pub async fn get_ready_tasks(&self) -> Result<Vec<Task>> {
        // Read from cache
        let cache = self.ready_tasks_cache.read().await;
        Ok(cache.clone())
    }

    pub async fn get_agent_workload(&self, agent_id: &AgentID) -> Result<AgentWorkload> {
        let cache = self.agent_workload_cache.read().await;
        cache.get(agent_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))
    }

    // Rebuild caches from events
    pub async fn rebuild_caches(&self, event_store: &EventStore) -> Result<()> {
        // Read all events
        let events = event_store.read_all_events().await?;

        // Rebuild ready tasks cache
        let mut ready_tasks = Vec::new();
        // ... (replay events to build cache)

        // Rebuild agent workload cache
        let mut agent_workload = HashMap::new();
        // ... (replay events to build cache)

        // Update caches
        *self.ready_tasks_cache.write().await = ready_tasks;
        *self.agent_workload_cache.write().await = agent_workload;

        Ok(())
    }
}

/// Orchestrator that combines both sides
pub struct CqrsOrchestrator {
    commands: CommandHandler,
    queries: QueryHandler,
}

impl CqrsOrchestrator {
    // Commands go to CommandHandler
    pub async fn execute_command(&self, cmd: Command) -> Result<()> {
        match cmd {
            Command::CreateTask(c) => {
                self.commands.create_task(c).await?;
            }
            Command::AssignTask(c) => {
                self.commands.assign_task(c).await?;
            }
            // ... other commands
        }

        // Trigger cache rebuild (async)
        self.refresh_query_caches().await?;

        Ok(())
    }

    // Queries go to QueryHandler
    pub async fn query(&self, q: Query) -> Result<QueryResult> {
        match q {
            Query::GetReadyTasks => {
                let tasks = self.queries.get_ready_tasks().await?;
                Ok(QueryResult::Tasks(tasks))
            }
            Query::GetAgentWorkload { agent_id } => {
                let workload = self.queries.get_agent_workload(&agent_id).await?;
                Ok(QueryResult::Workload(workload))
            }
            // ... other queries
        }
    }

    async fn refresh_query_caches(&self) -> Result<()> {
        // Rebuild caches in background
        tokio::spawn(async move {
            // ... rebuild logic
        });
        Ok(())
    }
}
```

---

## 6. CODE SKETCHES: Integration Examples

### 6.1 Complete Orchestration Layer

```rust
// crates/bd-orchestrator/src/orchestrator.rs

use crate::{
    task::TaskManager,
    handoff::HandoffGenerator,
    revsets::RevsetQueries,
};

/// Main orchestrator coordinating all agents and tasks
pub struct Orchestrator {
    // Core components
    task_manager: TaskManager,
    handoff_gen: HandoffGenerator,
    queries: RevsetQueries,

    // New components
    supervisor: AgentSupervisor,
    event_store: EventStore,
    load_balancer: LoadBalancer,

    // Configuration
    config: OrchestratorConfig,
}

pub struct OrchestratorConfig {
    pub max_concurrent_tasks_per_agent: usize,
    pub task_timeout: Duration,
    pub enable_auto_retry: bool,
    pub retry_strategy: RetryStrategy,
    pub enable_load_balancing: bool,
    pub rebalance_interval: Duration,
}

impl Orchestrator {
    pub async fn new(
        repo_path: PathBuf,
        config: OrchestratorConfig,
    ) -> Result<Self> {
        let jj = JjCommand::new(&repo_path);
        let task_manager = TaskManager::new(&repo_path, jj.clone());
        let handoff_gen = HandoffGenerator::new(&repo_path);
        let queries = RevsetQueries::new(jj);

        let supervisor = AgentSupervisor::new();
        let event_store = EventStore::new(repo_path.join(".tasks/events.jsonl"));
        let load_balancer = LoadBalancer::new();

        Ok(Self {
            task_manager,
            handoff_gen,
            queries,
            supervisor,
            event_store,
            load_balancer,
            config,
        })
    }

    /// Main orchestration loop
    pub async fn run(&mut self) -> Result<()> {
        // Spawn background tasks
        let mut join_handles = vec![];

        // 1. Oplog watcher
        let oplog_handle = self.spawn_oplog_watcher();
        join_handles.push(oplog_handle);

        // 2. Load balancer
        if self.config.enable_load_balancing {
            let lb_handle = self.spawn_load_balancer();
            join_handles.push(lb_handle);
        }

        // 3. Health checker
        let health_handle = self.spawn_health_checker();
        join_handles.push(health_handle);

        // 4. Task dispatcher
        let dispatch_handle = self.spawn_task_dispatcher();
        join_handles.push(dispatch_handle);

        // Wait for all tasks (or until one fails)
        futures::future::try_join_all(join_handles).await?;

        Ok(())
    }

    fn spawn_oplog_watcher(&self) -> tokio::task::JoinHandle<Result<()>> {
        let watcher = OpLogWatcher::new(OpLogWatcherConfig {
            repo_path: self.task_manager.repo_path().clone(),
            poll_interval: Duration::from_millis(100),
            tasks_dir: "tasks".to_string(),
            deps_dir: "deps".to_string(),
            last_op_id: None,
        }).expect("Failed to create oplog watcher");

        tokio::spawn(async move {
            watcher.watch(|entries| {
                for entry in entries {
                    tracing::info!("Operation detected: {}", entry.description);
                    // Trigger task refresh
                }
                Ok(())
            }).await
        })
    }

    fn spawn_load_balancer(&self) -> tokio::task::JoinHandle<Result<()>> {
        let mut lb = self.load_balancer.clone();
        let interval = self.config.rebalance_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                if lb.is_imbalanced().await? {
                    tracing::info!("Load imbalance detected, rebalancing...");
                    lb.rebalance().await?;
                }
            }
        })
    }

    fn spawn_health_checker(&self) -> tokio::task::JoinHandle<Result<()>> {
        let supervisor = self.supervisor.clone();
        let timeout = self.config.task_timeout;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;

                // Check agent health
                for agent_id in supervisor.list_agents() {
                    match supervisor.check_health(&agent_id).await {
                        Ok(health) if !health.is_alive => {
                            tracing::warn!("Agent {} is unhealthy", agent_id);
                            // Trigger reassignment
                        }
                        Err(e) => {
                            tracing::error!("Health check failed for {}: {}", agent_id, e);
                        }
                        _ => {}
                    }
                }
            }
        })
    }

    fn spawn_task_dispatcher(&self) -> tokio::task::JoinHandle<Result<()>> {
        let tm = self.task_manager.clone();
        let queries = self.queries.clone();
        let supervisor = self.supervisor.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(5));
            loop {
                ticker.tick().await;

                // Get ready tasks
                let ready_tasks = queries.ready_tasks().await?;

                // Get available agents
                let available_agents = supervisor.get_available_agents().await?;

                // Assign tasks to agents
                for (task_id, agent_id) in Self::match_tasks_to_agents(
                    &ready_tasks,
                    &available_agents,
                ) {
                    let task = tm.get_task(&task_id).await?;
                    supervisor.assign_task(&agent_id, task).await?;
                }
            }
        })
    }

    fn match_tasks_to_agents(
        tasks: &[String],
        agents: &[AgentInfo],
    ) -> Vec<(String, AgentID)> {
        // Priority-based matching
        // Consider agent capabilities, current load, task priority
        todo!()
    }
}
```

### 6.2 Production-Ready Agent Implementation

```rust
// crates/bd-orchestrator/src/agent.rs

use crate::{Task, HandoffContext};

/// Production-ready agent with full lifecycle management
pub struct ProductionAgent {
    id: AgentID,
    capabilities: Vec<String>,
    config: AgentConfig,

    // Actor components
    actor: AgentActor,
    mailbox: mpsc::Sender<AgentMessage>,

    // Task execution
    executor: Box<dyn TaskExecutor>,

    // State
    state: RwLock<AgentState>,
}

pub struct AgentConfig {
    pub max_concurrent_tasks: usize,
    pub heartbeat_interval: Duration,
    pub task_timeout: Duration,
    pub enable_auto_handoff: bool,
}

pub struct AgentState {
    pub current_tasks: HashMap<ChangeID, TaskExecution>,
    pub last_heartbeat: DateTime<Utc>,
    pub total_completed: usize,
    pub total_failed: usize,
}

pub struct TaskExecution {
    pub task: Task,
    pub started_at: DateTime<Utc>,
    pub saga: Option<Saga>,
    pub cancellation_token: CancellationToken,
}

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: &Task) -> Result<TaskResult>;
    async fn cancel(&self, task_id: &ChangeID) -> Result<()>;
}

pub enum TaskResult {
    Success {
        handoff: HandoffContext,
        artifacts: Vec<PathBuf>,
    },
    Failure {
        error: String,
        partial_progress: Option<HandoffContext>,
    },
}

impl ProductionAgent {
    pub fn spawn(
        id: AgentID,
        capabilities: Vec<String>,
        config: AgentConfig,
        executor: Box<dyn TaskExecutor>,
    ) -> Self {
        let mailbox = AgentActor::spawn(
            id.clone(),
            capabilities.clone(),
            config.max_concurrent_tasks,
        );

        let actor = AgentActor { /* ... */ };

        let state = RwLock::new(AgentState {
            current_tasks: HashMap::new(),
            last_heartbeat: Utc::now(),
            total_completed: 0,
            total_failed: 0,
        });

        let agent = Self {
            id,
            capabilities,
            config,
            actor,
            mailbox,
            executor,
            state,
        };

        // Start background tasks
        agent.start_heartbeat();
        agent.start_timeout_monitor();

        agent
    }

    fn start_heartbeat(&self) {
        let mailbox = self.mailbox.clone();
        let interval = self.config.heartbeat_interval;
        let id = self.id.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                // Send heartbeat
                tracing::debug!("Agent {} sending heartbeat", id);

                // Update last_heartbeat in state
                // (this would be done via message passing in real implementation)
            }
        });
    }

    fn start_timeout_monitor(&self) {
        let state = self.state.clone();
        let timeout = self.config.task_timeout;
        let mailbox = self.mailbox.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;

                let state = state.read().await;
                let now = Utc::now();

                for (task_id, execution) in &state.current_tasks {
                    let elapsed = now - execution.started_at;

                    if elapsed.to_std().unwrap_or_default() > timeout {
                        tracing::warn!(
                            "Task {} has exceeded timeout, cancelling",
                            task_id
                        );

                        let _ = mailbox.send(AgentMessage::CancelTask {
                            task_id: task_id.clone(),
                        }).await;
                    }
                }
            }
        });
    }

    pub async fn claim_task(&self, task: Task) -> Result<()> {
        // Check capacity
        let state = self.state.read().await;
        if state.current_tasks.len() >= self.config.max_concurrent_tasks {
            return Err(anyhow::anyhow!("Agent at capacity"));
        }
        drop(state);

        // Send message to actor
        self.mailbox.send(AgentMessage::AssignTask { task }).await
            .map_err(|e| anyhow::anyhow!("Failed to claim task: {}", e))
    }

    pub async fn execute_task(&self, task: Task) -> Result<TaskResult> {
        let task_id = task.change_id.clone();
        let cancellation_token = CancellationToken::new();

        // Add to current tasks
        {
            let mut state = self.state.write().await;
            state.current_tasks.insert(task_id.clone(), TaskExecution {
                task: task.clone(),
                started_at: Utc::now(),
                saga: None,
                cancellation_token: cancellation_token.clone(),
            });
        }

        // Execute with timeout
        let result = tokio::select! {
            result = self.executor.execute(&task) => result,
            _ = cancellation_token.cancelled() => {
                return Err(anyhow::anyhow!("Task cancelled"));
            }
            _ = tokio::time::sleep(self.config.task_timeout) => {
                return Err(anyhow::anyhow!("Task timeout"));
            }
        };

        // Remove from current tasks
        {
            let mut state = self.state.write().await;
            state.current_tasks.remove(&task_id);

            match &result {
                Ok(_) => state.total_completed += 1,
                Err(_) => state.total_failed += 1,
            }
        }

        result
    }

    pub async fn shutdown(self) -> Result<()> {
        // Send shutdown message
        self.mailbox.send(AgentMessage::Shutdown).await?;

        // Wait for current tasks to complete (with timeout)
        let timeout = Duration::from_secs(30);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let state = self.state.read().await;
            if state.current_tasks.is_empty() {
                break;
            }
            drop(state);

            if tokio::time::Instant::now() >= deadline {
                tracing::warn!("Shutdown timeout, cancelling remaining tasks");

                // Cancel all remaining tasks
                let state = self.state.read().await;
                for execution in state.current_tasks.values() {
                    execution.cancellation_token.cancel();
                }
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }
}
```

---

## 7. MIGRATION PATH: From Current to Production-Ready

### Phase 1: Foundation (Weeks 1-2)
1. ✅ **Complete Current Implementation**
   - Finish Rust port of Go reference
   - Full test coverage for existing patterns
   - Documentation of current architecture

2. **Add Event Sourcing**
   - Implement EventStore with JSONL backend
   - Define TaskEvent enum
   - Integrate event publishing in TaskManager
   - Add event replay capability

### Phase 2: Reliability (Weeks 3-4)
3. **Implement Saga Pattern**
   - Create Saga abstraction
   - Add compensation functions
   - Integrate with task execution
   - Test rollback scenarios

4. **Add Retry Logic**
   - Implement RetryStrategy
   - Add TaskAttempt tracking
   - Build stale assignment detector
   - Create automatic reassignment

### Phase 3: Scalability (Weeks 5-6)
5. **Actor Model for Agents**
   - Implement AgentActor
   - Build AgentSupervisor
   - Add message passing
   - Health check system

6. **CQRS Separation**
   - Create CommandHandler
   - Build QueryHandler
   - Denormalized read models
   - Cache management

### Phase 4: Intelligence (Weeks 7-8)
7. **Load Balancing**
   - Implement LoadBalancer
   - Work stealing protocol
   - Priority queuing
   - Resource-aware assignment

8. **Advanced Orchestration**
   - Parallel task coordination
   - Backpressure control
   - Resource contention handling
   - Performance optimization

### Phase 5: Production Hardening (Weeks 9-10)
9. **Monitoring & Observability**
   - Metrics collection
   - Distributed tracing
   - Dashboard
   - Alerting

10. **Production Testing**
    - Load testing
    - Chaos engineering
    - Failure injection
    - Performance benchmarking

---

## 8. CONCLUSIONS & RECOMMENDATIONS

### Key Strengths of Current Approach
1. ✅ **VCS-Native Design:** Leveraging jj's DAG for dependency tracking is brilliant
2. ✅ **Handoff Context:** Structured agent continuity is well-designed
3. ✅ **Oplog Watching:** Efficient change detection without filesystem polling
4. ✅ **Revset Queries:** Powerful, declarative task queries
5. ✅ **Distributed-First:** Natural support for remote collaboration

### Critical Gaps to Address
1. ❌ **No Failure Recovery:** Must add retry logic and timeout handling
2. ❌ **No Load Balancing:** Needs work stealing and dynamic assignment
3. ❌ **No Resource Management:** Missing capacity planning and contention handling
4. ❌ **Limited Observability:** Needs metrics, tracing, and monitoring
5. ❌ **Manual Coordination:** Requires autonomous agent operation

### Recommended Immediate Actions
1. **Implement Event Sourcing** (2 weeks)
   - Complete audit trail
   - Foundation for CQRS and analytics
   - Enables replay and time-travel debugging

2. **Add Retry Logic** (1 week)
   - Critical for production reliability
   - Automatic recovery from transient failures
   - Configurable retry strategies

3. **Build Agent Actor Model** (2 weeks)
   - Concurrent, isolated execution
   - Message-based coordination
   - Lifecycle management

4. **Create Load Balancer** (1 week)
   - Work stealing
   - Priority-aware assignment
   - Resource-aware scheduling

### Long-Term Vision
This system has the potential to become a **best-in-class VCS-native orchestration platform**. The unique integration with jujutsu provides capabilities that traditional orchestrators (Airflow, Temporal, etc.) cannot match:

- **Version-controlled task state**
- **Natural distributed collaboration**
- **Built-in conflict resolution**
- **Complete audit trail via VCS history**
- **Offline-first operation**

With the recommended patterns (Saga, Actor Model, Event Sourcing, CQRS), this can become a **production-grade multi-agent orchestration system** suitable for complex, distributed workflows.

---

**End of Analysis**
