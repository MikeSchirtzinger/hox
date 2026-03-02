//! Hox PRD schema for JJ change descriptions (hox:prd:v1 format).
//!
//! Provides the [`Prd`] type — a flat structured PRD optimized for storage in
//! JJ change descriptions and consumption by the LLM-based decomposition agent.
//! Coexists with the existing [`crate::prd::ProjectRequirementsDocument`] (epic/story).
//!
//! ## Format
//!
//! A PRD change description starts with:
//! ```text
//! hox:prd:v1 — [title]
//! ```
//! Followed by `## Section` markdown sections. See [`Prd::to_markdown`] and
//! [`Prd::from_markdown`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use hox_core::{HoxError, Result};

/// Sentinel prefix for all hox PRD change descriptions.
pub const PRD_SENTINEL: &str = "hox:prd:v1";

const SENTINEL_SEP: &str = " \u{2014} "; // " — "

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A structured PRD parsed from or serialized to a JJ change description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prd {
    /// Schema version — always `"v1"` for this format.
    pub version: String,
    /// Short title extracted from the sentinel line.
    pub title: String,
    /// Problem statement: what this solves and why it matters.
    pub vision: String,
    /// Stakeholders and users of the system.
    pub actors: Vec<String>,
    /// Measurable outcomes used for backpressure validation.
    pub success_criteria: Vec<String>,
    /// Items explicitly in scope.
    pub scope_in: Vec<String>,
    /// Items explicitly excluded from scope.
    pub scope_out: Vec<String>,
    /// Functional and non-functional requirements.
    pub requirements: Vec<Requirement>,
    /// Technical limits (e.g. "Must use existing NATS infrastructure").
    pub constraints: Vec<String>,
    /// Parallelization hints for the decomposition agent.
    pub decomposition_hints: Vec<DecompositionHint>,
}

/// A single requirement with a structured ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    /// Identifier, e.g. `REQ-001` or `NFR-001`.
    pub id: String,
    pub description: String,
    pub kind: RequirementKind,
}

/// Whether a requirement is functional or non-functional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequirementKind {
    Functional,
    NonFunctional,
}

/// A unit of parallel work identified during planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionHint {
    pub name: String,
    pub description: String,
    /// Glob patterns indicating which files/directories this hint touches.
    pub estimated_files: Vec<String>,
    /// REQ/NFR IDs covered by this hint.
    pub requirements: Vec<String>,
}

// ---------------------------------------------------------------------------
// Prd impl
// ---------------------------------------------------------------------------

impl Prd {
    /// Create a new empty PRD with sensible defaults.
    pub fn new(title: &str) -> Self {
        Self {
            version: "v1".to_owned(),
            title: title.to_owned(),
            vision: String::new(),
            actors: Vec::new(),
            success_criteria: Vec::new(),
            scope_in: Vec::new(),
            scope_out: Vec::new(),
            requirements: Vec::new(),
            constraints: Vec::new(),
            decomposition_hints: Vec::new(),
        }
    }

    /// Returns `true` when `description` begins with the `hox:prd:v1` sentinel.
    ///
    /// This checks for the exact `PRD_SENTINEL` constant — future format versions
    /// would return `false` here until explicit support is added, preventing
    /// silent misparses of `hox:prd:v2` or similar.
    pub fn is_prd(description: &str) -> bool {
        description.starts_with(PRD_SENTINEL)
    }

    /// Serialize to `hox:prd:v1` markdown format.
    ///
    /// The output is round-trip compatible with [`Prd::from_markdown`].
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        // Sentinel
        out.push_str(&format!(
            "hox:prd:{}{}{}\n",
            self.version, SENTINEL_SEP, self.title
        ));

        // Vision
        out.push_str("\n## Vision\n\n");
        let vision = self.vision.trim();
        if vision.is_empty() {
            out.push_str("(none)\n");
        } else {
            out.push_str(vision);
            out.push('\n');
        }

