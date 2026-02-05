//! Conflict resolution pipeline for Hox orchestration
//!
//! This module provides automated conflict resolution strategies:
//! - Auto-format resolution via `jj fix`
//! - Pick-side resolution (ours/theirs)
//! - Agent-based semantic resolution (future)
//! - Human review escalation

use hox_core::{HoxError, Result};
use hox_jj::{JjExecutor, RevsetQueries};
use tracing::{debug, info, warn};

/// Strategy for resolving a specific conflict
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionStrategy {
    /// Auto-format to resolve whitespace/formatting conflicts
    JjFix,
    /// Use one side of the conflict
    PickSide { side: ConflictSide },
    /// Spawn an AI agent to resolve semantically
    SpawnAgent { prompt_context: String },
    /// Escalate to human review
    HumanReview { reason: String },
}

/// Which side to pick in a conflict
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictSide {
    /// Use our version (current agent's work)
    Ours,
    /// Use their version (incoming changes)
    Theirs,
}

/// Information about a conflict
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub change_id: String,
    pub files: Vec<String>,
    pub is_formatting_only: bool,
}

/// Report of resolution attempt results
#[derive(Debug, Clone, Default)]
pub struct ResolutionReport {
    pub total_conflicts: usize,
    pub auto_resolved: usize,
    pub agent_resolved: usize,
    pub needs_human: usize,
    pub failed: usize,
}

impl ResolutionReport {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Conflict resolution pipeline
pub struct ConflictResolver<E: JjExecutor> {
    executor: E,
}

impl<E: JjExecutor + Clone> ConflictResolver<E> {
    /// Create a new conflict resolver
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Analyze conflicts on a change - find conflicted files
    pub async fn analyze(&self, change_id: &str) -> Result<Vec<ConflictInfo>> {
        debug!("Analyzing conflicts for change {}", change_id);

        // Get diff stats to see which files are conflicted
        let output = self
            .executor
            .exec(&["diff", "-r", change_id, "--stat"])
            .await?;

        if !output.success {
            return Err(HoxError::JjCommand(format!(
                "Failed to get diff for {}: {}",
                change_id, output.stderr
            )));
        }

        // Parse conflicted files from diff output
        let files: Vec<String> = output
            .stdout
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("Total:"))
            .filter_map(|line| {
                // Diff stat format: " file.rs | 10 +++++-----"
                line.split('|').next().map(|s| s.trim().to_string())
            })
            .collect();

        // Check if conflicts are formatting-only
        // Simple heuristic: if all files are .rs and diff is small, likely formatting
        let is_formatting_only =
            files.iter().all(|f| f.ends_with(".rs")) && output.stdout.lines().count() < 20;

