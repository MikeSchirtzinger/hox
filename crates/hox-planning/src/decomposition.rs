//! LLM-driven PRD decomposition into independent slices with dependency DAG.
//!
//! [`decompose`] takes a parsed [`Prd`] and produces a [`DecompositionResult`]
//! by making a single LLM call (not a loop). The result contains parallel work
//! slices, shared contracts, and dependency edges between slices.
//!
//! ## Validation
//!
//! After the LLM response is parsed three invariants are checked:
//!
//! 1. **Coverage** — every requirement ID in the PRD appears in at least one
//!    slice's `requirements_covered` list.
//! 2. **No circular dependencies** — the dependency graph is a DAG (acyclic).
//! 3. **Shared contracts are actually shared** — each contract in
//!    `shared_contracts` must be referenced by at least two slices via
//!    `used_by`.

use async_trait::async_trait;

use crate::hox_prd::Prd;

// ---------------------------------------------------------------------------
// LlmClient trait (defined locally — merged with importer when that lands)
// ---------------------------------------------------------------------------

/// Minimal async interface for a single-turn LLM completion.
///
/// Implement this to bridge the decomposer to any model backend.
/// Tests use [`MockLlmClient`].
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Run a single completion and return the model's text output, or an error
    /// message string on failure.
    async fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Rough complexity estimate for a work slice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SliceSize {
    Small,
    Medium,
    Large,
}

/// An independent unit of parallel work produced by decomposition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    /// Short name, e.g. "API Layer".
    pub name: String,
    /// One-sentence description of the work.
    pub description: String,
    /// Glob patterns for files this slice touches.
    pub estimated_files: Vec<String>,
    /// REQ-xxx / NFR-xxx IDs from the PRD that this slice covers.
    pub requirements_covered: Vec<String>,
    /// Rough complexity estimate.
    pub size_estimate: SliceSize,
}

/// A shared type, trait, or interface that multiple slices depend on.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SharedContract {
    /// Name of the trait/type/interface.
    pub name: String,
    /// Human-readable description of what it provides.
    pub description: String,
    /// Names of slices that use this contract.
    pub used_by: Vec<String>,
}

/// The complete output of a successful LLM-based decomposition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecompositionResult {
    /// Independent work slices — one sub-orchestrator per slice.
    pub slices: Vec<Slice>,
    /// Shared contracts implemented before any parallel slice begins.
    pub shared_contracts: Vec<SharedContract>,
    /// Directed dependency edges between slices: `(from_slice, to_slice)`.
    /// `from_slice` must run *after* `to_slice`.
    pub dependency_edges: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the decomposition pipeline.
#[derive(Debug)]
pub enum DecompositionError {
    /// A PRD requirement is not covered by any slice.
    MissingCoverage(String),
    /// The dependency graph contains a cycle.
    CyclicDependency,
    /// A "shared" contract is referenced by fewer than 2 slices.
    UnsharedContract(String),
}

impl std::fmt::Display for DecompositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCoverage(id) => {
                write!(f, "requirement '{}' is not covered by any slice", id)
            }
            Self::CyclicDependency => {
                write!(f, "dependency graph contains a cycle")
            }
            Self::UnsharedContract(name) => {
                write!(
                    f,
                    "shared contract '{}' is referenced by fewer than 2 slices",
                    name
                )
            }
        }
    }
}

