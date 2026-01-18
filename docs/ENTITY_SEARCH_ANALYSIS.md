# Entity Search & Tracking Analysis for HOX Agent Orchestration

**Analysis Date**: 2026-01-17
**Target System**: jj-beads-rs (HOX) - Rust agent orchestration with Jujutsu VCS integration
**Focus**: Entity management, search capabilities, and agent observability

---

## Executive Summary

HOX uses a **hybrid architecture** combining Jujutsu's native graph database (DAG) with Turso (SQLite) for query optimization and structured metadata. The system tracks four primary entity types through dual storage layers, but **lacks semantic search, full-text search, and comprehensive timeline reconstruction capabilities** needed for robust agent operations.

**Key Findings**:
- Strong graph traversal via Jujutsu revsets
- Limited semantic/intent-based search
- No full-text search across task descriptions
- Incomplete audit trail for human observability
- Missing embedding-based discovery for agents

---

## 1. Entity Inventory

### Primary Entities

| Entity | Storage | Identification | Lifecycle Tracking |
|--------|---------|----------------|-------------------|
| **Tasks (Issues)** | Turso DB + JSON files (`tasks/*.json`) | `id` (string), `content_hash` (SHA256) | `created_at`, `updated_at`, `closed_at`, `deleted_at` |
| **Dependencies** | Turso DB + JSON files (`deps/*.json`) | Composite: `(from_id, to_id, dep_type)` | `created_at` only |
| **Changes** | Jujutsu VCS DAG | `change_id` (jj native) | Operation log (jj oplog) |
| **Agents** | Bookmarks in jj (`agent-{id}/*`) | Agent ID in bookmark namespace | `last_activity` timestamp |

### Extended Entities (from Issue struct)

The `Issue` type in bd-core/types.rs contains **170+ fields** supporting advanced orchestration:

#### Agent Identity Fields
```rust
pub hook_bead: Option<String>,        // Hook script reference
pub role_bead: Option<String>,        // Role definition reference
pub agent_state: Option<AgentState>,  // idle, running, working, stuck, done, dead
pub last_activity: Option<DateTime<Utc>>,
pub role_type: Option<String>,        // Agent role classification
pub rig: Option<String>,              // Agent execution environment
```

#### Async Coordination Primitives
```rust
// Gates (fanout coordination)
pub await_type: Option<String>,
pub await_id: Option<String>,
pub timeout: Option<i64>,
pub waiters: Option<Vec<String>>,

// Slots (exclusive access)
pub holder: Option<String>,
```

#### Molecule Coordination (Swarm)
```rust
pub mol_type: Option<MolType>,        // Swarm, Patrol, Work
pub bonded_from: Option<Vec<BondRef>>, // Compound molecule lineage
```

#### HOP Entity Tracking (Human-Agent-Org Provenance)
```rust
pub creator: Option<EntityRef>,       // Who created this
pub validations: Option<Vec<Validation>>, // Who approved/validated
```

#### Event System
```rust
pub event_kind: Option<String>,
pub actor: Option<String>,
pub target: Option<String>,
pub payload: Option<String>,
```

### Relationship Types

**From `DependencyType` enum** (16 types):

**Workflow types** (affect ready work calculation):
- `Blocks` - Hard blocker
- `ParentChild` - Hierarchical dependency
- `ConditionalBlocks` - Conditional blocker
- `WaitsFor` - Async gate dependency

**Association types**:
- `Related` - Soft relationship
- `DiscoveredFrom` - Investigation trail

**Graph link types**:
- `RepliesTo` - Thread relationship
- `RelatesTo` - Cross-reference
- `Duplicates` - Duplicate tracking
- `Supersedes` - Replacement tracking

**HOP types** (entity tracking):
- `AuthoredBy` - Authorship
- `AssignedTo` - Assignment
- `ApprovedBy` - Approval

**Convoy tracking**:
- `Tracks` - Multi-entity tracking

**Reference types**:
- `Until` - Temporal dependency
- `CausedBy` - Causality tracking
- `Validates` - Validation relationship

---

