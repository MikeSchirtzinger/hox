//! Demonstration of the bd-core JJ-native types.
//!
//! This example shows how to create and use Task, HandoffContext, and AgentHandoff
//! types for JJ-native task orchestration.

use bd_core::{AgentHandoff, HandoffContext, Priority, Task, TaskMetadata, TaskStatus};

fn main() {
    println!("=== hox JJ-Native Types Demo ===\n");

    // Create a task (represents a jj change)
    let mut task = Task::new("xyzabc12", "Implement user authentication");
    task.description = Some("Add JWT-based authentication to the API".to_string());
    task.status = TaskStatus::InProgress;
    task.priority = Priority::High;
    task.agent = Some("agent-001".to_string());
    task.bookmark = Some("task/auth-feature".to_string());
    task.labels = vec!["authentication".to_string(), "security".to_string()];

    // Validate the task
    match task.validate() {
        Ok(_) => println!("[OK] Task validation passed"),
        Err(e) => println!("[ERR] Task validation failed: {}", e),
    }

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&task).expect("Failed to serialize task");
    println!("\n=== Task JSON ===");
    println!("{}\n", json);

    // Create handoff context (for agent transitions)
    let mut context = HandoffContext::new("Working on JWT token generation");
    context.add_progress("Set up auth middleware");
    context.add_progress("Implemented login endpoint");
    context.add_next_step("Add token refresh endpoint");
    context.add_next_step("Write integration tests");
    context.add_blocker("Waiting for secret key configuration");
    context.add_file("src/auth/mod.rs");
    context.add_file("src/auth/jwt.rs");

    // Attach context to task
    task.context = Some(context.clone());

    println!("=== Handoff Context JSON ===");
    let ctx_json = serde_json::to_string_pretty(&context).expect("Failed to serialize context");
    println!("{}\n", ctx_json);

    // Format task as jj change description
    println!("=== JJ Change Description ===");
    let description = task.format_description();
    println!("{}\n", description);

    // Create agent handoff (for complete task takeover)
    let mut handoff = AgentHandoff::new(task.clone());
    handoff.diff = r#"+pub fn generate_token(user: &User) -> Result<String, Error> {
+    let claims = Claims::new(user.id);
+    encode(&Header::default(), &claims, &KEYS.encoding)
+}
"#
    .to_string();
    handoff.parent_changes = vec!["abc12345".to_string(), "def67890".to_string()];

    println!("=== Agent Handoff Format ===");
    let handoff_prompt = handoff.format_for_agent();
    println!("{}\n", handoff_prompt);

    // Create task metadata (for .tasks/metadata.jsonl)
    let mut metadata = TaskMetadata::new("xyzabc12");
    metadata.priority = Priority::High;
    metadata.labels = vec!["authentication".to_string()];
    metadata.agent = Some("agent-001".to_string());

    println!("=== Task Metadata JSON ===");
    let meta_json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize metadata");
    println!("{}\n", meta_json);

    // Demonstrate status transitions
    println!("=== Status Information ===");
    println!("TaskStatus::Open is actionable: {}", TaskStatus::Open.is_actionable());
    println!(
        "TaskStatus::InProgress is actionable: {}",
        TaskStatus::InProgress.is_actionable()
    );
    println!(
        "TaskStatus::Done is terminal: {}",
        TaskStatus::Done.is_terminal()
    );
    println!(
        "TaskStatus::Abandoned is terminal: {}",
        TaskStatus::Abandoned.is_terminal()
    );

    // Demonstrate priority ordering
    println!("\n=== Priority Ordering ===");
    println!(
        "Critical < High: {}",
        Priority::Critical < Priority::High
    );
    println!("High < Medium: {}", Priority::High < Priority::Medium);
    println!("Medium < Low: {}", Priority::Medium < Priority::Low);

    // Demonstrate string parsing
    println!("\n=== String Parsing ===");
    println!("'wip' -> {:?}", "wip".parse::<TaskStatus>());
    println!("'p0' -> {:?}", "p0".parse::<Priority>());
    println!("'completed' -> {:?}", "completed".parse::<TaskStatus>());

    // Show type sizes (should be MUCH smaller than old Issue)
    println!("\n=== Type Sizes ===");
    println!(
        "Task: {} bytes (vs 1000+ for old Issue)",
        std::mem::size_of::<Task>()
    );
    println!("HandoffContext: {} bytes", std::mem::size_of::<HandoffContext>());
    println!("TaskMetadata: {} bytes", std::mem::size_of::<TaskMetadata>());
    println!("TaskStatus: {} bytes", std::mem::size_of::<TaskStatus>());
    println!("Priority: {} bytes", std::mem::size_of::<Priority>());

    println!("\n=== JJ-Native Paradigm ===");
    println!("- Tasks ARE jj changes (change_id is primary key)");
    println!("- Dependencies ARE ancestry in jj DAG");
    println!("- Assignments ARE bookmarks");
    println!("- No separate SQLite dependency graph needed!");
}