        if files.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![ConflictInfo {
                change_id: change_id.to_string(),
                files,
                is_formatting_only,
            }])
        }
    }

    /// Determine best strategy for a given conflict
    pub fn recommend_strategy(&self, info: &ConflictInfo) -> ResolutionStrategy {
        // Strategy selection logic:
        // 1. Formatting-only conflicts → JjFix
        if info.is_formatting_only {
            debug!("Recommending JjFix for formatting conflict");
            return ResolutionStrategy::JjFix;
        }

        // 2. Config/generated files → PickSide (prefer ours)
        let has_config_files = info.files.iter().any(|f| {
            f.ends_with(".toml")
                || f.ends_with(".json")
                || f.ends_with(".yaml")
                || f.ends_with(".yml")
                || f.contains("Cargo.lock")
                || f.contains("package-lock.json")
        });

        if has_config_files {
            debug!("Recommending PickSide(Ours) for config files");
            return ResolutionStrategy::PickSide {
                side: ConflictSide::Ours,
            };
        }

        // 3. Complex semantic conflicts → SpawnAgent (not yet implemented)
        // For now, we escalate complex conflicts to human review
        debug!("Recommending HumanReview for complex conflict");
        ResolutionStrategy::HumanReview {
            reason: format!("Complex semantic conflict in {} files", info.files.len()),
        }
    }

    /// Execute a resolution strategy
    pub async fn resolve(
        &self,
        info: &ConflictInfo,
        strategy: &ResolutionStrategy,
    ) -> Result<bool> {
        match strategy {
            ResolutionStrategy::JjFix => self.resolve_with_jj_fix(&info.change_id).await,
            ResolutionStrategy::PickSide { side } => {
                self.resolve_with_pick_side(&info.change_id, side).await
            }
            ResolutionStrategy::SpawnAgent { prompt_context } => {
                warn!(
                    "SpawnAgent resolution not yet implemented: {}",
                    prompt_context
                );
                Ok(false)
            }
            ResolutionStrategy::HumanReview { reason } => {
                info!(
                    "Conflict requires human review ({}): {}",
                    reason, info.change_id
                );
                Ok(false)
            }
        }
    }

    /// Run the full pipeline: detect -> analyze -> strategize -> resolve
    pub async fn resolve_all(&self) -> Result<ResolutionReport> {
        let mut report = ResolutionReport::new();

        // Find all conflicted changes
        let queries = RevsetQueries::new(self.executor.clone());
        let conflicts: Vec<String> = queries.conflicts().await?;

        report.total_conflicts = conflicts.len();

        if conflicts.is_empty() {
            info!("No conflicts found");
            return Ok(report);
        }

        info!("Found {} conflicted changes", conflicts.len());

        // Process each conflict
        for change_id in conflicts {
            // Analyze the conflict
            let conflict_infos = self.analyze(&change_id).await?;

            for info in conflict_infos {
                // Determine strategy
                let strategy = self.recommend_strategy(&info);
                debug!(
                    "Using strategy {:?} for change {}",
                    strategy, info.change_id
                );

                // Attempt resolution
                match self.resolve(&info, &strategy).await {
                    Ok(true) => {
                        info!("Successfully resolved conflict in {}", info.change_id);
                        report.auto_resolved += 1;
                    }
                    Ok(false) => {
                        // Resolution strategy was "don't auto-resolve"
                        match strategy {
                            ResolutionStrategy::HumanReview { .. } => {
                                report.needs_human += 1;
                            }
                            ResolutionStrategy::SpawnAgent { .. } => {
                                // Not implemented yet, count as needs human
                                report.needs_human += 1;
                            }
                            // JjFix and PickSide strategies that return false count as failed
                            _ => {
                                report.failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to resolve conflict in {}: {}", info.change_id, e);
                        report.failed += 1;
                    }
                }
            }
        }

        info!(
            "Conflict resolution complete: {} auto-resolved, {} need human review, {} failed",
            report.auto_resolved, report.needs_human, report.failed
        );

        Ok(report)
    }

    /// Resolve using jj fix (auto-format)
    async fn resolve_with_jj_fix(&self, change_id: &str) -> Result<bool> {
        info!("Attempting to resolve {} with jj fix", change_id);

        let output = self.executor.exec(&["fix", "-s", change_id]).await?;

        if output.success {
            // Check if conflicts are gone
            let queries = RevsetQueries::new(self.executor.clone());
            let remaining_conflicts = queries.conflicts().await?;

            if !remaining_conflicts.contains(&change_id.to_string()) {
                info!("Successfully resolved {} with jj fix", change_id);
                Ok(true)
            } else {
                warn!("jj fix did not resolve conflict in {}", change_id);
                Ok(false)
            }
        } else {
            warn!("jj fix failed for {}: {}", change_id, output.stderr);
            Ok(false)
        }
    }

    /// Resolve by picking one side
    async fn resolve_with_pick_side(&self, change_id: &str, side: &ConflictSide) -> Result<bool> {
        let side_str = match side {
            ConflictSide::Ours => ":ours",
            ConflictSide::Theirs => ":theirs",
        };

        info!("Resolving {} by picking {}", change_id, side_str);

        let output = self
            .executor
            .exec(&["resolve", "-r", change_id, "--tool", side_str])
            .await?;

        if output.success {
            info!("Successfully resolved {} with {}", change_id, side_str);
            Ok(true)
        } else {
            warn!(
                "Failed to resolve {} with {}: {}",
                change_id, side_str, output.stderr
            );
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hox_jj::{JjOutput, MockJjExecutor};

    #[tokio::test]
    async fn test_analyze_no_conflicts() {
        let executor = MockJjExecutor::new().with_response(
            "diff -r test-change --stat",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let resolver = ConflictResolver::new(executor);
        let result = resolver.analyze("test-change").await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_formatting_conflict() {
        let executor = MockJjExecutor::new().with_response(
            "diff -r test-change --stat",
            JjOutput {
                stdout: " src/main.rs | 5 ++---\n src/lib.rs  | 3 ++-\n".to_string(),
                stderr: String::new(),
                success: true,
            },
        );

        let resolver = ConflictResolver::new(executor);
        let result = resolver.analyze("test-change").await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].files.len(), 2);
        assert!(result[0].is_formatting_only);
    }

    #[test]
    fn test_recommend_strategy_formatting() {
        let executor = MockJjExecutor::new();
        let resolver = ConflictResolver::new(executor);

        let info = ConflictInfo {
            change_id: "test".to_string(),
            files: vec!["src/main.rs".to_string()],
            is_formatting_only: true,
        };

        let strategy = resolver.recommend_strategy(&info);
        assert_eq!(strategy, ResolutionStrategy::JjFix);
    }

    #[test]
    fn test_recommend_strategy_config_files() {
        let executor = MockJjExecutor::new();
        let resolver = ConflictResolver::new(executor);

        let info = ConflictInfo {
            change_id: "test".to_string(),
            files: vec!["Cargo.toml".to_string()],
            is_formatting_only: false,
        };

        let strategy = resolver.recommend_strategy(&info);
        assert_eq!(
            strategy,
            ResolutionStrategy::PickSide {
                side: ConflictSide::Ours
            }
        );
    }

    #[test]
    fn test_recommend_strategy_complex() {
        let executor = MockJjExecutor::new();
        let resolver = ConflictResolver::new(executor);

        let info = ConflictInfo {
            change_id: "test".to_string(),
            files: vec!["src/complex.rs".to_string(), "src/another.rs".to_string()],
            is_formatting_only: false,
        };

        let strategy = resolver.recommend_strategy(&info);
        assert!(matches!(strategy, ResolutionStrategy::HumanReview { .. }));
    }

    #[tokio::test]
    async fn test_resolve_with_jj_fix_success() {
        let executor = MockJjExecutor::new()
            .with_response(
                "fix -s test-change",
                JjOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                },
            )
            .with_response(
                "log -r conflicts() -T change_id ++ \"\\n\" --no-graph",
                JjOutput {
                    stdout: String::new(), // No conflicts remaining
                    stderr: String::new(),
                    success: true,
                },
            );

        let resolver = ConflictResolver::new(executor);
        let result = resolver.resolve_with_jj_fix("test-change").await.unwrap();

        assert!(result);
    }

    #[tokio::test]
    async fn test_resolve_with_pick_side() {
        let executor = MockJjExecutor::new().with_response(
            "resolve -r test-change --tool :ours",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let resolver = ConflictResolver::new(executor);
        let result = resolver
            .resolve_with_pick_side("test-change", &ConflictSide::Ours)
            .await
            .unwrap();

        assert!(result);
    }

    #[tokio::test]
    async fn test_resolve_all_no_conflicts() {
        let executor = MockJjExecutor::new().with_response(
            "log -r conflicts() -T change_id ++ \"\\n\" --no-graph",
            JjOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            },
        );

        let resolver = ConflictResolver::new(executor);
        let report = resolver.resolve_all().await.unwrap();

        assert_eq!(report.total_conflicts, 0);
        assert_eq!(report.auto_resolved, 0);
        assert_eq!(report.needs_human, 0);
        assert_eq!(report.failed, 0);
    }
}
