//! Hox CLI - JJ-native multi-agent orchestration
//!
//! Usage:
//!   hox init                    Initialize Hox in current repo
//!   hox orchestrate <plan>      Run orchestration on a plan
//!   hox status                  Show orchestration status
//!   hox patterns list           List learned patterns
//!   hox patterns propose <file> Propose a new pattern
//!   hox validate <change>       Run validation on a change

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use hox_agent::{BackpressureResult, ExternalLoopState, LoopConfig, Model};
use hox_core::{DelegationStrategy, HandoffContext, OrchestratorId, Task};
use hox_evolution::{builtin_patterns, PatternStore};
use hox_jj::{BookmarkManager, JjCommand, JjExecutor, MetadataManager, RevsetQueries};
use hox_orchestrator::{
    create_initial_state, load_state, run_external_iteration, save_state, Orchestrator,
    OrchestratorConfig, PhaseManager,
};
use hox_planning::{cli_tool_prd, example_prd, PrdDecomposer, ProjectRequirementsDocument};
use hox_validation::{ByzantineConsensus, ConsensusConfig, Validator, ValidatorConfig};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "hox")]
#[command(author, version, about = "JJ-native multi-agent orchestration")]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Hox in the current repository
    Init {
        /// Repository path (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Generate example PRD template
        #[arg(long)]
        prd: bool,

        /// Load PRD from existing JSON file
        #[arg(long, value_name = "FILE")]
        from_prd: Option<PathBuf>,

        /// Use CLI tool PRD template (requires --prd)
        #[arg(long)]
        cli_tool: bool,
    },

    /// Run orchestration on a plan
    Orchestrate {
        /// Plan description or file
        plan: String,

        /// Number of orchestrators to spawn
        #[arg(short = 'n', long, default_value = "1")]
        orchestrators: usize,

        /// Maximum agents per orchestrator
        #[arg(long, default_value = "4")]
        max_agents: usize,

        /// Enable hierarchical delegation (spawn child orchestrators for epics)
        #[arg(long)]
        delegate: bool,
    },

    /// Show orchestration status
    Status,

    /// Pattern management
    Patterns {
        #[command(subcommand)]
        action: PatternCommands,
    },

    /// Run validation on a change
    Validate {
        /// Change ID to validate
        change: String,

        /// Number of validators (3f+1 for f faulty)
        #[arg(short = 'n', long, default_value = "4")]
        validators: usize,
    },

    /// Query changes using Hox metadata
    Query {
        /// Revset query
        revset: String,
    },

    /// Set Hox metadata on current change
    Set {
        /// Priority (critical, high, medium, low)
        #[arg(long)]
        priority: Option<String>,

        /// Status (open, in_progress, blocked, review, done, abandoned)
        #[arg(long)]
        status: Option<String>,

        /// Agent identifier
        #[arg(long)]
        agent: Option<String>,

        /// Orchestrator identifier
        #[arg(long)]
        orchestrator: Option<String>,
    },

    /// Run Ralph-style autonomous loop on a task
    Loop {
        #[command(subcommand)]
        action: LoopCommands,
    },

    /// Launch the observability dashboard
    Dashboard {
        /// Refresh interval in milliseconds
        #[arg(short, long, default_value = "500")]
        refresh: u64,

        /// Maximum oplog entries to show
        #[arg(long, default_value = "50")]
        max_oplog: usize,
    },

    /// Manage bookmarks for task assignments
    Bookmark {
        #[command(subcommand)]
        action: BookmarkCommands,
    },

    /// Rollback operations (recovery from bad agent output)
    Rollback {
        /// Agent name to rollback (cleans workspace)
        #[arg(long)]
        agent: Option<String>,

        /// Operation ID to restore to
        #[arg(long)]
        operation: Option<String>,

        /// Number of operations to undo
        #[arg(long)]
        count: Option<usize>,

        /// Remove agent workspace after rollback
        #[arg(long)]
        remove_workspace: bool,
    },

    /// DAG manipulation commands for task restructuring
    Dag {
        #[command(subcommand)]
        action: DagCommands,
    },
}

/// DAG manipulation subcommands
#[derive(Subcommand)]
enum DagCommands {
    /// Convert sequential changes into parallel siblings
    Parallelize {
        /// Revset of changes to parallelize
        revset: String,
    },

    /// Auto-distribute working copy changes to ancestor commits
    Absorb {
        /// Optional paths to absorb (all changes if omitted)
        paths: Vec<String>,
    },

