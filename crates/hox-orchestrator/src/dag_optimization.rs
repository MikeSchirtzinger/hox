//! DAG optimizer — detect parallelizable tasks and extract shared contracts.
//!
//! Tasks with non-overlapping file sets are considered independent and can run
//! in parallel. Shared types/traits referenced by 2+ tasks are extracted as
//! contracts that should be defined in a Phase 0 before parallel work begins.

use std::collections::{HashMap, HashSet};

use hox_core::Task;

/// A group of tasks that can safely run in parallel.
///
/// Tasks in the same group have non-overlapping file sets, meaning they touch
/// disjoint parts of the codebase and can execute concurrently without conflict.
#[derive(Debug, Clone)]
pub struct ParallelGroup {
    /// Task or change IDs that can run in parallel.
    pub tasks: Vec<String>,
}

/// A shared contract (type, trait, or interface) needed by multiple tasks.
///
/// Contracts should be implemented in a Phase 0 blocking change before
/// parallel work begins, so all parallel agents share the same interface.
#[derive(Debug, Clone)]
pub struct SharedContract {
    /// Name of the type, trait, or interface.
    pub name: String,
    /// Human-readable description of what this contract provides.
    pub description: String,
    /// Task/change IDs that reference this contract.
    pub needed_by: Vec<String>,
}

pub struct DagOptimizer;

impl DagOptimizer {
    /// Group independent tasks that can run in parallel.
    ///
    /// Tasks with non-overlapping file sets are considered independent.
    /// Uses greedy grouping: iterate tasks, add to current group if independent
    /// of all members already in the group.
    ///
    /// Files are extracted from the `HandoffContext::files_touched` field and
    /// from the "## Files Touched" section of the task description.
    pub fn find_parallelizable(tasks: &[Task]) -> Vec<ParallelGroup> {
        if tasks.len() < 2 {
            return Vec::new();
        }

        // Build (change_id, file_set) for each task.
        let file_sets: Vec<(String, HashSet<String>)> = tasks
            .iter()
            .map(|t| (t.change_id.clone(), extract_files(t)))
            .collect();

        greedy_disjoint_groups(&file_sets)
    }

    /// Identify types and traits needed by 2+ tasks (shared contracts).
    ///
    /// These should be implemented in a Phase 0 blocking change so parallel
    /// agents share the same interfaces.
    pub fn extract_shared_contracts(tasks: &[Task]) -> Vec<SharedContract> {
        if tasks.len() < 2 {
            return Vec::new();
        }

        // Map from candidate name → list of change_ids that mention it.
        let mut name_to_tasks: HashMap<String, Vec<String>> = HashMap::new();

        for task in tasks {
            let candidates = extract_type_candidates(&task.description);
            for name in candidates {
                name_to_tasks
                    .entry(name)
                    .or_default()
                    .push(task.change_id.clone());
            }
        }

        // Keep only names referenced by 2+ tasks, deduplicate task lists.
        let mut contracts: Vec<SharedContract> = name_to_tasks
            .into_iter()
            .filter(|(_, ids)| {
                let mut unique: Vec<&String> = ids.iter().collect();
                unique.dedup();
                unique.len() >= 2
            })
            .map(|(name, needed_by)| {
                let mut deduped = needed_by;
                deduped.sort();
                deduped.dedup();
                SharedContract {
                    description: format!(
                        "Shared type/trait `{}` referenced in {} tasks. \
                         Define in a contracts phase before parallel work.",
                        name,
                        deduped.len()
                    ),
                    needed_by: deduped,
                    name,
                }
            })
            .collect();

        contracts.sort_by(|a, b| a.name.cmp(&b.name));
        contracts
    }
}