        // Actors
        out.push_str("\n## Actors\n\n");
        if self.actors.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for actor in &self.actors {
                out.push_str(&format!("- {}\n", actor));
            }
        }

        // Success Criteria
        out.push_str("\n## Success Criteria\n\n");
        if self.success_criteria.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for c in &self.success_criteria {
                out.push_str(&format!("- [ ] {}\n", c));
            }
        }

        // Scope
        out.push_str("\n## Scope\n\n### In Scope\n");
        if self.scope_in.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for item in &self.scope_in {
                out.push_str(&format!("- {}\n", item));
            }
        }
        out.push_str("\n### Out of Scope\n");
        if self.scope_out.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for item in &self.scope_out {
                out.push_str(&format!("- {}\n", item));
            }
        }

        // Requirements
        out.push_str("\n## Requirements\n\n### Functional\n");
        let functional: Vec<_> = self
            .requirements
            .iter()
            .filter(|r| r.kind == RequirementKind::Functional)
            .collect();
        if functional.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for r in &functional {
                out.push_str(&format!("- **[{}]**: {}\n", r.id, r.description));
            }
        }
        out.push_str("\n### Non-Functional\n");
        let non_functional: Vec<_> = self
            .requirements
            .iter()
            .filter(|r| r.kind == RequirementKind::NonFunctional)
            .collect();
        if non_functional.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for r in &non_functional {
                out.push_str(&format!("- **[{}]**: {}\n", r.id, r.description));
            }
        }

        // Technical Constraints
        out.push_str("\n## Technical Constraints\n\n");
        if self.constraints.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for c in &self.constraints {
                out.push_str(&format!("- {}\n", c));
            }
        }

        // Decomposition Hints
        out.push_str("\n## Decomposition Hints\n\n");
        if self.decomposition_hints.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for (i, hint) in self.decomposition_hints.iter().enumerate() {
                let files = if hint.estimated_files.is_empty() {
                    String::new()
                } else {
                    format!(
                        " Files: `{}`. ",
                        hint.estimated_files
                            .iter()
                            .map(|f| f.as_str())
                            .collect::<Vec<_>>()
                            .join("`, `")
                    )
                };
                let reqs = if hint.requirements.is_empty() {
                    String::new()
                } else {
                    format!(" Covers: {}.", hint.requirements.join(", "))
                };
                out.push_str(&format!(
                    "{}. **{}**: {}.{}{}\n",
                    i + 1,
                    hint.name,
                    hint.description,
                    files,
                    reqs,
                ));
            }
        }

        out
    }

    /// Parse a PRD from a JJ change description (hox:prd:v1 format).
    pub fn from_markdown(md: &str) -> Result<Self> {
        if !Self::is_prd(md) {
            return Err(HoxError::ValidationFailed(
                "missing sentinel: description does not start with 'hox:prd:'".to_owned(),
            ));
        }

        let mut lines = md.lines();
        let sentinel_line = lines.next().unwrap_or("");
        let (version, title) = parse_sentinel(sentinel_line)?;

        let body: String = lines.collect::<Vec<_>>().join("\n");
        let sections = split_sections(&body);

        let vision = sections
            .get("Vision")
            .map(|s| {
                let t = s.trim();
                if t == "(none)" { String::new() } else { t.to_owned() }
            })
            .unwrap_or_default();

        let actors = sections
            .get("Actors")
            .map(|s| parse_bullet_list(s))
            .unwrap_or_default();

        let success_criteria = sections
            .get("Success Criteria")
            .map(|s| parse_checkbox_list(s))
            .unwrap_or_default();

        let (scope_in, scope_out) = sections
            .get("Scope")
            .map(|s| parse_scope(s))
            .unwrap_or_default();

        let requirements = sections
            .get("Requirements")
            .map(|s| parse_requirements(s))
            .unwrap_or_default();

        let constraints = sections
            .get("Technical Constraints")
            .map(|s| parse_bullet_list(s))
            .unwrap_or_default();

        let decomposition_hints = sections
            .get("Decomposition Hints")
            .map(|s| parse_decomposition_hints(s))
            .unwrap_or_default();

        Ok(Self {
            version,
            title,
            vision,
            actors,
            success_criteria,
            scope_in,
            scope_out,
            requirements,
            constraints,
            decomposition_hints,
        })
    }

    /// Fill missing fields with sensible defaults (idempotent).
    pub fn backfill(&mut self) {
        if self.version.is_empty() {
            self.version = "v1".to_owned();
        }
        if self.title.is_empty() {
            self.title = "Untitled PRD".to_owned();
        }
        if self.vision.trim().is_empty() {
            self.vision = "TODO: describe the problem this solves.".to_owned();
        }
        if self.success_criteria.is_empty() {
            self.success_criteria
                .push("TODO: add at least one measurable success criterion.".to_owned());
        }
        if self.scope_in.is_empty() {
            self.scope_in
                .push("TODO: list at least one in-scope item.".to_owned());
        }
        if !self
            .requirements
            .iter()
            .any(|r| r.kind == RequirementKind::Functional)
        {
            self.requirements.push(Requirement {
                id: "REQ-001".to_owned(),
                description: "TODO: describe the primary functional requirement.".to_owned(),
                kind: RequirementKind::Functional,
            });
        }
        if self.decomposition_hints.is_empty() {
            self.decomposition_hints.push(DecompositionHint {
                name: "Core".to_owned(),
                description: "TODO: describe the primary work slice.".to_owned(),
                estimated_files: Vec::new(),
                requirements: Vec::new(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse `hox:prd:v1 — title` into `("v1", "title")`.
fn parse_sentinel(line: &str) -> Result<(String, String)> {
    if !line.starts_with(PRD_SENTINEL) {
        return Err(HoxError::ValidationFailed(format!(
            "malformed sentinel line: '{}'",
            line
        )));
    }

    let after = &line[PRD_SENTINEL.len()..];
    let title = if let Some(rest) = after.strip_prefix(SENTINEL_SEP) {
        rest.trim().to_owned()
    } else if let Some(rest) = after.strip_prefix(" - ") {
        rest.trim().to_owned()
    } else {
        String::new()
    };

    Ok(("v1".to_owned(), title))
}

/// Split a markdown body into `section_name -> content` by `## Heading`.
fn split_sections(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut current_heading: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(h) = current_heading.take() {
                map.insert(h, current_lines.join("\n"));
                current_lines.clear();
            }
            current_heading = Some(heading.trim().to_owned());
        } else if current_heading.is_some() {
            current_lines.push(line);
        }
    }

    if let Some(h) = current_heading {
        map.insert(h, current_lines.join("\n"));
    }

    map
}

/// Split a section body into `subsection_name -> content` by `### Heading`.
fn split_subsections(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut current_heading: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            if let Some(h) = current_heading.take() {
                map.insert(h, current_lines.join("\n"));
                current_lines.clear();
            }
            current_heading = Some(heading.trim().to_owned());
        } else if current_heading.is_some() {
            current_lines.push(line);
        }
    }

    if let Some(h) = current_heading {
        map.insert(h, current_lines.join("\n"));
    }

    map
}

/// Parse a plain bullet list (`- item`) into `Vec<String>`, skipping `(none)`.
fn parse_bullet_list(section: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = item.trim();
            if !item.is_empty() && item != "(none)" {
                items.push(item.to_owned());
            }
        }
    }
    items
}