## 2. Current Search Capabilities

### A. Database Layer (Turso/SQLite) - `bd-storage/db.rs`

**Query Methods**:

```rust
// Basic retrieval
get_task_by_id(id: &str) -> Result<TaskFile>

// Filtered listing
list_tasks(filter: ListTasksFilter) -> Result<Vec<TaskFile>>
  - Filter by: status, type, priority, assigned_agent, tag
  - Pagination: limit, offset
  - Ordering: priority ASC, created_at ASC

// Ready work (core orchestration query)
get_ready_tasks(opts: ReadyTasksOptions) -> Result<Vec<TaskFile>>
  - status = 'open'
  - is_blocked = 0
  - defer_until <= now (unless include_deferred)
  - Optional: assigned_agent filter

// Dependency queries
get_deps_for_task(task_id: &str) -> Result<Vec<DepFile>>
get_blocking_tasks(task_id: &str) -> Result<Vec<TaskFile>>
  - Transitive closure via BFS
  - Finds all tasks blocking given task

// Statistics
get_task_count() -> Result<i64>
get_dep_count() -> Result<i64>
```

**Indexes**:
```sql
idx_tasks_status          -- Single column
idx_tasks_priority        -- Single column
idx_tasks_assigned        -- Single column
idx_tasks_defer           -- Single column
idx_tasks_blocked         -- Single column
idx_tasks_type            -- Single column
idx_tasks_ready_work      -- Composite: (status, is_blocked, defer_until, priority)
idx_deps_to               -- Dependency target
idx_deps_from             -- Dependency source
idx_deps_type             -- Dependency type
```

**Tag Search**: Uses JSON LIKE query (inefficient):
```rust
if let Some(tag) = &filter.tag {
    conditions.push("t.tags LIKE ?");
    params_vec.push(format!("%\"{}\"%%", tag).into());
}
```

**LIMITATION**: No full-text search on title, description, notes, acceptance_criteria, design, or any text fields.

### B. Revset Layer (Jujutsu) - `bd-orchestrator/revsets.rs`

**Powerful graph traversal queries**:

```rust
// Status-based
ready_tasks()          // heads(bookmarks(glob:"task-*")) - conflicts()
blocked_tasks()        // bookmarks(glob:"task-*") & descendants(mutable())
conflicting_tasks()    // bookmarks(glob:"task-*") & conflicts()
in_progress_tasks()    // bookmarks(glob:"agent-*/*")

// Agent-based
agent_tasks(agent_id)  // bookmarks(glob:"agent-{id}/*")
unassigned_tasks()     // bookmarks(glob:"task-*") - bookmarks(glob:"agent-*/*")

// Graph traversal
task_dependencies(change_id)  // ancestors({id}) & mutable()
dependent_tasks(change_id)    // descendants({id}) - {id}

// Visualization
build_dependency_graph() // Complete graph with nodes + edges
```

**Query Result**:
```rust
pub struct QueryResult {
    pub change_id: String,
    pub bookmark: String,
    pub description: String,    // FIRST LINE ONLY
    pub author: String,
    pub timestamp: String,
}
```

**LIMITATION**:
- Only searches first line of description
- No full description search
- No semantic similarity queries
- No intent-based search ("find tasks about authentication")

### C. File I/O Layer - `bd-storage/task_io.rs`, `dep_io.rs`

**Basic file operations only**:
```rust
read_task_file(path)         // Read single JSON file
write_task_file(tasks_dir, task)
read_all_task_files(tasks_dir)
delete_task_file(tasks_dir, id)

read_dep_file(path)
write_dep_file(deps_dir, dep)
read_all_dep_files(deps_dir)
delete_dep_file(deps_dir, from, dep_type, to)
find_deps_for_task(deps_dir, task_id) // Filename-based search
```

**Filename Convention**:
- Tasks: `{id}.json`
- Dependencies: `{from}--{type}--{to}.json`

**LIMITATION**: No search across file contents beyond basic filename parsing.

---

## 3. Tracking Mechanisms

### A. Entity Lifecycle Tracking

