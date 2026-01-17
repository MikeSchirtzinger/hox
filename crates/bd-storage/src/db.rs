//! Database layer for jj-beads-rs using Turso.
//!
//! This module provides Turso database integration for jj-beads-rs,
//! implementing the query cache layer for the jj-turso architecture.
//!
//! Architecture:
//!   - Database file: .beads/turso.db
//!   - WAL mode: Write-Ahead Logging for concurrent reads during writes
//!   - Schema: tasks, deps, blocked_cache tables
//!   - Indexes: Optimized for ready work queries (status, priority, defer_until)

use bd_core::{DepFile, TaskFile};
use chrono::{DateTime, Utc};
use turso::{params, Builder, Connection};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

/// Database connection wrapper for Turso
pub struct Database {
    conn: Connection,
    path: String,
}

/// Database errors
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("turso error: {0}")]
    Turso(#[from] turso::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("core error: {0}")]
    Core(#[from] bd_core::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Options for querying ready tasks
#[derive(Debug, Clone, Default)]
pub struct ReadyTasksOptions {
    /// Include tasks that are deferred but otherwise ready
    pub include_deferred: bool,

    /// Limit the number of results (0 = no limit)
    pub limit: usize,

    /// Filter to tasks assigned to a specific agent (None = all)
    pub assigned_agent: Option<String>,
}

/// Filter options for listing tasks
#[derive(Debug, Clone, Default)]
pub struct ListTasksFilter {
    /// Filter by task status (None = all statuses)
    pub status: Option<String>,

    /// Filter by task type (None = all types)
    pub task_type: Option<String>,

    /// Filter by exact priority (None = all priorities)
    pub priority: Option<i32>,

    /// Filter by assigned agent (None = all agents)
    pub assigned_agent: Option<String>,

    /// Filter by tag (None = all tags)
    pub tag: Option<String>,

    /// Limit the number of results (0 = no limit)
    pub limit: usize,

    /// Skip the first N results (for pagination)
    pub offset: usize,
}

impl Database {
    /// Open creates a new database connection at the specified path using Turso.
    ///
    /// The database is opened in embedded mode with WAL for concurrent reads.
    /// If the database doesn't exist, it will be created along with the schema.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use bd_storage::db::Database;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let db = Database::open(".beads/turso.db").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Ensure parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Open database using Turso builder with file URL
        let db = Builder::new_local(&path_str).build().await?;
        let conn = db.connect()?;

        // Configure pragmas for WAL mode and performance
        // Use query() for PRAGMA statements as they may return results
        let _ = conn.query("PRAGMA journal_mode=WAL", params![]).await?;
        let _ = conn.query("PRAGMA busy_timeout=5000", params![]).await?;
        let _ = conn.query("PRAGMA foreign_keys=ON", params![]).await?;

        Ok(Database {
            conn,
            path: path_str,
        })
    }

    /// Close closes the database connection
    pub async fn close(self) -> Result<()> {
        // Turso Connection doesn't require explicit close in Rust
        // Drop handles cleanup automatically
        Ok(())
    }

    /// Returns the database file path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// InitSchema creates the database schema if it doesn't exist.
    ///
    /// This creates the tasks, deps, and blocked_cache tables along with
    /// necessary indexes for fast queries. This is idempotent - safe to call
    /// multiple times.
    pub async fn init_schema(&self) -> Result<()> {
        let statements = vec![
            // Tasks table
            r#"CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                priority INTEGER NOT NULL DEFAULT 2,
                assigned_agent TEXT,
                description TEXT,
                tags TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                due_at TEXT,
                defer_until TEXT,
                is_blocked INTEGER NOT NULL DEFAULT 0,
                blocking_count INTEGER NOT NULL DEFAULT 0
            )"#,
            // Dependencies table
            r#"CREATE TABLE IF NOT EXISTS deps (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (from_id, to_id, type),
                FOREIGN KEY (from_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (to_id) REFERENCES tasks(id) ON DELETE CASCADE
            )"#,
            // Blocked cache table
            r#"CREATE TABLE IF NOT EXISTS blocked_cache (
                task_id TEXT PRIMARY KEY,
                blocked_by TEXT,
                computed_at TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            )"#,
            // Indexes for tasks
            "CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_priority ON tasks(priority)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_assigned ON tasks(assigned_agent)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_defer ON tasks(defer_until)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_blocked ON tasks(is_blocked)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_type ON tasks(type)",
            "CREATE INDEX IF NOT EXISTS idx_tasks_ready_work ON tasks(status, is_blocked, defer_until, priority)",
            // Indexes for deps
            "CREATE INDEX IF NOT EXISTS idx_deps_to ON deps(to_id)",
            "CREATE INDEX IF NOT EXISTS idx_deps_from ON deps(from_id)",
            "CREATE INDEX IF NOT EXISTS idx_deps_type ON deps(type)",
        ];

        for stmt in statements {
            self.conn.execute(stmt, params![]).await?;
        }

        Ok(())
    }

    /// UpsertTask inserts or updates a task in the database.
    ///
    /// If a task with the same ID exists, it is updated.
    /// Tags are stored as a JSON array string.
    pub async fn upsert_task(&self, task: &TaskFile) -> Result<()> {
        task.validate()?;

        // Serialize tags to JSON
        let tags_json = serde_json::to_string(&task.tags)?;

        let query = r#"
            INSERT INTO tasks (
                id, title, description, type, status, priority,
                assigned_agent, tags, created_at, updated_at,
                due_at, defer_until, is_blocked, blocking_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                description = excluded.description,
                type = excluded.type,
                status = excluded.status,
                priority = excluded.priority,
                assigned_agent = excluded.assigned_agent,
                tags = excluded.tags,
                updated_at = excluded.updated_at,
                due_at = excluded.due_at,
                defer_until = excluded.defer_until
        "#;

        self.conn
            .execute(
                query,
                params![
                    task.id.clone(),
                    task.title.clone(),
                    task.description.clone(),
                    task.task_type.clone(),
                    task.status.clone(),
                    task.priority,
                    task.assigned_agent.clone(),
                    tags_json,
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                    task.due_at.map(|dt| dt.to_rfc3339()),
                    task.defer_until.map(|dt| dt.to_rfc3339()),
                ],
            )
            .await?;

        Ok(())
    }

    /// DeleteTask removes a task from the database.
    ///
    /// This also cascades to remove dependencies and blocked cache entries.
    /// Returns Ok if the task doesn't exist (idempotent).
    pub async fn delete_task(&self, task_id: &str) -> Result<()> {
        let query = "DELETE FROM tasks WHERE id = ?";
        self.conn.execute(query, params![task_id]).await?;
        Ok(())
    }

    /// GetTaskByID retrieves a single task by ID.
    /// Returns TaskNotFound error if the task is not found.
    pub async fn get_task_by_id(&self, id: &str) -> Result<TaskFile> {
        let query = r#"
            SELECT id, title, description, type, status, priority,
                   assigned_agent, tags, created_at, updated_at,
                   due_at, defer_until
            FROM tasks
            WHERE id = ?
        "#;

        let mut rows = self.conn.query(query, params![id]).await?;

        if let Some(row) = rows.next().await? {
            Ok(parse_task_row(&row)?)
        } else {
            Err(DbError::TaskNotFound(id.to_string()))
        }
    }

    /// ListTasks retrieves tasks matching the given filters.
    /// Results are ordered by priority ASC, then created_at ASC.
    pub async fn list_tasks(&self, filter: ListTasksFilter) -> Result<Vec<TaskFile>> {
        let mut conditions = Vec::new();
        let mut params_vec: Vec<turso::Value> = Vec::new();

        if let Some(status) = &filter.status {
            conditions.push("t.status = ?");
            params_vec.push(status.clone().into());
        }

        if let Some(task_type) = &filter.task_type {
            conditions.push("t.type = ?");
            params_vec.push(task_type.clone().into());
        }

        if let Some(priority) = filter.priority {
            conditions.push("t.priority = ?");
            params_vec.push(priority.into());
        }

        if let Some(assigned_agent) = &filter.assigned_agent {
            conditions.push("t.assigned_agent = ?");
            params_vec.push(assigned_agent.clone().into());
        }

        let mut query = String::from(
            "SELECT t.id, t.title, t.description, t.type, t.status, t.priority,
                    t.assigned_agent, t.tags, t.created_at, t.updated_at,
                    t.due_at, t.defer_until
             FROM tasks t",
        );

        // Filter by tag using LIKE (tags stored as JSON array)
        if let Some(tag) = &filter.tag {
            conditions.push("t.tags LIKE ?");
            params_vec.push(format!("%\"{}\"%%", tag).into());
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY t.priority ASC, t.created_at ASC");

        if filter.limit > 0 {
            query.push_str(" LIMIT ?");
            params_vec.push((filter.limit as i64).into());
        }

        if filter.offset > 0 {
            query.push_str(" OFFSET ?");
            params_vec.push((filter.offset as i64).into());
        }

        let mut rows = self.conn.query(&query, params_vec).await?;
        let mut tasks = Vec::new();

        while let Some(row) = rows.next().await? {
            tasks.push(parse_task_row(&row)?);
        }

        Ok(tasks)
    }

    /// GetReadyTasks finds tasks that are ready to work on.
    /// A task is ready if:
    ///   - status = 'open'
    ///   - is_blocked = 0 (no blocking dependencies)
    ///   - defer_until IS NULL OR defer_until <= now (unless include_deferred is true)
    ///
    /// Results are ordered by priority ASC (P0 first), then created_at ASC.
    pub async fn get_ready_tasks(&self, opts: ReadyTasksOptions) -> Result<Vec<TaskFile>> {
        let mut conditions = vec!["status = ?"];
        let mut params_vec: Vec<turso::Value> = vec!["open".into()];

        conditions.push("is_blocked = 0");

        if !opts.include_deferred {
            conditions.push("(defer_until IS NULL OR defer_until <= ?)");
            params_vec.push(Utc::now().to_rfc3339().into());
        }

        if let Some(assigned_agent) = &opts.assigned_agent {
            conditions.push("assigned_agent = ?");
            params_vec.push(assigned_agent.clone().into());
        }

        let mut query = format!(
            "SELECT id, title, description, type, status, priority,
                    assigned_agent, tags, created_at, updated_at,
                    due_at, defer_until
             FROM tasks
             WHERE {}
             ORDER BY priority ASC, created_at ASC",
            conditions.join(" AND ")
        );

        if opts.limit > 0 {
            query.push_str(" LIMIT ?");
            params_vec.push((opts.limit as i64).into());
        }

        let mut rows = self.conn.query(&query, params_vec).await?;
        let mut tasks = Vec::new();

        while let Some(row) = rows.next().await? {
            tasks.push(parse_task_row(&row)?);
        }

        Ok(tasks)
    }

    /// UpsertDep inserts or updates a dependency in the database.
    pub async fn upsert_dep(&self, dep: &DepFile) -> Result<()> {
        dep.validate()?;

        let query = r#"
            INSERT INTO deps (from_id, to_id, type, created_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(from_id, to_id, type) DO UPDATE SET
                created_at = excluded.created_at
        "#;

        self.conn
            .execute(
                query,
                params![
                    dep.from.clone(),
                    dep.to.clone(),
                    dep.dep_type.clone(),
                    dep.created_at.to_rfc3339(),
                ],
            )
            .await?;

        Ok(())
    }

    /// DeleteDep removes a dependency from the database.
    ///
    /// Returns Ok if the dependency doesn't exist (idempotent).
    pub async fn delete_dep(&self, from: &str, to: &str, dep_type: &str) -> Result<()> {
        let query = "DELETE FROM deps WHERE from_id = ? AND to_id = ? AND type = ?";
        self.conn
            .execute(query, params![from, to, dep_type])
            .await?;
        Ok(())
    }

    /// GetDepsForTask returns all dependencies for a given task.
    /// This includes both dependencies (tasks this task depends on)
    /// and dependents (tasks that depend on this task).
    pub async fn get_deps_for_task(&self, task_id: &str) -> Result<Vec<DepFile>> {
        let query = r#"
            SELECT from_id, to_id, type, created_at
            FROM deps
            WHERE from_id = ? OR to_id = ?
            ORDER BY created_at ASC
        "#;

        let mut rows = self.conn.query(query, params![task_id, task_id]).await?;
        let mut deps = Vec::new();

        while let Some(row) = rows.next().await? {
            deps.push(parse_dep_row(&row)?);
        }

        Ok(deps)
    }

    /// RefreshBlockedCache recomputes the blocked status for all tasks.
    ///
    /// This performs a transitive closure query to find all tasks that are
    /// blocked by open tasks with "blocks" dependencies.
    /// Uses iterative approach for compatibility (no recursive CTEs).
    pub async fn refresh_blocked_cache(&mut self) -> Result<()> {
        // Start transaction
        let tx = self.conn.transaction().await?;

        // Clear existing cache
        tx.execute("DELETE FROM blocked_cache", params![]).await?;

        // Reset all is_blocked flags
        tx.execute("UPDATE tasks SET is_blocked = 0", params![])
            .await?;

        // Get all open task IDs for filtering
        let mut open_tasks = HashSet::new();
        let mut rows = tx
            .query("SELECT id FROM tasks WHERE status != 'closed'", params![])
            .await?;

        while let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            open_tasks.insert(id);
        }
        drop(rows);

        // Get all blocking dependencies
        let mut blocked_by: HashMap<String, Vec<String>> = HashMap::new();
        let mut rows = tx
            .query(
                "SELECT from_id, to_id FROM deps WHERE type = 'blocks'",
                params![],
            )
            .await?;

        while let Some(row) = rows.next().await? {
            let from_id: String = row.get(0)?;
            let to_id: String = row.get(1)?;

            // Only count if blocker is open
            if open_tasks.contains(&from_id) {
                blocked_by.entry(to_id).or_default().push(from_id);
            }
        }
        drop(rows);

        // Compute transitive closure iteratively
        let mut blocked: HashMap<String, HashSet<String>> = HashMap::new();

        // Initialize with direct blockers
        for (task_id, blockers) in &blocked_by {
            let mut blocker_set = HashSet::new();
            for blocker in blockers {
                blocker_set.insert(blocker.clone());
            }
            blocked.insert(task_id.clone(), blocker_set);
        }

        // Iterate until no changes (fixed point)
        let mut changed = true;
        while changed {
            changed = false;
            let task_ids: Vec<String> = blocked.keys().cloned().collect();

            for task_id in task_ids {
                let current_blockers: Vec<String> = blocked
                    .get(&task_id)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();

                for blocker_id in current_blockers {
                    // Add transitive blockers
                    // Clone the transitive blockers to avoid borrow checker issues
                    if let Some(transitive_blockers) = blocked.get(&blocker_id) {
                        let trans_clone: Vec<String> = transitive_blockers.iter().cloned().collect();
                        let task_set = blocked.get_mut(&task_id).unwrap();
                        for tb in trans_clone {
                            if task_set.insert(tb) {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Insert into blocked_cache and update is_blocked
        let now = Utc::now().to_rfc3339();

        for (task_id, blockers) in blocked {
            if blockers.is_empty() {
                continue;
            }

            // Build JSON array of blockers
            let blocker_list: Vec<String> = blockers.into_iter().collect();
            let blocked_by_json = serde_json::to_string(&blocker_list)?;

            // Insert into cache
            tx.execute(
                "INSERT INTO blocked_cache (task_id, blocked_by, computed_at) VALUES (?, ?, ?)",
                params![task_id.clone(), blocked_by_json, now.clone()],
            )
            .await?;

            // Update is_blocked flag
            tx.execute(
                "UPDATE tasks SET is_blocked = 1 WHERE id = ?",
                params![task_id],
            )
            .await?;
        }

        // Commit transaction
        tx.commit().await?;

        Ok(())
    }

    /// GetTaskCount returns the total number of tasks in the database.
    pub async fn get_task_count(&self) -> Result<i64> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM tasks", params![])
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(row.get(0)?)
        } else {
            Ok(0)
        }
    }

    /// GetDepCount returns the total number of dependencies in the database.
    pub async fn get_dep_count(&self) -> Result<i64> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM deps", params![])
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(row.get(0)?)
        } else {
            Ok(0)
        }
    }

    /// GetBlockingTasks returns the list of tasks that are blocking the given task.
    /// This performs a transitive closure over "blocks" dependencies to find all
    /// blocking tasks, not just direct dependencies.
    /// Uses iterative BFS approach for compatibility.
    pub async fn get_blocking_tasks(&self, task_id: &str) -> Result<Vec<TaskFile>> {
        // Build blocking graph using BFS
        let mut blocked_by: HashMap<String, Vec<String>> = HashMap::new();

        let mut rows = self
            .conn
            .query(
                "SELECT from_id, to_id FROM deps WHERE type = 'blocks'",
                params![],
            )
            .await?;

        while let Some(row) = rows.next().await? {
            let from_id: String = row.get(0)?;
            let to_id: String = row.get(1)?;
            blocked_by.entry(to_id).or_default().push(from_id);
        }
        drop(rows);

        // Find all transitive blockers using BFS
        let mut all_blockers = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(initial_blockers) = blocked_by.get(task_id) {
            for blocker in initial_blockers {
                queue.push_back(blocker.clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            if all_blockers.contains(&current) {
                continue; // Already visited
            }
            all_blockers.insert(current.clone());

            // Add this task's blockers to the queue
            if let Some(blockers) = blocked_by.get(&current) {
                for blocker in blockers {
                    queue.push_back(blocker.clone());
                }
            }
        }

        if all_blockers.is_empty() {
            return Ok(Vec::new());
        }

        // Build query for blocking tasks
        let placeholders: Vec<String> = all_blockers.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT id, title, description, type, status, priority,
                    assigned_agent, tags, created_at, updated_at,
                    due_at, defer_until
             FROM tasks
             WHERE id IN ({}) AND status != 'closed'
             ORDER BY priority ASC, created_at ASC",
            placeholders.join(",")
        );

        let params_vec: Vec<turso::Value> =
            all_blockers.into_iter().map(|id| id.into()).collect();

        let mut rows = self.conn.query(&query, params_vec).await?;
        let mut tasks = Vec::new();

        while let Some(row) = rows.next().await? {
            tasks.push(parse_task_row(&row)?);
        }

        Ok(tasks)
    }

    /// List all tasks from the database
    ///
    /// Returns all tasks regardless of status. Useful for export operations.
    ///
    /// # Returns
    /// * `Ok(Vec<TaskFile>)` - All tasks in the database
    /// * `Err(_)` - Database query failed
    pub async fn list_all_tasks(&self) -> Result<Vec<TaskFile>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, title, description, type, status, priority,
                        assigned_agent, tags, created_at, updated_at,
                        due_at, defer_until
                 FROM tasks
                 ORDER BY created_at DESC",
                params![],
            )
            .await?;

        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await? {
            tasks.push(parse_task_row(&row)?);
        }

        Ok(tasks)
    }

    /// List all dependencies from the database
    ///
    /// Returns all dependencies regardless of related task status.
    /// Useful for export operations.
    ///
    /// # Returns
    /// * `Ok(Vec<DepFile>)` - All dependencies in the database
    /// * `Err(_)` - Database query failed
    pub async fn list_all_deps(&self) -> Result<Vec<DepFile>> {
        let mut rows = self
            .conn
            .query(
                "SELECT from_id, to_id, type, created_at
                 FROM deps
                 ORDER BY created_at DESC",
                params![],
            )
            .await?;

        let mut deps = Vec::new();
        while let Some(row) = rows.next().await? {
            let created_at_str: String = row.get(3)?;
            deps.push(DepFile {
                from: row.get(0)?,
                to: row.get(1)?,
                dep_type: row.get(2)?,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| DbError::Other(format!("failed to parse created_at: {}", e)))?
                    .with_timezone(&Utc),
            });
        }

        Ok(deps)
    }
}

/// Helper function to parse a task row from query results
fn parse_task_row(row: &turso::Row) -> Result<TaskFile> {
    let tags_json: String = row.get(7)?;
    let tags: Vec<String> = if tags_json.is_empty() || tags_json == "null" {
        Vec::new()
    } else {
        serde_json::from_str(&tags_json)?
    };

    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;
    let due_at_str: Option<String> = row.get(10)?;
    let defer_until_str: Option<String> = row.get(11)?;

    Ok(TaskFile {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        task_type: row.get(3)?,
        status: row.get(4)?,
        priority: row.get(5)?,
        assigned_agent: row.get(6)?,
        tags,
        created_at: DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| DbError::Other(format!("failed to parse created_at: {}", e)))?
            .with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
            .map_err(|e| DbError::Other(format!("failed to parse updated_at: {}", e)))?
            .with_timezone(&Utc),
        due_at: due_at_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
        defer_until: defer_until_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
    })
}

