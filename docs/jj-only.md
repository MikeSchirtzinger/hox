Here's how the jj-only approach implements all beads functionality natively in JJ:

  ---
  JJ-Native Beads: The Core Insight

  jj IS a graph database. Instead of maintaining SQLite + git sync, the VCS itself becomes the source of truth.

  ---
  Feature-by-Feature Mapping
  ┌─────────────────────┬────────────────────────────┬─────────────────────────────────────┐
  │    Beads Feature    │ Git/SQLite Implementation  │      JJ-Native Implementation       │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Issue/Task          │ SQLite row + git sync      │ JJ change (each task = a change)    │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Dependencies        │ blocks table in SQLite     │ DAG ancestry (parent-child)         │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Status: in_progress │ SQLite field               │ Bookmark agent-{id}/{task}          │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Status: ready       │ Complex SQL query          │ Revset: heads(task-*) - conflicts() │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Assigned to         │ SQLite field               │ Bookmark namespace: agent-42/*      │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Task metadata       │ SQLite columns             │ .tasks/metadata.jsonl + description │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Worktree isolation  │ Git worktrees (~450 lines) │ JJ workspaces (native)              │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Sync branch         │ Complex worktree manager   │ jj git push -b beads-sync           │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Conflict resolution │ Blocks operation           │ First-class (continue working)      │
  ├─────────────────────┼────────────────────────────┼─────────────────────────────────────┤
  │ Undo                │ Manual git reflog          │ jj op undo (one command)            │
  └─────────────────────┴────────────────────────────┴─────────────────────────────────────┘
  ---
  ![[Pasted image 20260117165731.png]]
  How Each Core Function Works

  1. Creating a Task

  // OLD: SQLite insert + JSONL export + git worktree commit
  db.Exec("INSERT INTO issues...")
  exportToJSONL()
  worktreeManager.Commit()

  // JJ-NATIVE: Create change + set bookmark
  jj.Exec("new", "-m", task.FormatDescription())
  jj.Exec("bookmark", "create", "task-"+taskID)

  2. Finding Ready Work

  // OLD: Complex SQL join across tables
  SELECT * FROM issues
  WHERE id NOT IN (SELECT blocker_id FROM blocks WHERE blocked_id = ...)

  // JJ-NATIVE: Single revset query
  revset := `heads(bookmarks(glob:"task-*")) - conflicts()`
  jj.Exec("log", "-r", revset, "-T", "change_id")

  3. Assigning to Agent

  // OLD: Update SQLite + trigger sync
  UPDATE issues SET checked_out_by = 'agent-42' WHERE id = ?

  // JJ-NATIVE: Create namespaced bookmark
  jj.Exec("bookmark", "create", "agent-42/task-xyz", "-r", changeID)

  4. Agent Handoff (Context Preservation)

  // JJ-NATIVE: Context lives in change description
  task.Context = &HandoffContext{
      CurrentFocus: "Implementing VCS interface",
      Progress:     []string{"Designed interface", "Implemented git backend"},
      NextSteps:    []string{"Implement jj backend", "Add tests"},
      Blockers:     []string{},
      FilesTouched: []string{"internal/vcs/vcs.go", "internal/vcs/git/"},
  }
  jj.Exec("describe", "-m", task.FormatDescription())
  // No explicit commit - jj auto-tracks all file changes!

  5. Querying Dependencies

  // What blocks task-xyz?
  jj.Exec("log", "-r", "ancestors(task-xyz) & mutable()")

  // What does task-xyz block?
  jj.Exec("log", "-r", "descendants(task-xyz)")

  6. Parallel Agent Workspaces

  # Each agent gets isolated workspace
  jj workspace add --name agent-42 /tmp/agent-42-workspace

  # Agent works in isolation, no stepping on each other
  # Merge when ready:
  jj new agent-42/task-1 agent-99/task-2 -m "Merge parallel work"

  ---
  The Structured Description Format

  Every task change has machine-parseable context in its description:

  Task: Implement VCS abstraction layer
  Priority: 1
  Status: in_progress
  Agent: agent-42

  ## Context
  Working on the VCS interface. Need to support both git and jj.

  ## Progress
  - [x] Designed interface
  - [x] Implemented git backend
  - [ ] Implementing jj backend

  ## Next Steps
  - Fix bookmark parsing
  - Add workspace support

  ## Blockers
  None currently

  ## Files Touched
  internal/vcs/vcs.go
  internal/vcs/git/
  internal/vcs/jj/

  ---
  Key Advantages

  1. No sync problem - JJ IS the source of truth (no SQLite ↔ git drift)
  2. Natural dependencies - Code ancestry = task dependencies
  3. Distributed native - Multiple agents, concurrent work, built-in conflict resolution
  4. Change evolution - Rebase, split, squash - graph auto-updates
  5. Safe experimentation - jj op undo for any mistake
  6. Audit trail - Operation log shows all agent actions

  ---
  File Reduction

  DELETED (no longer needed):
  - internal/git/worktree.go (449 lines)
  - internal/syncbranch/worktree.go (1141 lines)
  - cmd/bd/sync_git.go (583 lines)
  - cmd/bd/daemon_sync_branch.go (334 lines)

  ADDED:
  - internal/orchestrator/task.go (561 lines)
  - internal/orchestrator/revsets.go (246 lines)
  - internal/orchestrator/handoff.go (267 lines)

  NET: ~2500 lines → ~1100 lines (56% reduction)

  The jj-only branch has the working prototype with 1656 new lines implementing this architecture.