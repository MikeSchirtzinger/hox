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
use hox_core::{HandoffContext, OrchestratorId, Task};
use hox_evolution::{builtin_patterns, PatternStore};
use hox_jj::{JjCommand, JjExecutor, MetadataManager, RevsetQueries};
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
        } => cmd_orchestrate(plan, orchestrators, max_agents).await,
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

async fn cmd_orchestrate(plan: String, orchestrator_count: usize, max_agents: usize) -> Result<()> {
    info!("Starting orchestration: {}", plan);

    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;

    for i in 0..orchestrator_count {
        let id = OrchestratorId::new('A', (i + 1) as u32);
        let config = OrchestratorConfig::new(id.clone(), jj.repo_root())
            .with_max_agents(max_agents);

        let mut orchestrator = Orchestrator::with_executor(config, jj.clone()).await?;

        // Setup standard phases
        let phases = PhaseManager::standard_feature_phases(&plan);
        for phase in phases.phases() {
            orchestrator.add_phase(phase.clone());
        }

        // Initialize and run
        orchestrator.initialize().await?;
        println!("Started orchestrator {}", id);
    }

    println!("Orchestration started with {} orchestrator(s)", orchestrator_count);
    println!("Use 'hox status' to check progress");

    Ok(())
}

async fn cmd_status() -> Result<()> {
    let jj = JjCommand::detect().await.context("Not in a JJ repository")?;
    let queries = RevsetQueries::new(jj);

    println!("Hox Status");
    println!("==========");

    // Find orchestrators
    let orchestrators = queries
        .query("description(glob:\"Orchestrator: O-*\")")
        .await?;
    println!("\nOrchestrators: {}", orchestrators.len());

    // Find in-progress tasks
    let in_progress = queries.by_status("in_progress").await?;
    println!("In Progress: {}", in_progress.len());

    // Find blocked tasks
    let blocked = queries.by_status("blocked").await?;
    println!("Blocked: {}", blocked.len());

    // Find conflicts
    let conflicts = queries.conflicts().await?;
    if !conflicts.is_empty() {
        println!("\nConflicts: {}", conflicts.len());
        for c in &conflicts {
            println!("  - {}", c);
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
                println!("  Tests: {}", if result.final_status.tests_passed { "PASSED" } else { "FAILED" });
                println!("  Lints: {}", if result.final_status.lints_passed { "PASSED" } else { "FAILED" });
                println!("  Builds: {}", if result.final_status.builds_passed { "PASSED" } else { "FAILED" });
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
                    backpressure: Some(BackpressureResult {
                        tests_passed: result.success,
                        lints_passed: result.success,
                        builds_passed: result.success,
                        errors: Vec::new(),
                    }),
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