    /// Split a change into multiple changes by file groups
    Split {
        /// Change ID to split
        change_id: String,

        /// Files for the split (creates sibling with these files)
        files: Vec<String>,
    },

    /// Squash a change into its parent
    Squash {
        /// Change ID to squash
        change_id: String,
    },

    /// Squash specific files from source into target
    SquashInto {
        /// Source change ID
        #[arg(long)]
        from: String,

        /// Target change ID
        #[arg(long)]
        into: String,

        /// Optional paths to squash (all if omitted)
        paths: Vec<String>,
    },

    /// Duplicate a change for speculative execution
    Duplicate {
        /// Change ID to duplicate
        change_id: String,

        /// Optional destination change (parent for duplicate)
        #[arg(short, long)]
        destination: Option<String>,
    },

    /// Create a change that undoes another (safe revert)
    Backout {
        /// Change ID to backout
        change_id: String,
    },

    /// Show evolution log for a change (audit trail)
    Evolog {
        /// Change ID to show evolution for
        change_id: String,
    },

    /// Clean up redundant parent relationships
    SimplifyParents {
        /// Change ID to simplify
        change_id: String,
    },
}

/// Bookmark management subcommands
#[derive(Subcommand)]
enum BookmarkCommands {
    /// Assign a task to an agent
    Assign {
        /// Agent name
        agent: String,

        /// Change ID (defaults to current change if omitted)
        change_id: Option<String>,
    },

    /// List bookmarks
    List {
        /// Filter pattern (e.g., "task/*", "agent/*/task/*")
        #[arg(default_value = "*")]
        pattern: String,
    },

    /// Find which agent owns a task
    Owner {
        /// Change ID
        change_id: String,
    },

    /// Unassign a task from an agent
    Unassign {
        /// Agent name
        agent: String,

        /// Change ID
        change_id: String,
    },

    /// List all tasks assigned to an agent
    AgentTasks {
        /// Agent name
        agent: String,
    },
}

/// Loop subcommands for Ralph-style autonomous iteration
#[derive(Subcommand)]
enum LoopCommands {
    /// Start a loop on a task
    Start {
        /// JJ change ID of the task to work on
        change_id: String,

        /// Maximum number of iterations
        #[arg(short = 'n', long, default_value = "20")]
        max_iterations: usize,

        /// Model to use (opus, sonnet, haiku)
        #[arg(short, long, default_value = "sonnet")]
        model: CliModel,

        /// Disable backpressure checks (tests/lints/builds)
        #[arg(long)]
        no_backpressure: bool,
    },

    /// Show loop status for a task
    Status {
        /// JJ change ID
        change_id: String,
    },

    /// Stop a running loop (marks task as blocked)
    Stop {
        /// JJ change ID
        change_id: String,
    },

    /// Run single external iteration (bash-orchestratable mode)
    External {
        /// JJ change ID of the task to work on
        #[arg(long)]
        change_id: String,

        /// Load state from JSON file (omit for first iteration)
        #[arg(long)]
        state_file: Option<PathBuf>,

        /// Write updated state to JSON file
        #[arg(long)]
        output_state: Option<PathBuf>,

        /// Disable backpressure checks (tests/lints/builds)
        #[arg(long)]
        no_backpressure: bool,

        /// Model to use (opus, sonnet, haiku)
        #[arg(short, long, default_value = "sonnet")]
        model: CliModel,

        /// Maximum tokens for agent response
        #[arg(long, default_value = "16000")]
        max_tokens: usize,

        /// Maximum iterations (for progress display)
        #[arg(short = 'n', long, default_value = "20")]
        max_iterations: usize,
    },
}

/// CLI-friendly model enum
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliModel {
    Opus,
    Sonnet,
    Haiku,
}

impl From<CliModel> for Model {
    fn from(m: CliModel) -> Self {
        match m {
            CliModel::Opus => Model::Opus,
            CliModel::Sonnet => Model::Sonnet,
            CliModel::Haiku => Model::Haiku,
        }
    }
}

#[derive(Subcommand)]
enum PatternCommands {
    /// List all patterns
    List {
        /// Show only pending patterns
        #[arg(long)]
        pending: bool,
    },

    /// Propose a new pattern from file
    Propose {
        /// Pattern file (JSON)
        file: PathBuf,
    },

    /// Approve a pending pattern
    Approve {
        /// Pattern ID
        id: String,
    },