/// Helper function to parse a dependency row from query results
fn parse_dep_row(row: &turso::Row) -> Result<DepFile> {
    let created_at_str: String = row.get(3)?;

    Ok(DepFile {
        from: row.get(0)?,
        to: row.get(1)?,
        dep_type: row.get(2)?,
        created_at: DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| DbError::Other(format!("failed to parse created_at: {}", e)))?
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_open_and_init() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_jj_beads.db");

        // Clean up if exists
        let _ = std::fs::remove_file(&db_path);

        let db = Database::open(&db_path).await.unwrap();
        db.init_schema().await.unwrap();

        let count = db.get_task_count().await.unwrap();
        assert_eq!(count, 0);

        // Clean up
        drop(db);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_upsert_and_get_task() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_jj_beads_upsert.db");
        let _ = std::fs::remove_file(&db_path);

        let db = Database::open(&db_path).await.unwrap();
        db.init_schema().await.unwrap();

        let task = TaskFile {
            id: "test-1".to_string(),
            title: "Test Task".to_string(),
            description: Some("Test description".to_string()),
            task_type: "task".to_string(),
            status: "open".to_string(),
            priority: 2,
            assigned_agent: None,
            tags: vec!["test".to_string()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            due_at: None,
            defer_until: None,
        };

        db.upsert_task(&task).await.unwrap();

        let retrieved = db.get_task_by_id("test-1").await.unwrap();
        assert_eq!(retrieved.id, "test-1");
        assert_eq!(retrieved.title, "Test Task");

        // Clean up
        drop(db);
        let _ = std::fs::remove_file(&db_path);
    }
}
