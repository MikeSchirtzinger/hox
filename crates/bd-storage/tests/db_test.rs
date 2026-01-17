//! Integration tests for the database layer
//!
//! Tests the full database functionality including:
//! - Schema initialization
//! - Task CRUD operations
//! - Dependency management
//! - Blocked cache computation with transitive closure

use bd_core::{DepFile, TaskFile};
use bd_storage::db::{Database, ListTasksFilter, ReadyTasksOptions};
use chrono::Utc;
use std::path::PathBuf;

/// Helper to create a temporary database for testing
async fn create_test_db() -> (Database, PathBuf) {
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!("test_jj_beads_{}.db", uuid::Uuid::new_v4()));

    // Clean up if exists
    let _ = std::fs::remove_file(&db_path);

    let db = Database::open(&db_path).await.expect("Failed to open database");
    db.init_schema().await.expect("Failed to init schema");

    (db, db_path)
}

/// Helper to create a test task
fn create_task(id: &str, title: &str, status: &str) -> TaskFile {
    TaskFile {
        id: id.to_string(),
        title: title.to_string(),
        description: Some(format!("Description for {}", title)),
        task_type: "task".to_string(),
        status: status.to_string(),
        priority: 2,
        assigned_agent: None,
        tags: vec!["test".to_string()],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        due_at: None,
        defer_until: None,
    }
}