    /// Show builtin patterns
    Builtin,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Init { path, prd, from_prd, cli_tool } => cmd_init(path, prd, from_prd, cli_tool).await,
        Commands::Orchestrate {
            plan,
            orchestrators,
            max_agents,
            delegate,
        } => cmd_orchestrate(plan, orchestrators, max_agents, delegate).await,
        Commands::Status => cmd_status().await,
        Commands::Patterns { action } => cmd_patterns(action).await,
        Commands::Validate { change, validators } => cmd_validate(change, validators).await,
        Commands::Query { revset } => cmd_query(revset).await,
        Commands::Set {
            priority,
            status,
            agent,
            orchestrator,
        } => cmd_set(priority, status, agent, orchestrator).await,
        Commands::Loop { action } => cmd_loop(action).await,
        Commands::Dashboard { refresh, max_oplog } => cmd_dashboard(refresh, max_oplog).await,
        Commands::Bookmark { action } => cmd_bookmark(action).await,
        Commands::Rollback {
            agent,
            operation,
            count,
            remove_workspace,
        } => cmd_rollback(agent, operation, count, remove_workspace).await,
        Commands::Dag { action } => cmd_dag(action).await,
    }
}

async fn cmd_init(path: PathBuf, prd: bool, from_prd: Option<PathBuf>, cli_tool: bool) -> Result<()> {
    info!("Initializing Hox in {:?}", path);

    // Create .hox directory structure
    let hox_dir = path.join(".hox");
    tokio::fs::create_dir_all(&hox_dir).await?;
    tokio::fs::create_dir_all(hox_dir.join("patterns")).await?;
    tokio::fs::create_dir_all(hox_dir.join("metrics")).await?;

    // Create initial config
    let config = serde_json::json!({
        "version": "0.1.0",
        "patterns_branch": "hox-patterns",
        "validation": {
            "fault_tolerance": 1,
            "threshold": 0.75
        }
    });

    tokio::fs::write(
        hox_dir.join("config.json"),
        serde_json::to_string_pretty(&config)?,
    )
    .await?;

    println!("Initialized Hox in {:?}", path);
    println!("Created:");
    println!("  .hox/config.json");
    println!("  .hox/patterns/");
    println!("  .hox/metrics/");
    println!();
    println!("To enable auto-formatting with jj fix, add to .jj/repo/config.toml:");
    println!("  [fix.tools.rustfmt]");
    println!("  command = [\"rustfmt\", \"--edition\", \"2021\"]");
    println!("  patterns = [\"glob:*.rs\"]");

    // Handle PRD generation/loading
    let prd_doc = if let Some(prd_file) = from_prd {
        // Load existing PRD from file
        let content = tokio::fs::read_to_string(&prd_file).await
            .context("Failed to read PRD file")?;
        let doc: ProjectRequirementsDocument = serde_json::from_str(&content)
            .context("Failed to parse PRD JSON")?;

        println!("\nLoaded PRD from: {:?}", prd_file);
        Some(doc)
    } else if prd {
        // Generate new PRD based on template
        let doc = if cli_tool {
            // Extract project name from current directory
            let project_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("my-cli-tool");
            cli_tool_prd(project_name)
        } else {
            example_prd()
        };

        println!("\nGenerated PRD template");
        Some(doc)
    } else {
        None
    };

    // If we have a PRD, save it and show decomposition
    if let Some(doc) = prd_doc {
        // Save PRD to .hox/prd.json
        let prd_path = hox_dir.join("prd.json");
        tokio::fs::write(
            &prd_path,
            serde_json::to_string_pretty(&doc)?,
        )
        .await?;

        println!("  .hox/prd.json");

        // Decompose and show summary
        let summary = PrdDecomposer::summarize(&doc);
        println!("\n{}", summary);

        // Optionally save decomposition details
        let (phases, tasks) = PrdDecomposer::decompose(&doc);

        let decomposition = serde_json::json!({
            "phases": phases,
            "tasks": tasks.iter().map(|t| serde_json::json!({
                "id": t.id,
                "title": t.title,
                "description": t.description,
                "priority": t.priority,
                "phase": t.phase,
                "status": t.status,
            })).collect::<Vec<_>>(),
        });

        tokio::fs::write(
            hox_dir.join("decomposition.json"),
            serde_json::to_string_pretty(&decomposition)?,
        )
        .await?;

        println!("  .hox/decomposition.json");
        println!("\nNext steps:");
        println!("  1. Review and edit .hox/prd.json as needed");
        println!("  2. Run 'hox orchestrate <plan>' to start execution");
        println!("  3. Use 'hox status' to monitor progress");
    }

    Ok(())
}