/// Check if adding directed edges to a DAG would create a cycle.
///
/// Each edge is `(from, to)` meaning `from` must complete before `to` starts.
/// Uses DFS-based cycle detection (topological sort attempt).
pub fn has_cycles(edges: &[(String, String)]) -> bool {
    // Collect all nodes and build adjacency list.
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut all_nodes: HashSet<&str> = HashSet::new();

    for (from, to) in edges {
        adj.entry(from.as_str()).or_default().push(to.as_str());
        all_nodes.insert(from.as_str());
        all_nodes.insert(to.as_str());
    }

    // DFS with three-color marking: 0=unvisited, 1=in-stack, 2=done.
    let mut state: HashMap<&str, u8> = HashMap::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, u8>,
    ) -> bool {
        match state.get(node).copied() {
            Some(2) => return false, // already fully processed
            Some(1) => return true,  // back edge → cycle
            _ => {}
        }
        state.insert(node, 1); // mark in-stack
        if let Some(neighbors) = adj.get(node) {
            for &neighbor in neighbors {
                if dfs(neighbor, adj, state) {
                    return true;
                }
            }
        }
        state.insert(node, 2); // mark done
        false
    }

    for node in all_nodes {
        if state.get(node).copied().unwrap_or(0) == 0 && dfs(node, &adj, &mut state) {
            return true;
        }
    }

    false
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Extract the file set a task touches.
///
/// Parses the "## Files Touched" section from the structured description format.
fn extract_files(task: &Task) -> HashSet<String> {
    let mut files = HashSet::new();

    // Parse "## Files Touched" section from description.
    let mut in_files_section = false;
    for line in task.description.lines() {
        let trimmed = line.trim();
        if trimmed == "## Files Touched" {
            in_files_section = true;
            continue;
        }
        if in_files_section {
            if trimmed.starts_with("## ") {
                break; // next section
            }
            if !trimmed.is_empty() {
                files.insert(trimmed.to_string());
            }
        }
    }

    files
}

/// Greedy grouping: build maximal groups of tasks with disjoint file sets.
fn greedy_disjoint_groups(file_sets: &[(String, HashSet<String>)]) -> Vec<ParallelGroup> {
    let mut groups: Vec<ParallelGroup> = Vec::new();
    let mut used = vec![false; file_sets.len()];

    for i in 0..file_sets.len() {
        if used[i] {
            continue;
        }

        let mut group_ids = vec![file_sets[i].0.clone()];
        let mut group_files: HashSet<String> = file_sets[i].1.iter().cloned().collect();
        used[i] = true;

        for j in (i + 1)..file_sets.len() {
            if used[j] {
                continue;
            }
            // Only group if both tasks have non-empty file sets and they're disjoint.
            let candidate = &file_sets[j].1;
            if !candidate.is_empty()
                && !group_files.is_empty()
                && candidate.is_disjoint(&group_files)
            {
                group_ids.push(file_sets[j].0.clone());
                group_files.extend(candidate.iter().cloned());
                used[j] = true;
            }
        }

        if group_ids.len() >= 2 {
            groups.push(ParallelGroup { tasks: group_ids });
        }
    }

    groups
}

const STOPWORDS: &[&str] = &[
    "I", "A", "The", "An", "This", "That", "These", "Those", "It", "Is", "Be", "Are", "Was",
    "Were", "Have", "Has", "Do", "Does", "Did", "Will", "Would", "Could", "Should", "May",
    "Might", "Must", "Shall", "Can", "And", "Or", "But", "If", "When", "Where", "How", "What",
    "Which", "Who", "Implement", "Create", "Add", "Remove", "Update", "Get", "Set", "Run",
    "Build", "Test", "Fix", "Use", "Make", "Ensure", "Allow", "Enable", "Task", "Phase",
    "Status", "Agent", "Priority",
];