**Task Lifecycle**:
```rust
created_at: DateTime<Utc>   // Birth
updated_at: DateTime<Utc>   // Last modification
closed_at: Option<DateTime<Utc>>  // Completion
deleted_at: Option<DateTime<Utc>> // Soft delete (tombstone)
```

**State Transitions**:
```rust
pub enum Status {
    Open,
    InProgress,
    Blocked,
    Deferred,
    Closed,
    Tombstone,  // Soft-deleted
    Pinned,
    Hooked,     // Automation hook attached
}
```

**Agent State Transitions**:
```rust
pub enum AgentState {
    Idle,      // Waiting for work
    Spawning,  // Being created
    Running,   // Executing
    Working,   // Actively modifying tasks
    Stuck,     // Blocked/needs intervention
    Done,      // Completed successfully
    Stopped,   // Intentionally stopped
    Dead,      // Crashed/failed
}
```

**Validation System**:
```rust
pub struct Validation {
    pub validator: Option<EntityRef>,  // Who validated
    pub outcome: String,               // accepted, rejected, revision_requested
    pub timestamp: DateTime<Utc>,
    pub score: Option<f32>,
}
```

### B. Audit Trail

**Limited Event System** (in bd-core/types.rs):
```rust
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

pub struct Event {
    pub id: i64,
    pub issue_id: String,
    pub event_type: EventType,
    pub actor: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

**Jujutsu Operation Log**:
- Every jj operation is recorded in the oplog
- Can reconstruct any historical state
- Shows who did what, when
- **NOT currently integrated with search layer**

### C. Content Integrity

**Content Hashing** (for deduplication/conflict detection):
```rust
pub fn compute_content_hash(&self) -> String {
    // SHA256 hash of all substantive fields
    // Excludes: id, timestamps, compaction metadata
    // Includes: title, description, status, priority, all content fields
}
```

**Purpose**: Detect identical issues across repositories in distributed scenarios.

---

## 4. Gaps for Agent Use

### Critical Missing Capabilities

#### 1. **Semantic Search**
**Gap**: No way to find tasks by meaning, intent, or context.

**Examples that don't work**:
- "Find all authentication-related tasks" (semantic similarity)
- "Show me tasks about fixing bugs in the payment system" (intent-based)
- "What tasks are similar to this one?" (content similarity)

**Impact**:
- Agents can't discover relevant past work
- Can't find duplicate efforts
- Poor task recommendation/assignment
- Manual tag curation required

#### 2. **Full-Text Search**
**Gap**: Can only search by exact tag match or ID.

**Missing searches**:
- Search across: title, description, notes, acceptance_criteria, design
- Search across change descriptions in jj
- Fuzzy matching
- Wildcard queries

**Impact**:
- Can't find tasks by keywords in descriptions
- Can't search meeting notes, design docs embedded in tasks
- Poor human observability

#### 3. **Timeline Reconstruction**
**Gap**: No unified timeline view across task events, agent actions, and VCS operations.

**What's missing**:
- "Show me everything that happened to task-123" (events + changes + comments)
- "What did agent-42 do yesterday?" (activity timeline)
- "How did this task evolve?" (state transitions + content changes)

**Partial capabilities**:
- Jujutsu oplog tracks VCS operations
- Event table tracks some state changes
- **NOT CONNECTED**: No way to correlate them

**Impact**:
- Hard to debug agent behavior
- Poor human observability
- Can't do post-mortem analysis
- No replay capability

#### 4. **Cross-Entity Search**
**Gap**: Can't search across multiple entity types simultaneously.

**Examples that don't work**:
- "Find all tasks, dependencies, and agents related to project X"
- "Show me everything touching file src/auth.rs"
- "What's the blast radius of changing this API?"

**Impact**:
- No impact analysis
- Poor change planning
- Can't assess risk

#### 5. **Provenance Tracking**
**Gap**: HOP (Human-Org-Platform) entity references exist but aren't searchable.

**Partial implementation**:
```rust
pub creator: Option<EntityRef>,  // WHO created this
pub validations: Option<Vec<Validation>>,  // WHO validated this
```

**But can't query**:
- "Show all tasks created by human:alice"
- "Show all tasks validated by agent:senior-reviewer"
- "Show work attribution for team:platform"

**Impact**:
- No work attribution
- Can't track AI vs human contributions
- Poor compliance/audit capability

#### 6. **Dependency Impact Analysis**
**Gap**: Can find blocking tasks but not full impact analysis.

**Current**: `get_blocking_tasks(id)` - transitive blockers
**Missing**:
- "If I close this task, what becomes unblocked?" (reverse transitive)
- "What's the critical path?" (longest dependency chain)
- "What's the blast radius?" (all affected tasks, not just blockers)

#### 7. **Agent Context Retrieval**
**Gap**: Agents need rich context to resume work.

**Handoff Context Needed**:
```rust
// What agent needs when resuming task
struct HandoffContext {
    task_description: String,       // AVAILABLE
    recent_changes: Vec<Change>,    // AVAILABLE (via jj diff)
    related_tasks: Vec<Issue>,      // NOT AVAILABLE (no semantic search)
    past_decisions: Vec<Comment>,   // AVAILABLE but not searchable
    file_context: Vec<FileDiff>,    // AVAILABLE
    similar_completed: Vec<Issue>,  // NOT AVAILABLE (no similarity search)
}
```

**Impact**:
- Cold start problem for new agents
- Context loss between handoffs
- Duplicate work
- Poor decision quality

---

## 5. Observability Integration

### Current Observability Capabilities

#### What Works Well

**1. VCS-Native Observability**:
```bash
# See all task changes
jj log -r 'bookmarks(glob:"task-*")'

