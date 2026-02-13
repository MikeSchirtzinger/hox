//! Pattern capture and storage

use chrono::{DateTime, Utc};
use hox_core::{ChangeId, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// Trace data from an orchestration run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationTrace {
    /// Number of iterations taken
    pub iterations: usize,
    /// Type of task (e.g., "rust-feature", "debugging", "refactor")
    pub task_type: String,
    /// Whether the orchestration succeeded
    pub success: bool,
    /// Backpressure configuration used
    pub backpressure_config: Option<String>,
    /// Agent performance metrics
    pub agent_performance: Option<AgentPerformance>,
}

/// Performance metrics for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerformance {
    /// Agent identifier
    pub agent_id: String,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Number of tasks completed
    pub task_count: usize,
}

/// A pattern-based suggestion for task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Name of the pattern being suggested
    pub pattern_name: String,
    /// Descriptive message explaining the suggestion
    pub message: String,
    /// Whether this suggestion is actionable (can be applied automatically)
    pub actionable: bool,
}

/// Context about a task for pattern matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    /// Type of task
    pub task_type: String,
    /// Programming language (if applicable)
    pub language: Option<String>,
}

/// Category of orchestration pattern
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PatternCategory {
    /// Task decomposition patterns
    Decomposition,
    /// Communication patterns
    Communication,
    /// Validation patterns
    Validation,
    /// Integration patterns
    Integration,
    /// Error handling patterns
    ErrorHandling,
    /// Custom category
    Custom(String),
}

impl std::fmt::Display for PatternCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decomposition => write!(f, "decomposition"),
            Self::Communication => write!(f, "communication"),
            Self::Validation => write!(f, "validation"),
            Self::Integration => write!(f, "integration"),
            Self::ErrorHandling => write!(f, "error_handling"),
            Self::Custom(name) => write!(f, "custom/{}", name),
        }
    }
}

/// A learned orchestration pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Unique identifier
    pub id: String,
    /// Pattern name
    pub name: String,
    /// Category
    pub category: PatternCategory,
    /// Description of the pattern
    pub description: String,
    /// When to apply this pattern
    pub when: String,
    /// The pattern content (instruction text)
    pub content: String,
    /// Success rate when this pattern was used
    pub success_rate: f32,
    /// Number of times this pattern has been used
    pub usage_count: u32,
    /// When this pattern was captured
    pub captured_at: DateTime<Utc>,
    /// Source change where this was learned
    pub source_change: Option<ChangeId>,
    /// Whether this pattern is approved
    pub approved: bool,
}

impl Pattern {
    pub fn new(
        name: impl Into<String>,
        category: PatternCategory,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            category,
            description: description.into(),
            when: String::new(),
            content: String::new(),
            success_rate: 0.0,
            usage_count: 0,
            captured_at: Utc::now(),
            source_change: None,
            approved: false,
        }
    }

    pub fn with_when(mut self, when: impl Into<String>) -> Self {
        self.when = when.into();
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_source(mut self, change_id: ChangeId) -> Self {
        self.source_change = Some(change_id);
        self
    }

    pub fn approve(&mut self) {
        self.approved = true;
    }

    pub fn record_usage(&mut self, success: bool) {
        self.usage_count += 1;
        // Update success rate with exponential moving average
        let alpha = 0.1;
        let result = if success { 1.0 } else { 0.0 };
        self.success_rate = alpha * result + (1.0 - alpha) * self.success_rate;
    }
}

/// Store for orchestration patterns (backed by hox-patterns branch)
pub struct PatternStore {
    patterns: HashMap<String, Pattern>,
    /// Path to the patterns directory (in hox-patterns branch)
    patterns_path: std::path::PathBuf,
}

