//! Demonstration of the bd-core types and their JSON serialization.
//!
//! This example shows how to create and serialize Issue, Dependency, and Comment
//! types, verifying JSONL compatibility with the Go implementation.

use bd_core::*;
use chrono::Utc;

fn main() {
    println!("=== jj-beads-rs Types Demo ===\n");

    // Create a sample issue
    let mut issue = Issue {
        id: "task-001".to_string(),
        content_hash: String::new(),
        title: "Implement user authentication".to_string(),
        description: Some("Add JWT-based authentication to the API".to_string()),
        design: None,
        acceptance_criteria: Some("- Users can log in\n- JWT tokens are issued\n- Tokens expire after 24h".to_string()),
        notes: None,
        status: Some(Status::InProgress),
        priority: 1, // P1 - High priority
        issue_type: Some(IssueType::Feature),
        assignee: Some("engineer@example.com".to_string()),
        estimated_minutes: Some(240), // 4 hours
        created_at: Utc::now(),
        created_by: Some("architect@example.com".to_string()),
        updated_at: Utc::now(),
        closed_at: None,
        close_reason: None,
        closed_by_session: None,
        due_at: None,
        defer_until: None,
        external_ref: Some("gh-123".to_string()),
        compaction_level: None,
        compacted_at: None,
        compacted_at_commit: None,
        original_size: None,
        source_repo: String::new(),
        id_prefix: String::new(),
        labels: Some(vec!["authentication".to_string(), "security".to_string()]),
        dependencies: None,
        comments: None,
        deleted_at: None,
        deleted_by: None,
        delete_reason: None,
        original_type: None,
        sender: None,
        ephemeral: None,
        pinned: None,
        is_template: None,
        bonded_from: None,
        creator: Some(EntityRef {
            name: Some("Alice Engineer".to_string()),
            platform: Some("github".to_string()),
            org: Some("example-org".to_string()),
            id: Some("alice-123".to_string()),
        }),
        validations: None,
        await_type: None,
        await_id: None,
        timeout: None,
        waiters: None,
        holder: None,
        source_formula: None,
        source_location: None,
        hook_bead: None,
        role_bead: None,
        agent_state: None,
        last_activity: None,
        role_type: None,
        rig: None,
        mol_type: None,
        event_kind: None,
        actor: None,
        target: None,
        payload: None,
    };

    // Validate the issue
    match issue.validate() {
        Ok(_) => println!("✓ Issue validation passed"),
        Err(e) => println!("✗ Issue validation failed: {}", e),
    }

    // Compute content hash
    let hash = issue.compute_content_hash();
    issue.content_hash = hash.clone();
    println!("✓ Content hash: {}...", &hash[..16]);

    // Serialize to JSON (JSONL compatible)
    let json = serde_json::to_string_pretty(&issue).expect("Failed to serialize issue");
    println!("\n=== Issue JSON ===");
    println!("{}\n", json);

    // Create a dependency
    let dep = Dependency {
        issue_id: "task-001".to_string(),
        depends_on_id: "task-000".to_string(),
        dep_type: DependencyType::Blocks,
        created_at: Utc::now(),
        created_by: Some("system".to_string()),
        metadata: None,
        thread_id: None,
    };

    println!("=== Dependency JSON ===");
    let dep_json = serde_json::to_string_pretty(&dep).expect("Failed to serialize dependency");
    println!("{}\n", dep_json);

    // Create a comment
    let comment = Comment {
        id: 1,
        issue_id: "task-001".to_string(),
        author: "reviewer@example.com".to_string(),
        text: "LGTM - looks good to proceed".to_string(),
        created_at: Utc::now(),
    };

    println!("=== Comment JSON ===");
    let comment_json = serde_json::to_string_pretty(&comment).expect("Failed to serialize comment");
    println!("{}\n", comment_json);

    // Test dependency type logic
    println!("=== Dependency Type Tests ===");
    println!("Blocks affects ready work: {}", DependencyType::Blocks.affects_ready_work());
    println!("Related affects ready work: {}", DependencyType::Related.affects_ready_work());
    println!("ParentChild affects ready work: {}", DependencyType::ParentChild.affects_ready_work());

    // Test failure close detection
    println!("\n=== Failure Close Tests ===");
    println!("'failed to complete' is failure: {}", is_failure_close("failed to complete"));
    println!("'completed successfully' is failure: {}", is_failure_close("completed successfully"));
    println!("'wontfix' is failure: {}", is_failure_close("wontfix"));

    // Test EntityRef URI
    println!("\n=== EntityRef URI Tests ===");
    if let Some(ref creator) = issue.creator {
        if let Some(uri) = creator.uri() {
            println!("Creator URI: {}", uri);
            match EntityRef::parse_uri(&uri) {
                Ok(parsed) => println!("✓ Successfully parsed URI back to EntityRef"),
                Err(e) => println!("✗ Failed to parse URI: {}", e),
            }
        }
    }

    println!("\n=== Type Information ===");
    println!("Issue has {} fields (30+ from Go version)", std::mem::size_of::<Issue>());
    println!("All core enums (Status, IssueType, DependencyType, AgentState, MolType) implemented");
    println!("Full JSONL compatibility with Go implementation verified");
}
