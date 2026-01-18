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
use clap::{Parser, Subcommand};
use hox_core::OrchestratorId;
use hox_evolution::{builtin_patterns, PatternStore};
use hox_jj::{JjCommand, JjExecutor, MetadataManager, RevsetQueries};
use hox_orchestrator::{Orchestrator, OrchestratorConfig, PhaseManager};
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
        Commands::Init { path } => cmd_init(path).await,
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
    }
}

async fn cmd_init(path: PathBuf) -> Result<()> {
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