/// Extract CamelCase / UpperCase type/trait candidate names from a description.
fn extract_type_candidates(description: &str) -> HashSet<String> {
    let stopwords: HashSet<&str> = STOPWORDS.iter().copied().collect();

    description
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| {
            w.len() >= 2
                && w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !stopwords.contains(*w)
        })
        .map(|w| w.to_string())
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hox_core::Task;

    fn task_with_files(id: &str, desc: &str, files: &[&str]) -> Task {
        let files_section = if files.is_empty() {
            String::new()
        } else {
            format!("\n\n## Files Touched\n{}", files.join("\n"))
        };
        Task::new(id, format!("{}{}", desc, files_section))
    }

    // ── find_parallelizable ───────────────────────────────────────────────────

    #[test]
    fn empty_tasks_returns_no_groups() {
        let groups = DagOptimizer::find_parallelizable(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_task_returns_no_groups() {
        let tasks = vec![task_with_files("abc", "do something", &["src/a.rs"])];
        let groups = DagOptimizer::find_parallelizable(&tasks);
        assert!(groups.is_empty());
    }

    #[test]
    fn independent_tasks_grouped_together() {
        let tasks = vec![
            task_with_files("t1", "implement auth", &["src/auth.rs"]),
            task_with_files("t2", "implement storage", &["src/storage.rs"]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tasks.len(), 2);
        assert!(groups[0].tasks.contains(&"t1".to_string()));
        assert!(groups[0].tasks.contains(&"t2".to_string()));
    }

    #[test]
    fn overlapping_tasks_kept_separate() {
        let tasks = vec![
            task_with_files("t1", "update lib", &["src/lib.rs", "src/auth.rs"]),
            task_with_files("t2", "fix lib bug", &["src/lib.rs", "src/storage.rs"]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks);
        assert!(groups.is_empty(), "overlapping tasks must not be grouped");
    }

    #[test]
    fn three_tasks_two_independent_one_overlapping() {
        let tasks = vec![
            task_with_files("t1", "auth module", &["src/auth.rs"]),
            task_with_files("t2", "storage module", &["src/storage.rs"]),
            task_with_files("t3", "shared lib", &["src/auth.rs", "src/extra.rs"]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks);
        assert_eq!(groups.len(), 1);
        let ids = &groups[0].tasks;
        assert!(ids.contains(&"t1".to_string()));
        assert!(ids.contains(&"t2".to_string()));
        assert!(!ids.contains(&"t3".to_string()));
    }

    #[test]
    fn tasks_without_file_sets_not_grouped() {
        // Tasks with empty file sets are conservative — don't group them.
        let tasks = vec![
            task_with_files("t1", "do work", &[]),
            task_with_files("t2", "do other work", &[]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks);
        assert!(groups.is_empty());
    }

    // ── extract_shared_contracts ──────────────────────────────────────────────

    #[test]
    fn single_task_no_contracts() {
        let tasks = vec![Task::new("t1", "implement Storage trait")];
        let contracts = DagOptimizer::extract_shared_contracts(&tasks);
        assert!(contracts.is_empty());
    }

    #[test]
    fn shared_type_extracted_as_contract() {
        let tasks = vec![
            Task::new("t1", "implement Storage backend using Redis"),
            Task::new("t2", "add metrics to Storage backend"),
        ];
        let contracts = DagOptimizer::extract_shared_contracts(&tasks);
        let names: Vec<&str> = contracts.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"Storage"),
            "Storage should be a shared contract, got: {:?}",
            names
        );
    }

    #[test]
    fn contract_lists_all_referencing_tasks() {
        let tasks = vec![
            Task::new("t1", "implement Executor trait"),
            Task::new("t2", "mock Executor for testing"),
        ];
        let contracts = DagOptimizer::extract_shared_contracts(&tasks);
        let executor_contract = contracts.iter().find(|c| c.name == "Executor");
        assert!(executor_contract.is_some());
        let needed_by = &executor_contract.unwrap().needed_by;
        assert!(needed_by.contains(&"t1".to_string()));
        assert!(needed_by.contains(&"t2".to_string()));
    }

    #[test]
    fn unique_types_not_extracted_as_contracts() {
        let tasks = vec![
            Task::new("t1", "implement Redis backend"),
            Task::new("t2", "implement Postgres backend"),
        ];
        let contracts = DagOptimizer::extract_shared_contracts(&tasks);
        let names: Vec<&str> = contracts.iter().map(|c| c.name.as_str()).collect();
        // Redis and Postgres each appear only once — should not be contracts.
        assert!(!names.contains(&"Redis"), "Redis appears once, not a contract");
        assert!(!names.contains(&"Postgres"), "Postgres appears once, not a contract");
    }

    // ── has_cycles ────────────────────────────────────────────────────────────

    #[test]
    fn empty_edges_no_cycle() {
        assert!(!has_cycles(&[]));
    }

    #[test]
    fn linear_chain_no_cycle() {
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ];
        assert!(!has_cycles(&edges));
    }

    #[test]
    fn direct_cycle_detected() {
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];
        assert!(has_cycles(&edges));
    }

    #[test]
    fn indirect_cycle_detected() {
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
            ("c".to_string(), "a".to_string()),
        ];
        assert!(has_cycles(&edges));
    }

    #[test]
    fn diamond_dag_no_cycle() {
        // a → b, a → c, b → d, c → d — valid DAG
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("a".to_string(), "c".to_string()),
            ("b".to_string(), "d".to_string()),
            ("c".to_string(), "d".to_string()),
        ];
        assert!(!has_cycles(&edges));
    }
}