impl std::error::Error for DecompositionError {}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Decompose a PRD into independent slices using a single LLM call.
///
/// # Errors
///
/// Returns `Err(String)` when:
/// - The LLM call fails.
/// - The response cannot be parsed into a valid `DecompositionResult`.
/// - Post-parse validation fails (missing coverage, cycles, orphan contracts).
pub async fn decompose(prd: &Prd, llm: &dyn LlmClient) -> Result<DecompositionResult, String> {
    let system = DECOMPOSITION_SYSTEM_PROMPT;
    let user = build_user_prompt(prd);

    let raw = llm
        .complete(system, &user)
        .await
        .map_err(|e| format!("LLM call failed: {}", e))?;

    let result = parse_response(&raw).map_err(|e| format!("parse failed: {}", e))?;

    let errors = validate_decomposition(prd, &result);
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(format!("validation failed: {}", msgs.join("; ")));
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Run all three invariant checks and return any violations found.
pub fn validate_decomposition(prd: &Prd, result: &DecompositionResult) -> Vec<DecompositionError> {
    let mut errors = Vec::new();

    // 1. Coverage: every REQ-* in prd appears in at least one slice.
    let covered: std::collections::HashSet<&str> = result
        .slices
        .iter()
        .flat_map(|s| s.requirements_covered.iter().map(String::as_str))
        .collect();

    for req in &prd.requirements {
        if !covered.contains(req.id.as_str()) {
            errors.push(DecompositionError::MissingCoverage(req.id.clone()));
        }
    }

    // 2. No circular dependencies.
    if has_cycles(&result.dependency_edges, &result.slices) {
        errors.push(DecompositionError::CyclicDependency);
    }

    // 3. Shared contracts actually shared (used_by has 2+ entries).
    for contract in &result.shared_contracts {
        if contract.used_by.len() < 2 {
            errors.push(DecompositionError::UnsharedContract(contract.name.clone()));
        }
    }

    errors
}

/// DFS cycle detection on the dependency edge list.
///
/// Edges are `(from, to)` meaning `from` depends on `to` (from runs after to).
fn has_cycles(edges: &[(String, String)], slices: &[Slice]) -> bool {
    let n = slices.len();
    if n == 0 {
        return false;
    }

    // Map name -> index.
    let idx: std::collections::HashMap<&str, usize> = slices
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    // Build adjacency list.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (from, to) in edges {
        if let (Some(&a), Some(&b)) = (idx.get(from.as_str()), idx.get(to.as_str())) {
            adj[a].push(b);
        }
    }

    // Three-color DFS: 0=unvisited, 1=in-stack, 2=done.
    let mut color = vec![0u8; n];
    let mut cycle = false;

    for start in 0..n {
        if color[start] == 0 {
            dfs(start, &adj, &mut color, &mut cycle);
        }
    }

    cycle
}

fn dfs(node: usize, adj: &[Vec<usize>], color: &mut Vec<u8>, cycle: &mut bool) {
    if *cycle {
        return;
    }
    color[node] = 1;
    for &next in &adj[node] {
        match color[next] {
            1 => *cycle = true,
            0 => dfs(next, adj, color, cycle),
            _ => {}
        }
    }
    color[node] = 2;
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn build_user_prompt(prd: &Prd) -> String {
    let prd_md = prd.to_markdown();
    let req_ids: Vec<&str> = prd.requirements.iter().map(|r| r.id.as_str()).collect();
    let req_list = if req_ids.is_empty() {
        "(no requirements)".to_owned()
    } else {
        req_ids.join(", ")
    };

    format!(
        "Decompose the following PRD into independent work slices.\n\
         \n\
         Requirements that MUST be covered: {req_list}\n\
         \n\
         ## PRD\n\
         \n\
         {prd_md}"
    )
}

const DECOMPOSITION_SYSTEM_PROMPT: &str = r#"You are a senior software architect decomposing a PRD into independent work slices for parallel execution.

Respond with EXACTLY the following structure (section headers must match exactly):

### SLICES
SLICE: <name>
DESCRIPTION: <one sentence>
FILES: <comma-separated glob patterns, or NONE>
REQUIREMENTS: <comma-separated REQ-*/NFR-* IDs, or NONE>
SIZE: <Small|Medium|Large>
END_SLICE

### SHARED_CONTRACTS
CONTRACT: <name>
DESCRIPTION: <what it provides>
USED_BY: <comma-separated slice names>
END_CONTRACT

### DEPENDENCIES
DEPENDS: <slice_name> AFTER <other_slice_name>

Rules:
- Every requirement ID listed in the prompt MUST appear in at least one slice.
- Shared contracts must be used by 2+ slices; omit contracts used by only one slice.
- List NONE for DEPENDENCIES if there are no ordering constraints.
- Maximize parallelism: slices should touch non-overlapping files.
"#;

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

fn parse_response(response: &str) -> Result<DecompositionResult, String> {
    let slices_sec = extract_section(response, "SLICES").unwrap_or_default();
    let contracts_sec = extract_section(response, "SHARED_CONTRACTS").unwrap_or_default();
    let deps_sec = extract_section(response, "DEPENDENCIES").unwrap_or_default();

    let slices = parse_slices(slices_sec)?;
    let shared_contracts = parse_contracts(contracts_sec)?;
    let dependency_edges = parse_deps(deps_sec);

    if slices.is_empty() {
        return Err("response contained no slices".to_owned());
    }

    Ok(DecompositionResult {
        slices,
        shared_contracts,
        dependency_edges,
    })
}

/// Extract content of `### SECTION_NAME` up to the next `###` or end of string.
fn extract_section<'a>(text: &'a str, section: &str) -> Option<&'a str> {
    let header = format!("### {}", section);
    let start = text.find(header.as_str())?;
    let content_start = start + header.len();
    let content = &text[content_start..];
    let end = content
        .find("\n### ")
        .map(|p| p + 1)
        .unwrap_or(content.len());
    Some(content[..end].trim())
}