impl PatternStore {
    pub fn new(patterns_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            patterns: HashMap::new(),
            patterns_path: patterns_path.into(),
        }
    }

    /// Load patterns from the hox-patterns branch
    pub async fn load(&mut self) -> Result<()> {
        let path = &self.patterns_path;

        if !path.exists() {
            info!("Patterns directory does not exist: {:?}", path);
            return Ok(());
        }

        // Load all .json files in the patterns directory
        let mut entries = tokio::fs::read_dir(path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();

            if entry_path.extension().is_some_and(|e| e == "json") {
                match self.load_pattern_file(&entry_path).await {
                    Ok(pattern) => {
                        debug!("Loaded pattern: {}", pattern.name);
                        self.patterns.insert(pattern.id.clone(), pattern);
                    }
                    Err(e) => {
                        debug!("Failed to load pattern from {:?}: {}", entry_path, e);
                    }
                }
            }
        }

        info!("Loaded {} patterns", self.patterns.len());
        Ok(())
    }

    async fn load_pattern_file(&self, path: &std::path::Path) -> Result<Pattern> {
        let content = tokio::fs::read_to_string(path).await?;
        let pattern: Pattern = serde_json::from_str(&content)?;
        Ok(pattern)
    }

    /// Save a pattern to the store
    pub async fn save(&mut self, pattern: Pattern) -> Result<()> {
        let filename = format!("{}.json", pattern.id);
        let path = self.patterns_path.join(&filename);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = serde_json::to_string_pretty(&pattern)?;
        tokio::fs::write(&path, content).await?;

        info!("Saved pattern {} to {:?}", pattern.name, path);
        self.patterns.insert(pattern.id.clone(), pattern);

        Ok(())
    }

    /// Get a pattern by ID
    pub fn get(&self, id: &str) -> Option<&Pattern> {
        self.patterns.get(id)
    }

    /// Get patterns by category
    pub fn by_category(&self, category: &PatternCategory) -> Vec<&Pattern> {
        self.patterns
            .values()
            .filter(|p| &p.category == category && p.approved)
            .collect()
    }

    /// Get all approved patterns
    pub fn approved(&self) -> Vec<&Pattern> {
        self.patterns.values().filter(|p| p.approved).collect()
    }

    /// Get pending patterns (not yet approved)
    pub fn pending(&self) -> Vec<&Pattern> {
        self.patterns.values().filter(|p| !p.approved).collect()
    }

    /// Format patterns for inclusion in orchestrator prompt
    pub fn format_for_prompt(&self) -> String {
        let mut output = String::from("## Learned Patterns\n\n");

        let categories = [
            PatternCategory::Decomposition,
            PatternCategory::Communication,
            PatternCategory::Validation,
            PatternCategory::Integration,
            PatternCategory::ErrorHandling,
        ];

        for category in categories {
            let patterns = self.by_category(&category);
            if !patterns.is_empty() {
                output.push_str(&format!("### {}\n\n", category));

                for pattern in patterns {
                    output.push_str(&format!(
                        "**{}** (success: {:.0}%)\n",
                        pattern.name,
                        pattern.success_rate * 100.0
                    ));
                    output.push_str(&format!("- When: {}\n", pattern.when));
                    output.push_str(&format!("- {}\n\n", pattern.content));
                }
            }
        }

        output
    }

    /// Propose a new pattern (pending approval)
    pub async fn propose(&mut self, pattern: Pattern) -> Result<()> {
        info!("Proposing pattern: {}", pattern.name);
        self.save(pattern).await
    }

    /// Approve a pending pattern
    pub async fn approve(&mut self, id: &str) -> Result<()> {
        if let Some(pattern) = self.patterns.get_mut(id) {
            pattern.approve();
            let pattern = pattern.clone();
            self.save(pattern).await?;
            info!("Approved pattern: {}", id);
        }
        Ok(())
    }
}

/// Pattern extraction from successful orchestration runs
pub struct PatternExtractor {
    store: PatternStore,
}

impl PatternExtractor {
    pub fn new(store: PatternStore) -> Self {
        Self { store }
    }

