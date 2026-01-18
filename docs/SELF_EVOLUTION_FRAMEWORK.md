# Self-Evolution and Peer Validation Framework for HOX

**Version:** 1.0.0
**Date:** 2026-01-17
**Status:** Proposed Architecture

---

## Executive Summary

This document proposes a concrete framework for enabling self-evolution and peer validation in the HOX agent orchestration system. The framework leverages HOX's existing jj-native architecture to create feedback loops where agents can evaluate their own work, validate peer outputs, and improve their task decomposition and execution strategies over time.

**Key Design Principle:** Agent-first design with human observability layer. The system should work autonomously while providing clear visibility into decision-making and allowing human intervention at critical checkpoints.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Evaluation Hooks and Checkpoints](#2-evaluation-hooks-and-checkpoints)
3. [Peer Validation Patterns](#3-peer-validation-patterns)
4. [Learning Loops](#4-learning-loops)
5. [Self-Improvement Vectors](#5-self-improvement-vectors)
6. [Observability for Evolution](#6-observability-for-evolution)
7. [Human-in-the-Loop Checkpoints](#7-human-in-the-loop-checkpoints)
8. [Implementation Roadmap](#8-implementation-roadmap)

---

## 1. Architecture Overview

### 1.1 Current HOX Architecture (Reference)

```
┌─────────────────────────────────────────────────────────────────┐
│                         HOX System                               │
├─────────────────────────────────────────────────────────────────┤
│  bd-cli          Command-line interface                         │
│  bd-orchestrator Task decomposition, handoff generation         │
│  bd-daemon       File watching, sync, oplog monitoring          │
│  bd-storage      Turso DB, query cache, blocked cache           │
│  bd-core         Types, schemas, validation                     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │   jj (Jujutsu)  │
                    │   Task DAG      │
                    └─────────────────┘
```

### 1.2 Proposed Self-Evolution Layer

```
┌─────────────────────────────────────────────────────────────────┐
│                    SELF-EVOLUTION LAYER                         │
├─────────────────────────────────────────────────────────────────┤
│  bd-evolution    Learning capture, pattern storage, selection   │
│  bd-validation   Peer review, consensus, quality scoring        │
│  bd-metrics      Run quality, success tracking, trend analysis  │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌──────────┐   ┌──────────┐   ┌──────────┐
        │ Eval     │   │ Learning │   │ Pattern  │
        │ Hooks    │   │ Store    │   │ Registry │
        └──────────┘   └──────────┘   └──────────┘
```

---

## 2. Evaluation Hooks and Checkpoints

### 2.1 Hook Insertion Points

The following locations in the codebase are prime candidates for evaluation hooks:

#### 2.1.1 Task State Transitions (bd-orchestrator/src/types.rs)

**Location:** `TaskStatus` enum transitions

```rust
// NEW: Add to bd-orchestrator/src/types.rs

/// Evaluation result for a task state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionEvaluation {
    pub from_status: TaskStatus,
    pub to_status: TaskStatus,
    pub transition_time: DateTime<Utc>,
    pub agent_id: String,
    pub metrics: TransitionMetrics,
    pub self_score: Option<f64>,       // 0.0-1.0 agent self-assessment
    pub peer_score: Option<f64>,       // 0.0-1.0 peer validation score
    pub human_override: Option<bool>,  // Was human intervention required?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionMetrics {
    pub duration_seconds: u64,
    pub files_modified: usize,
    pub lines_changed: i64,
    pub test_pass_rate: Option<f64>,
    pub error_count: usize,
    pub retry_count: usize,
}
```

**Hook Implementation:**

```rust
// NEW: Add to bd-orchestrator/src/eval_hooks.rs

use crate::types::{Task, TaskStatus, TransitionEvaluation, TransitionMetrics};
use anyhow::Result;
use chrono::Utc;

pub trait EvaluationHook: Send + Sync {
    /// Called before a task status transition
    fn pre_transition(&self, task: &Task, new_status: TaskStatus) -> Result<()>;

    /// Called after a task status transition completes
    fn post_transition(&self, task: &Task, evaluation: &TransitionEvaluation) -> Result<()>;

    /// Called when an agent requests self-scoring
    fn self_evaluate(&self, task: &Task) -> Result<f64>;
}

pub struct DefaultEvaluationHook {
    metrics_store: Arc<dyn MetricsStore>,
    learning_store: Arc<dyn LearningStore>,
}

impl EvaluationHook for DefaultEvaluationHook {
    fn pre_transition(&self, task: &Task, new_status: TaskStatus) -> Result<()> {
        // Record transition start time
        self.metrics_store.record_transition_start(
            &task.change_id,
            task.status,
            new_status,
            Utc::now(),
        )
    }

    fn post_transition(&self, task: &Task, evaluation: &TransitionEvaluation) -> Result<()> {
        // Store evaluation for learning
        self.metrics_store.record_evaluation(evaluation)?;

        // If success rate is below threshold, flag for review
        if let Some(score) = evaluation.self_score {
            if score < 0.7 {
                self.learning_store.flag_for_review(&task.change_id, "low_self_score")?;
            }
        }

        Ok(())
    }

    fn self_evaluate(&self, task: &Task) -> Result<f64> {
        // Self-scoring algorithm (see Section 2.3)
        let metrics = self.compute_task_metrics(task)?;
        Ok(self.score_from_metrics(&metrics))
    }
}
```

#### 2.1.2 Handoff Generation (bd-orchestrator/src/handoff.rs)

**Location:** `HandoffGenerator::generate_handoff()` and `HandoffGenerator::prepare_handoff()`

```rust
// NEW: Add handoff quality evaluation to bd-orchestrator/src/handoff.rs

impl HandoffGenerator {
    /// Generate handoff with quality evaluation
    pub async fn generate_handoff_with_eval(
        &self,
        change_id: &str,
        summary: HandoffSummary,
        eval_hook: &dyn EvaluationHook,
    ) -> Result<HandoffEvaluation> {
        // Generate the handoff context
        let handoff = self.generate_handoff(change_id, summary.clone()).await?;

        // Evaluate handoff quality
        let quality = self.evaluate_handoff_quality(&summary)?;

        Ok(HandoffEvaluation {
            handoff,
            quality,
            completeness_score: self.score_completeness(&summary),
            clarity_score: self.score_clarity(&summary),
            actionability_score: self.score_actionability(&summary),
        })
    }

    fn score_completeness(&self, summary: &HandoffSummary) -> f64 {
        let mut score = 0.0;
        let mut max_score = 0.0;

        // Current focus is required
        max_score += 1.0;
        if !summary.current_focus.is_empty() { score += 1.0; }

        // Progress should have at least one item
        max_score += 1.0;
        if !summary.progress.is_empty() { score += 1.0; }

        // Next steps should have actionable items
        max_score += 1.0;
        if !summary.next_steps.is_empty() { score += 1.0; }

        // Files touched should be documented
        max_score += 1.0;
        if !summary.files_touched.is_empty() { score += 1.0; }

        // Blockers and questions should be explicit (even if empty)
        max_score += 0.5;
        score += 0.5; // Always get partial credit for explicit state

        score / max_score
    }
}
```

#### 2.1.3 Database Operations (bd-storage/src/db.rs)

**Location:** `upsert_task()`, `refresh_blocked_cache()`

```rust
// NEW: Add to bd-storage/src/db.rs

impl Database {
    /// Upsert task with metrics tracking
    pub async fn upsert_task_with_metrics(
        &self,
        task: &TaskFile,
        metrics_collector: &mut MetricsCollector,
    ) -> Result<()> {
        let start = Instant::now();

        self.upsert_task(task).await?;

        metrics_collector.record_db_operation(
            "upsert_task",
            start.elapsed(),
            task.change_id.clone(),
        );

        Ok(())
    }

    /// Store evaluation results in dedicated table
    pub async fn store_evaluation(&self, eval: &TransitionEvaluation) -> Result<()> {
        let query = r#"
            INSERT INTO evaluations (
                change_id, from_status, to_status, transition_time,
                agent_id, duration_seconds, files_modified, lines_changed,
                test_pass_rate, error_count, retry_count,
                self_score, peer_score, human_override
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#;

        sqlx::query(query)
            .bind(&eval.change_id)
            // ... bind all fields
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
```

### 2.2 Checkpoint Schema

New database tables for evaluation persistence:

```sql
-- Add to bd-storage/src/schema.sql

-- Evaluation results for each task transition
CREATE TABLE IF NOT EXISTS evaluations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    change_id TEXT NOT NULL,
    from_status TEXT NOT NULL,
    to_status TEXT NOT NULL,
    transition_time TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL,
    files_modified INTEGER NOT NULL,
    lines_changed INTEGER NOT NULL,
    test_pass_rate REAL,
    error_count INTEGER NOT NULL,
    retry_count INTEGER NOT NULL,
    self_score REAL,
    peer_score REAL,
    human_override INTEGER,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_evaluations_change_id ON evaluations(change_id);
CREATE INDEX idx_evaluations_agent_id ON evaluations(agent_id);
CREATE INDEX idx_evaluations_time ON evaluations(transition_time);

-- Aggregated metrics per agent
CREATE TABLE IF NOT EXISTS agent_metrics (
    agent_id TEXT PRIMARY KEY,
    total_tasks INTEGER DEFAULT 0,
    completed_tasks INTEGER DEFAULT 0,
    failed_tasks INTEGER DEFAULT 0,
    avg_duration_seconds REAL,
    avg_self_score REAL,
    avg_peer_score REAL,
    success_rate REAL,
    last_active TEXT,
    trend_direction TEXT, -- 'improving', 'stable', 'declining'
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- Learning patterns captured from successful runs
CREATE TABLE IF NOT EXISTS learning_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_type TEXT NOT NULL, -- 'task_decomposition', 'error_recovery', 'optimization'
    pattern_name TEXT NOT NULL,
    pattern_description TEXT NOT NULL,
    success_count INTEGER DEFAULT 1,
    failure_count INTEGER DEFAULT 0,
    confidence_score REAL DEFAULT 0.5,
    applicable_contexts TEXT, -- JSON array of context tags
    source_change_ids TEXT,   -- JSON array of change IDs that contributed
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_learning_patterns_type ON learning_patterns(pattern_type);
CREATE INDEX idx_learning_patterns_confidence ON learning_patterns(confidence_score DESC);
```

### 2.3 Self-Scoring Algorithm

```rust
// NEW: Add to bd-evolution/src/scoring.rs

use crate::types::TransitionMetrics;

/// Self-scoring algorithm for agent task completion
pub struct SelfScorer {
    /// Weight for time efficiency (faster is better, within bounds)
    time_weight: f64,
    /// Weight for code quality (fewer errors, more tests)
    quality_weight: f64,
    /// Weight for completeness (all criteria met)
    completeness_weight: f64,
    /// Weight for minimal retries
    efficiency_weight: f64,
}

impl Default for SelfScorer {
    fn default() -> Self {
        Self {
            time_weight: 0.2,
            quality_weight: 0.35,
            completeness_weight: 0.3,
            efficiency_weight: 0.15,
        }
    }
}

impl SelfScorer {
    pub fn score(&self, metrics: &TransitionMetrics, context: &ScoringContext) -> f64 {
        let time_score = self.score_time_efficiency(
            metrics.duration_seconds,
            context.expected_duration_seconds,
        );

        let quality_score = self.score_quality(
            metrics.test_pass_rate,
            metrics.error_count,
        );

        let completeness_score = self.score_completeness(
            &context.acceptance_criteria,
            &context.criteria_met,
        );

        let efficiency_score = self.score_efficiency(metrics.retry_count);

        // Weighted average
        let raw_score =
            time_score * self.time_weight +
            quality_score * self.quality_weight +
            completeness_score * self.completeness_weight +
            efficiency_score * self.efficiency_weight;

        // Clamp to [0.0, 1.0]
        raw_score.clamp(0.0, 1.0)
    }

    fn score_time_efficiency(&self, actual: u64, expected: u64) -> f64 {
        if expected == 0 { return 0.5; }

        let ratio = actual as f64 / expected as f64;

        match ratio {
            r if r <= 0.5 => 1.0,      // Completed in half the time
            r if r <= 1.0 => 1.0 - (r - 0.5) * 0.4, // Linear decay
            r if r <= 2.0 => 0.8 - (r - 1.0) * 0.4, // Slower decay
            _ => 0.4,                   // Minimum score for completion
        }
    }

    fn score_quality(&self, test_pass_rate: Option<f64>, error_count: usize) -> f64 {
        let test_score = test_pass_rate.unwrap_or(0.5);
        let error_penalty = (error_count as f64 * 0.1).min(0.5);
        (test_score - error_penalty).max(0.0)
    }

    fn score_completeness(&self, criteria: &[String], met: &[bool]) -> f64 {
        if criteria.is_empty() { return 1.0; }
        let met_count = met.iter().filter(|&&m| m).count();
        met_count as f64 / criteria.len() as f64
    }

    fn score_efficiency(&self, retry_count: usize) -> f64 {
        match retry_count {
            0 => 1.0,
            1 => 0.9,
            2 => 0.75,
            3 => 0.5,
            _ => 0.25,
        }
    }
}
```

---

## 3. Peer Validation Patterns

### 3.1 Multi-Agent Validation Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    PEER VALIDATION SYSTEM                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐     │
│   │ Agent A │    │ Agent B │    │ Agent C │    │ Agent D │     │
│   │ (Work)  │    │(Review) │    │(Review) │    │(Arbiter)│     │
│   └────┬────┘    └────┬────┘    └────┬────┘    └────┬────┘     │
│        │              │              │              │           │
│        ▼              ▼              ▼              ▼           │
│   ┌─────────────────────────────────────────────────────┐      │
│   │              Consensus Engine                        │      │
│   │  - Majority voting for simple decisions             │      │
│   │  - Weighted voting for technical decisions          │      │
│   │  - Arbiter override for deadlocks                   │      │
│   └─────────────────────────────────────────────────────┘      │
│                           │                                     │
│                           ▼                                     │
│                  ┌─────────────────┐                           │
│                  │  Final Decision │                           │
│                  │  + Confidence   │                           │
│                  └─────────────────┘                           │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Validation Types

```rust
// NEW: Add to bd-validation/src/types.rs

/// Types of peer validation available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationType {
    /// Quick sanity check - single reviewer
    QuickReview,
    /// Standard review - two reviewers, majority wins
    StandardReview,
    /// Critical review - three reviewers, consensus required
    CriticalReview,
    /// Security review - specialized security agent required
    SecurityReview,
}

/// Peer validation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRequest {
    pub id: String,
    pub change_id: String,
    pub validation_type: ValidationType,
    pub requesting_agent: String,
    pub artifacts: ValidationArtifacts,
    pub deadline: Option<DateTime<Utc>>,
    pub priority: Priority,
}

/// Artifacts to validate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationArtifacts {
    pub code_diff: String,
    pub test_results: Option<TestResults>,
    pub handoff_context: Option<HandoffContext>,
    pub acceptance_criteria: Vec<String>,
    pub claimed_criteria_met: Vec<bool>,
}

/// Result of a single peer's review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReviewResult {
    pub reviewer_agent: String,
    pub verdict: ReviewVerdict,
    pub score: f64,
    pub comments: Vec<ReviewComment>,
    pub suggested_improvements: Vec<String>,
    pub time_spent_seconds: u64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Reject,
    NeedsMoreContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub file_path: Option<String>,
    pub line_number: Option<usize>,
    pub comment_type: CommentType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommentType {
    Issue,
    Suggestion,
    Question,
    Praise,
}
```

### 3.3 Consensus Mechanism

```rust
// NEW: Add to bd-validation/src/consensus.rs

use crate::types::{PeerReviewResult, ReviewVerdict, ValidationType};

/// Consensus engine for aggregating peer reviews
pub struct ConsensusEngine {
    /// Minimum agreement ratio for consensus
    consensus_threshold: f64,
    /// Weight for agent expertise in domain
    expertise_weight: f64,
    /// Weight for agent's historical accuracy
    accuracy_weight: f64,
}

impl ConsensusEngine {
    pub fn compute_consensus(&self, reviews: &[PeerReviewResult]) -> ConsensusResult {
        if reviews.is_empty() {
            return ConsensusResult::Inconclusive;
        }

        // Count verdicts
        let approve_count = reviews.iter()
            .filter(|r| matches!(r.verdict, ReviewVerdict::Approve))
            .count();
        let reject_count = reviews.iter()
            .filter(|r| matches!(r.verdict, ReviewVerdict::Reject))
            .count();
        let changes_count = reviews.iter()
            .filter(|r| matches!(r.verdict, ReviewVerdict::RequestChanges))
            .count();

        let total = reviews.len();

        // Weighted scoring
        let weighted_score: f64 = reviews.iter()
            .map(|r| r.score * r.confidence)
            .sum::<f64>() / reviews.iter().map(|r| r.confidence).sum::<f64>();

        // Determine consensus
        if approve_count as f64 / total as f64 >= self.consensus_threshold {
            ConsensusResult::Approved {
                confidence: weighted_score,
                dissenting_comments: self.collect_dissenting_comments(reviews),
            }
        } else if reject_count as f64 / total as f64 >= self.consensus_threshold {
            ConsensusResult::Rejected {
                confidence: 1.0 - weighted_score,
                critical_issues: self.collect_critical_issues(reviews),
            }
        } else if changes_count > 0 {
            ConsensusResult::ChangesRequested {
                changes: self.aggregate_requested_changes(reviews),
            }
        } else {
            ConsensusResult::Deadlock {
                reviews: reviews.to_vec(),
                requires_arbiter: true,
            }
        }
    }

    /// Handle Byzantine fault tolerance for critical decisions
    pub fn byzantine_consensus(&self, reviews: &[PeerReviewResult], f: usize) -> ConsensusResult {
        // For 3f+1 reviewers, can tolerate f Byzantine (malicious/faulty) agents
        let n = reviews.len();
        let required_agreement = n - f;

        // Sort reviews by score
        let mut sorted_reviews = reviews.to_vec();
        sorted_reviews.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Check if we have enough agreeing reviews
        let approve_count = sorted_reviews.iter()
            .filter(|r| matches!(r.verdict, ReviewVerdict::Approve))
            .count();

        if approve_count >= required_agreement {
            // Exclude outliers and compute consensus
            let trusted_reviews: Vec<_> = sorted_reviews.iter()
                .filter(|r| r.confidence >= 0.5)
                .collect();

            ConsensusResult::Approved {
                confidence: trusted_reviews.iter()
                    .map(|r| r.score)
                    .sum::<f64>() / trusted_reviews.len() as f64,
                dissenting_comments: vec![],
            }
        } else {
            ConsensusResult::Inconclusive
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConsensusResult {
    Approved {
        confidence: f64,
        dissenting_comments: Vec<String>,
    },
    Rejected {
        confidence: f64,
        critical_issues: Vec<String>,
    },
    ChangesRequested {
        changes: Vec<String>,
    },
    Deadlock {
        reviews: Vec<PeerReviewResult>,
        requires_arbiter: bool,
    },
    Inconclusive,
}
```

### 3.4 Validation Orchestration

```rust
// NEW: Add to bd-validation/src/orchestrator.rs

use crate::consensus::{ConsensusEngine, ConsensusResult};
use crate::types::{ValidationRequest, ValidationType, PeerReviewResult};

pub struct ValidationOrchestrator {
    consensus_engine: ConsensusEngine,
    agent_pool: AgentPool,
    arbiter_agent: String,
}

impl ValidationOrchestrator {
    pub async fn validate(&self, request: ValidationRequest) -> Result<ValidationOutcome> {
        // Select reviewers based on validation type
        let reviewers = self.select_reviewers(&request)?;

        // Dispatch review requests in parallel
        let review_futures: Vec<_> = reviewers.iter()
            .map(|agent| self.request_review(agent, &request))
            .collect();

        let reviews: Vec<PeerReviewResult> = futures::future::join_all(review_futures)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        // Compute consensus
        let consensus = match request.validation_type {
            ValidationType::CriticalReview => {
                self.consensus_engine.byzantine_consensus(&reviews, 1)
            }
            _ => self.consensus_engine.compute_consensus(&reviews),
        };

        // Handle deadlock with arbiter
        let final_result = match consensus {
            ConsensusResult::Deadlock { reviews, .. } => {
                self.invoke_arbiter(&request, &reviews).await?
            }
            other => other,
        };

        Ok(ValidationOutcome {
            request_id: request.id,
            change_id: request.change_id,
            result: final_result,
            reviews,
            timestamp: Utc::now(),
        })
    }

    fn select_reviewers(&self, request: &ValidationRequest) -> Result<Vec<String>> {
        let count = match request.validation_type {
            ValidationType::QuickReview => 1,
            ValidationType::StandardReview => 2,
            ValidationType::CriticalReview => 3,
            ValidationType::SecurityReview => 2, // 1 security specialist + 1 general
        };

        // Select agents based on expertise and availability
        // Exclude the requesting agent
        self.agent_pool.select_available(
            count,
            &request.requesting_agent,
            &request.artifacts,
        )
    }

    async fn invoke_arbiter(
        &self,
        request: &ValidationRequest,
        reviews: &[PeerReviewResult],
    ) -> Result<ConsensusResult> {
        // Arbiter makes final decision with full context
        let arbiter_review = self.request_arbiter_review(
            &self.arbiter_agent,
            request,
            reviews,
        ).await?;

        Ok(ConsensusResult::Approved {
            confidence: arbiter_review.confidence,
            dissenting_comments: vec![format!(
                "Arbiter decision after deadlock: {}",
                arbiter_review.comments.first()
                    .map(|c| c.message.as_str())
                    .unwrap_or("No comment")
            )],
        })
    }
}
```

---

## 4. Learning Loops

### 4.1 Pattern Capture System

```rust
// NEW: Add to bd-evolution/src/learning.rs

use crate::types::LearningPattern;

/// Learning store for capturing and retrieving patterns
pub struct LearningStore {
    db: Arc<Database>,
    pattern_cache: RwLock<HashMap<String, Vec<LearningPattern>>>,
}

impl LearningStore {
    /// Capture a successful pattern from a completed task
    pub async fn capture_success_pattern(
        &self,
        task: &Task,
        evaluation: &TransitionEvaluation,
        context: &TaskContext,
    ) -> Result<Option<LearningPattern>> {
        // Only capture from high-quality completions
        if evaluation.self_score.unwrap_or(0.0) < 0.8 {
            return Ok(None);
        }

        // Extract pattern characteristics
        let pattern = LearningPattern {
            id: uuid::Uuid::new_v4().to_string(),
            pattern_type: self.classify_pattern(task, context),
            pattern_name: self.generate_pattern_name(task),
            pattern_description: self.generate_pattern_description(task, context),
            success_count: 1,
            failure_count: 0,
            confidence_score: evaluation.self_score.unwrap_or(0.8),
            applicable_contexts: self.extract_context_tags(context),
            source_change_ids: vec![task.change_id.clone()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.store_pattern(&pattern).await?;

        Ok(Some(pattern))
    }

    /// Retrieve applicable patterns for a new task
    pub async fn find_applicable_patterns(
        &self,
        context: &TaskContext,
        min_confidence: f64,
    ) -> Result<Vec<LearningPattern>> {
        let context_tags = self.extract_context_tags(context);

        let patterns = self.db.query_patterns_by_context(
            &context_tags,
            min_confidence,
            10, // max results
        ).await?;

        // Sort by relevance (confidence * context match score)
        let mut scored: Vec<_> = patterns.iter()
            .map(|p| {
                let match_score = self.compute_context_match(&p.applicable_contexts, &context_tags);
                (p, p.confidence_score * match_score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        Ok(scored.into_iter().map(|(p, _)| p.clone()).collect())
    }

    /// Update pattern confidence based on new outcome
    pub async fn update_pattern_confidence(
        &self,
        pattern_id: &str,
        success: bool,
    ) -> Result<()> {
        let mut pattern = self.db.get_pattern(pattern_id).await?
            .ok_or_else(|| anyhow!("Pattern not found: {}", pattern_id))?;

        if success {
            pattern.success_count += 1;
        } else {
            pattern.failure_count += 1;
        }

        // Bayesian update of confidence
        let total = pattern.success_count + pattern.failure_count;
        pattern.confidence_score =
            (pattern.success_count as f64 + 1.0) / (total as f64 + 2.0);

        pattern.updated_at = Utc::now();

        self.db.update_pattern(&pattern).await
    }

    fn classify_pattern(&self, task: &Task, context: &TaskContext) -> PatternType {
        // Classification heuristics
        if context.is_decomposition_task {
            PatternType::TaskDecomposition
        } else if context.had_errors && context.recovered {
            PatternType::ErrorRecovery
        } else if context.optimized_previous {
            PatternType::Optimization
        } else {
            PatternType::General
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternType {
    TaskDecomposition,
    ErrorRecovery,
    Optimization,
    General,
}
```

### 4.2 Feedback Integration

```rust
// NEW: Add to bd-evolution/src/feedback.rs

/// Feedback loop manager for continuous improvement
pub struct FeedbackLoop {
    learning_store: Arc<LearningStore>,
    metrics_store: Arc<MetricsStore>,
    improvement_selector: ImprovementSelector,
}

impl FeedbackLoop {
    /// Process feedback from a completed task
    pub async fn process_feedback(&self, feedback: TaskFeedback) -> Result<()> {
        // Store raw feedback
        self.metrics_store.store_feedback(&feedback).await?;

        // Check if this feedback indicates a pattern worth capturing
        if feedback.was_successful && feedback.quality_score >= 0.8 {
            self.learning_store.capture_success_pattern(
                &feedback.task,
                &feedback.evaluation,
                &feedback.context,
            ).await?;
        }

        // Check if this feedback invalidates an existing pattern
        if !feedback.was_successful && feedback.applied_pattern.is_some() {
            let pattern_id = feedback.applied_pattern.as_ref().unwrap();
            self.learning_store.update_pattern_confidence(pattern_id, false).await?;
        }

        // Trigger improvement cycle if enough feedback accumulated
        let recent_feedback_count = self.metrics_store
            .count_recent_feedback(Duration::hours(1))
            .await?;

        if recent_feedback_count >= 10 {
            self.trigger_improvement_cycle().await?;
        }

        Ok(())
    }

    /// Periodic improvement cycle
    pub async fn trigger_improvement_cycle(&self) -> Result<ImprovementReport> {
        // Analyze recent performance
        let metrics = self.metrics_store.get_recent_metrics(Duration::hours(24)).await?;

        // Identify improvement opportunities
        let opportunities = self.improvement_selector.identify_opportunities(&metrics)?;

        // Select top improvements to apply
        let selected = self.improvement_selector.select_improvements(
            &opportunities,
            3, // max improvements per cycle
        )?;

        // Apply improvements (see Section 5 for implementation)
        let applied = self.apply_improvements(&selected).await?;

        Ok(ImprovementReport {
            cycle_time: Utc::now(),
            metrics_analyzed: metrics.len(),
            opportunities_found: opportunities.len(),
            improvements_applied: applied,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TaskFeedback {
    pub task: Task,
    pub evaluation: TransitionEvaluation,
    pub context: TaskContext,
    pub was_successful: bool,
    pub quality_score: f64,
    pub applied_pattern: Option<String>,
    pub human_feedback: Option<HumanFeedback>,
}

#[derive(Debug, Clone)]
pub struct HumanFeedback {
    pub rating: i32,  // 1-5
    pub comments: Option<String>,
    pub corrections: Vec<String>,
}
```

### 4.3 Failure Analysis

```rust
// NEW: Add to bd-evolution/src/failure_analysis.rs

/// Analyze failures to extract learning opportunities
pub struct FailureAnalyzer {
    db: Arc<Database>,
    pattern_store: Arc<LearningStore>,
}

impl FailureAnalyzer {
    /// Analyze a failed task to extract lessons
    pub async fn analyze_failure(&self, task: &Task, error: &TaskError) -> FailureAnalysis {
        // Categorize the failure
        let category = self.categorize_failure(error);

        // Find similar past failures
        let similar_failures = self.find_similar_failures(&category, task).await;

        // Check if there's an established recovery pattern
        let recovery_pattern = self.pattern_store
            .find_recovery_pattern(&category)
            .await
            .ok()
            .flatten();

        // Generate improvement suggestions
        let suggestions = self.generate_suggestions(
            &category,
            &similar_failures,
            recovery_pattern.as_ref(),
        );

        FailureAnalysis {
            task_id: task.change_id.clone(),
            error_category: category,
            root_cause: self.identify_root_cause(error, task),
            similar_failure_count: similar_failures.len(),
            recovery_pattern,
            suggestions,
            preventable: self.was_preventable(&similar_failures),
        }
    }

    fn categorize_failure(&self, error: &TaskError) -> FailureCategory {
        match error {
            TaskError::Timeout { .. } => FailureCategory::Timeout,
            TaskError::DependencyFailed { .. } => FailureCategory::Dependency,
            TaskError::ValidationFailed { .. } => FailureCategory::Validation,
            TaskError::ResourceExhausted { .. } => FailureCategory::Resource,
            TaskError::ExternalService { .. } => FailureCategory::External,
            TaskError::Unknown { .. } => FailureCategory::Unknown,
        }
    }

    fn identify_root_cause(&self, error: &TaskError, task: &Task) -> String {
        // Heuristic root cause analysis
        match error {
            TaskError::Timeout { duration, .. } => {
                format!(
                    "Task exceeded timeout of {:?}. Consider breaking into smaller subtasks.",
                    duration
                )
            }
            TaskError::ValidationFailed { criteria, .. } => {
                format!(
                    "Failed acceptance criteria: {}. Review requirements.",
                    criteria.join(", ")
                )
            }
            _ => "Root cause requires further investigation.".to_string(),
        }
    }

    fn was_preventable(&self, similar_failures: &[FailureRecord]) -> bool {
        // If we've seen this failure pattern 3+ times, it's preventable
        similar_failures.len() >= 3
    }
}

#[derive(Debug, Clone)]
pub enum FailureCategory {
    Timeout,
    Dependency,
    Validation,
    Resource,
    External,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct FailureAnalysis {
    pub task_id: String,
    pub error_category: FailureCategory,
    pub root_cause: String,
    pub similar_failure_count: usize,
    pub recovery_pattern: Option<LearningPattern>,
    pub suggestions: Vec<String>,
    pub preventable: bool,
}
```

---

## 5. Self-Improvement Vectors

### 5.1 Prompt Modification System

```rust
// NEW: Add to bd-evolution/src/prompt_evolution.rs

/// Manages prompt templates and their evolution
pub struct PromptEvolution {
    template_store: Arc<TemplateStore>,
    metrics_store: Arc<MetricsStore>,
    ab_test_manager: ABTestManager,
}

impl PromptEvolution {
    /// Get the best-performing prompt template for a task type
    pub async fn get_optimal_template(
        &self,
        task_type: &str,
        context: &TaskContext,
    ) -> Result<PromptTemplate> {
        // Check for active A/B test
        if let Some(test) = self.ab_test_manager.get_active_test(task_type).await? {
            return self.ab_test_manager.select_variant(&test, context);
        }

        // Return highest-performing template
        self.template_store.get_best_template(task_type).await
    }

    /// Propose a prompt modification based on feedback
    pub async fn propose_modification(
        &self,
        template_id: &str,
        feedback: &PromptFeedback,
    ) -> Result<Option<PromptModification>> {
        let current = self.template_store.get_template(template_id).await?;

        // Analyze feedback patterns
        let issues = self.analyze_feedback_patterns(feedback)?;

        if issues.is_empty() {
            return Ok(None);
        }

        // Generate modification proposals
        let modifications: Vec<PromptModification> = issues.iter()
            .filter_map(|issue| self.generate_modification(&current, issue))
            .collect();

        // Return highest-confidence modification
        Ok(modifications.into_iter()
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap()))
    }

    /// Apply a prompt modification after validation
    pub async fn apply_modification(
        &self,
        modification: &PromptModification,
    ) -> Result<()> {
        // Create new template version
        let new_template = PromptTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: Some(modification.original_template_id.clone()),
            task_type: modification.task_type.clone(),
            template: modification.new_template.clone(),
            version: modification.new_version,
            performance_score: 0.5, // Start at neutral
            usage_count: 0,
            created_at: Utc::now(),
        };

        self.template_store.store_template(&new_template).await?;

        // Set up A/B test between old and new
        self.ab_test_manager.create_test(
            &modification.original_template_id,
            &new_template.id,
            TestConfig {
                sample_size: 100,
                min_duration: Duration::hours(24),
                confidence_threshold: 0.95,
            },
        ).await
    }
}

#[derive(Debug, Clone)]
pub struct PromptTemplate {
    pub id: String,
    pub parent_id: Option<String>,
    pub task_type: String,
    pub template: String,
    pub version: u32,
    pub performance_score: f64,
    pub usage_count: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PromptModification {
    pub original_template_id: String,
    pub task_type: String,
    pub new_template: String,
    pub new_version: u32,
    pub modification_type: ModificationType,
    pub rationale: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub enum ModificationType {
    Clarification,      // Make instructions clearer
    Constraint,         // Add constraints to reduce errors
    Example,            // Add examples for better understanding
    Simplification,     // Reduce complexity
    Specialization,     // Make more specific to task type
}
```

### 5.2 Task Decomposition Learning

```rust
// NEW: Add to bd-evolution/src/decomposition_learning.rs

/// Learn optimal task decomposition strategies
pub struct DecompositionLearner {
    pattern_store: Arc<LearningStore>,
    metrics_store: Arc<MetricsStore>,
}

impl DecompositionLearner {
    /// Suggest decomposition for a new task based on learned patterns
    pub async fn suggest_decomposition(
        &self,
        task: &Task,
        context: &TaskContext,
    ) -> Result<DecompositionSuggestion> {
        // Find similar past tasks
        let similar_tasks = self.find_similar_tasks(task, context).await?;

        // Analyze their decomposition patterns
        let patterns = self.analyze_decomposition_patterns(&similar_tasks).await?;

        // Select best pattern based on success metrics
        let best_pattern = patterns.iter()
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())
            .cloned();

        match best_pattern {
            Some(pattern) => Ok(DecompositionSuggestion {
                subtasks: self.apply_pattern_to_task(&pattern, task),
                confidence: pattern.success_rate,
                source_pattern: Some(pattern.id.clone()),
                estimated_duration: self.estimate_duration(&pattern, task),
            }),
            None => Ok(DecompositionSuggestion {
                subtasks: self.default_decomposition(task),
                confidence: 0.5,
                source_pattern: None,
                estimated_duration: None,
            }),
        }
    }

    /// Learn from a successful task decomposition
    pub async fn learn_from_success(
        &self,
        task: &Task,
        subtasks: &[Task],
        metrics: &DecompositionMetrics,
    ) -> Result<()> {
        // Only learn from high-quality decompositions
        if metrics.overall_success_rate < 0.8 {
            return Ok(());
        }

        let pattern = DecompositionPattern {
            id: uuid::Uuid::new_v4().to_string(),
            task_characteristics: self.extract_characteristics(task),
            subtask_structure: self.extract_structure(subtasks),
            success_rate: metrics.overall_success_rate,
            avg_parallel_efficiency: metrics.parallel_efficiency,
            sample_count: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Check if similar pattern exists
        if let Some(existing) = self.find_similar_pattern(&pattern).await? {
            self.merge_patterns(&existing, &pattern).await
        } else {
            self.pattern_store.store_decomposition_pattern(&pattern).await
        }
    }

    fn default_decomposition(&self, task: &Task) -> Vec<SubtaskSpec> {
        // Heuristic decomposition for unknown task types
        vec![
            SubtaskSpec {
                title: format!("Analyze requirements: {}", task.title),
                estimated_effort: Effort::Small,
                dependencies: vec![],
            },
            SubtaskSpec {
                title: format!("Implement: {}", task.title),
                estimated_effort: Effort::Medium,
                dependencies: vec![0],
            },
            SubtaskSpec {
                title: format!("Test: {}", task.title),
                estimated_effort: Effort::Small,
                dependencies: vec![1],
            },
            SubtaskSpec {
                title: format!("Document: {}", task.title),
                estimated_effort: Effort::Small,
                dependencies: vec![1],
            },
        ]
    }
}

#[derive(Debug, Clone)]
pub struct DecompositionPattern {
    pub id: String,
    pub task_characteristics: TaskCharacteristics,
    pub subtask_structure: SubtaskStructure,
    pub success_rate: f64,
    pub avg_parallel_efficiency: f64,
    pub sample_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DecompositionSuggestion {
    pub subtasks: Vec<SubtaskSpec>,
    pub confidence: f64,
    pub source_pattern: Option<String>,
    pub estimated_duration: Option<Duration>,
}
```

### 5.3 Self-Bug-Fixing

```rust
// NEW: Add to bd-evolution/src/self_fix.rs

/// Automated bug detection and fixing system
pub struct SelfFixSystem {
    error_analyzer: FailureAnalyzer,
    fix_generator: FixGenerator,
    test_runner: TestRunner,
    db: Arc<Database>,
}

impl SelfFixSystem {
    /// Attempt to automatically fix a failed task
    pub async fn attempt_fix(
        &self,
        task: &Task,
        error: &TaskError,
    ) -> Result<FixAttempt> {
        // Analyze the failure
        let analysis = self.error_analyzer.analyze_failure(task, error).await;

        // Check if we have a known fix pattern
        if let Some(pattern) = &analysis.recovery_pattern {
            return self.apply_known_fix(task, pattern).await;
        }

        // Generate fix candidates
        let candidates = self.fix_generator.generate_candidates(
            task,
            error,
            &analysis,
        ).await?;

        // Try each candidate
        for candidate in candidates {
            let result = self.test_fix_candidate(task, &candidate).await?;

            if result.success {
                // Store this as a new fix pattern
                self.store_fix_pattern(task, error, &candidate).await?;

                return Ok(FixAttempt {
                    task_id: task.change_id.clone(),
                    status: FixStatus::Fixed,
                    fix_applied: Some(candidate),
                    attempts: 1,
                    automated: true,
                });
            }
        }

        // No automatic fix found
        Ok(FixAttempt {
            task_id: task.change_id.clone(),
            status: FixStatus::RequiresHuman,
            fix_applied: None,
            attempts: candidates.len(),
            automated: false,
        })
    }

    async fn apply_known_fix(
        &self,
        task: &Task,
        pattern: &LearningPattern,
    ) -> Result<FixAttempt> {
        // Extract fix actions from pattern
        let actions = self.extract_fix_actions(pattern)?;

        // Apply actions
        for action in &actions {
            self.execute_fix_action(task, action).await?;
        }

        // Verify fix
        let verification = self.test_runner.run_task_tests(task).await?;

        if verification.passed {
            Ok(FixAttempt {
                task_id: task.change_id.clone(),
                status: FixStatus::Fixed,
                fix_applied: Some(FixCandidate {
                    description: pattern.pattern_description.clone(),
                    actions,
                    confidence: pattern.confidence_score,
                }),
                attempts: 1,
                automated: true,
            })
        } else {
            Ok(FixAttempt {
                task_id: task.change_id.clone(),
                status: FixStatus::PartialFix,
                fix_applied: None,
                attempts: 1,
                automated: true,
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct FixCandidate {
    pub description: String,
    pub actions: Vec<FixAction>,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub enum FixAction {
    RetryWithTimeout { new_timeout: Duration },
    SkipDependency { dep_id: String },
    ModifyInput { modification: String },
    ResetState,
    Rollback { to_change_id: String },
    EscalateToHuman,
}

#[derive(Debug, Clone)]
pub enum FixStatus {
    Fixed,
    PartialFix,
    RequiresHuman,
    Unfixable,
}
```

---

## 6. Observability for Evolution

### 6.1 Metrics Collection

```rust
// NEW: Add to bd-metrics/src/collector.rs

/// Comprehensive metrics collection for agent evolution
pub struct MetricsCollector {
    db: Arc<Database>,
    buffer: Arc<RwLock<MetricsBuffer>>,
    flush_interval: Duration,
}

impl MetricsCollector {
    /// Record a task-level metric
    pub fn record_task_metric(&self, metric: TaskMetric) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.task_metrics.push(metric);

        if buffer.should_flush() {
            drop(buffer);
            self.flush();
        }
    }

    /// Record an agent-level metric
    pub fn record_agent_metric(&self, metric: AgentMetric) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.agent_metrics.push(metric);
    }

    /// Record a system-level metric
    pub fn record_system_metric(&self, metric: SystemMetric) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.system_metrics.push(metric);
    }

    /// Get aggregated metrics for a time range
    pub async fn get_aggregated_metrics(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<AggregatedMetrics> {
        let task_metrics = self.db.query_task_metrics(start, end).await?;
        let agent_metrics = self.db.query_agent_metrics(start, end).await?;

        Ok(AggregatedMetrics {
            period_start: start,
            period_end: end,
            total_tasks: task_metrics.len(),
            success_rate: self.compute_success_rate(&task_metrics),
            avg_duration: self.compute_avg_duration(&task_metrics),
            agent_performance: self.compute_agent_rankings(&agent_metrics),
            trend: self.compute_trend(&task_metrics),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetric {
    pub change_id: String,
    pub agent_id: String,
    pub metric_type: TaskMetricType,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskMetricType {
    Duration,
    LinesChanged,
    FilesModified,
    TestsRun,
    TestsPassed,
    ErrorCount,
    RetryCount,
    SelfScore,
    PeerScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetric {
    pub agent_id: String,
    pub metric_type: AgentMetricType,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentMetricType {
    TasksCompleted,
    TasksFailed,
    AvgTaskDuration,
    SuccessRate,
    PeerValidationAccuracy,
    LearningRate, // How quickly performance improves
}
```

### 6.2 Signal vs. Noise Filtering

```rust
// NEW: Add to bd-metrics/src/filtering.rs

/// Filter metrics to distinguish signal from noise
pub struct MetricsFilter {
    outlier_detector: OutlierDetector,
    trend_analyzer: TrendAnalyzer,
}

impl MetricsFilter {
    /// Filter out noisy metrics, keeping only significant signals
    pub fn filter_for_learning(&self, metrics: &[TaskMetric]) -> Vec<TaskMetric> {
        metrics.iter()
            .filter(|m| !self.is_outlier(m))
            .filter(|m| self.is_significant(m))
            .cloned()
            .collect()
    }

    /// Determine if a metric is an outlier
    fn is_outlier(&self, metric: &TaskMetric) -> bool {
        self.outlier_detector.is_outlier(metric.value, &metric.metric_type)
    }

    /// Determine if a metric is significant for learning
    fn is_significant(&self, metric: &TaskMetric) -> bool {
        match metric.metric_type {
            // High self-scores might indicate learning opportunity
            TaskMetricType::SelfScore if metric.value >= 0.9 => true,
            // Very low scores indicate failure patterns to learn from
            TaskMetricType::SelfScore if metric.value <= 0.3 => true,
            // Significant peer score deviations
            TaskMetricType::PeerScore => {
                let recent_avg = self.trend_analyzer.get_recent_average(
                    &metric.agent_id,
                    &metric.metric_type,
                );
                (metric.value - recent_avg).abs() > 0.2
            }
            // Multiple retries indicate potential improvement area
            TaskMetricType::RetryCount if metric.value >= 2.0 => true,
            _ => false,
        }
    }
}

/// Detect outliers using statistical methods
pub struct OutlierDetector {
    window_size: usize,
    z_threshold: f64, // Standard deviations for outlier
}

impl OutlierDetector {
    pub fn is_outlier(&self, value: f64, metric_type: &TaskMetricType) -> bool {
        // Get historical values for this metric type
        let historical = self.get_historical_values(metric_type);

        if historical.len() < self.window_size {
            return false; // Not enough data
        }

        let mean = historical.iter().sum::<f64>() / historical.len() as f64;
        let variance = historical.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / historical.len() as f64;
        let std_dev = variance.sqrt();

        let z_score = (value - mean) / std_dev;
        z_score.abs() > self.z_threshold
    }
}
```

### 6.3 Run Quality Metrics

```rust
// NEW: Add to bd-metrics/src/quality.rs

/// Compute comprehensive run quality metrics
pub struct RunQualityAnalyzer {
    metrics_store: Arc<MetricsStore>,
}

impl RunQualityAnalyzer {
    /// Analyze a complete run (set of related tasks)
    pub async fn analyze_run(&self, run_id: &str) -> Result<RunQualityReport> {
        let tasks = self.metrics_store.get_run_tasks(run_id).await?;
        let metrics = self.metrics_store.get_run_metrics(run_id).await?;

        let completion_rate = self.compute_completion_rate(&tasks);
        let avg_quality = self.compute_average_quality(&metrics);
        let parallelization_efficiency = self.compute_parallel_efficiency(&tasks);
        let resource_efficiency = self.compute_resource_efficiency(&metrics);
        let handoff_quality = self.compute_handoff_quality(&tasks);

        let overall_score =
            completion_rate * 0.25 +
            avg_quality * 0.30 +
            parallelization_efficiency * 0.20 +
            resource_efficiency * 0.15 +
            handoff_quality * 0.10;

        Ok(RunQualityReport {
            run_id: run_id.to_string(),
            overall_score,
            completion_rate,
            average_quality: avg_quality,
            parallelization_efficiency,
            resource_efficiency,
            handoff_quality,
            bottlenecks: self.identify_bottlenecks(&tasks, &metrics),
            recommendations: self.generate_recommendations(
                completion_rate,
                avg_quality,
                parallelization_efficiency,
            ),
        })
    }

    fn compute_parallel_efficiency(&self, tasks: &[Task]) -> f64 {
        // Ratio of actual parallelization to potential parallelization
        let total_duration = self.compute_total_duration(tasks);
        let critical_path = self.compute_critical_path_duration(tasks);

        if total_duration == 0.0 { return 1.0; }

        (critical_path / total_duration).min(1.0)
    }

    fn identify_bottlenecks(&self, tasks: &[Task], metrics: &[TaskMetric]) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        // Find tasks with high retry counts
        for metric in metrics {
            if matches!(metric.metric_type, TaskMetricType::RetryCount) && metric.value >= 3.0 {
                bottlenecks.push(Bottleneck {
                    task_id: metric.change_id.clone(),
                    bottleneck_type: BottleneckType::HighRetryCount,
                    severity: metric.value / 5.0, // Normalize to 0-1
                    description: format!("{} retries for task", metric.value as i32),
                });
            }
        }

        // Find blocking tasks
        for task in tasks {
            if matches!(task.status, TaskStatus::Blocked) {
                bottlenecks.push(Bottleneck {
                    task_id: task.change_id.clone(),
                    bottleneck_type: BottleneckType::Blocked,
                    severity: 0.8,
                    description: "Task blocked on dependencies".to_string(),
                });
            }
        }

        bottlenecks
    }
}

#[derive(Debug, Clone)]
pub struct RunQualityReport {
    pub run_id: String,
    pub overall_score: f64,
    pub completion_rate: f64,
    pub average_quality: f64,
    pub parallelization_efficiency: f64,
    pub resource_efficiency: f64,
    pub handoff_quality: f64,
    pub bottlenecks: Vec<Bottleneck>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Bottleneck {
    pub task_id: String,
    pub bottleneck_type: BottleneckType,
    pub severity: f64,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum BottleneckType {
    HighRetryCount,
    Blocked,
    SlowExecution,
    ResourceContention,
    PoorHandoff,
}
```

---

## 7. Human-in-the-Loop Checkpoints

### 7.1 Intervention Points

```rust
// NEW: Add to bd-evolution/src/human_loop.rs

/// Human intervention point definitions
pub struct HumanCheckpoint {
    pub checkpoint_type: CheckpointType,
    pub trigger: CheckpointTrigger,
    pub severity: Severity,
    pub timeout: Duration,
    pub default_action: DefaultAction,
}

#[derive(Debug, Clone)]
pub enum CheckpointType {
    /// Approve/reject a task completion
    TaskApproval,
    /// Validate a peer consensus decision
    ConsensusValidation,
    /// Review a proposed prompt modification
    PromptModification,
    /// Confirm a self-fix before application
    SelfFixConfirmation,
    /// Review pattern before adding to learning store
    PatternReview,
    /// Critical security or safety decision
    SecurityDecision,
}

#[derive(Debug, Clone)]
pub enum CheckpointTrigger {
    /// Always require human input
    Always,
    /// Only when confidence is below threshold
    LowConfidence { threshold: f64 },
    /// When stakes are high (production changes, etc.)
    HighStakes,
    /// When there's disagreement among agents
    Disagreement,
    /// After N consecutive failures
    ConsecutiveFailures { count: usize },
    /// Random sampling for quality assurance
    RandomSample { rate: f64 },
}

#[derive(Debug, Clone)]
pub enum DefaultAction {
    /// Proceed if no human response
    Proceed,
    /// Block until human response
    Block,
    /// Reject if no human response
    Reject,
    /// Use agent's recommendation
    UseAgentRecommendation,
}

/// Human checkpoint manager
pub struct CheckpointManager {
    checkpoints: Vec<HumanCheckpoint>,
    notification_service: Arc<dyn NotificationService>,
    response_store: Arc<ResponseStore>,
}

impl CheckpointManager {
    /// Check if human intervention is needed
    pub fn needs_intervention(
        &self,
        checkpoint_type: CheckpointType,
        context: &DecisionContext,
    ) -> bool {
        let checkpoint = self.checkpoints.iter()
            .find(|c| std::mem::discriminant(&c.checkpoint_type) == std::mem::discriminant(&checkpoint_type));

        match checkpoint {
            Some(cp) => self.evaluate_trigger(&cp.trigger, context),
            None => false, // No checkpoint configured
        }
    }

    /// Request human intervention
    pub async fn request_intervention(
        &self,
        checkpoint_type: CheckpointType,
        context: &DecisionContext,
    ) -> Result<HumanResponse> {
        let checkpoint = self.get_checkpoint(&checkpoint_type)?;

        // Create intervention request
        let request = InterventionRequest {
            id: uuid::Uuid::new_v4().to_string(),
            checkpoint_type,
            context: context.clone(),
            created_at: Utc::now(),
            timeout: checkpoint.timeout,
            default_action: checkpoint.default_action.clone(),
        };

        // Notify human
        self.notification_service.send_intervention_request(&request).await?;

        // Wait for response or timeout
        let response = self.wait_for_response(&request).await;

        match response {
            Some(r) => Ok(r),
            None => Ok(self.apply_default_action(&checkpoint.default_action, context)),
        }
    }

    fn evaluate_trigger(&self, trigger: &CheckpointTrigger, context: &DecisionContext) -> bool {
        match trigger {
            CheckpointTrigger::Always => true,
            CheckpointTrigger::LowConfidence { threshold } => {
                context.confidence < *threshold
            }
            CheckpointTrigger::HighStakes => context.is_high_stakes,
            CheckpointTrigger::Disagreement => context.has_disagreement,
            CheckpointTrigger::ConsecutiveFailures { count } => {
                context.recent_failure_count >= *count
            }
            CheckpointTrigger::RandomSample { rate } => {
                rand::random::<f64>() < *rate
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub task_id: Option<String>,
    pub agent_id: String,
    pub confidence: f64,
    pub is_high_stakes: bool,
    pub has_disagreement: bool,
    pub recent_failure_count: usize,
    pub description: String,
    pub options: Vec<String>,
    pub agent_recommendation: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct HumanResponse {
    pub request_id: String,
    pub decision: HumanDecision,
    pub comments: Option<String>,
    pub responded_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum HumanDecision {
    Approve,
    Reject,
    SelectOption(usize),
    Defer,
    Custom(String),
}
```

### 7.2 Decision Surfacing

```rust
// NEW: Add to bd-evolution/src/decision_surface.rs

/// Surface important decisions to humans in a digestible format
pub struct DecisionSurfacer {
    formatter: DecisionFormatter,
    priority_calculator: PriorityCalculator,
}

impl DecisionSurfacer {
    /// Surface a decision with appropriate context
    pub fn surface_decision(&self, decision: &PendingDecision) -> SurfacedDecision {
        let priority = self.priority_calculator.calculate(&decision);
        let formatted = self.formatter.format(decision);

        SurfacedDecision {
            id: decision.id.clone(),
            priority,
            title: formatted.title,
            summary: formatted.summary,
            details: formatted.details,
            options: formatted.options,
            recommendation: formatted.recommendation,
            deadline: decision.deadline,
            impact_assessment: self.assess_impact(decision),
        }
    }

    fn assess_impact(&self, decision: &PendingDecision) -> ImpactAssessment {
        ImpactAssessment {
            scope: self.determine_scope(decision),
            reversibility: self.determine_reversibility(decision),
            confidence_in_recommendation: decision.confidence,
            potential_risks: self.identify_risks(decision),
        }
    }

    fn identify_risks(&self, decision: &PendingDecision) -> Vec<String> {
        let mut risks = Vec::new();

        if decision.confidence < 0.5 {
            risks.push("Low confidence in recommendation".to_string());
        }

        if decision.is_irreversible {
            risks.push("This decision is irreversible".to_string());
        }

        if decision.affects_production {
            risks.push("Affects production systems".to_string());
        }

        risks
    }
}

#[derive(Debug, Clone)]
pub struct SurfacedDecision {
    pub id: String,
    pub priority: DecisionPriority,
    pub title: String,
    pub summary: String,
    pub details: String,
    pub options: Vec<SurfacedOption>,
    pub recommendation: Option<usize>,
    pub deadline: Option<DateTime<Utc>>,
    pub impact_assessment: ImpactAssessment,
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum DecisionPriority {
    Critical,  // Needs immediate attention
    High,      // Should be addressed today
    Medium,    // Can wait a day or two
    Low,       // Informational, no urgency
}

#[derive(Debug, Clone)]
pub struct SurfacedOption {
    pub index: usize,
    pub description: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub is_recommended: bool,
}

#[derive(Debug, Clone)]
pub struct ImpactAssessment {
    pub scope: ImpactScope,
    pub reversibility: Reversibility,
    pub confidence_in_recommendation: f64,
    pub potential_risks: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ImpactScope {
    SingleTask,
    MultipleAgents,
    SystemWide,
    External,
}

#[derive(Debug, Clone)]
pub enum Reversibility {
    FullyReversible,
    PartiallyReversible,
    Irreversible,
}
```

### 7.3 Escalation Patterns

```rust
// NEW: Add to bd-evolution/src/escalation.rs

/// Escalation management for unresolved issues
pub struct EscalationManager {
    escalation_rules: Vec<EscalationRule>,
    notification_service: Arc<dyn NotificationService>,
    db: Arc<Database>,
}

impl EscalationManager {
    /// Check if escalation is needed based on current state
    pub async fn check_escalation(&self, context: &EscalationContext) -> Option<Escalation> {
        for rule in &self.escalation_rules {
            if self.rule_matches(rule, context) {
                return Some(self.create_escalation(rule, context));
            }
        }
        None
    }

    /// Escalate an issue
    pub async fn escalate(&self, escalation: &Escalation) -> Result<EscalationResult> {
        // Record escalation
        self.db.store_escalation(escalation).await?;

        // Notify appropriate parties
        match escalation.level {
            EscalationLevel::Agent => {
                // Escalate to senior agent
                self.notify_agent(&escalation.target_agent.unwrap()).await?;
            }
            EscalationLevel::Human => {
                // Escalate to human
                self.notify_human(escalation).await?;
            }
            EscalationLevel::Emergency => {
                // Emergency escalation - notify all available channels
                self.emergency_notify(escalation).await?;
            }
        }

        Ok(EscalationResult {
            escalation_id: escalation.id.clone(),
            notified_at: Utc::now(),
            expected_response_time: self.get_expected_response_time(&escalation.level),
        })
    }

    fn rule_matches(&self, rule: &EscalationRule, context: &EscalationContext) -> bool {
        // Check each condition in the rule
        for condition in &rule.conditions {
            let matches = match condition {
                EscalationCondition::RetryCountExceeds(n) => context.retry_count > *n,
                EscalationCondition::DurationExceeds(d) => context.duration > *d,
                EscalationCondition::ConsensusDeadlock => context.is_deadlocked,
                EscalationCondition::SecurityConcern => context.has_security_concern,
                EscalationCondition::CriticalPath => context.is_critical_path,
            };

            if !matches {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct EscalationRule {
    pub name: String,
    pub conditions: Vec<EscalationCondition>,
    pub level: EscalationLevel,
    pub priority: Priority,
}

#[derive(Debug, Clone)]
pub enum EscalationCondition {
    RetryCountExceeds(usize),
    DurationExceeds(Duration),
    ConsensusDeadlock,
    SecurityConcern,
    CriticalPath,
}

#[derive(Debug, Clone)]
pub enum EscalationLevel {
    Agent,     // Escalate to senior agent
    Human,     // Escalate to human operator
    Emergency, // Emergency escalation
}

#[derive(Debug, Clone)]
pub struct Escalation {
    pub id: String,
    pub level: EscalationLevel,
    pub context: EscalationContext,
    pub target_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub priority: Priority,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct EscalationContext {
    pub task_id: String,
    pub agent_id: String,
    pub retry_count: usize,
    pub duration: Duration,
    pub is_deadlocked: bool,
    pub has_security_concern: bool,
    pub is_critical_path: bool,
    pub error_summary: Option<String>,
}
```

---

## 8. Implementation Roadmap

### 8.1 Phase 1: Foundation (Weeks 1-2)

**Objective:** Establish metrics collection and basic evaluation hooks.

**Tasks:**
1. Create `bd-metrics` crate with collector and storage
2. Add evaluation hooks to `bd-orchestrator` state transitions
3. Implement basic self-scoring algorithm
4. Create database schema for metrics and evaluations
5. Add metrics collection to daemon

**Files to Create/Modify:**
- NEW: `crates/bd-metrics/src/lib.rs`
- NEW: `crates/bd-metrics/src/collector.rs`
- NEW: `crates/bd-metrics/src/quality.rs`
- MODIFY: `crates/bd-storage/src/schema.sql`
- MODIFY: `crates/bd-orchestrator/src/types.rs`
- NEW: `crates/bd-orchestrator/src/eval_hooks.rs`

### 8.2 Phase 2: Peer Validation (Weeks 3-4)

**Objective:** Implement multi-agent validation system.

**Tasks:**
1. Create `bd-validation` crate
2. Implement validation types and request handling
3. Build consensus engine with majority voting
4. Add Byzantine fault tolerance for critical decisions
5. Implement validation orchestrator

**Files to Create:**
- NEW: `crates/bd-validation/src/lib.rs`
- NEW: `crates/bd-validation/src/types.rs`
- NEW: `crates/bd-validation/src/consensus.rs`
- NEW: `crates/bd-validation/src/orchestrator.rs`

### 8.3 Phase 3: Learning System (Weeks 5-6)

**Objective:** Build pattern capture and learning infrastructure.

**Tasks:**
1. Create `bd-evolution` crate
2. Implement learning store for pattern capture
3. Build feedback loop manager
4. Implement failure analyzer
5. Add pattern retrieval for new tasks

**Files to Create:**
- NEW: `crates/bd-evolution/src/lib.rs`
- NEW: `crates/bd-evolution/src/learning.rs`
- NEW: `crates/bd-evolution/src/feedback.rs`
- NEW: `crates/bd-evolution/src/failure_analysis.rs`

### 8.4 Phase 4: Self-Improvement (Weeks 7-8)

**Objective:** Implement active self-improvement capabilities.

**Tasks:**
1. Add prompt evolution system
2. Implement task decomposition learning
3. Build self-fix system
4. Add A/B testing for prompt improvements
5. Integrate learning into task execution

**Files to Create:**
- NEW: `crates/bd-evolution/src/prompt_evolution.rs`
- NEW: `crates/bd-evolution/src/decomposition_learning.rs`
- NEW: `crates/bd-evolution/src/self_fix.rs`

### 8.5 Phase 5: Human Integration (Weeks 9-10)

**Objective:** Complete human-in-the-loop integration.

**Tasks:**
1. Implement checkpoint manager
2. Build decision surfacing system
3. Add escalation management
4. Create notification service integration
5. Build human response handling

**Files to Create:**
- NEW: `crates/bd-evolution/src/human_loop.rs`
- NEW: `crates/bd-evolution/src/decision_surface.rs`
- NEW: `crates/bd-evolution/src/escalation.rs`

### 8.6 Phase 6: Polish & Integration (Weeks 11-12)

**Objective:** Integration, testing, and documentation.

**Tasks:**
1. Integration testing across all components
2. Performance optimization
3. Documentation and examples
4. CLI commands for evolution management
5. Observability dashboard integration

---

## Appendix A: Metrics Definitions

| Metric | Type | Description | Collection Point |
|--------|------|-------------|-----------------|
| `task.duration` | Gauge | Time from start to completion | Task state change |
| `task.retry_count` | Counter | Number of retry attempts | Each retry |
| `task.self_score` | Gauge | Agent's self-assessment (0-1) | Task completion |
| `task.peer_score` | Gauge | Peer validation score (0-1) | After peer review |
| `agent.success_rate` | Gauge | Ratio of successful tasks | Periodic aggregation |
| `agent.learning_rate` | Gauge | Performance improvement velocity | Weekly calculation |
| `run.parallelization` | Gauge | Parallel efficiency ratio | Run completion |
| `run.bottleneck_count` | Counter | Number of bottlenecks identified | Run completion |
| `pattern.confidence` | Gauge | Pattern reliability score | Pattern update |
| `consensus.agreement_rate` | Gauge | Reviewer agreement percentage | Each consensus |

---

## Appendix B: Configuration Schema

```yaml
# bd-evolution configuration
evolution:
  # Evaluation settings
  evaluation:
    self_score_threshold: 0.7
    peer_review_required: true
    min_reviewers: 2

  # Learning settings
  learning:
    pattern_capture_threshold: 0.8
    max_patterns_per_type: 100
    confidence_decay_rate: 0.01

  # Self-improvement settings
  improvement:
    prompt_ab_test_sample_size: 100
    max_concurrent_experiments: 3
    improvement_cycle_interval: 1h

  # Human checkpoint settings
  checkpoints:
    - type: prompt_modification
      trigger: always
      timeout: 24h
      default: reject
    - type: self_fix_confirmation
      trigger: low_confidence
      threshold: 0.6
      timeout: 1h
      default: use_agent_recommendation

  # Escalation settings
  escalation:
    retry_threshold: 5
    duration_threshold: 2h
    emergency_channels:
      - email
      - slack
```

---

## Appendix C: Testing Strategy

1. **Unit Tests:** Each component (scorer, consensus engine, learning store) with isolated tests
2. **Integration Tests:** End-to-end flows (task completion -> evaluation -> learning -> improvement)
3. **Property-Based Tests:** For consensus algorithms and scoring functions
4. **Chaos Tests:** Simulated failures to verify escalation and recovery
5. **A/B Test Validation:** Statistical significance testing for prompt improvements

---

*Document generated by Atlas, Principal Software Architect*
*For HOX Agent Orchestration System*