# See agent's work
jj log -r 'bookmarks(glob:"agent-42/*")'

# See what changed
jj diff -r 'agent-42/task-xyz'

# See operation history
jj op log
```

**2. Graph Visualization**:
```rust
pub struct DependencyGraph {
    pub nodes: Vec<GraphNode>,    // Tasks with status
    pub edges: Vec<GraphEdge>,    // Dependency relationships
}
```
- Can generate DOT/Graphviz
- Shows task status (Ready, Blocked, InProgress, Conflict)
- Shows dependency types (Blocks, ParentChild)

**3. Blocked Task Cache**:
```rust
refresh_blocked_cache() -> Result<()>
```
- Precomputes transitive closure of blocking relationships
- Stores in `blocked_cache` table with JSON blocker list
- Updates `is_blocked` flag for fast filtering

#### Critical Gaps for Human Observability

**1. No Unified Activity Stream**
```
MISSING: Interleaved timeline of:
- Task state changes (Event table)
- Agent actions (jj oplog)
- File modifications (jj changes)
- Comments/discussions (Comment table)
- Dependency updates (Dependency table)
```

**2. No Agent Telemetry Dashboard**
```
MISSING:
- Agent uptime/health
- Task completion rates
- Error rates per agent
- Context handoff success rate
- Agent efficiency metrics
```

**3. No Search History/Audit**
```
MISSING:
- Who searched for what, when
- What agents discovered what tasks
- Query performance metrics
- Search result relevance feedback
```

**4. No Provenance Visualization**
```
AVAILABLE: EntityRef, Validation structs
MISSING:
- UI to show "created by X, validated by Y"
- Contribution graphs
- Work attribution reports
- Chain of custody for decisions
```

### Human-Friendly Observability Needs

**1. Natural Language Timeline**:
```
# What humans want to see:
"2026-01-17 14:32 - Agent-42 started work on task-auth-refactor"
"2026-01-17 14:45 - Agent-42 modified src/auth.rs, src/middleware.rs"
"2026-01-17 15:01 - Agent-42 marked task blocked, waiting on task-db-migration"
"2026-01-17 15:15 - Agent-99 completed task-db-migration"
"2026-01-17 15:16 - Agent-42 resumed work on task-auth-refactor"
```

**2. Contextual Drill-Down**:
```
# Click on any event to see:
- Full jj diff
- Related tasks (via dependencies)
- Agent state at that moment
- Files touched
- Why decision was made (from description/comments)
```

**3. Multi-Agent Coordination View**:
```
# See concurrent work:
┌─────────────────────────────────────────────┐
│ Agent-42: Working on task-auth-refactor     │
│ Agent-99: Working on task-db-migration      │
│ Agent-17: Blocked on task-auth-refactor     │
└─────────────────────────────────────────────┘
```

**4. Impact Analysis UI**:
```
# Before closing/changing a task:
"Closing this will unblock: 3 tasks, 2 agents"
"Changing priority will affect: 5 dependent tasks"
"This task blocks: 1 epic, 4 features, 2 bugs"
```

---

## 6. Architecture Recommendations

### Tier 1: Quick Wins (High Impact, Low Effort)

#### 1.1 Full-Text Search (SQLite FTS5)

**Implementation**:
```sql
CREATE VIRTUAL TABLE tasks_fts USING fts5(
    id UNINDEXED,
    title,
    description,
    notes,
    acceptance_criteria,
    design,
    tokenize='porter unicode61'
);

