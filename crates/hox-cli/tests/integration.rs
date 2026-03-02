//! Integration tests for the hox CLI pipeline

use hox_core::HoxError;
use hox_isolation::safety::{CompiledSafetyRules, SafetyRulesConfig, load_safety_rules};
use hox_isolation::{FilesystemBackend, GuardedBackend, IsolationBackend};
use hox_jj::repo_mode::{detect_repo_mode, RepoMode};
use hox_orchestrator::dag_optimization::DagOptimizer;
use hox_orchestrator::{transition, Action, Event, State};
use hox_planning::hox_prd::{DecompositionHint, Prd, Requirement, RequirementKind};
use hox_planning::validator::{validate_prd, ValidationError};
use hox_planning::writer::write_to_file;
use hox_jj::MetadataBatch;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn complete_prd() -> Prd {
    Prd {
        version: "v1".to_owned(),
        title: "Integration Test Feature".to_owned(),
        vision: "Enable end-to-end pipeline validation for hox.".to_owned(),
        actors: vec!["Developer".to_owned()],
        success_criteria: vec!["Pipeline completes within 500ms for 99% of runs".to_owned()],
        scope_in: vec!["Core pipeline".to_owned()],
        scope_out: vec![],
        requirements: vec![Requirement {
            id: "REQ-001".to_owned(),
            description: "System must validate PRDs end-to-end.".to_owned(),
            kind: RequirementKind::Functional,
        }],
        constraints: vec![],
        decomposition_hints: vec![DecompositionHint {
            name: "Pipeline".to_owned(),
            description: "End-to-end pipeline implementation".to_owned(),
            estimated_files: vec!["crates/hox-cli/".to_owned()],
            requirements: vec!["REQ-001".to_owned()],
        }],
    }
}

// ---------------------------------------------------------------------------
// Test 1: Plan → Validate → Write pipeline
// ---------------------------------------------------------------------------

#[test]
fn test_plan_validate_write_pipeline() {
    let dir = TempDir::new().expect("tempdir");
    let output_path = dir.path().join("prd.md");

    // 1. Create a Prd manually
    let prd = complete_prd();

    // 2. Validate it (should pass)
    let result = validate_prd(&prd);
    assert!(
        result.is_valid(),
        "PRD should be valid: {}",
        result.format_report()
    );

    // 3. Write to temp file
    write_to_file(&prd, &output_path).expect("write_to_file");

    // 4. Read back and parse
    let contents = std::fs::read_to_string(&output_path).expect("read file");
    let parsed = Prd::from_markdown(&contents).expect("parse written PRD");

    // 5. Validate parsed result
    let parsed_result = validate_prd(&parsed);
    assert!(
        parsed_result.is_valid(),
        "Parsed PRD should be valid: {}",
        parsed_result.format_report()
    );

    // 6. Assert round-trip
    assert_eq!(parsed.title, prd.title);
    assert_eq!(parsed.version, prd.version);
    assert_eq!(parsed.vision, prd.vision);
    assert_eq!(parsed.actors.len(), prd.actors.len());
    assert_eq!(parsed.requirements.len(), prd.requirements.len());
    assert_eq!(
        parsed.decomposition_hints.len(),
        prd.decomposition_hints.len()
    );
    assert_eq!(parsed.requirements[0].id, prd.requirements[0].id);
}

// ---------------------------------------------------------------------------
// Test 2: Validation catches errors
// ---------------------------------------------------------------------------

