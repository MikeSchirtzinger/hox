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
    ///
    /// `dependency_edges` encodes known JJ ancestry: each `(from, to)` pair
    /// means `from` must complete before `to` starts. Tasks connected by a
    /// dependency edge are placed in different groups even when their file
    /// sets are disjoint.
    pub fn find_parallelizable(
        tasks: &[Task],
        dependency_edges: &[(String, String)],
    ) -> Vec<ParallelGroup> {
        if tasks.len() < 2 {
            return Vec::new();
        }

        // Build transitive dependency set: for each task, what other tasks
        // must it NOT run alongside (i.e. tasks in the same dependency chain).
        let dependent_pairs = transitive_dependency_pairs(dependency_edges);

        // Build (change_id, file_set) for each task.
        let file_sets: Vec<(String, HashSet<String>)> = tasks
            .iter()
            .map(|t| (t.change_id.clone(), extract_files(t)))
            .collect();

        greedy_disjoint_groups(&file_sets, &dependent_pairs)
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

/// Compute the set of (a, b) pairs where a and b are in the same dependency
/// chain (either directly or transitively). Tasks in such a pair must NOT be
/// placed in the same parallel group.
///
/// Each edge `(from, to)` means `from` must complete before `to`. We build
/// the full transitive closure so that A→B→C means (A,B), (A,C), and (B,C)
/// are all conflicting pairs.
fn transitive_dependency_pairs(edges: &[(String, String)]) -> HashSet<(String, String)> {
    if edges.is_empty() {
        return HashSet::new();
    }

    // Build adjacency list (successors).
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        adj.entry(from.as_str()).or_default().push(to.as_str());
    }

    // Collect all node names.
    let mut all_nodes: HashSet<&str> = HashSet::new();
    for (from, to) in edges {
        all_nodes.insert(from.as_str());
        all_nodes.insert(to.as_str());
    }

    // For each node, BFS/DFS to find all descendants.
    let mut pairs: HashSet<(String, String)> = HashSet::new();
    for &start in &all_nodes {
        let mut visited: HashSet<&str> = HashSet::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &succ in adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]) {
                if visited.insert(succ) {
                    // (start, succ) are in a dependency chain.
                    pairs.insert((start.to_string(), succ.to_string()));
                    // Also the reverse so the check is symmetric.
                    pairs.insert((succ.to_string(), start.to_string()));
                    stack.push(succ);
                }
            }
        }
    }

    pairs
}

/// Greedy grouping: build maximal groups of tasks with disjoint file sets,
/// while respecting the given dependency pairs. Tasks that share a dependency
/// edge (directly or transitively) are never placed in the same group.
fn greedy_disjoint_groups(
    file_sets: &[(String, HashSet<String>)],
    dependent_pairs: &HashSet<(String, String)>,
) -> Vec<ParallelGroup> {
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
            let candidate_id = &file_sets[j].0;
            let candidate_files = &file_sets[j].1;

            // Skip if any existing group member is in a dependency chain with
            // this candidate.
            let has_dependency = group_ids.iter().any(|gid| {
                dependent_pairs.contains(&(gid.clone(), candidate_id.clone()))
                    || dependent_pairs.contains(&(candidate_id.clone(), gid.clone()))
            });
            if has_dependency {
                continue;
            }

            // Only group if both tasks have non-empty file sets and they're disjoint.
            if !candidate_files.is_empty()
                && !group_files.is_empty()
                && candidate_files.is_disjoint(&group_files)
            {
                group_ids.push(candidate_id.clone());
                group_files.extend(candidate_files.iter().cloned());
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
        let groups = DagOptimizer::find_parallelizable(&[], &[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_task_returns_no_groups() {
        let tasks = vec![task_with_files("abc", "do something", &["src/a.rs"])];
        let groups = DagOptimizer::find_parallelizable(&tasks, &[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn independent_tasks_grouped_together() {
        let tasks = vec![
            task_with_files("t1", "implement auth", &["src/auth.rs"]),
            task_with_files("t2", "implement storage", &["src/storage.rs"]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks, &[]);
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
        let groups = DagOptimizer::find_parallelizable(&tasks, &[]);
        assert!(groups.is_empty(), "overlapping tasks must not be grouped");
    }

    #[test]
    fn three_tasks_two_independent_one_overlapping() {
        let tasks = vec![
            task_with_files("t1", "auth module", &["src/auth.rs"]),
            task_with_files("t2", "storage module", &["src/storage.rs"]),
            task_with_files("t3", "shared lib", &["src/auth.rs", "src/extra.rs"]),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks, &[]);
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
        let groups = DagOptimizer::find_parallelizable(&tasks, &[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn dependent_tasks_not_grouped_despite_disjoint_files() {
        // t1 → t2 dependency: they have disjoint files but must not be parallel.
        let tasks = vec![
            task_with_files("t1", "define contracts", &["src/contracts.rs"]),
            task_with_files("t2", "implement feature", &["src/feature.rs"]),
        ];
        let edges = vec![("t1".to_string(), "t2".to_string())];
        let groups = DagOptimizer::find_parallelizable(&tasks, &edges);
        assert!(
            groups.is_empty(),
            "tasks in a dependency chain must not be parallelized"
        );
    }

    #[test]
    fn transitive_dependency_blocks_grouping() {
        // t1 → t2 → t3: t1 and t3 are transitively dependent.
        let tasks = vec![
            task_with_files("t1", "step one", &["src/a.rs"]),
            task_with_files("t2", "step two", &["src/b.rs"]),
            task_with_files("t3", "step three", &["src/c.rs"]),
        ];
        let edges = vec![
            ("t1".to_string(), "t2".to_string()),
            ("t2".to_string(), "t3".to_string()),
        ];
        let groups = DagOptimizer::find_parallelizable(&tasks, &edges);
        // All three are in the same chain — no valid parallel group exists.
        assert!(
            groups.is_empty(),
            "transitive dependents must not be grouped"
        );
    }

    #[test]
    fn independent_tasks_grouped_when_dependency_edges_present() {
        // t1 → t2, but t3 is independent of both.
        let tasks = vec![
            task_with_files("t1", "foundation", &["src/core.rs"]),
            task_with_files("t2", "builds on t1", &["src/feature.rs"]),
            task_with_files("t3", "unrelated work", &["src/util.rs"]),
        ];
        // t3 can run in parallel with t1 (no dep) but NOT with t2 in a group
        // alongside t1 (since t1→t2). t1 and t3 have disjoint files and no edge.
        let edges = vec![("t1".to_string(), "t2".to_string())];
        let groups = DagOptimizer::find_parallelizable(&tasks, &edges);
        // t1 and t3 can be grouped; t2 cannot join that group because t2→t1.
        // Exactly one group should exist and it should not contain t2.
        let all_group_tasks: Vec<String> = groups.iter().flat_map(|g| g.tasks.clone()).collect();
        assert!(
            !all_group_tasks.contains(&"t2".to_string()),
            "t2 must not be in any parallel group alongside t1"
        );
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