-- Trigger to keep FTS in sync
CREATE TRIGGER tasks_ai AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts(id, title, description, notes, acceptance_criteria, design)
    VALUES (new.id, new.title, new.description, new.notes, new.acceptance_criteria, new.design);
END;
```

**Query Interface**:
```rust
pub async fn search_tasks_fulltext(&self, query: &str, limit: usize) -> Result<Vec<TaskFile>> {
    let sql = r#"
        SELECT t.* FROM tasks t
        JOIN tasks_fts fts ON t.id = fts.id
        WHERE tasks_fts MATCH ?
        ORDER BY rank
        LIMIT ?
    "#;
    // Execute query...
}
```

**Benefit**: Enables keyword search across all task text fields immediately.

#### 1.2 Unified Activity Timeline

**Schema**:
```sql
CREATE TABLE activity_stream (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    event_type TEXT NOT NULL,  -- task_created, agent_action, file_modified, etc.
    actor TEXT,                 -- agent-42, human:alice, etc.
    target_id TEXT,             -- task ID, change ID, etc.
    target_type TEXT,           -- task, change, dependency, etc.
    action TEXT NOT NULL,       -- created, updated, blocked, etc.
    details TEXT,               -- JSON blob with type-specific data
    source TEXT,                -- db, jj_oplog, file_watcher, etc.

    INDEX idx_activity_timestamp (timestamp DESC),
    INDEX idx_activity_actor (actor),
    INDEX idx_activity_target (target_id)
);
```

**Population Sources**:
1. Task events (from Event table)
2. Jujutsu oplog (parse and insert)
3. File watcher events (from daemon)
4. Agent state transitions

**Query Interface**:
```rust
pub async fn get_activity_timeline(
    &self,
    filter: ActivityFilter  // by actor, target, type, time range
) -> Result<Vec<ActivityEvent>>
```

**Benefit**: Single source for "what happened" across all entity types.

#### 1.3 Enhanced Revset Queries

**Add missing queries**:
```rust
// Impact analysis
pub async fn unblocks_what(&self, change_id: &str) -> Result<Vec<String>> {
    // Tasks that would become unblocked if this completes
    let revset = format!("descendants({}) - {}", change_id, change_id);
    self.query_change_ids(&revset).await
}

// Critical path
pub async fn critical_path(&self, from: &str, to: &str) -> Result<Vec<Vec<String>>> {
    // All paths from `from` to `to` in dependency graph
    // Return sorted by length (longest = critical path)
}

