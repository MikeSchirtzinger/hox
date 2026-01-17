//! Beads CLI - Command-line interface for the beads issue tracking system.

use anyhow::Result;
use bd_core::TaskFile;
use bd_daemon::Daemon;
use bd_storage::task_io::{delete_task_file, read_task_file, write_task_file};
use bd_storage::{sync::SyncManager, Database, ListTasksFilter};
use chrono::Utc;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::env;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "beads")]
#[command(about = "Beads - File-based issue tracking", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new beads repository
    Init {
        /// Path to initialize (defaults to current directory)
        path: Option<String>,
    },

    /// Create a new issue
    New {
        /// Issue title
        title: String,

        /// Issue description
        #[arg(short, long)]
        description: Option<String>,
    },

    /// List issues
    List {
        /// Filter by status
        #[arg(short, long)]
        status: Option<String>,

        /// Filter by label
        #[arg(short, long)]
        label: Option<String>,
    },

    /// Show issue details
    Show {
        /// Issue ID
        id: String,
    },

    /// Update an issue
    Update {
        /// Issue ID
        id: String,

        /// New title
        #[arg(short, long)]
        title: Option<String>,

        /// New status
        #[arg(short, long)]
        status: Option<String>,

        /// New priority (0-4, where P0=critical, P4=backlog)
        #[arg(short, long)]
        priority: Option<i32>,

        /// New description
        #[arg(short, long)]
        description: Option<String>,

        /// New assignee
        #[arg(short, long)]
        assignee: Option<String>,

        /// Add a tag
        #[arg(long)]
        add_tag: Option<Vec<String>>,

        /// Remove a tag
        #[arg(long)]
        remove_tag: Option<Vec<String>>,
    },

    /// Close an issue
    Close {
        /// Issue ID
        id: String,

        /// Resolution comment
        #[arg(short, long)]
        comment: Option<String>,
    },

    /// Delete an issue
    Delete {
        /// Issue ID
        id: String,

        /// Force delete without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Start the file watcher daemon
    Daemon {
        /// Run in foreground
        #[arg(short, long)]
        foreground: bool,
    },

    /// Sync files to database
    Sync {
        /// Export from database to files instead of syncing files to database
        #[arg(short, long)]
        export: bool,

        /// Only sync changes since this VCS commit (e.g., "HEAD~1", "abc123")
        #[arg(short, long)]
        since: Option<String>,
    },

    /// Show version information
    Version,
}