    /// Extract patterns from an orchestration trace
    pub fn extract_from_trace(&self, trace: &OrchestrationTrace) -> Vec<Pattern> {
        let mut patterns = Vec::new();

        // Extract fast convergence pattern
        if trace.iterations < 10 && trace.success {
            let mut pattern = Pattern::new(
                "fast_convergence",
                PatternCategory::Decomposition,
                format!(
                    "Task type '{}' converged in {} iterations",
                    trace.task_type, trace.iterations
                ),
            )
            .with_when(format!("Working on {} tasks", trace.task_type))
            .with_content(format!(
                "This task type typically converges quickly. Consider using similar decomposition strategies."
            ));

            pattern.success_rate = 0.7;
            patterns.push(pattern);
        }

        // Extract effective agent pattern
        if let Some(ref perf) = trace.agent_performance {
            if perf.success_rate > 0.8 {
                let mut pattern = Pattern::new(
                    "effective_agent",
                    PatternCategory::Communication,
                    format!(
                        "Agent {} has {:.0}% success rate on {} tasks",
                        perf.agent_id,
                        perf.success_rate * 100.0,
                        perf.task_count
                    ),
                )
                .with_when(format!("Assigning tasks to {}", perf.agent_id))
                .with_content(format!(
                    "Agent {} has proven effective. Consider prioritizing this agent for similar tasks.",
                    perf.agent_id
                ));

                pattern.success_rate = perf.success_rate as f32;
                patterns.push(pattern);
            }
        }

        patterns
    }

    /// Suggest patterns based on task context
    pub fn suggest(&self, context: &TaskContext) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();

        // Find patterns matching this task type
        let all_patterns: Vec<&Pattern> = self.store.approved().into_iter().collect();

        for pattern in all_patterns {
            // Filter to patterns with high confidence and matching context
            if pattern.success_rate > 0.6
                && (pattern.when.contains(&context.task_type)
                    || pattern.description.contains(&context.task_type))
            {
                suggestions.push(Suggestion {
                    pattern_name: pattern.name.clone(),
                    message: format!(
                        "{} (success rate: {:.0}%) - {}",
                        pattern.name,
                        pattern.success_rate * 100.0,
                        pattern.content
                    ),
                    actionable: pattern.approved,
                });
            }
        }

        suggestions
    }
}