// Blast radius
pub async fn blast_radius(&self, change_id: &str) -> Result<BlastRadius> {
    // All tasks, agents, files affected by this change
}
```

### Tier 2: Semantic Search (Medium Effort, High Value)

#### 2.1 Embedding-Based Search

**Architecture**:
```
┌─────────────────────────────────────────────────────────────────┐
│                     Task Embedding Pipeline                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Task Created/Updated                                           │
│         │                                                        │
│         ▼                                                        │
│  Generate Embedding                                             │
│    - title + description + notes → text                         │
│    - Use: OpenAI text-embedding-3-small (1536 dims)            │
│    - Or: sentence-transformers/all-MiniLM-L6-v2 (384 dims)     │
│         │                                                        │
│         ▼                                                        │
│  Store in Vector DB                                             │
│    - Option A: SQLite-vec extension (if available)             │
│    - Option B: Separate Qdrant/Meilisearch instance            │
│    - Option C: In-memory HNSW index (usearch-rs)               │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**Schema (if using SQLite-vec)**:
```sql
-- Requires sqlite-vec extension
CREATE TABLE task_embeddings (
    task_id TEXT PRIMARY KEY,
    embedding BLOB NOT NULL,     -- F32 vector
    dims INTEGER NOT NULL,
    model TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);

-- Build vector index
CREATE INDEX idx_task_embeddings_vec ON task_embeddings(embedding);
```

**Query Interface**:
```rust
pub async fn search_tasks_semantic(
    &self,
    query: &str,           // Natural language query
    limit: usize,
    threshold: f32         // Similarity threshold (0.0-1.0)
) -> Result<Vec<(TaskFile, f32)>> {
    // 1. Generate query embedding
    let query_vec = self.embedding_service.embed(query).await?;

    // 2. Vector similarity search
    let similar_ids = self.vector_db.search(query_vec, limit, threshold).await?;

    // 3. Hydrate full tasks from DB
    let tasks = self.get_tasks_by_ids(&similar_ids).await?;

    Ok(tasks)
}
```

**Benefit**:
- "Find tasks about authentication" works
- Discover similar/duplicate tasks
- Better task recommendation for agents
- Intent-based search

#### 2.2 Hybrid Search (Full-Text + Semantic)

**Combine FTS and vector search**:
```rust
pub async fn search_tasks_hybrid(
    &self,
    query: &str,
    limit: usize
) -> Result<Vec<TaskFile>> {
    // 1. FTS search (fast, exact keyword match)
    let fts_results = self.search_tasks_fulltext(query, limit * 2).await?;

    // 2. Semantic search (slower, intent-based)
    let semantic_results = self.search_tasks_semantic(query, limit * 2, 0.7).await?;

    // 3. Merge + rerank
    let merged = self.merge_and_rerank(fts_results, semantic_results, limit);

    Ok(merged)
}
```

**Reranking Algorithm**:
```rust
// RRF (Reciprocal Rank Fusion)
fn merge_and_rerank(fts: Vec<T>, semantic: Vec<T>, limit: usize) -> Vec<T> {
    let mut scores: HashMap<String, f32> = HashMap::new();

    for (rank, task) in fts.iter().enumerate() {
        scores.entry(task.id.clone())
            .or_insert(0.0)
            .add_assign(1.0 / (rank as f32 + 60.0));
    }

    for (rank, (task, sim)) in semantic.iter().enumerate() {
        scores.entry(task.id.clone())
            .or_insert(0.0)
            .add_assign(sim * 0.5 + 1.0 / (rank as f32 + 60.0));
    }

    // Sort by combined score, take top N
}
```

### Tier 3: Advanced Observability (Higher Effort)

#### 3.1 Agent Telemetry System

**Metrics Collection**:
```rust
pub struct AgentMetrics {
    agent_id: String,

    // Lifecycle
    spawned_at: DateTime<Utc>,
    last_heartbeat: DateTime<Utc>,
    state: AgentState,

    // Work tracking
    tasks_completed: u32,
    tasks_failed: u32,
    avg_task_duration: Duration,

    // Context tracking
    tokens_consumed: u64,
    handoffs_received: u32,
    handoffs_given: u32,

    // Error tracking
    errors: Vec<AgentError>,
    stuck_count: u32,
    recovery_count: u32,
}
```

**Storage**:
```sql
CREATE TABLE agent_metrics (
    id INTEGER PRIMARY KEY,
    agent_id TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    metric_value REAL NOT NULL,
    timestamp TEXT NOT NULL,
    metadata TEXT,  -- JSON

    INDEX idx_agent_metrics_agent (agent_id),
    INDEX idx_agent_metrics_time (timestamp DESC)
);
```