/// Find the .beads directory by walking up from the current directory.
/// Returns the path to the .beads directory if found, otherwise an error.
fn find_beads_dir() -> Result<PathBuf> {
    let mut current = env::current_dir()?;

    loop {
        let beads_path = current.join(".beads");
        if beads_path.is_dir() {
            return Ok(beads_path);
        }

        // Try parent directory
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => {
                return Err(anyhow::anyhow!(
                    "No .beads directory found. Run 'beads init' to initialize a repository."
                ));
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    info!("Beads CLI starting");

    match cli.command {
        Commands::Init { path } => {
            let target = path.unwrap_or_else(|| ".".to_string());
            let target_path = Path::new(&target);

            // Create .beads directory structure
            let beads_dir = target_path.join(".beads");
            let tasks_dir = beads_dir.join("tasks");
            let deps_dir = beads_dir.join("deps");

            tokio::fs::create_dir_all(&tasks_dir).await?;
            tokio::fs::create_dir_all(&deps_dir).await?;

            // Initialize database
            let db_path = beads_dir.join("turso.db");
            let db = Database::open(&db_path).await?;
            db.init_schema().await?;

            println!("{}", "✓ Initialized beads repository".green().bold());
            println!("  Database: {}", db_path.display());
            println!("  Tasks:    {}", tasks_dir.display());
            println!("  Deps:     {}", deps_dir.display());

            Ok(())
        }

        Commands::New { title, description } => {
            let beads_dir = find_beads_dir()?;
            let tasks_dir = beads_dir.join("tasks");

            // Generate UUID for issue ID
            let id = Uuid::new_v4().to_string();

            // Create TaskFile with defaults
            let now = Utc::now();
            let task = TaskFile {
                id: id.clone(),
                title: title.clone(),
                description,
                task_type: "task".to_string(),
                status: "open".to_string(),
                priority: 2,
                assigned_agent: None,
                tags: vec![],
                created_at: now,
                updated_at: now,
                due_at: None,
                defer_until: None,
            };

            // Write task file
            write_task_file(&tasks_dir, &task).await?;

            // Insert into database
            let db_path = beads_dir.join("turso.db");
            let db = Database::open(&db_path).await?;
            db.upsert_task(&task).await?;

            println!("{}", "✓ Created new issue".green().bold());
            println!("  ID:    {}", id.bright_cyan());
            println!("  Title: {}", title);
            println!("  File:  {}", tasks_dir.join(format!("{}.json", id)).display());

            Ok(())
        }

        Commands::List { status, label } => {
            let beads_dir = find_beads_dir()?;
            let db_path = beads_dir.join("turso.db");

            // Open database
            let db = Database::open(&db_path).await?;

            // Build filter
            let filter = ListTasksFilter {
                status,
                tag: label,
                ..Default::default()
            };

            // Query issues
            let tasks = db.list_tasks(filter).await?;

            if tasks.is_empty() {
                println!("{}", "No issues found".yellow());
                return Ok(());
            }

            // Print header
            println!(
                "{:<38} {:<12} {:<8} {}",
                "ID".bold(),
                "STATUS".bold(),
                "PRIORITY".bold(),
                "TITLE".bold()
            );
            println!("{}", "─".repeat(80));

            // Print tasks
            for task in tasks {
                let status_colored = match task.status.as_str() {
                    "open" => task.status.green(),
                    "in_progress" => task.status.yellow(),
                    "closed" => task.status.bright_black(),
                    "blocked" => task.status.red(),
                    _ => task.status.normal(),
                };

                let priority_str = format!("P{}", task.priority);
                let priority_colored = match task.priority {
                    0 => priority_str.red().bold(),
                    1 => priority_str.yellow(),
                    2 => priority_str.normal(),
                    _ => priority_str.bright_black(),
                };

                println!(
                    "{:<38} {:<12} {:<8} {}",
                    task.id.bright_cyan(),
                    status_colored,
                    priority_colored,
                    task.title
                );
            }

            Ok(())
        }

        Commands::Show { id } => {
            let beads_dir = find_beads_dir()?;
            let db_path = beads_dir.join("turso.db");

            // Try to get task from database
            let db = Database::open(&db_path).await?;
            let task = match db.get_task_by_id(&id).await {
                Ok(task) => task,
                Err(_) => {
                    // If not in database, try reading from file
                    let task_file = beads_dir.join("tasks").join(format!("{}.json", id));
                    if !task_file.exists() {
                        return Err(anyhow::anyhow!("Issue not found: {}", id));
                    }
                    read_task_file(&task_file).await?
                }
            };

            // Print detailed issue info
            println!("{}", "━".repeat(80));
            println!("{} {}", "Issue:".bold(), task.id.bright_cyan());
            println!("{}", "━".repeat(80));
            println!();
            println!("{:<15} {}", "Title:".bold(), task.title);
            println!("{:<15} {}", "Type:".bold(), task.task_type);

            let status_colored = match task.status.as_str() {
                "open" => task.status.green(),
                "in_progress" => task.status.yellow(),
                "closed" => task.status.bright_black(),
                "blocked" => task.status.red(),
                _ => task.status.normal(),
            };
            println!("{:<15} {}", "Status:".bold(), status_colored);

            let priority_str = format!("P{}", task.priority);
            let priority_colored = match task.priority {
                0 => priority_str.red().bold(),
                1 => priority_str.yellow(),
                2 => priority_str.normal(),
                _ => priority_str.bright_black(),
            };
            println!("{:<15} {}", "Priority:".bold(), priority_colored);

            if let Some(assigned) = &task.assigned_agent {
                println!("{:<15} {}", "Assigned:".bold(), assigned);
            }

            if !task.tags.is_empty() {
                println!("{:<15} {}", "Tags:".bold(), task.tags.join(", "));
            }

            println!("{:<15} {}", "Created:".bold(), task.created_at.format("%Y-%m-%d %H:%M:%S"));
            println!("{:<15} {}", "Updated:".bold(), task.updated_at.format("%Y-%m-%d %H:%M:%S"));

            if let Some(due) = task.due_at {
                println!("{:<15} {}", "Due:".bold(), due.format("%Y-%m-%d %H:%M:%S"));
            }

            if let Some(defer) = task.defer_until {
                println!("{:<15} {}", "Deferred:".bold(), defer.format("%Y-%m-%d %H:%M:%S"));
            }

            if let Some(desc) = &task.description {
                println!();
                println!("{}", "Description:".bold());
                println!("{}", desc);
            }

            println!();
            println!("{}", "━".repeat(80));

            Ok(())
        }

        Commands::Update {
            id,
            title,
            status,
            priority,
            description,
            assignee,
            add_tag,
            remove_tag,
        } => {
            // Find .beads directory
            let beads_dir = find_beads_dir()?;
            let tasks_dir = beads_dir.join("tasks");
            let task_file = tasks_dir.join(format!("{}.json", id));

            // Read the existing task file
            let mut task = read_task_file(&task_file).await.map_err(|e| {
                anyhow::anyhow!("Failed to load task {}: {}", id, e)
            })?;

            // Update fields that were provided
            let mut updated = false;

            if let Some(new_title) = title {
                task.title = new_title;
                updated = true;
            }

            if let Some(new_status) = status {
                task.status = new_status;
                updated = true;
            }

            if let Some(new_priority) = priority {
                if !(0..=4).contains(&new_priority) {
                    return Err(anyhow::anyhow!(
                        "Priority must be between 0 and 4 (got {})",
                        new_priority
                    ));
                }
                task.priority = new_priority;
                updated = true;
            }

            if let Some(new_description) = description {
                task.description = Some(new_description);
                updated = true;
            }

            if let Some(new_assignee) = assignee {
                task.assigned_agent = if new_assignee.is_empty() {
                    None
                } else {
                    Some(new_assignee)
                };
                updated = true;
            }

            // Add tags
            if let Some(tags_to_add) = add_tag {
                for tag in tags_to_add {
                    if !task.tags.contains(&tag) {
                        task.tags.push(tag);
                        updated = true;
                    }
                }
            }

            // Remove tags
            if let Some(tags_to_remove) = remove_tag {
                for tag in tags_to_remove {
                    if let Some(pos) = task.tags.iter().position(|t| t == &tag) {
                        task.tags.remove(pos);
                        updated = true;
                    }
                }
            }

            if !updated {
                println!("{}", "No changes specified".yellow());
                return Ok(());
            }

            // Update timestamp
            task.update_timestamp();

            // Write back to file
            write_task_file(&tasks_dir, &task).await?;

            // Update database
            let db_path = beads_dir.join("turso.db");
            let db = Database::open(&db_path).await?;
            db.upsert_task(&task).await?;

            println!("{}", format!("✓ Updated task {}", id).green().bold());
            println!("  Title:    {}", task.title);
            println!("  Status:   {}", task.status);
            println!("  Priority: P{}", task.priority);

            Ok(())
        }

        Commands::Close { id, comment } => {
            // Find .beads directory
            let beads_dir = find_beads_dir()?;
            let tasks_dir = beads_dir.join("tasks");
            let task_file = tasks_dir.join(format!("{}.json", id));

            // Read the existing task file
            let mut task = read_task_file(&task_file).await.map_err(|e| {
                anyhow::anyhow!("Failed to load task {}: {}", id, e)
            })?;

            // Update status to closed
            task.status = "closed".to_string();
            task.update_timestamp();

            // If comment provided, add it to the description
            if let Some(comment_text) = comment {
                let closure_note = format!(
                    "\n\n---\nClosed on {} with comment:\n{}",
                    Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
                    comment_text
                );
                task.description = Some(match task.description {
                    Some(existing) => format!("{}{}", existing, closure_note),
                    None => closure_note,
                });
            }

            // Write back to file
            write_task_file(&tasks_dir, &task).await?;

            // Update database
            let db_path = beads_dir.join("turso.db");
            let db = Database::open(&db_path).await?;
            db.upsert_task(&task).await?;

            println!("{}", format!("✓ Closed task {}", id).green().bold());
            println!("  Title: {}", task.title);

            Ok(())
        }

        Commands::Delete { id, force } => {
            // Find .beads directory
            let beads_dir = find_beads_dir()?;
            let tasks_dir = beads_dir.join("tasks");
            let task_file = tasks_dir.join(format!("{}.json", id));

            // Check if task exists
            if !task_file.exists() {
                return Err(anyhow::anyhow!("Task {} not found", id));
            }

            // Read task to show details
            let task = read_task_file(&task_file).await.map_err(|e| {
                anyhow::anyhow!("Failed to load task {}: {}", id, e)
            })?;

            // Prompt for confirmation unless --force
            if !force {
                println!(
                    "{}",
                    format!("About to delete task: {}", task.title)
                        .yellow()
                        .bold()
                );
                print!("Are you sure? [y/N]: ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("{}", "Deletion cancelled".yellow());
                    return Ok(());
                }
            }

            // Delete from file system
            delete_task_file(&tasks_dir, &id).await?;

            // Delete from database
            let db_path = beads_dir.join("turso.db");
            let db = Database::open(&db_path).await?;
            db.delete_task(&id).await?;

            println!("{}", format!("✓ Deleted task {}", id).green().bold());

            Ok(())
        }

        Commands::Daemon { foreground } => {
            let beads_dir = find_beads_dir()?;
            let db_path = beads_dir.join("turso.db");
            let beads_root = beads_dir.parent().ok_or_else(|| {
                anyhow::anyhow!("Invalid .beads directory structure")
            })?;

            if !foreground {
                println!("{}", "Note: Background daemonization is OS-specific and not yet implemented.".yellow());
                println!("Run with --foreground to run in the current terminal:");
                println!("  beads daemon --foreground");
                return Ok(());
            }

            println!("{}", "Starting daemon in foreground...".green());
            println!("  Database: {}", db_path.display());
            println!("  Watching: {}", beads_root.display());
            println!("  Press Ctrl+C to stop");
            println!();

            // Open database with Arc<RwLock> for sharing between tasks
            let db = Database::open(&db_path).await?;
            let db_arc = Arc::new(RwLock::new(db));

            // Create daemon instance
            let mut daemon = Daemon::new(db_arc, beads_root);

            // Set up graceful shutdown with Ctrl+C
            tokio::select! {
                result = daemon.run() => {
                    if let Err(e) = result {
                        eprintln!("Daemon error: {}", e);
                        return Err(e.into());
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n{}", "Shutting down...".yellow());
                    daemon.stop().await?;
                }
            }

            println!("{}", "✓ Daemon stopped".green());
            Ok(())
        }

        Commands::Sync { export, since } => {
            let beads_dir = find_beads_dir()?;
            let db_path = beads_dir.join("turso.db");
            let beads_root = beads_dir.parent().ok_or_else(|| {
                anyhow::anyhow!("Invalid .beads directory structure")
            })?;
            let tasks_dir = beads_root.join("tasks");
            let deps_dir = beads_root.join("deps");

            // Open database
            let db = Database::open(&db_path).await?;

            // Create sync manager
            let mut sync_manager = SyncManager::new(db, &tasks_dir, &deps_dir);

            if export {
                // Export from database to files
                println!("{}", "Exporting from database to files...".cyan());
                let stats = sync_manager.export_all().await?;

                println!("{}", "✓ Export complete".green().bold());
                println!("  Tasks exported:    {}", stats.tasks_exported.to_string().bright_green());
                println!("  Deps exported:     {}", stats.deps_exported.to_string().bright_green());

                if stats.has_errors() {
                    println!("{}", "⚠ Some exports failed:".yellow().bold());
                    println!("  Tasks failed:      {}", stats.tasks_failed.to_string().bright_red());
                    println!("  Deps failed:       {}", stats.deps_failed.to_string().bright_red());
                }
            } else if let Some(commit) = since {
                // Incremental sync since specified commit
                println!("{}", format!("Syncing changes since commit: {}", commit).cyan());
                let stats = sync_manager.sync_changed(&commit).await?;

                println!("{}", "✓ Incremental sync complete".green().bold());
                println!("  Tasks synced:      {}", stats.tasks_synced.to_string().bright_green());
                println!("  Deps synced:       {}", stats.deps_synced.to_string().bright_green());
                println!("  Deleted:           {}", stats.deleted.to_string().bright_yellow());

                if stats.has_errors() {
                    println!("{}", "⚠ Some syncs failed:".yellow().bold());
                    println!("  Tasks failed:      {}", stats.tasks_failed.to_string().bright_red());
                    println!("  Deps failed:       {}", stats.deps_failed.to_string().bright_red());
                }
            } else {
                // Full sync from files to database
                println!("{}", "Syncing all files to database...".cyan());
                let stats = sync_manager.sync_all().await?;

                println!("{}", "✓ Full sync complete".green().bold());
                println!("  Tasks synced:      {}", stats.tasks_synced.to_string().bright_green());
                println!("  Deps synced:       {}", stats.deps_synced.to_string().bright_green());

                if stats.has_errors() {
                    println!("{}", "⚠ Some syncs failed:".yellow().bold());
                    println!("  Tasks failed:      {}", stats.tasks_failed.to_string().bright_red());
                    println!("  Deps failed:       {}", stats.deps_failed.to_string().bright_red());
                }
            }

            Ok(())
        }

        Commands::Version => {
            println!("beads {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
