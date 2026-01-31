# JJ Features for Agent Parallel Development: Full Compiled Analysis

**Date:** 2026-01-30
**Context:** Comprehensive research on JJ VCS features for Hox agent parallel development
**Method:** 5 parallel research agents covering parallelize/core commands, workspaces/colocated repos, advanced DAG/revsets, ecosystem/future, and Hox gap analysis
**Project:** Hox (~/dev/hox)

---

## Part 1: Commands We SHOULD Be Using But Aren't

### 1. `jj parallelize` — DAG Restructuring (HIGH PRIORITY)

**What it does:** Transforms sequential commits into parallel siblings sharing a common parent.

```
Before:  3 - 2 - 1 - 0       After:     3
                                        / \
                                       1   2
                                        \ /
                                         0
```

**Why Hox needs it:**
- Orchestrator creates tasks linearly during planning, then discovers independence
- `jj parallelize` restructures the DAG with a single command
- Bookmarks auto-track through rewrites (agent assignments stay stable)
- Perfect for "plan then optimize" workflow

**Current gap:** Not implemented in any Hox crate. No DAG restructuring capability exists.

**Limitation:** Group-based parallelization not yet supported (GitHub #5324). Workaround: create merge points first, then parallelize roots.

---

### 2. `jj absorb` — Auto-Distribute Changes (HIGH PRIORITY)

**What it does:** Takes changes in the working copy and automatically routes each hunk to the correct ancestor commit where those lines were last modified. Line-level precision.

```bash
jj absorb              # Routes all changes to correct ancestors
jj absorb src/auth.rs  # Only absorb changes to specific files
```

**Why Hox needs it:**
- **Megamerge pattern**: Multiple agents work in parallel → orchestrator creates merge commit for integration testing → makes fixes → `jj absorb` distributes fixes back to correct agent branches
- Ralph loop integration: Agent makes small fix → `jj absorb` applies to correct ancestor
- Eliminates manual "which commit does this fix belong to?" decisions

**Current gap:** Not used anywhere in Hox. Not integrated into backpressure system.

---

### 3. `jj split` / `jj squash` — Task Granularity (MEDIUM-HIGH)

**What they do:**
- `jj split`: Splits a change into smaller pieces (interactive or by file)
- `jj squash`: Folds changes together (into parent or arbitrary target)

**Why Hox needs them:**
- Agent realizes task is too large → `jj split` creates subtasks
- Agent completes multiple small changes → `jj squash` consolidates
- "Checkpoint then consolidate" pattern for long-running agents

**Current gap:** No task decomposition/composition logic exists.

---

### 4. `jj fix` — Automated Code Fixers (MEDIUM)

**What it does:** Runs configured formatters/linters on all mutable commits retroactively. Applies fixes to historical commits and automatically rebases descendants.

```toml
# .jj/repo/config.toml
[fix.tools.rustfmt]
command = ["rustfmt", "--edition", "2021"]
patterns = ["glob:*.rs"]
```

```bash
jj fix           # Fix all mutable commits
jj fix -s @      # Fix only current commit
```

**Why Hox needs it:**
- Agent writes code → `jj fix` auto-formats → clean output
- Resolves formatting-only conflicts automatically
- Should be part of backpressure system (currently exists in `hox-agent/src/backpressure.rs` but doesn't use `jj fix`)

---

### 5. `jj op undo` / `jj op restore` — Agent Rollback (HIGH PRIORITY)

**What they do:**
- `jj undo`: Sequential undo (can call multiple times)
- `jj redo`: Redo after undo
- `jj op restore <op-id>`: Restore entire repo to earlier state
- `jj op revert <op-id>`: Revert specific non-recent operation

**Why Hox needs it:**
- Agent makes bad decision → rollback to before agent started
- No recovery mechanism currently exists (oplog is watched but never manipulated)
- Operation log captures complete repo snapshots including all workspace states

**Current gap:** `OpLogWatcher` polls but never manipulates. No `rollback_agent()` function.

---

### 6. `jj bookmark` — Task Assignment (CRITICAL)

**What it does:** Named pointers to changes that auto-track through rewrites.

**Why Hox needs it:**
- CLAUDE.md says "Assignments = bookmarks" but **no bookmark management code exists**
- Currently parsing descriptions for `Agent: agent-42` (regex, O(n))
- Bookmarks are indexed → `bookmarks(glob:"agent-42/*")` is O(1)
- Bookmarks auto-update when changes are rewritten

**Current gap:** Mentioned in docs, completely unimplemented. This is the single biggest gap.

---

### 7. `jj duplicate` — Speculative Execution (MEDIUM)

**What it does:** Creates copies of changes with flexible positioning.

```bash
jj duplicate task-refactor -d main  # Copy task for experimentation
```

**Why Hox needs it:**
- Try multiple approaches to same task in parallel
- Agents can fork a task without affecting the original
- Create backup before risky operations

---

### 8. `jj backout` — Safe Reversion (LOW-MEDIUM)

**What it does:** Creates a new change that undoes the effect of a specified change.

**Why Hox needs it:** Agents can safely revert work without destructive history editing.

---

### 9. `jj evolog` — Change Evolution Tracking (LOW)

**What it does:** Shows the evolution of a specific change over time (all rewrites, amends).

**Why Hox needs it:** Agent audit trails — see how a task evolved through multiple agent iterations.

---

### 10. `jj simplify-parents` — DAG Cleanup (LOW)

**What it does:** Removes redundant parent relationships (transitive ancestors).

**Why Hox needs it:** Clean up merge commits after multi-agent integration.

---

## Part 2: Features We Use But Could Use Better

### Advanced Revsets

**Currently using:** Basic predicates (`description(glob:...)`, `conflicts()`, `ancestors()`, `descendants()`)

**Should also use:**

| Revset | Purpose | Agent Use Case |
|--------|---------|---------------|
| `bookmarks(glob:"agent-42/*")` | Find agent's tasks | O(1) vs description grep |
| `mine()` | Current user's commits | Agent identity |
| `latest(x, n)` | Most recent n commits | "What did agent do last?" |
| `present(x)` | Safe reference (no error if missing) | Robust queries |
| `mutable()` / `immutable()` | Rewritable vs protected | Agent safety boundaries |
| `empty()` | Find empty commits | Cleanup abandoned tasks |
| `connected(x)` | Connected component | Find task subgraphs |
| `file(path)` | Changes touching file | Find who changed what |
| `author(pattern)` | Filter by author | Agent attribution |

**Power queries for Hox:**
```bash
# Ready tasks (no blockers, no conflicts)
heads(description(glob:"Status: open")) - conflicts() - ancestors(conflicts())

# Agent's active work
bookmarks(glob:"agent-42/*") & ~description(glob:"Status: done")

# Parallelizable tasks (independent heads)
heads(mutable()) & ~merges() & ~conflicts()

# What blocks this task?
ancestors(task-xyz) & mutable() & conflicts()
```

### Workspace Configuration

**New feature (2025-2026):** Per-workspace config at `.jj/workspace-config.toml`

```bash
jj config set --workspace ui.default-command log
```

**Hox should:** Configure agent-specific settings per workspace (e.g., different immutable_heads per agent).

### Template System for Metadata

**Currently:** Parse descriptions with regex
**Should:** Use JJ's template system for structured extraction

```bash
# Extract task metadata as JSON
jj log --no-graph -T 'json({
  id: change_id,
  status: description.substr(0, 20),
  agent: author.name()
})'
```

---

## Part 3: Conflict Handling — We're Leaving Value on the Table

**Current state:** Detect conflicts, warn, do nothing.

**What JJ actually offers:**

1. **Conflicts are committable** — agents don't block each other
2. **Automatic descendant rebasing** — conflict resolution propagates
3. **Algebraic conflict representation** — no nested conflict markers
4. **`jj resolve`** — interactive or automated resolution
5. **`jj fix` resolves formatting conflicts** automatically

**Recommended architecture:**
```
Agent detects conflict →
  If formatting-only → jj fix (auto-resolve)
  If :ours/:theirs sufficient → jj resolve --tool=:ours
  If complex → spawn conflict-resolver agent
  Else → queue for human review
```

---

## Part 4: Forward-Thinking Possibilities

### RPC API (Planned)

JJ roadmap includes an RPC API for programmatic access. Currently we shell out to `jj` CLI and parse output. When the RPC API lands, Hox could use it directly from Rust via `jj-lib`.

### Cloud-Based Repository Server (Planned)

Google has an internal JJ server backed by database for cloud storage of commits with local daemon for caching. When available externally, this could enable distributed agent orchestration across machines.

### `jj script` (Prototype)

User-level scripting language for JJ automation. Could replace shell scripts for agent orchestration.

### VFS / Lazy Loading (Planned)

Virtual filesystem that only materializes files agents actually touch. Would dramatically reduce workspace creation overhead for large repos.

### Forge Integration (Planned)

Native `jj github submit` / `jj gitlab submit` commands. Would simplify the GitHub push workflow in Hox.

---

## Part 5: Alternatives Comparison

| Feature | JJ | Sapling | git-branchless | Pijul |
|---------|-----|---------|---------------|-------|
| Committable conflicts | Yes | No | No | Yes (patches) |
| Auto-rebase | Yes | Yes (blocks on conflict) | No | N/A |
| Stable change IDs | Yes | Yes | No (git hashes) | Yes (patch IDs) |
| Operation log/undo | Yes | Yes (less accessible) | No | No |
| Git compatibility | Full colocated | Basic clone/push | Native git | Limited |
| Multiple workspaces | Native | No | git worktree | No |
| Template system | Rich | Basic | N/A | No |
| Rust native | Yes | Partially | Python wrapper | Yes |

**Verdict:** JJ is the strongest choice for AI agent parallel development. No purpose-built agent VCS exists, and JJ's design accidentally aligns perfectly with agent needs.

---

## Part 6: Hox Gap Analysis

### What Hox Currently Uses

| Feature | Implementation | Location |
|---------|---------------|----------|
| Basic change ops | `jj new`, `jj describe`, `jj log` | `hox-jj/src/command.rs` |
| Metadata parsing | Regex on descriptions | `hox-jj/src/metadata.rs` |
| Basic revsets | `heads()`, `conflicts()`, `ancestors()`, `descendants()` | `hox-jj/src/revsets.rs` |
| Workspace mgmt | `jj workspace add/forget/list` | `hox-orchestrator/src/workspace.rs` |
| OpLog watching | Poll `jj op log` at 500ms | `hox-jj/src/oplog.rs` |
| Conflict detection | Query `conflicts()` revset | Multiple locations |
| Basic templates | `-T description`, `-T change_id` | All `jj log` calls |

### What's Missing

| Gap | Priority | Current Workaround |
|-----|----------|-------------------|
| Bookmark management | CRITICAL | Description parsing (O(n)) |
| Operation rollback | HIGH | None — no recovery |
| `jj parallelize` | HIGH | Manual branch creation |
| `jj absorb` | HIGH | None |
| Conflict resolution pipeline | HIGH | Warn and do nothing |
| `jj split`/`jj squash` | MEDIUM-HIGH | None |
| `jj fix` integration | MEDIUM | Manual formatting |
| Advanced revsets | MEDIUM | Basic description greps |
| `jj duplicate` | MEDIUM | None |
| Rich templates | LOW-MEDIUM | String concatenation |
| `jj evolog` | LOW | None |
| `jj backout` | LOW | None |

---

## Part 7: Prioritized Implementation Roadmap

### Phase 1: Foundation (Blocking)
1. **Implement bookmark management** — replace description parsing
2. **Add `jj op restore/undo`** — agent rollback capability
3. **Build conflict resolution pipeline** — auto-resolve + escalation

### Phase 2: Task Manipulation (High Value)
4. **Integrate `jj split`** — task decomposition
5. **Integrate `jj squash`** — task consolidation
6. **Integrate `jj absorb`** — megamerge workflow for integration testing

### Phase 3: DAG Optimization (Performance)
7. **Refactor to bookmark-based revsets** — faster queries
8. **Add `jj parallelize`** — DAG restructuring
9. **Integrate `jj fix`** — automated formatting

### Phase 4: Advanced (Future)
10. **Rich templates** for dashboard/observability
11. **`jj duplicate`** for speculative execution
12. **`jj evolog`** for audit trails
13. **Monitor RPC API** — migrate from CLI shelling when available

### Proposed New Files
```
crates/hox-jj/src/bookmarks.rs         # NEW - Bookmark management
crates/hox-jj/src/changes.rs           # NEW - split/squash/absorb/parallelize
crates/hox-jj/src/oplog.rs             # EXTEND - Add undo/restore
crates/hox-orchestrator/src/conflict_resolver.rs  # NEW - Conflict pipeline
crates/hox-agent/src/backpressure.rs    # EXTEND - Add jj fix
```

---

## Sources

### Official Documentation
- [JJ GitHub Repository](https://github.com/jj-vcs/jj)
- [Templating Language Docs](https://jj-vcs.github.io/jj/latest/templates/)
- [Revset Language Docs](https://jj-vcs.github.io/jj/latest/revsets/)
- [Settings & Configuration Docs](https://docs.jj-vcs.dev/latest/config/)
- [Development Roadmap](http://docs.jj-vcs.dev/latest/roadmap/)
- [Operation Log Docs](https://jj-vcs.github.io/jj/latest/operation-log/)
- [Conflicts Docs](https://jj-vcs.github.io/jj/latest/conflicts/)
- [Working with GitHub](https://jj-vcs.github.io/jj/latest/github/)
- [CLI Reference](https://docs.jj-vcs.dev/latest/cli-reference/)
- [Changelog](https://docs.jj-vcs.dev/latest/changelog/)
- [Sapling Comparison](http://docs.jj-vcs.dev/latest/sapling-comparison/)

### GitHub Issues & Discussions
- [FR: Allow parallelize to work with groups · Issue #5324](https://github.com/jj-vcs/jj/issues/5324)
- [Stacked PR Workflow Discussion](https://github.com/jj-vcs/jj/discussions/5509)
- [Working branches and the JJ "way"](https://github.com/jj-vcs/jj/discussions/2425)
- [Git Hook Support Discussion](https://github.com/jj-vcs/jj/discussions/403)
- [JJ Run Design Doc](https://jj-vcs.github.io/jj/latest/design/run/)

### AI Agent Integration
- [Integrate JJ for Multi-Agent Collaboration (agentic-flow)](https://github.com/ruvnet/agentic-flow/issues/54)
- [Parallel Claude Code with Jujutsu](https://slavakurilyak.com/posts/parallel-claude-code-with-jujutsu)

### Community & Tutorials
- [JJ Con 2025 Wiki](https://github.com/jj-vcs/jj/wiki/JJ-Con-2025)
- [Steve's Jujutsu Tutorial](https://steveklabnik.github.io/jujutsu-tutorial/)
- [Chris Krycho — Jujutsu Megamerges and jj absorb](https://v5.chriskrycho.com/journal/jujutsu-megamerges-and-jj-absorb)
- [Deferred Conflict Resolution in Jujutsu](https://v5.chriskrycho.com/journal/deferred-conflict-resolution-in-jujutsu/)
- [Jujutsu VCS Introduction and Patterns](https://kubamartin.com/posts/introduction-to-the-jujutsu-vcs/)
- [jj part 2: commands & revsets](https://andre.arko.net/2025/10/02/jj-part-2-commands/)
- [Jujutsu in practice](https://arne.me/blog/jj-in-practice)
- [What I've learned from jj](https://zerowidth.com/2025/what-ive-learned-from-jj/)
- [jj-stack Tool](https://github.com/keanemind/jj-stack)
- [Automatically resolve formatting conflicts with jj fix](https://dev.to/nyctef/automatically-resolve-formatting-conflicts-with-jj-fix-b92)
- [Jujutsu Operation Log Workflows](https://kristofferbalintona.me/posts/202503270335/)