async fn cmd_orchestrate(plan: String, orchestrator_count: usize, max_agents: usize, delegate: bool) -> Result<()> {
    info!("Starting orchestration: {}", plan);

    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;

    for i in 0..orchestrator_count {
        let id = OrchestratorId::new('A', (i + 1) as u32);
        let mut config = OrchestratorConfig::new(id.clone(), jj.repo_root())
            .with_max_agents(max_agents);

        if delegate {
            config = config.with_delegation_strategy(DelegationStrategy::PhasePerChild);
        }

        let mut orchestrator = Orchestrator::with_executor(config, jj.clone()).await?;

        // Setup standard phases
        let phases = PhaseManager::standard_feature_phases(&plan);
        for phase in phases.phases() {
            orchestrator.add_phase(phase.clone());
        }

        // Initialize and run
        orchestrator.initialize().await?;

        if delegate {
            println!("Started orchestrator {} with hierarchical delegation", id);
            orchestrator.run_with_delegation().await?;
        } else {
            println!("Started orchestrator {}", id);
        }
    }

    println!("Orchestration {} with {} orchestrator(s)",
        if delegate { "completed" } else { "started" },
        orchestrator_count);
    if !delegate {
        println!("Use 'hox status' to check progress");
    }

    Ok(())
}

async fn cmd_status() -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let queries = RevsetQueries::new(jj);

    println!("Hox Status");
    println!("==========");

    // Find orchestrators - try bookmark query first, fallback to description search
    let orchestrators = match queries.all_orchestrators_by_bookmark().await {
        Ok(orcks) => orcks,
        Err(_) => queries.query("description(glob:\"Orchestrator: O-*\")").await?,
    };
    println!("\nOrchestrators: {}", orchestrators.len());

    // Find in-progress tasks
    let in_progress = queries.by_status("in_progress").await?;
    println!("In Progress: {}", in_progress.len());

    // Find blocked tasks
    let blocked = queries.by_status("blocked").await?;
    println!("Blocked: {}", blocked.len());

    // Find parallelizable tasks (Phase 6 power query)
    let parallelizable = queries.parallelizable_tasks().await?;
    println!("Parallelizable (independent heads): {}", parallelizable.len());

    // Find empty/abandoned changes (Phase 6 power query)
    let empty = queries.empty_changes().await?;
    if !empty.is_empty() {
        println!("Empty/Abandoned: {}", empty.len());
    }

    // Find conflicts
    let conflicts = queries.conflicts().await?;
    if !conflicts.is_empty() {
        println!("\nConflicts: {}", conflicts.len());
        for c in &conflicts {
            println!("  - {}", c);

            // Show what blocks this conflict (Phase 6 power query)
            let blockers = queries.blocking_conflicts(&c).await.unwrap_or_default();
            if !blockers.is_empty() {
                println!("    Blocked by: {} conflicting ancestor(s)", blockers.len());
            }
        }
    }

    Ok(())
}

async fn cmd_patterns(action: PatternCommands) -> Result<()> {
    let hox_dir = PathBuf::from(".hox");
    let mut store = PatternStore::new(hox_dir.join("patterns"));
    store.load().await?;

    match action {
        PatternCommands::List { pending } => {
            let patterns = if pending {
                store.pending()
            } else {
                store.approved()
            };

            if patterns.is_empty() {
                println!("No patterns found");
                return Ok(());
            }

            println!("Patterns:");
            for p in patterns {
                println!(
                    "  {} - {} ({})",
                    p.id,
                    p.name,
                    if p.approved { "approved" } else { "pending" }
                );
                println!("    Category: {}", p.category);
                println!("    Success: {:.0}%", p.success_rate * 100.0);
            }
        }

        PatternCommands::Propose { file } => {
            let content = tokio::fs::read_to_string(&file).await?;
            let pattern: hox_evolution::Pattern = serde_json::from_str(&content)?;

            store.propose(pattern.clone()).await?;
            println!("Proposed pattern: {} ({})", pattern.name, pattern.id);
            println!("Use 'hox patterns approve {}' to approve", pattern.id);
        }

        PatternCommands::Approve { id } => {
            store.approve(&id).await?;
            println!("Approved pattern: {}", id);
        }

        PatternCommands::Builtin => {
            println!("Builtin Patterns:");
            for p in builtin_patterns() {
                println!("\n{}", p.name);
                println!("  Category: {}", p.category);
                println!("  When: {}", p.when);
                println!("  Content: {}", p.content);
            }
        }
    }

    Ok(())
}