**Dashboard Queries**:
```rust
// Agent health
pub async fn get_agent_health(&self, agent_id: &str) -> Result<AgentHealth>

// Agent efficiency
pub async fn get_agent_efficiency(&self, agent_id: &str, window: Duration) -> Result<f32>

// Fleet overview
pub async fn get_fleet_status(&self) -> Result<FleetStatus>
```

#### 3.2 Provenance Query API

**Index HOP entities**:
```sql
CREATE TABLE entity_refs (
    id INTEGER PRIMARY KEY,
    entity_uri TEXT NOT NULL,     -- entity://hop/platform/org/id
    entity_name TEXT,
    entity_platform TEXT,
    entity_org TEXT,
    entity_id TEXT,

    ref_type TEXT NOT NULL,       -- creator, validator, author, assignee
    target_type TEXT NOT NULL,    -- task, dependency, comment
    target_id TEXT NOT NULL,

    timestamp TEXT NOT NULL,

    INDEX idx_entity_refs_uri (entity_uri),
    INDEX idx_entity_refs_target (target_type, target_id),
    INDEX idx_entity_refs_type (ref_type)
);
```

**Query Interface**:
```rust
pub async fn find_by_creator(&self, entity_ref: &EntityRef) -> Result<Vec<Issue>>
pub async fn find_by_validator(&self, entity_ref: &EntityRef) -> Result<Vec<Issue>>
pub async fn get_attribution(&self, task_id: &str) -> Result<Attribution>
```

#### 3.3 Graph Query Language

**Add a mini query language for complex graph queries**:
```rust
// Example queries:
"tasks where creator.platform = 'claude' and status = 'open'"
"tasks with dependencies.type = 'blocks' and count(dependencies) > 3"
"agents where state = 'stuck' and last_activity < '2h ago'"
```

**Parser + Executor**:
```rust
pub struct GraphQuery {
    entity_type: EntityType,    // tasks, agents, dependencies
    filters: Vec<Filter>,        // Field comparisons
    traversals: Vec<Traversal>, // Graph navigation
    aggregations: Vec<Agg>,     // count, sum, avg, etc.
}

pub async fn execute_graph_query(&self, query: &str) -> Result<QueryResult>
```

### Tier 4: Advanced Features (Future)

#### 4.1 Time-Travel Queries

**Reconstruct any historical state**:
```rust
pub async fn get_task_at_time(&self, task_id: &str, timestamp: DateTime<Utc>) -> Result<TaskFile> {
    // Use jj oplog + event table to reconstruct
}

pub async fn get_agent_activity_between(
    &self,
    agent_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>
) -> Result<Vec<ActivityEvent>>
```

#### 4.2 Anomaly Detection

**ML-based detection**:
```rust
pub async fn detect_anomalies(&self) -> Result<Vec<Anomaly>> {
    // - Tasks stuck in same state too long
    // - Agents with abnormal failure rates
    // - Dependency cycles introduced
    // - Unusual activity patterns
}
```

#### 4.3 Predictive Analytics

```rust
pub async fn predict_completion_time(&self, task_id: &str) -> Result<Duration>
pub async fn recommend_next_tasks(&self, agent_id: &str) -> Result<Vec<TaskFile>>
pub async fn estimate_blast_radius(&self, change_id: &str) -> Result<ImpactEstimate>
```

---

## 7. Implementation Roadmap

### Phase 1: Foundation (Week 1-2)
- [ ] Add FTS5 full-text search to Turso DB
- [ ] Create unified activity stream table
- [ ] Implement activity stream population from existing sources
- [ ] Add enhanced revset queries (unblocks_what, blast_radius)

**Deliverable**: Keyword search + basic timeline work

### Phase 2: Semantic Search (Week 3-4)
- [ ] Choose embedding model (recommend: text-embedding-3-small)
- [ ] Add embedding generation pipeline
- [ ] Choose vector storage (recommend: usearch-rs for in-memory, or sqlite-vec)
- [ ] Implement semantic search + hybrid search
- [ ] Build task similarity API