fn parse_slices(section: &str) -> Result<Vec<Slice>, String> {
    let mut slices = Vec::new();
    for raw in section.split("END_SLICE") {
        let block = raw.trim();
        if block.is_empty() || !block.contains("SLICE:") {
            continue;
        }
        slices.push(parse_single_slice(block)?);
    }
    Ok(slices)
}

fn parse_single_slice(block: &str) -> Result<Slice, String> {
    let name = extract_field(block, "SLICE")
        .ok_or_else(|| "slice block missing SLICE field".to_owned())?;
    let description = extract_field(block, "DESCRIPTION").unwrap_or_default();
    let files_raw = extract_field(block, "FILES").unwrap_or_default();
    let estimated_files = parse_list(&files_raw);
    let reqs_raw = extract_field(block, "REQUIREMENTS").unwrap_or_default();
    let requirements_covered = parse_list(&reqs_raw);
    let size_raw = extract_field(block, "SIZE").unwrap_or_default();
    let size_estimate = match size_raw.trim().to_uppercase().as_str() {
        "SMALL" | "S" => SliceSize::Small,
        "LARGE" | "L" => SliceSize::Large,
        _ => SliceSize::Medium,
    };

    Ok(Slice {
        name,
        description,
        estimated_files,
        requirements_covered,
        size_estimate,
    })
}

fn parse_contracts(section: &str) -> Result<Vec<SharedContract>, String> {
    let mut contracts = Vec::new();
    for raw in section.split("END_CONTRACT") {
        let block = raw.trim();
        if block.is_empty() || !block.contains("CONTRACT:") {
            continue;
        }
        let name = extract_field(block, "CONTRACT")
            .ok_or_else(|| "contract block missing CONTRACT field".to_owned())?;
        let description = extract_field(block, "DESCRIPTION").unwrap_or_default();
        let used_by_raw = extract_field(block, "USED_BY").unwrap_or_default();
        let used_by = parse_list(&used_by_raw);
        contracts.push(SharedContract {
            name,
            description,
            used_by,
        });
    }
    Ok(contracts)
}

fn parse_deps(section: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let trimmed = section.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NONE") {
        return deps;
    }
    for line in section.lines() {
        let line = line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("NONE") {
            continue;
        }
        let content = line.strip_prefix("DEPENDS:").map(str::trim).unwrap_or(line);
        if let Some(pos) = content.find(" AFTER ") {
            let from = content[..pos].trim().to_owned();
            let to = content[pos + " AFTER ".len()..].trim().to_owned();
            if !from.is_empty() && !to.is_empty() {
                deps.push((from, to));
            }
        }
    }
    deps
}

// ---------------------------------------------------------------------------
// Field helpers
// ---------------------------------------------------------------------------

fn extract_field(block: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    for line in block.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
            return Some(rest.trim().to_owned());
        }
    }
    None
}