async fn cmd_validate(change: String, validator_count: usize) -> Result<()> {
    info!("Validating change: {}", change);

    let config = ConsensusConfig {
        fault_tolerance: (validator_count - 1) / 3,
        threshold: 0.75,
    };

    let mut consensus = ByzantineConsensus::new(config);

    // Run validators
    for i in 0..validator_count {
        let validator_config = ValidatorConfig::default();
        let validator = Validator::new(validator_config);

        let report = validator.validate(&change).await?;
        println!(
            "Validator {}: {:?} (score: {:.2})",
            i + 1,
            report.result,
            report.score
        );

        consensus.add_vote(hox_validation::Vote {
            validator_id: validator.id().to_string(),
            change_id: change.clone(),
            result: report.result.clone(),
            score: report.score,
            report,
        });
    }

    // Check consensus
    let result = consensus.reach_consensus(&change);
    println!("\nConsensus: {:?}", result);

    Ok(())
}

async fn cmd_query(revset: String) -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let queries = RevsetQueries::new(jj);

    let changes = queries.query(&revset).await?;

    if changes.is_empty() {
        println!("No changes match: {}", revset);
        return Ok(());
    }

    println!("Matching changes ({}):", changes.len());
    for c in changes {
        println!("  {}", c);
    }

    Ok(())
}

async fn cmd_set(
    priority: Option<String>,
    status: Option<String>,
    agent: Option<String>,
    orchestrator: Option<String>,
) -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let queries = RevsetQueries::new(jj.clone());

    let change_id = queries
        .current()
        .await?
        .ok_or_else(|| anyhow::anyhow!("No current change"))?;

    let manager = MetadataManager::new(jj);
    let mut metadata = manager.read(&change_id).await?;

    if let Some(p) = priority {
        metadata.priority = Some(p.parse().map_err(|e: String| anyhow::anyhow!(e))?);
    }

    if let Some(s) = status {
        metadata.status = Some(s.parse().map_err(|e: String| anyhow::anyhow!(e))?);
    }

    if let Some(a) = agent {
        metadata.agent = Some(a);
    }

    if let Some(o) = orchestrator {
        metadata.orchestrator = Some(o);
    }

    manager.set(&change_id, &metadata).await?;

    println!("Updated metadata on {}", change_id);

    Ok(())
}