#[test]
fn test_validation_catches_missing_fields() {
    // Create minimal Prd missing required fields
    let empty_prd = Prd::new("Incomplete PRD");
    // version and title are set, but vision, success_criteria, scope_in,
    // requirements, and decomposition_hints are all empty.

    let result = validate_prd(&empty_prd);

    assert!(!result.is_valid(), "empty PRD should be invalid");
    assert!(
        result.errors.contains(&ValidationError::EmptyVision),
        "should report EmptyVision"
    );
    assert!(
        result.errors.contains(&ValidationError::NoSuccessCriteria),
        "should report NoSuccessCriteria"
    );
    assert!(
        result.errors.contains(&ValidationError::EmptyScopeIn),
        "should report EmptyScopeIn"
    );
    assert!(
        result.errors.contains(&ValidationError::NoFunctionalRequirements),
        "should report NoFunctionalRequirements"
    );
    assert!(
        result.errors.contains(&ValidationError::NoDecompositionHints),
        "should report NoDecompositionHints"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Repo mode detection
// ---------------------------------------------------------------------------

#[test]
fn test_repo_mode_detection() {
    // Colocated: both .jj/ and .git/ present
    let colocated = TempDir::new().expect("tempdir");
    std::fs::create_dir(colocated.path().join(".jj")).unwrap();
    std::fs::create_dir(colocated.path().join(".git")).unwrap();
    assert_eq!(
        detect_repo_mode(colocated.path()),
        RepoMode::Colocated,
        "should detect colocated mode"
    );

    // Native: only .jj/ present
    let native = TempDir::new().expect("tempdir");
    std::fs::create_dir(native.path().join(".jj")).unwrap();
    assert_eq!(
        detect_repo_mode(native.path()),
        RepoMode::Native,
        "should detect native jj mode"
    );

    // Uninitialized: neither .jj/ nor .git/ present
    let uninit = TempDir::new().expect("tempdir");
    assert_eq!(
        detect_repo_mode(uninit.path()),
        RepoMode::NotInitialized,
        "should detect uninitialized"
    );

    // Colocated with .git as file (worktree / submodule)
    let git_file = TempDir::new().expect("tempdir");
    std::fs::create_dir(git_file.path().join(".jj")).unwrap();
    std::fs::write(git_file.path().join(".git"), "gitdir: ../something").unwrap();
    assert_eq!(
        detect_repo_mode(git_file.path()),
        RepoMode::Colocated,
        "should detect colocated when .git is a file"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Safety rules enforcement
// ---------------------------------------------------------------------------

#[test]
fn test_safety_rules_block_protected_paths() {
    let config = SafetyRulesConfig {
        deny_paths: vec!["**/.env".to_owned(), "**/secrets/**".to_owned()],
        deny_commands: vec!["rm\\s+-rf\\s+/".to_owned()],
        max_file_size_bytes: 1024 * 1024,
    };
    let rules = CompiledSafetyRules::compile(&config).expect("compile rules");

    // Denied paths
    assert!(
        rules.check_path(Path::new(".env")).is_err(),
        ".env should be denied"
    );
    assert!(
        rules.check_path(Path::new("config/.env")).is_err(),
        "nested .env should be denied"
    );
    assert!(
        rules.check_path(Path::new("infra/secrets/key.pem")).is_err(),
        "secrets dir should be denied"
    );

    // Allowed paths
    assert!(
        rules.check_path(Path::new("src/main.rs")).is_ok(),
        "src/main.rs should be allowed"
    );
    assert!(
        rules.check_path(Path::new("Cargo.toml")).is_ok(),
        "Cargo.toml should be allowed"
    );

    // Denied command
    assert!(
        rules.check_command("rm -rf /").is_err(),
        "rm -rf / should be denied"
    );

    // Allowed command
    assert!(
        rules.check_command("cargo build").is_ok(),
        "cargo build should be allowed"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Safety rules fail-open on missing config
// ---------------------------------------------------------------------------

#[test]
fn test_safety_rules_fail_open_on_missing_config() {
    let dir = TempDir::new().expect("tempdir");
    // No .hox/safety-rules.toml — load_safety_rules should return empty rules

    let rules = load_safety_rules(dir.path()).expect("should succeed even without config");

    // Empty rules: everything is allowed
    assert!(
        rules.check_path(Path::new(".env")).is_ok(),
        "no rules → .env should be allowed"
    );
    assert!(
        rules.check_command("rm -rf /").is_ok(),
        "no rules → dangerous command should be allowed"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Batch metadata accumulation
// ---------------------------------------------------------------------------

#[test]
fn test_batch_metadata_accumulation() {
    let mut batch = MetadataBatch::new();

    assert!(batch.is_empty(), "fresh batch should be empty");
    assert_eq!(batch.len(), 0);

    batch.set("Status", "open");
    assert_eq!(batch.len(), 1);
    assert!(!batch.is_empty());

    batch.set("Priority", "high");
    batch.set("Agent", "agent-42");
    assert_eq!(batch.len(), 3, "should have 3 entries");

    // Overwrite existing key
    batch.set("Status", "in_progress");
    assert_eq!(batch.len(), 3, "overwrite should not add entry");
}

// ---------------------------------------------------------------------------
// Test 7: DAG optimizer groups independent tasks
// ---------------------------------------------------------------------------

#[test]
fn test_dag_optimizer_groups_independent_tasks() {
    use hox_core::Task;

    fn task_with_files(id: &str, files: &[&str]) -> Task {
        let files_section = format!("Do work\n\n## Files Touched\n{}", files.join("\n"));
        Task::new(id, files_section)
    }

    // Two tasks with non-overlapping file sets → one parallel group
    let tasks = vec![
        task_with_files("auth-task", &["crates/hox-agent/src/auth.rs"]),
        task_with_files("storage-task", &["crates/hox-agent/src/storage.rs"]),
    ];

    let groups = DagOptimizer::find_parallelizable(&tasks);
    assert_eq!(groups.len(), 1, "should produce one parallel group");
    assert_eq!(groups[0].tasks.len(), 2, "group should contain both tasks");
    assert!(
        groups[0].tasks.contains(&"auth-task".to_string()),
        "auth-task should be in group"
    );
    assert!(
        groups[0].tasks.contains(&"storage-task".to_string()),
        "storage-task should be in group"
    );

    // Overlapping file sets → no groups
    let overlapping = vec![
        task_with_files("t1", &["crates/hox-core/src/lib.rs"]),
        task_with_files("t2", &["crates/hox-core/src/lib.rs", "crates/other/src/main.rs"]),
    ];
    let no_groups = DagOptimizer::find_parallelizable(&overlapping);
    assert!(
        no_groups.is_empty(),
        "overlapping tasks should not be grouped"
    );

    // Single task → no groups
    let single = vec![task_with_files("solo", &["src/solo.rs"])];
    let solo_groups = DagOptimizer::find_parallelizable(&single);
    assert!(solo_groups.is_empty(), "single task produces no groups");
}

// ---------------------------------------------------------------------------
// Test 8: Orchestrate dry-run — pure state machine, no I/O
// ---------------------------------------------------------------------------

#[test]
fn test_orchestrate_dry_run_full_lifecycle() {
    // Idle → Planning
    let (state, actions) = transition(
        State::Idle,
        Event::StartOrchestration { goal: "Add webhook support".to_owned() },
    );
    assert!(matches!(state, State::Planning { .. }), "should enter Planning");
    assert!(
        actions.iter().any(|a| matches!(a, Action::SpawnPlanningAgent { .. })),
        "should emit SpawnPlanningAgent"
    );

    // Planning → Executing (3 tasks)
    let (state, actions) = transition(state, Event::PlanningComplete { task_count: 3 });
    assert!(
        matches!(state, State::Executing { active_tasks: 3, .. }),
        "should enter Executing with 3 tasks"
    );
    assert!(
        actions.iter().any(|a| matches!(a, Action::SpawnTaskAgents { count: 3 })),
        "should emit SpawnTaskAgents(3)"
    );

    // Executing → Integrating
    let (state, _) = transition(state, Event::AllTasksComplete);
    assert!(matches!(state, State::Integrating { .. }), "should enter Integrating");

    // Integrating → Validating
    let (state, _) = transition(state, Event::IntegrationClean);
    assert!(matches!(state, State::Validating { .. }), "should enter Validating");

    // Validating → Complete
    let (state, actions) = transition(state, Event::ValidationPassed);
    assert!(matches!(state, State::Complete { .. }), "should reach Complete");
    assert!(
        actions.iter().any(|a| matches!(a, Action::RecordPattern { .. })),
        "should record pattern on completion"
    );
}

#[test]
fn test_orchestrate_dry_run_zero_tasks_skips_execution() {
    let (planning_state, _) = transition(
        State::Idle,
        Event::StartOrchestration { goal: "No-op".to_owned() },
    );
    let (state, actions) = transition(planning_state, Event::PlanningComplete { task_count: 0 });
    assert!(
        matches!(state, State::Complete { .. }),
        "zero tasks → Complete without executing"
    );
    assert!(actions.iter().any(|a| matches!(a, Action::LogActivity { .. })));
}

#[test]
fn test_orchestrate_dry_run_validation_failure_goes_to_failed() {
    let (s, _) = transition(State::Idle, Event::StartOrchestration { goal: "X".to_owned() });
    let (s, _) = transition(s, Event::PlanningComplete { task_count: 1 });
    let (s, _) = transition(s, Event::AllTasksComplete);
    let (s, _) = transition(s, Event::IntegrationClean);

    let (state, _) = transition(s, Event::ValidationFailed { reason: "tests failed".to_owned() });
    assert!(matches!(state, State::Failed { .. }), "validation failure → Failed");
}

#[test]
fn test_orchestrate_dry_run_invalid_transition_is_safe() {
    // Invalid event from Idle → must not panic, must produce Failed
    let (state, _) = transition(State::Idle, Event::AllTasksComplete);
    assert!(matches!(state, State::Failed { .. }), "invalid transition → Failed, not panic");
}

// ---------------------------------------------------------------------------
// Test 9: Agent workspace isolation lifecycle (filesystem, no real JJ)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_workspace_isolation_lifecycle() {
    let tmp = TempDir::new().expect("tempdir");
    let base_dir = tmp.path().join("workspaces");
    let repo_root = tmp.path().join("repo");
    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::create_dir_all(&repo_root).unwrap();

    let backend = FilesystemBackend::new(base_dir.clone(), repo_root);

    // Empty to start
    let agents = backend.list().await.expect("list");
    assert!(agents.is_empty());

    // Manually populate (bypassing jj workspace add)
    tokio::fs::create_dir_all(base_dir.join("agent-alpha")).await.unwrap();
    tokio::fs::create_dir_all(base_dir.join("agent-beta")).await.unwrap();

    let mut agents = backend.list().await.expect("list");
    agents.sort();
    assert_eq!(agents, vec!["agent-alpha", "agent-beta"]);

    // destroy() removes directory
    backend.destroy("agent-alpha").await.expect("destroy");
    assert!(!base_dir.join("agent-alpha").exists());

    let agents = backend.list().await.expect("list after destroy");
    assert_eq!(agents, vec!["agent-beta"]);

    // destroy non-existent is a no-op
    backend.destroy("never-existed").await.expect("destroy non-existent is ok");
}

#[tokio::test]
async fn test_agent_workspace_paths_are_isolated() {
    let tmp = TempDir::new().expect("tempdir");
    let base_dir = tmp.path().join("workspaces");
    std::fs::create_dir_all(&base_dir).unwrap();

    tokio::fs::create_dir_all(base_dir.join("agent-1")).await.unwrap();
    tokio::fs::create_dir_all(base_dir.join("agent-2")).await.unwrap();
    tokio::fs::write(base_dir.join("agent-1/work.txt"), b"agent-1 work").await.unwrap();
    tokio::fs::write(base_dir.join("agent-2/work.txt"), b"agent-2 work").await.unwrap();

    let c1 = std::fs::read_to_string(base_dir.join("agent-1/work.txt")).unwrap();
    let c2 = std::fs::read_to_string(base_dir.join("agent-2/work.txt")).unwrap();

    assert_ne!(c1, c2, "each agent has its own workspace");
    assert_eq!(c1, "agent-1 work");
    assert_eq!(c2, "agent-2 work");
}

// ---------------------------------------------------------------------------
// Test 10: Colocated mode + safety rules integration
// ---------------------------------------------------------------------------

#[test]
fn test_colocated_mode_with_safety_rules() {
    let dir = TempDir::new().expect("tempdir");

    // Colocated repo structure
    std::fs::create_dir(dir.path().join(".jj")).unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir_all(dir.path().join(".hox")).unwrap();

    let toml = "deny_paths = [\"**/.env\", \"**/secrets/**\"]\ndeny_commands = [\"rm\\\\s+-rf\\\\s+/\"]\n";
    std::fs::write(dir.path().join(".hox/safety-rules.toml"), toml).unwrap();

    // Repo is Colocated
    assert_eq!(detect_repo_mode(dir.path()), RepoMode::Colocated);

    // Rules load from the colocated .hox dir
    let rules = load_safety_rules(dir.path()).expect("load rules");

    // Enforced
    assert!(rules.check_path(Path::new(".env")).is_err());
    assert!(rules.check_path(Path::new("infra/secrets/db.key")).is_err());
    assert!(rules.check_command("rm -rf /").is_err());

    // Allowed
    assert!(rules.check_path(Path::new("src/main.rs")).is_ok());
    assert!(rules.check_command("cargo test").is_ok());
}

#[tokio::test]
async fn test_guarded_backend_enforces_safety_rules() {
    use async_trait::async_trait;
    use hox_isolation::IsolatedEnv;
    use std::path::PathBuf;

    struct StubBackend;

    #[async_trait]
    impl IsolationBackend for StubBackend {
        async fn create(&self, agent_id: &str) -> hox_core::Result<IsolatedEnv> {
            Ok(IsolatedEnv {
                agent_id: agent_id.to_string(),
                workspace_path: PathBuf::from("/tmp").join(agent_id),
            })
        }
        async fn destroy(&self, _: &str) -> hox_core::Result<()> {
            Ok(())
        }
        async fn list(&self) -> hox_core::Result<Vec<String>> {
            Ok(vec![])
        }
    }

    let config = SafetyRulesConfig {
        deny_paths: vec!["**/.env".to_owned()],
        deny_commands: vec!["rm\\s+-rf\\s+/".to_owned()],
        max_file_size_bytes: 1024 * 1024,
    };
    let rules = CompiledSafetyRules::compile(&config).expect("compile");
    let guarded = GuardedBackend::new(StubBackend, rules);

    // create() delegates through
    let env = guarded.create("my-agent").await.expect("create");
    assert_eq!(env.agent_id, "my-agent");

    // denied path
    let err = guarded.check_path(Path::new(".env")).unwrap_err();
    assert!(matches!(err, HoxError::ProtectedFile(_)));

    // allowed path
    assert!(guarded.check_path(Path::new("src/lib.rs")).is_ok());

    // denied command
    let err = guarded.check_command("rm -rf /").unwrap_err();
    assert!(matches!(err, HoxError::ProtectedFile(_)));

    // safe command
    assert!(guarded.check_command("cargo build").is_ok());
}