/// Parse `- [ ] text` and `- [x] text` checkbox bullets into plain strings.
fn parse_checkbox_list(section: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [") {
            if let Some(close) = rest.find(']') {
                let text = rest[close + 1..].trim().to_owned();
                if !text.is_empty() && text != "(none)" {
                    items.push(text);
                }
            }
        }
    }
    items
}

/// Parse the Scope section into `(scope_in, scope_out)`.
fn parse_scope(section: &str) -> (Vec<String>, Vec<String>) {
    let subs = split_subsections(section);
    let scope_in = subs
        .get("In Scope")
        .map(|s| parse_bullet_list(s))
        .unwrap_or_default();
    let scope_out = subs
        .get("Out of Scope")
        .map(|s| parse_bullet_list(s))
        .unwrap_or_default();
    (scope_in, scope_out)
}

/// Parse the Requirements section (Functional + Non-Functional subsections).
fn parse_requirements(section: &str) -> Vec<Requirement> {
    let subs = split_subsections(section);
    let mut requirements = Vec::new();

    for (sub_name, content) in &subs {
        let kind = if sub_name == "Non-Functional" {
            RequirementKind::NonFunctional
        } else {
            RequirementKind::Functional
        };
        for line in content.lines() {
            let trimmed = line.trim();
            // Match: `- **[REQ-001]**: description`
            if let Some(rest) = trimmed.strip_prefix("- **[") {
                if let Some(close) = rest.find("]**:") {
                    let id = rest[..close].to_owned();
                    let description = rest[close + 4..].trim().to_owned();
                    if !id.is_empty() && id != "none" {
                        requirements.push(Requirement {
                            id,
                            description,
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }
    }

    // Stable ordering: functional first, then by ID
    requirements.sort_by(|a, b| {
        let a_func = a.kind == RequirementKind::Functional;
        let b_func = b.kind == RequirementKind::Functional;
        b_func.cmp(&a_func).then_with(|| a.id.cmp(&b.id))
    });

    requirements
}

/// Parse decomposition hint numbered entries:
/// `1. **Name**: Description. Files: \`crates/foo/\`. Covers: REQ-001.`
fn parse_decomposition_hints(section: &str) -> Vec<DecompositionHint> {
    let mut hints = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "- (none)" || trimmed == "(none)" {
            continue;
        }

        // Strip leading `N. ` (numbered list)
        let rest = strip_numbered_prefix(trimmed);

        // Expect `**Name**: body...`
        if let Some(rest) = rest.strip_prefix("**") {
            if let Some(close) = rest.find("**:") {
                let name = rest[..close].to_owned();
                let body = rest[close + 3..].trim();

                let (body_no_covers, requirements) = extract_covers(body);
                let (description, estimated_files) = extract_files(&body_no_covers);

                hints.push(DecompositionHint {
                    name,
                    description: description.trim().trim_end_matches('.').trim().to_owned(),
                    estimated_files,
                    requirements,
                });
            }
        }
    }
    hints
}

/// Strip leading `N. ` numbered list prefix if present.
fn strip_numbered_prefix(s: &str) -> &str {
    if let Some(dot_pos) = s.find(". ") {
        let prefix = &s[..dot_pos];
        if prefix.chars().all(|c| c.is_ascii_digit()) {
            return &s[dot_pos + 2..];
        }
    }
    s
}

/// Extract `Covers: REQ-001, NFR-001.` from the end of a body fragment.
fn extract_covers(text: &str) -> (String, Vec<String>) {
    if let Some(pos) = text.find("Covers:") {
        let before = text[..pos].trim_end_matches(['.', ' ']).to_owned();
        let covers_text = &text[pos + "Covers:".len()..];
        let reqs: Vec<String> = covers_text
            .trim_end_matches('.')
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        (before, reqs)
    } else {
        (text.to_owned(), Vec::new())
    }
}

/// Extract `Files: \`...\`` from a text fragment.
fn extract_files(text: &str) -> (String, Vec<String>) {
    if let Some(files_pos) = text.find("Files:") {
        let before = text[..files_pos].trim_end_matches(['.', ' ']).to_owned();
        let files_section = &text[files_pos + "Files:".len()..];

        let mut files = Vec::new();
        let mut remaining = files_section;
        while let Some(open) = remaining.find('`') {
            remaining = &remaining[open + 1..];
            if let Some(close) = remaining.find('`') {
                let file = remaining[..close].to_owned();
                if !file.is_empty() {
                    files.push(file);
                }
                remaining = &remaining[close + 1..];
            } else {
                break;
            }
        }

        (before, files)
    } else {
        (text.to_owned(), Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn example_prd_markdown() -> String {
        r#"hox:prd:v1 — Add webhook support to the event system

## Vision

Enable external services to subscribe to Brevity events via HTTP webhooks.

## Actors

- Service Owner
- External Service

## Success Criteria

- [ ] Webhook endpoint registration succeeds via REST API
- [ ] Delivery retries occur on transient failure (3 attempts)

## Scope

### In Scope
- Webhook registration REST API
- Event delivery with retry logic

### Out of Scope
- GraphQL subscriptions

## Requirements

### Functional
- **[REQ-001]**: System must accept webhook URL registrations via POST /webhooks
- **[REQ-002]**: Events must be delivered within 5 seconds under normal load

### Non-Functional
- **[NFR-001]**: Delivery retry must use exponential backoff capped at 60 seconds

## Technical Constraints

- Must use existing NATS infrastructure

## Decomposition Hints

1. **API Layer**: Implement registration endpoints. Files: `crates/brevity-api/`. Covers: REQ-001.
2. **Delivery Engine**: Event fanout and retry logic. Files: `crates/brevity-webhooks/`. Covers: REQ-002, NFR-001.
"#
        .to_owned()
    }

    #[test]
    fn test_is_prd_true() {
        assert!(Prd::is_prd("hox:prd:v1 — Something"));
        // The sentinel must be the exact v1 string; unknown versions are rejected.
        assert!(!Prd::is_prd("hox:prd:v2 — Future version"));
    }

    #[test]
    fn test_is_prd_false() {
        assert!(!Prd::is_prd("This is a normal commit message"));
        assert!(!Prd::is_prd(""));
        assert!(!Prd::is_prd("HOX:PRD:V1 — uppercase"));
        assert!(!Prd::is_prd("hox:prd: — no version"));
    }

    #[test]
    fn test_parse_full_example() {
        let md = example_prd_markdown();
        let prd = Prd::from_markdown(&md).expect("parse should succeed");

        assert_eq!(prd.version, "v1");
        assert_eq!(prd.title, "Add webhook support to the event system");
        assert!(!prd.vision.is_empty());

        assert_eq!(prd.actors.len(), 2);
        assert_eq!(prd.actors[0], "Service Owner");

        assert_eq!(prd.success_criteria.len(), 2);

        assert_eq!(prd.scope_in.len(), 2);
        assert_eq!(prd.scope_out.len(), 1);

        let functional: Vec<_> = prd
            .requirements
            .iter()
            .filter(|r| r.kind == RequirementKind::Functional)
            .collect();
        let non_functional: Vec<_> = prd
            .requirements
            .iter()
            .filter(|r| r.kind == RequirementKind::NonFunctional)
            .collect();
        assert_eq!(functional.len(), 2);
        assert_eq!(non_functional.len(), 1);
        assert_eq!(functional[0].id, "REQ-001");
        assert_eq!(non_functional[0].id, "NFR-001");

        assert_eq!(prd.constraints.len(), 1);

        assert_eq!(prd.decomposition_hints.len(), 2);
        assert_eq!(prd.decomposition_hints[0].name, "API Layer");
        assert_eq!(
            prd.decomposition_hints[0].estimated_files,
            vec!["crates/brevity-api/"]
        );
        assert_eq!(prd.decomposition_hints[0].requirements, vec!["REQ-001"]);
        assert_eq!(prd.decomposition_hints[1].name, "Delivery Engine");
        assert_eq!(
            prd.decomposition_hints[1].requirements,
            vec!["REQ-002", "NFR-001"]
        );
    }

    #[test]
    fn test_round_trip() {
        let md = example_prd_markdown();
        let prd = Prd::from_markdown(&md).expect("first parse");
        let re_serialized = prd.to_markdown();
        let prd2 = Prd::from_markdown(&re_serialized).expect("second parse");

        assert_eq!(prd.title, prd2.title);
        assert_eq!(prd.version, prd2.version);
        assert_eq!(prd.vision, prd2.vision);
        assert_eq!(prd.actors.len(), prd2.actors.len());
        assert_eq!(prd.success_criteria.len(), prd2.success_criteria.len());
        assert_eq!(prd.scope_in.len(), prd2.scope_in.len());
        assert_eq!(prd.scope_out.len(), prd2.scope_out.len());
        assert_eq!(prd.requirements.len(), prd2.requirements.len());
        assert_eq!(prd.constraints.len(), prd2.constraints.len());
        assert_eq!(
            prd.decomposition_hints.len(),
            prd2.decomposition_hints.len()
        );

        let ids1: Vec<_> = prd.requirements.iter().map(|r| r.id.as_str()).collect();
        let ids2: Vec<_> = prd2.requirements.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn test_missing_sentinel_error() {
        let result = Prd::from_markdown("This is not a PRD");
        assert!(result.is_err());
    }

    #[test]
    fn test_to_markdown_starts_with_sentinel() {
        let prd = Prd::new("Test PRD");
        let out = prd.to_markdown();
        assert!(out.starts_with("hox:prd:v1"), "must start with sentinel");
    }

    #[test]
    fn test_backfill_empty_prd() {
        let mut prd = Prd {
            version: String::new(),
            title: String::new(),
            vision: String::new(),
            actors: Vec::new(),
            success_criteria: Vec::new(),
            scope_in: Vec::new(),
            scope_out: Vec::new(),
            requirements: Vec::new(),
            constraints: Vec::new(),
            decomposition_hints: Vec::new(),
        };

        prd.backfill();

        assert_eq!(prd.version, "v1");
        assert!(!prd.title.is_empty());
        assert!(!prd.vision.trim().is_empty());
        assert!(!prd.success_criteria.is_empty());
        assert!(!prd.scope_in.is_empty());
        assert!(prd
            .requirements
            .iter()
            .any(|r| r.kind == RequirementKind::Functional));
        assert!(!prd.decomposition_hints.is_empty());
    }

    #[test]
    fn test_backfill_idempotent_on_complete_prd() {
        let md = example_prd_markdown();
        let mut prd = Prd::from_markdown(&md).unwrap();
        let before_title = prd.title.clone();
        let before_req_count = prd.requirements.len();

        prd.backfill();

        assert_eq!(prd.title, before_title);
        assert_eq!(prd.requirements.len(), before_req_count);
    }

    #[test]
    fn test_empty_sections_produce_defaults() {
        let md = "hox:prd:v1 \u{2014} Minimal\n\n## Vision\n\nSomething\n";
        let prd = Prd::from_markdown(md).unwrap();
        assert!(prd.actors.is_empty());
        assert!(prd.success_criteria.is_empty());
        assert!(prd.requirements.is_empty());
    }
}