async fn cmd_loop(action: LoopCommands) -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;

    match action {
        LoopCommands::Start {
            change_id,
            max_iterations,
            model,
            no_backpressure,
        } => {
            info!(
                "Starting loop on {} with model {:?}, max {} iterations",
                change_id, model, max_iterations
            );

            // Get task description from change
            let output = jj
                .exec(&["log", "-r", &change_id, "-T", "description", "--no-graph"])
                .await?;

            if !output.success {
                anyhow::bail!("Failed to get change description: {}", output.stderr);
            }

            let task = Task::new(&change_id, output.stdout.trim());

            // Create loop config
            let config = LoopConfig {
                max_iterations,
                model: model.into(),
                backpressure_enabled: !no_backpressure,
                max_tokens: 16000,
                max_budget_usd: None,
            };

            // Create and run orchestrator
            let orch_config = OrchestratorConfig::new(OrchestratorId::root(), jj.repo_root());
            let mut orchestrator = Orchestrator::with_executor(orch_config, jj).await?;

            println!("Starting Ralph-style loop...");
            println!("  Task: {}", task.description.lines().next().unwrap_or(""));
            println!("  Model: {:?}", model);
            println!("  Max iterations: {}", max_iterations);
            println!("  Backpressure: {}", if no_backpressure { "disabled" } else { "enabled" });
            println!();

            let result = orchestrator.run_loop(task, Some(config)).await?;

            println!();
            println!("Loop completed!");
            println!("  Iterations: {}", result.iterations);
            println!("  Success: {}", result.success);
            println!("  Stop reason: {:?}", result.stop_reason);
            println!("  Files created: {}", result.files_created.len());
            println!("  Files modified: {}", result.files_modified.len());
            println!(
                "  Tokens used: {} input, {} output",
                result.total_usage.input_tokens, result.total_usage.output_tokens
            );

            if !result.success {
                println!();
                println!("Final backpressure status:");
                for check in &result.final_status.checks {
                    println!("  {}: {}", check.name, if check.passed { "PASSED" } else { "FAILED" });
                }
            }
        }

        LoopCommands::Status { change_id } => {
            let manager = MetadataManager::new(jj.clone());
            let metadata = manager.read(&change_id).await?;

            println!("Loop status for {}:", change_id);

            if let Some(iteration) = metadata.loop_iteration {
                println!("  Current iteration: {}", iteration);
            } else {
                println!("  No loop in progress");
            }

            if let Some(max) = metadata.loop_max_iterations {
                println!("  Max iterations: {}", max);
            }

            if let Some(status) = metadata.status {
                println!("  Status: {}", status);
            }

            // Show task description
            let output = jj
                .exec(&["log", "-r", &change_id, "-T", "description", "--no-graph"])
                .await?;

            if output.success && !output.stdout.is_empty() {
                println!();
                println!("Description:");
                for line in output.stdout.lines().take(20) {
                    println!("  {}", line);
                }
            }
        }

        LoopCommands::Stop { change_id } => {
            let manager = MetadataManager::new(jj.clone());
            let mut metadata = manager.read(&change_id).await?;

            metadata.status = Some(hox_core::TaskStatus::Blocked);
            manager.set(&change_id, &metadata).await?;

            println!("Marked {} as blocked (loop stopped)", change_id);
        }

        LoopCommands::External {
            change_id,
            state_file,
            output_state,
            no_backpressure,
            model,
            max_tokens,
            max_iterations,
        } => {
            info!(
                "Running external iteration for {} with model {:?}",
                change_id, model
            );

            // Get task description from change
            let output = jj
                .exec(&["log", "-r", &change_id, "-T", "description", "--no-graph"])
                .await?;

            if !output.success {
                anyhow::bail!("Failed to get change description: {}", output.stderr);
            }

            let task = Task::new(&change_id, output.stdout.trim());

            // Load or create state
            let state = if let Some(state_path) = &state_file {
                load_state(state_path).await?
            } else {
                create_initial_state(jj.clone(), &task).await?
            };

            // Deserialize context from state
            let context: HandoffContext = serde_json::from_value(state.context.clone())
                .context("Failed to deserialize context from state")?;

            // Get backpressure from state or create initial
            let backpressure = state.backpressure.unwrap_or_else(BackpressureResult::all_pass);

            // Next iteration number
            let iteration = state.iteration + 1;

            // Run single iteration
            let result = run_external_iteration(
                &task,
                &context,
                &backpressure,
                iteration,
                max_iterations,
                model.into(),
                max_tokens,
                &jj.repo_root(),
                &jj,
                !no_backpressure,
            )
            .await?;

            // Output result as JSON to stdout
            let json = serde_json::to_string_pretty(&result)?;
            println!("{}", json);

            // Save updated state if requested
            if let Some(output_path) = output_state {
                let new_state = ExternalLoopState {
                    change_id: task.change_id.clone(),
                    iteration,
                    context: result.context.clone(),
                    backpressure: Some(BackpressureResult::all_pass()),
                    files_touched: {
                        let mut files = state.files_touched.clone();
                        files.extend(result.files_created.clone());
                        files.extend(result.files_modified.clone());
                        files
                    },
                };

                save_state(&new_state, &output_path).await?;
            }
        }
    }

    Ok(())
}

async fn cmd_dashboard(refresh_ms: u64, max_oplog: usize) -> Result<()> {
    info!("Launching observability dashboard");

    let config = hox_dashboard::DashboardConfig {
        refresh_ms,
        max_oplog_entries: max_oplog,
        local_time: true,
        metrics_path: None,
    };

    hox_dashboard::run(config).await?;

    Ok(())
}