**Deliverable**: Intent-based search, duplicate detection

### Phase 3: Observability (Week 5-6)
- [ ] Design agent metrics schema
- [ ] Implement agent telemetry collection
- [ ] Build provenance index for HOP entities
- [ ] Create dashboard queries
- [ ] Build activity timeline API

**Deliverable**: Agent health monitoring, work attribution

### Phase 4: Advanced Queries (Week 7-8)
- [ ] Implement time-travel queries
- [ ] Build graph query language parser
- [ ] Add cross-entity search
- [ ] Implement impact analysis
- [ ] Create recommendation engine

**Deliverable**: Power user queries, advanced agent capabilities

---

## 8. Technology Choices

### Embedding Service
**Recommendation**: OpenAI `text-embedding-3-small`
- **Pros**: 1536 dims, good quality, fast, affordable
- **Cons**: API dependency, latency, cost at scale
- **Alternative**: `sentence-transformers/all-MiniLM-L6-v2` (local, 384 dims, free)

### Vector Database
**Recommendation**: `usearch` (Rust crate) for in-memory
- **Pros**: Fast, no external deps, good for <100K vectors
- **Cons**: RAM usage, no persistence (need to rebuild on restart)
- **Alternative**: `qdrant` (separate service, production-grade, persistent)

### Full-Text Search
**Recommendation**: SQLite FTS5 (already have SQLite/Turso)
- **Pros**: Zero dependencies, fast, well-tested
- **Cons**: Less sophisticated than Elasticsearch
- **Alternative**: `tantivy` (Rust native, Lucene-like, overkill for this)

### Graph Query
**Recommendation**: Build custom DSL on top of revsets
- **Pros**: Leverages jj's power, type-safe
- **Cons**: Development effort
- **Alternative**: Cypher subset (Neo4j-like, but requires parser)

---

## 9. Security & Privacy Considerations

### Embedding Privacy
- **Risk**: Embeddings sent to OpenAI API expose task content
- **Mitigation**:
  - Use local embedding models for sensitive data
  - Or: PII scrubbing before embedding
  - Or: Self-hosted embedding service

### Search Audit
- **Risk**: No record of who searched for what
- **Mitigation**: Add search_history table with actor, query, timestamp

### Access Control
- **Risk**: Full-text and semantic search bypass any future access control
- **Mitigation**: Filter results by actor's permissions before returning

---

## 10. Performance Estimates

### Full-Text Search (FTS5)
- **Index size**: ~2-3x task data size
- **Query latency**: <10ms for most queries
- **Index update**: <1ms per task

### Semantic Search (usearch, 1536 dims)
- **Index size**: ~6KB per task (1536 dims × 4 bytes)
- **Query latency**: <50ms for 10K tasks, <500ms for 100K tasks
- **Index build**: ~1-2s for 10K tasks

### Activity Stream
- **Growth rate**: ~10-100 events per task
- **Query latency**: <20ms with proper indexes
- **Retention**: Partition by month, archive after 6 months

---

## Conclusion

HOX has a **solid foundation** for entity tracking via the hybrid Jujutsu + Turso architecture, but significant gaps exist for agent-friendly search and human observability:

**Strengths**:
- Excellent graph traversal via revsets
- Strong lifecycle tracking
- Content-addressed deduplication
- Rich entity model (170+ fields)

**Critical Gaps**:
- No semantic/intent-based search
- Limited full-text search
- No unified timeline
- Poor provenance tracking
- Incomplete observability for humans

**Recommended Priority**:
1. **Full-text search** (FTS5) - immediate value
2. **Activity timeline** - enables observability
3. **Semantic search** - unlocks agent intelligence
4. **Provenance index** - enables compliance/attribution
5. **Advanced queries** - power user features

**Estimated Effort**: 6-8 weeks for Phases 1-3 (foundation + semantic search + observability)

**Expected Impact**:
- 10x improvement in task discovery
- 5x reduction in duplicate work
- Near-complete timeline reconstruction
- Production-ready observability for multi-agent systems