/// Helper to create a test dependency
fn create_dep(from: &str, to: &str, dep_type: &str) -> DepFile {
    DepFile {
        from: from.to_string(),
        to: to.to_string(),
        dep_type: dep_type.to_string(),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_database_initialization() {
    let (db, db_path) = create_test_db().await;

    // Verify tables were created by checking counts
    let task_count = db.get_task_count().await.expect("Failed to get task count");
    assert_eq!(task_count, 0);

    let dep_count = db.get_dep_count().await.expect("Failed to get dep count");
    assert_eq!(dep_count, 0);

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_task_crud() {
    let (db, db_path) = create_test_db().await;

    // Create
    let task = create_task("test-1", "Test Task", "open");
    db.upsert_task(&task).await.expect("Failed to upsert task");

    // Read
    let retrieved = db.get_task_by_id("test-1").await.expect("Failed to get task");
    assert_eq!(retrieved.id, "test-1");
    assert_eq!(retrieved.title, "Test Task");
    assert_eq!(retrieved.status, "open");

    // Update
    let mut updated_task = task.clone();
    updated_task.title = "Updated Title".to_string();
    db.upsert_task(&updated_task).await.expect("Failed to update task");

    let retrieved = db.get_task_by_id("test-1").await.expect("Failed to get task");
    assert_eq!(retrieved.title, "Updated Title");

    // Delete
    db.delete_task("test-1").await.expect("Failed to delete task");

    let result = db.get_task_by_id("test-1").await;
    assert!(result.is_err(), "Task should be deleted");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_dependency_operations() {
    let (db, db_path) = create_test_db().await;

    // Create tasks
    let task1 = create_task("task-1", "Task 1", "open");
    let task2 = create_task("task-2", "Task 2", "open");
    db.upsert_task(&task1).await.expect("Failed to upsert task1");
    db.upsert_task(&task2).await.expect("Failed to upsert task2");

    // Create dependency: task-1 blocks task-2
    let dep = create_dep("task-1", "task-2", "blocks");
    db.upsert_dep(&dep).await.expect("Failed to upsert dep");

    // Get dependencies for task-2
    let deps = db.get_deps_for_task("task-2").await.expect("Failed to get deps");
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].from, "task-1");
    assert_eq!(deps[0].to, "task-2");
    assert_eq!(deps[0].dep_type, "blocks");

    // Delete dependency
    db.delete_dep("task-1", "task-2", "blocks").await.expect("Failed to delete dep");
    let deps = db.get_deps_for_task("task-2").await.expect("Failed to get deps");
    assert_eq!(deps.len(), 0);

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_blocked_cache_direct() {
    let (mut db, db_path) = create_test_db().await;

    // Create tasks: A blocks B
    let task_a = create_task("task-a", "Task A", "open");
    let task_b = create_task("task-b", "Task B", "open");
    db.upsert_task(&task_a).await.expect("Failed to upsert task A");
    db.upsert_task(&task_b).await.expect("Failed to upsert task B");

    // A blocks B
    let dep = create_dep("task-a", "task-b", "blocks");
    db.upsert_dep(&dep).await.expect("Failed to upsert dep");

    // Refresh blocked cache
    db.refresh_blocked_cache().await.expect("Failed to refresh blocked cache");

    // Note: is_blocked flag is set in the database but not in TaskFile struct
    // We need to verify by checking if it's in the blocked_cache or not in ready tasks

    // Get ready tasks - task B should NOT be in ready tasks
    let ready = db.get_ready_tasks(ReadyTasksOptions::default()).await.expect("Failed to get ready tasks");
    let task_b_ready = ready.iter().any(|t| t.id == "task-b");
    assert!(!task_b_ready, "Task B should not be ready (it's blocked)");

    // Task A should be ready
    let task_a_ready = ready.iter().any(|t| t.id == "task-a");
    assert!(task_a_ready, "Task A should be ready");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_blocked_cache_transitive() {
    let (mut db, db_path) = create_test_db().await;

    // Create tasks: A blocks B, B blocks C
    // This means C is transitively blocked by A
    let task_a = create_task("task-a", "Task A", "open");
    let task_b = create_task("task-b", "Task B", "open");
    let task_c = create_task("task-c", "Task C", "open");
    db.upsert_task(&task_a).await.expect("Failed to upsert task A");
    db.upsert_task(&task_b).await.expect("Failed to upsert task B");
    db.upsert_task(&task_c).await.expect("Failed to upsert task C");

    // A blocks B, B blocks C
    let dep1 = create_dep("task-a", "task-b", "blocks");
    let dep2 = create_dep("task-b", "task-c", "blocks");
    db.upsert_dep(&dep1).await.expect("Failed to upsert dep1");
    db.upsert_dep(&dep2).await.expect("Failed to upsert dep2");

    // Refresh blocked cache
    db.refresh_blocked_cache().await.expect("Failed to refresh blocked cache");

    // Get ready tasks
    let ready = db.get_ready_tasks(ReadyTasksOptions::default()).await.expect("Failed to get ready tasks");

    // Only task A should be ready
    let ready_ids: Vec<String> = ready.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ready_ids.len(), 1);
    assert!(ready_ids.contains(&"task-a".to_string()));
    assert!(!ready_ids.contains(&"task-b".to_string()), "Task B should be blocked");
    assert!(!ready_ids.contains(&"task-c".to_string()), "Task C should be transitively blocked");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_blocked_cache_complex_graph() {
    let (mut db, db_path) = create_test_db().await;

    // Create a more complex graph:
    //   A blocks B
    //   A blocks C
    //   B blocks D
    //   C blocks D
    // Result: D is blocked by A (transitively through both B and C)

    for id in &["task-a", "task-b", "task-c", "task-d"] {
        let task = create_task(id, &format!("Task {}", id), "open");
        db.upsert_task(&task).await.expect("Failed to upsert task");
    }

    let deps = vec![
        create_dep("task-a", "task-b", "blocks"),
        create_dep("task-a", "task-c", "blocks"),
        create_dep("task-b", "task-d", "blocks"),
        create_dep("task-c", "task-d", "blocks"),
    ];

    for dep in deps {
        db.upsert_dep(&dep).await.expect("Failed to upsert dep");
    }

    // Refresh blocked cache
    db.refresh_blocked_cache().await.expect("Failed to refresh blocked cache");

    // Get ready tasks
    let ready = db.get_ready_tasks(ReadyTasksOptions::default()).await.expect("Failed to get ready tasks");
    let ready_ids: Vec<String> = ready.iter().map(|t| t.id.clone()).collect();

    // Only task A should be ready
    assert_eq!(ready_ids.len(), 1);
    assert!(ready_ids.contains(&"task-a".to_string()));

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_get_blocking_tasks() {
    let (db, db_path) = create_test_db().await;

    // Create graph: A blocks B, B blocks C
    for id in &["task-a", "task-b", "task-c"] {
        let task = create_task(id, &format!("Task {}", id), "open");
        db.upsert_task(&task).await.expect("Failed to upsert task");
    }

    let dep1 = create_dep("task-a", "task-b", "blocks");
    let dep2 = create_dep("task-b", "task-c", "blocks");
    db.upsert_dep(&dep1).await.expect("Failed to upsert dep1");
    db.upsert_dep(&dep2).await.expect("Failed to upsert dep2");

    // Get blocking tasks for task C
    let blockers = db.get_blocking_tasks("task-c").await.expect("Failed to get blocking tasks");
    let blocker_ids: Vec<String> = blockers.iter().map(|t| t.id.clone()).collect();

    // Task C should be blocked by both A and B (transitively)
    assert_eq!(blocker_ids.len(), 2);
    assert!(blocker_ids.contains(&"task-a".to_string()));
    assert!(blocker_ids.contains(&"task-b".to_string()));

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_blocked_cache_with_closed_tasks() {
    let (mut db, db_path) = create_test_db().await;

    // Create tasks: A blocks B, but A is closed
    let task_a = create_task("task-a", "Task A", "closed");
    let task_b = create_task("task-b", "Task B", "open");
    db.upsert_task(&task_a).await.expect("Failed to upsert task A");
    db.upsert_task(&task_b).await.expect("Failed to upsert task B");

    // A blocks B
    let dep = create_dep("task-a", "task-b", "blocks");
    db.upsert_dep(&dep).await.expect("Failed to upsert dep");

    // Refresh blocked cache
    db.refresh_blocked_cache().await.expect("Failed to refresh blocked cache");

    // Task B should be ready because A is closed (not blocking)
    let ready = db.get_ready_tasks(ReadyTasksOptions::default()).await.expect("Failed to get ready tasks");
    let task_b_ready = ready.iter().any(|t| t.id == "task-b");
    assert!(task_b_ready, "Task B should be ready (blocker is closed)");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_list_tasks_with_filters() {
    let (db, db_path) = create_test_db().await;

    // Create multiple tasks with different attributes
    let mut task1 = create_task("task-1", "Task 1", "open");
    task1.priority = 0;
    task1.tags = vec!["urgent".to_string()];

    let mut task2 = create_task("task-2", "Task 2", "in_progress");
    task2.priority = 2;
    task2.tags = vec!["feature".to_string()];

    let mut task3 = create_task("task-3", "Task 3", "open");
    task3.priority = 4;
    task3.tags = vec!["backlog".to_string()];

    db.upsert_task(&task1).await.expect("Failed to upsert task1");
    db.upsert_task(&task2).await.expect("Failed to upsert task2");
    db.upsert_task(&task3).await.expect("Failed to upsert task3");

    // Filter by status
    let filter = ListTasksFilter {
        status: Some("open".to_string()),
        ..Default::default()
    };
    let tasks = db.list_tasks(filter).await.expect("Failed to list tasks");
    assert_eq!(tasks.len(), 2);

    // Filter by priority
    let filter = ListTasksFilter {
        priority: Some(0),
        ..Default::default()
    };
    let tasks = db.list_tasks(filter).await.expect("Failed to list tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task-1");

    // Filter by tag
    let filter = ListTasksFilter {
        tag: Some("urgent".to_string()),
        ..Default::default()
    };
    let tasks = db.list_tasks(filter).await.expect("Failed to list tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task-1");

    // Test limit
    let filter = ListTasksFilter {
        limit: 2,
        ..Default::default()
    };
    let tasks = db.list_tasks(filter).await.expect("Failed to list tasks");
    assert_eq!(tasks.len(), 2);

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_ready_tasks_priority_ordering() {
    let (db, db_path) = create_test_db().await;

    // Create tasks with different priorities
    let mut task1 = create_task("task-1", "Task P2", "open");
    task1.priority = 2;

    let mut task2 = create_task("task-2", "Task P0", "open");
    task2.priority = 0;

    let mut task3 = create_task("task-3", "Task P4", "open");
    task3.priority = 4;

    db.upsert_task(&task1).await.expect("Failed to upsert task1");
    db.upsert_task(&task2).await.expect("Failed to upsert task2");
    db.upsert_task(&task3).await.expect("Failed to upsert task3");

    // Get ready tasks (should be ordered by priority ASC)
    let ready = db.get_ready_tasks(ReadyTasksOptions::default()).await.expect("Failed to get ready tasks");

    assert_eq!(ready.len(), 3);
    assert_eq!(ready[0].id, "task-2"); // P0 first
    assert_eq!(ready[1].id, "task-1"); // P2 second
    assert_eq!(ready[2].id, "task-3"); // P4 last

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&db_path);
}