fn parse_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NONE") {
        return Vec::new();
    }
    trimmed
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hox_prd::{Prd, Requirement, RequirementKind};

    // -----------------------------------------------------------------------
    // Mock LLM client
    // -----------------------------------------------------------------------

    struct MockLlmClient {
        response: String,
    }

    impl MockLlmClient {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok(self.response.clone())
        }
    }

    struct FailingLlmClient;

    #[async_trait]
    impl LlmClient for FailingLlmClient {
        async fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Err("network timeout".to_owned())
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_prd(req_ids: &[&str]) -> Prd {
        let mut prd = Prd::new("Test PRD");
        prd.vision = "Enable feature X.".to_owned();
        for id in req_ids {
            prd.requirements.push(Requirement {
                id: id.to_string(),
                description: format!("Requirement {}", id),
                kind: RequirementKind::Functional,
            });
        }
        prd
    }

    fn valid_response_for(req_ids: &[&str]) -> String {
        let reqs = if req_ids.is_empty() {
            "NONE".to_owned()
        } else {
            req_ids.join(", ")
        };
        format!(
            r#"### SLICES
SLICE: API Layer
DESCRIPTION: REST endpoints
FILES: crates/api/
REQUIREMENTS: {reqs}
SIZE: Medium
END_SLICE

SLICE: Storage Layer
DESCRIPTION: Persistence layer
FILES: crates/storage/
REQUIREMENTS: {reqs}
SIZE: Small
END_SLICE

### SHARED_CONTRACTS
CONTRACT: DataRecord
DESCRIPTION: Shared record type
USED_BY: API Layer, Storage Layer
END_CONTRACT

### DEPENDENCIES
NONE
"#
        )
    }

    // -----------------------------------------------------------------------
    // decompose() integration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_decompose_success() {
        let prd = make_prd(&["REQ-001", "REQ-002"]);
        let llm = MockLlmClient::new(valid_response_for(&["REQ-001", "REQ-002"]));
        let result = decompose(&prd, &llm).await.unwrap();

        assert_eq!(result.slices.len(), 2);
        assert_eq!(result.slices[0].name, "API Layer");
        assert_eq!(result.shared_contracts.len(), 1);
        assert_eq!(result.shared_contracts[0].name, "DataRecord");
        assert!(result.dependency_edges.is_empty());
    }

    #[tokio::test]
    async fn test_decompose_llm_failure() {
        let prd = make_prd(&["REQ-001"]);
        let result = decompose(&prd, &FailingLlmClient).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("LLM call failed"));
    }

    #[tokio::test]
    async fn test_decompose_with_dependency_edge() {
        let prd = make_prd(&["REQ-001", "REQ-002"]);
        let response = r#"### SLICES
SLICE: API Layer
DESCRIPTION: REST endpoints
FILES: crates/api/
REQUIREMENTS: REQ-001
SIZE: Medium
END_SLICE

SLICE: Storage Layer
DESCRIPTION: Persistence
FILES: crates/storage/
REQUIREMENTS: REQ-002
SIZE: Small
END_SLICE

### SHARED_CONTRACTS
CONTRACT: DataRecord
DESCRIPTION: Shared record
USED_BY: API Layer, Storage Layer
END_CONTRACT

### DEPENDENCIES
DEPENDS: API Layer AFTER Storage Layer
"#;
        let llm = MockLlmClient::new(response);
        let result = decompose(&prd, &llm).await.unwrap();
        assert_eq!(result.dependency_edges.len(), 1);
        assert_eq!(
            result.dependency_edges[0],
            ("API Layer".to_owned(), "Storage Layer".to_owned())
        );
    }

    // -----------------------------------------------------------------------
    // validate_decomposition: coverage
    // -----------------------------------------------------------------------

    #[test]
    fn test_coverage_missing_requirement() {
        let prd = make_prd(&["REQ-001", "REQ-002"]);
        let result = DecompositionResult {
            slices: vec![Slice {
                name: "Slice A".to_owned(),
                description: "Work".to_owned(),
                estimated_files: vec![],
                requirements_covered: vec!["REQ-001".to_owned()], // REQ-002 missing
                size_estimate: SliceSize::Small,
            }],
            shared_contracts: vec![],
            dependency_edges: vec![],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, DecompositionError::MissingCoverage(id) if id == "REQ-002")),
            "expected MissingCoverage for REQ-002"
        );
    }

    #[test]
    fn test_coverage_all_covered() {
        let prd = make_prd(&["REQ-001"]);
        let result = DecompositionResult {
            slices: vec![Slice {
                name: "Slice A".to_owned(),
                description: "Work".to_owned(),
                estimated_files: vec![],
                requirements_covered: vec!["REQ-001".to_owned()],
                size_estimate: SliceSize::Medium,
            }],
            shared_contracts: vec![],
            dependency_edges: vec![],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            !errors.iter().any(|e| matches!(e, DecompositionError::MissingCoverage(_))),
            "no coverage errors expected"
        );
    }

    // -----------------------------------------------------------------------
    // validate_decomposition: cycle detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_cycle_detected() {
        let prd = make_prd(&[]);
        let slices = vec![
            Slice {
                name: "A".to_owned(),
                description: String::new(),
                estimated_files: vec![],
                requirements_covered: vec![],
                size_estimate: SliceSize::Small,
            },
            Slice {
                name: "B".to_owned(),
                description: String::new(),
                estimated_files: vec![],
                requirements_covered: vec![],
                size_estimate: SliceSize::Small,
            },
        ];
        let result = DecompositionResult {
            slices,
            shared_contracts: vec![],
            dependency_edges: vec![
                ("A".to_owned(), "B".to_owned()),
                ("B".to_owned(), "A".to_owned()),
            ],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            errors.iter().any(|e| matches!(e, DecompositionError::CyclicDependency)),
            "expected CyclicDependency error"
        );
    }

    #[test]
    fn test_no_cycle_linear_dependency() {
        let prd = make_prd(&[]);
        let slices = vec![
            Slice {
                name: "A".to_owned(),
                description: String::new(),
                estimated_files: vec![],
                requirements_covered: vec![],
                size_estimate: SliceSize::Small,
            },
            Slice {
                name: "B".to_owned(),
                description: String::new(),
                estimated_files: vec![],
                requirements_covered: vec![],
                size_estimate: SliceSize::Small,
            },
        ];
        let result = DecompositionResult {
            slices,
            shared_contracts: vec![],
            dependency_edges: vec![("A".to_owned(), "B".to_owned())],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            !errors.iter().any(|e| matches!(e, DecompositionError::CyclicDependency)),
            "no cycle expected"
        );
    }

    // -----------------------------------------------------------------------
    // validate_decomposition: shared contract usage
    // -----------------------------------------------------------------------

    #[test]
    fn test_unshared_contract_caught() {
        let prd = make_prd(&[]);
        let result = DecompositionResult {
            slices: vec![],
            shared_contracts: vec![SharedContract {
                name: "OnlyOne".to_owned(),
                description: String::new(),
                used_by: vec!["Slice A".to_owned()], // only 1 user
            }],
            dependency_edges: vec![],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            errors.iter().any(|e| matches!(e, DecompositionError::UnsharedContract(n) if n == "OnlyOne")),
            "expected UnsharedContract for OnlyOne"
        );
    }

    #[test]
    fn test_shared_contract_with_two_users_passes() {
        let prd = make_prd(&[]);
        let result = DecompositionResult {
            slices: vec![],
            shared_contracts: vec![SharedContract {
                name: "Shared".to_owned(),
                description: String::new(),
                used_by: vec!["Slice A".to_owned(), "Slice B".to_owned()],
            }],
            dependency_edges: vec![],
        };
        let errors = validate_decomposition(&prd, &result);
        assert!(
            !errors.iter().any(|e| matches!(e, DecompositionError::UnsharedContract(_))),
            "no contract errors expected"
        );
    }

    // -----------------------------------------------------------------------
    // MockLlmClient prompt verification
    // -----------------------------------------------------------------------

    struct CapturingLlmClient {
        captured_user: std::sync::Mutex<String>,
        response: String,
    }

    impl CapturingLlmClient {
        fn new(response: impl Into<String>) -> Self {
            Self {
                captured_user: std::sync::Mutex::new(String::new()),
                response: response.into(),
            }
        }

        fn get_user(&self) -> String {
            self.captured_user.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmClient for CapturingLlmClient {
        async fn complete(&self, _system: &str, user: &str) -> Result<String, String> {
            *self.captured_user.lock().unwrap() = user.to_owned();
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn test_prompt_includes_requirement_ids() {
        let prd = make_prd(&["REQ-001", "NFR-001"]);
        let llm = CapturingLlmClient::new(valid_response_for(&["REQ-001", "NFR-001"]));
        let _ = decompose(&prd, &llm).await;
        let user_prompt = llm.get_user();
        assert!(
            user_prompt.contains("REQ-001"),
            "prompt should include REQ-001"
        );
        assert!(
            user_prompt.contains("NFR-001"),
            "prompt should include NFR-001"
        );
    }

    #[tokio::test]
    async fn test_prompt_includes_prd_title() {
        let prd = make_prd(&[]);
        let llm = CapturingLlmClient::new(valid_response_for(&[]));
        let _ = decompose(&prd, &llm).await;
        let user_prompt = llm.get_user();
        assert!(
            user_prompt.contains("Test PRD"),
            "prompt should contain PRD title"
        );
    }

    // -----------------------------------------------------------------------
    // Slice size parsing
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_slice_sizes_parsed() {
        let prd = make_prd(&["REQ-001"]);
        let response = r#"### SLICES
SLICE: Small Thing
DESCRIPTION: Small
FILES: NONE
REQUIREMENTS: REQ-001
SIZE: Small
END_SLICE

SLICE: Large Thing
DESCRIPTION: Large
FILES: NONE
REQUIREMENTS: REQ-001
SIZE: Large
END_SLICE

### SHARED_CONTRACTS
CONTRACT: Connector
DESCRIPTION: Shared connector
USED_BY: Small Thing, Large Thing
END_CONTRACT

### DEPENDENCIES
NONE
"#;
        let llm = MockLlmClient::new(response);
        let result = decompose(&prd, &llm).await.unwrap();
        assert_eq!(result.slices[0].size_estimate, SliceSize::Small);
        assert_eq!(result.slices[1].size_estimate, SliceSize::Large);
    }
}