async fn cmd_bookmark(action: BookmarkCommands) -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let bookmark_manager = BookmarkManager::new(jj.clone());

    match action {
        BookmarkCommands::Assign { agent, change_id } => {
            let change = if let Some(id) = change_id {
                id
            } else {
                // Use current change
                let queries = RevsetQueries::new(jj);
                queries
                    .current()
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("No current change"))?
            };

            bookmark_manager.assign_task(&agent, &change).await?;
            println!("Assigned task {} to agent {}", change, agent);
        }

        BookmarkCommands::List { pattern } => {
            let pattern_opt = if pattern == "*" { None } else { Some(pattern.as_str()) };
            let bookmarks = bookmark_manager.list(pattern_opt).await?;

            if bookmarks.is_empty() {
                println!("No bookmarks found");
                return Ok(());
            }

            println!("Bookmarks:");
            for bookmark in bookmarks {
                if let Some(tracking) = bookmark.tracking {
                    println!("  {} -> {} (tracking: {})", bookmark.name, bookmark.change_id, tracking);
                } else {
                    println!("  {} -> {}", bookmark.name, bookmark.change_id);
                }
            }
        }

        BookmarkCommands::Owner { change_id } => {
            let agent = bookmark_manager.task_agent(&change_id).await?;

            if let Some(agent_name) = agent {
                println!("Task {} is assigned to agent: {}", change_id, agent_name);
            } else {
                println!("Task {} is not assigned to any agent", change_id);
            }
        }

        BookmarkCommands::Unassign { agent, change_id } => {
            bookmark_manager.unassign_task(&agent, &change_id).await?;
            println!("Unassigned task {} from agent {}", change_id, agent);
        }

        BookmarkCommands::AgentTasks { agent } => {
            let tasks = bookmark_manager.agent_tasks(&agent).await?;

            if tasks.is_empty() {
                println!("Agent {} has no assigned tasks", agent);
                return Ok(());
            }

            println!("Tasks assigned to agent {}:", agent);
            for (task_id, change_id) in tasks {
                println!("  {} -> {}", task_id, change_id);
            }
        }
    }

    Ok(())
}

async fn cmd_rollback(
    agent: Option<String>,
    operation: Option<String>,
    count: Option<usize>,
    remove_workspace: bool,
) -> Result<()> {
    use hox_orchestrator::RecoveryManager;

    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let recovery_manager = RecoveryManager::new(jj.clone(), jj.repo_root().to_path_buf());

    // Determine rollback mode
    match (agent, operation, count) {
        // Rollback specific agent to snapshot
        (Some(agent_name), Some(op_id), _) => {
            info!("Rolling back agent {} to operation {}", agent_name, op_id);
            println!(
                "Rolling back agent {} to operation {}...",
                agent_name, op_id
            );

            let result = recovery_manager
                .rollback_agent(&agent_name, &op_id, remove_workspace)
                .await?;

            println!("Rollback complete:");
            println!("  Operations undone: {}", result.operations_undone);
            println!("  Agent cleaned: {}", result.agent_cleaned);
            println!("  Workspace removed: {}", result.workspace_removed);
        }

        // Restore to specific operation
        (None, Some(op_id), _) => {
            info!("Restoring to operation {}", op_id);
            println!("Restoring to operation {}...", op_id);

            // Create a recovery point from the operation ID
            let recovery_point = hox_orchestrator::RecoveryPoint::new(
                op_id.clone(),
                format!("Manual restore to {}", op_id),
            );

            let result = recovery_manager.restore_from(&recovery_point).await?;

            println!("Restore complete:");
            println!("  Operations undone: {}", result.operations_undone);
        }

        // Undo last N operations
        (None, None, Some(n)) => {
            info!("Undoing last {} operations", n);
            println!("Undoing last {} operations...", n);

            let result = recovery_manager.rollback_operations(n).await?;

            println!("Rollback complete:");
            println!("  Operations undone: {}", result.operations_undone);
        }

        // List recent operations (no rollback)
        (None, None, None) => {
            println!("Recent operations:");

            let operations = recovery_manager.recent_operations(10).await?;

            if operations.is_empty() {
                println!("  No operations found");
                return Ok(());
            }

            for op in operations {
                println!("  {} - {} ({})", op.id, op.description, op.timestamp);
            }

            println!("\nUsage:");
            println!("  hox rollback --operation <op-id>        # Restore to specific operation");
            println!("  hox rollback --count <n>                # Undo last N operations");
            println!("  hox rollback --agent <name> --operation <op-id> # Rollback agent work");
        }

        // Invalid combinations
        _ => {
            anyhow::bail!("Invalid rollback options. Use --help for usage information.");
        }
    }

    Ok(())
}

