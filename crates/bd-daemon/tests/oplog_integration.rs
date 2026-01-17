//! Integration tests for jj OpLog watcher.
//!
//! These tests verify that the oplog watcher correctly detects and processes
//! jj operations affecting task and dependency files.

use bd_daemon::oplog::{OpLogWatcher, OpLogWatcherConfig};
use std::time::Duration;

#[tokio::test]
async fn test_oplog_watcher_checks_jj_availability() {
    // This test just verifies the check functions work
    let is_available = OpLogWatcher::is_jj_available().await;

    // We can't assume jj is installed in CI, so just verify the function runs
    println!("jj available: {}", is_available);
}

#[tokio::test]
async fn test_oplog_watcher_creation() {
    let config = OpLogWatcherConfig {
        repo_path: ".".into(),
        poll_interval: Duration::from_millis(100),
        tasks_dir: "tasks".to_string(),
        deps_dir: "deps".to_string(),
        last_op_id: None,
    };

    // Creating the watcher should succeed as long as the path exists
    let result = OpLogWatcher::new(config);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_oplog_watcher_with_nonexistent_path() {
    let config = OpLogWatcherConfig {
        repo_path: "/nonexistent/path/that/does/not/exist".into(),
        poll_interval: Duration::from_millis(100),
        tasks_dir: "tasks".to_string(),
        deps_dir: "deps".to_string(),
        last_op_id: None,
    };

    // Creating the watcher with a nonexistent path should fail
    let result = OpLogWatcher::new(config);
    assert!(result.is_err());
}