/// Built-in patterns that are always available
pub fn builtin_patterns() -> Vec<Pattern> {
    vec![
        Pattern::new(
            "Types First",
            PatternCategory::Decomposition,
            "Define shared types before parallel implementation",
        )
        .with_when("Starting a feature with multiple parallel agents")
        .with_content("Create a Phase 0 (contracts) that defines all shared types and interfaces. All implementation agents wait for this phase to complete before starting their work."),

        Pattern::new(
            "Early Alignment",
            PatternCategory::Communication,
            "Request alignment at phase start, not mid-work",
        )
        .with_when("An agent needs clarification on shared decisions")
        .with_content("When joining a phase, immediately review the contracts branch for existing decisions. If something is unclear, send an alignment request BEFORE starting implementation work."),

        Pattern::new(
            "Integration Phase",
            PatternCategory::Integration,
            "Always plan an integration phase",
        )
        .with_when("Decomposing parallel work")
        .with_content("After all parallel implementation phases, always include an integration phase. This phase merges all agent work and resolves any conflicts before validation."),

        Pattern::new(
            "Mutation Compliance",
            PatternCategory::Communication,
            "Orchestrator mutations are non-negotiable",
        )
        .with_when("Receiving a mutation message from orchestrator")
        .with_content("When you receive a mutation conflict, STOP current work, review the mutation, and refactor your work to comply. Do not attempt to work around or ignore mutations."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_pattern_store() {
        let dir = tempdir().unwrap();
        let mut store = PatternStore::new(dir.path());

        let pattern = Pattern::new(
            "Test Pattern",
            PatternCategory::Decomposition,
            "A test pattern",
        )
        .with_content("Do the thing");

        store.save(pattern).await.unwrap();

        // Reload
        let mut store2 = PatternStore::new(dir.path());
        store2.load().await.unwrap();

        assert_eq!(store2.patterns.len(), 1);
    }

    #[test]
    fn test_builtin_patterns() {
        let patterns = builtin_patterns();
        assert!(!patterns.is_empty());

        for pattern in &patterns {
            assert!(!pattern.name.is_empty());
            assert!(!pattern.content.is_empty());
        }
    }

    #[tokio::test]
    async fn test_extract_fast_convergence() {
        let dir = tempdir().unwrap();
        let store = PatternStore::new(dir.path());
        let extractor = PatternExtractor::new(store);

        let trace = OrchestrationTrace {
            iterations: 8,
            task_type: "rust-feature".to_string(),
            success: true,
            backpressure_config: None,
            agent_performance: None,
        };

        let patterns = extractor.extract_from_trace(&trace);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].name, "fast_convergence");
        assert_eq!(patterns[0].success_rate, 0.7);
    }

    #[tokio::test]
    async fn test_extract_no_patterns_on_failure() {
        let dir = tempdir().unwrap();
        let store = PatternStore::new(dir.path());
        let extractor = PatternExtractor::new(store);

        let trace = OrchestrationTrace {
            iterations: 8,
            task_type: "rust-feature".to_string(),
            success: false,
            backpressure_config: None,
            agent_performance: None,
        };

        let patterns = extractor.extract_from_trace(&trace);
        assert!(patterns.is_empty());
    }

    #[tokio::test]
    async fn test_extract_effective_agent() {
        let dir = tempdir().unwrap();
        let store = PatternStore::new(dir.path());
        let extractor = PatternExtractor::new(store);

        let trace = OrchestrationTrace {
            iterations: 15,
            task_type: "debugging".to_string(),
            success: true,
            backpressure_config: None,
            agent_performance: Some(AgentPerformance {
                agent_id: "agent-123".to_string(),
                success_rate: 0.85,
                task_count: 20,
            }),
        };

        let patterns = extractor.extract_from_trace(&trace);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].name, "effective_agent");
        assert_eq!(patterns[0].success_rate, 0.85);
    }

    #[tokio::test]
    async fn test_extract_multiple_patterns() {
        let dir = tempdir().unwrap();
        let store = PatternStore::new(dir.path());
        let extractor = PatternExtractor::new(store);

        let trace = OrchestrationTrace {
            iterations: 9,
            task_type: "refactor".to_string(),
            success: true,
            backpressure_config: None,
            agent_performance: Some(AgentPerformance {
                agent_id: "agent-456".to_string(),
                success_rate: 0.9,
                task_count: 30,
            }),
        };

        let patterns = extractor.extract_from_trace(&trace);
        assert_eq!(patterns.len(), 2);
    }

    #[tokio::test]
    async fn test_suggest_patterns() {
        let dir = tempdir().unwrap();
        let mut store = PatternStore::new(dir.path());

        // Create and save a high-confidence pattern
        let mut pattern = Pattern::new(
            "test_pattern",
            PatternCategory::Decomposition,
            "A test pattern for rust-feature tasks",
        )
        .with_when("Working on rust-feature tasks")
        .with_content("Use this approach for Rust features");

        pattern.success_rate = 0.75;
        pattern.approve();
        store.save(pattern).await.unwrap();

        let extractor = PatternExtractor::new(store);

        let context = TaskContext {
            task_type: "rust-feature".to_string(),
            language: Some("rust".to_string()),
        };

        let suggestions = extractor.suggest(&context);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].pattern_name, "test_pattern");
        assert!(suggestions[0].actionable);
    }

    #[tokio::test]
    async fn test_suggest_filters_low_confidence() {
        let dir = tempdir().unwrap();
        let mut store = PatternStore::new(dir.path());

        // Create low-confidence pattern
        let mut pattern = Pattern::new(
            "low_confidence",
            PatternCategory::Decomposition,
            "Low confidence pattern",
        )
        .with_when("Working on rust-feature tasks")
        .with_content("Don't use this");

        pattern.success_rate = 0.5;
        pattern.approve();
        store.save(pattern).await.unwrap();

        let extractor = PatternExtractor::new(store);

        let context = TaskContext {
            task_type: "rust-feature".to_string(),
            language: Some("rust".to_string()),
        };

        let suggestions = extractor.suggest(&context);
        assert!(suggestions.is_empty());
    }
}