async fn cmd_dag(action: DagCommands) -> Result<()> {
    use hox_jj::DagOperations;

    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let dag_ops = DagOperations::new(jj);

    match action {
        DagCommands::Parallelize { revset } => {
            info!("Parallelizing changes in revset: {}", revset);
            println!("Parallelizing changes: {}", revset);

            let result = dag_ops.parallelize(&revset).await?;

            println!("Parallelize complete:");
            println!("  Changes restructured: {}", result.changes_restructured);
            println!("  Clean: {}", result.clean);

            if !result.conflicts.is_empty() {
                println!("  Conflicts detected:");
                for conflict in &result.conflicts {
                    println!("    - {}", conflict);
                }
            }
        }

        DagCommands::Absorb { paths } => {
            info!("Absorbing changes");
            println!("Absorbing changes into ancestor commits...");

            let paths_refs: Option<Vec<&str>> = if paths.is_empty() {
                None
            } else {
                Some(paths.iter().map(|s| s.as_str()).collect())
            };

            let result = dag_ops
                .absorb(paths_refs.as_deref())
                .await?;

            println!("Absorb complete:");
            println!("  Hunks absorbed: {}", result.hunks_absorbed);
            println!("  Affected changes: {}", result.affected_changes.len());

            if !result.affected_changes.is_empty() {
                println!("  Changes modified:");
                for change_id in &result.affected_changes {
                    println!("    - {}", change_id);
                }
            }
        }

        DagCommands::Split { change_id, files } => {
            info!("Splitting change {} by files", change_id);
            println!("Splitting change {}...", change_id);

            if files.is_empty() {
                anyhow::bail!("No files provided for split. Specify at least one file.");
            }

            let file_groups = vec![files];
            let result = dag_ops.split_by_files(&change_id, &file_groups).await?;

            println!("Split complete:");
            println!("  New changes created: {}", result.new_changes.len());

            if !result.new_changes.is_empty() {
                println!("  New change IDs:");
                for new_change in &result.new_changes {
                    println!("    - {}", new_change);
                }
            }
        }

        DagCommands::Squash { change_id } => {
            info!("Squashing change {}", change_id);
            println!("Squashing change {} into parent...", change_id);

            dag_ops.squash(&change_id).await?;

            println!("Squash complete: {} folded into parent", change_id);
        }

        DagCommands::SquashInto { from, into, paths } => {
            info!("Squashing from {} into {}", from, into);
            println!("Squashing from {} into {}...", from, into);

            let paths_refs: Option<Vec<&str>> = if paths.is_empty() {
                None
            } else {
                Some(paths.iter().map(|s| s.as_str()).collect())
            };

            dag_ops
                .squash_into(&from, &into, paths_refs.as_deref())
                .await?;

            if let Some(path_list) = paths_refs {
                println!(
                    "Squash complete: moved {} files from {} to {}",
                    path_list.len(),
                    from,
                    into
                );
            } else {
                println!("Squash complete: moved all changes from {} to {}", from, into);
            }
        }

        DagCommands::Duplicate { change_id, destination } => {
            info!("Duplicating change {}", change_id);
            println!("Duplicating change {}...", change_id);

            let new_change_id = dag_ops
                .duplicate(&change_id, destination.as_deref())
                .await?;

            println!("Duplicate complete:");
            println!("  Original: {}", change_id);
            println!("  New change ID: {}", new_change_id);

            if let Some(dest) = destination {
                println!("  Destination: {}", dest);
            }
        }

        DagCommands::Backout { change_id } => {
            info!("Creating backout for {}", change_id);
            println!("Creating backout change for {}...", change_id);

            let backout_id = dag_ops.backout(&change_id).await?;

            println!("Backout complete:");
            println!("  Original change: {}", change_id);
            println!("  Backout change ID: {}", backout_id);
            println!("\nThis change reverses the effects of {} without destructive history editing.", change_id);
        }

        DagCommands::Evolog { change_id } => {
            info!("Getting evolution log for {}", change_id);
            println!("Evolution log for {}:", change_id);
            println!();

            let entries = dag_ops.evolution_log(&change_id).await?;

            if entries.is_empty() {
                println!("No evolution history found");
                return Ok(());
            }

            println!("Complete audit trail ({} entries):", entries.len());
            for (i, entry) in entries.iter().enumerate() {
                println!("  {}. {} ({})", i + 1, entry.commit_id, entry.timestamp);
                println!("     {}", entry.description);
            }
        }

        DagCommands::SimplifyParents { change_id } => {
            info!("Simplifying parents for {}", change_id);
            println!("Simplifying parent relationships for {}...", change_id);

            dag_ops.simplify_parents(&change_id).await?;

            println!("Simplify complete: removed redundant parent relationships");
        }
    }

    Ok(())
}
