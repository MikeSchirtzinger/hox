package orchestrator

import (
	"context"
	"fmt"
	"strings"
)

// RevsetQueries provides common revset patterns for task orchestration.
type RevsetQueries struct {
	jj JJExecutor
}

// NewRevsetQueries creates a revset query helper.
func NewRevsetQueries(jj JJExecutor) *RevsetQueries {
	return &RevsetQueries{jj: jj}
}

// ReadyTasks returns tasks that have no incomplete dependencies.
// These are leaf nodes in the task DAG that aren't in conflict.
func (r *RevsetQueries) ReadyTasks(ctx context.Context) ([]string, error) {
	// heads() gives us leaf nodes (no children)
	// We filter to task bookmarks and exclude conflicts
	revset := `heads(bookmarks(glob:"task-*")) - conflicts()`
	return r.queryChangeIDs(ctx, revset)
}

// BlockedTasks returns tasks that have incomplete ancestors.
func (r *RevsetQueries) BlockedTasks(ctx context.Context) ([]string, error) {
	// Tasks that have non-immutable ancestors (incomplete work)
	// This finds tasks whose dependencies aren't done yet
	revset := `bookmarks(glob:"task-*") & descendants(mutable())`
	return r.queryChangeIDs(ctx, revset)
}

// AgentTasks returns all tasks assigned to a specific agent.
func (r *RevsetQueries) AgentTasks(ctx context.Context, agentID string) ([]string, error) {
	revset := fmt.Sprintf(`bookmarks(glob:"agent-%s/*")`, agentID)
	return r.queryChangeIDs(ctx, revset)
}

// UnassignedTasks returns tasks with no agent bookmark.
func (r *RevsetQueries) UnassignedTasks(ctx context.Context) ([]string, error) {
	// Task bookmarks minus agent bookmarks
	revset := `bookmarks(glob:"task-*") - bookmarks(glob:"agent-*/*")`
	return r.queryChangeIDs(ctx, revset)
}

// TaskDependencies returns all changes that must complete before the given task.
func (r *RevsetQueries) TaskDependencies(ctx context.Context, changeID string) ([]string, error) {
	// All ancestors of this change (except immutable/root)
	revset := fmt.Sprintf(`ancestors(%s) & mutable()`, changeID)
	return r.queryChangeIDs(ctx, revset)
}

// DependentTasks returns all changes that depend on the given task.
func (r *RevsetQueries) DependentTasks(ctx context.Context, changeID string) ([]string, error) {
	// All descendants of this change
	revset := fmt.Sprintf(`descendants(%s) - %s`, changeID, changeID)
	return r.queryChangeIDs(ctx, revset)
}

// ConflictingTasks returns tasks that have conflicts.
func (r *RevsetQueries) ConflictingTasks(ctx context.Context) ([]string, error) {
	revset := `bookmarks(glob:"task-*") & conflicts()`
	return r.queryChangeIDs(ctx, revset)
}

// TasksByLabel returns tasks matching a label pattern.
// Labels are stored in metadata.jsonl, so this is a two-step query:
// 1. Get all task changes
// 2. Filter by metadata
// For now, this returns all task bookmarks (metadata filtering happens at higher level)
func (r *RevsetQueries) TasksByLabel(ctx context.Context, label string) ([]string, error) {
	// Note: jj doesn't have native label support, so we just return all tasks
	// The caller filters by metadata.jsonl
	return r.AllTasks(ctx)
}

// AllTasks returns all task changes.
func (r *RevsetQueries) AllTasks(ctx context.Context) ([]string, error) {
	revset := `bookmarks(glob:"task-*")`
	return r.queryChangeIDs(ctx, revset)
}

// InProgressTasks returns tasks that are actively being worked on.
// These have agent bookmarks pointing to them.
func (r *RevsetQueries) InProgressTasks(ctx context.Context) ([]string, error) {
	revset := `bookmarks(glob:"agent-*/*")`
	return r.queryChangeIDs(ctx, revset)
}

// queryChangeIDs executes a revset query and returns change IDs.
func (r *RevsetQueries) queryChangeIDs(ctx context.Context, revset string) ([]string, error) {
	output, err := r.jj.Exec(ctx, "log",
		"-r", revset,
		"--no-graph",
		"-T", `change_id ++ "\n"`,
	)
	if err != nil {
		// Empty result is not an error
		if strings.Contains(string(output), "no matching") {
			return nil, nil
		}
		return nil, fmt.Errorf("revset query failed: %w", err)
	}

	var ids []string
	for _, line := range strings.Split(string(output), "\n") {
		id := strings.TrimSpace(line)
		if id != "" {
			ids = append(ids, id)
		}
	}

	return ids, nil
}

// QueryResult holds the result of a complex query.
type QueryResult struct {
	ChangeID    string
	Bookmark    string
	Description string
	Author      string
	Timestamp   string
}

// QueryTasks executes a revset and returns detailed results.
func (r *RevsetQueries) QueryTasks(ctx context.Context, revset string) ([]QueryResult, error) {
	// Use a custom template to get structured output
	template := `change_id ++ "|" ++ bookmarks ++ "|" ++ description.first_line() ++ "|" ++ author ++ "|" ++ committer.timestamp() ++ "\n"`

	output, err := r.jj.Exec(ctx, "log",
		"-r", revset,
		"--no-graph",
		"-T", template,
	)
	if err != nil {
		return nil, fmt.Errorf("query failed: %w", err)
	}

	var results []QueryResult
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.SplitN(line, "|", 5)
		if len(parts) < 5 {
			continue
		}

		results = append(results, QueryResult{
			ChangeID:    strings.TrimSpace(parts[0]),
			Bookmark:    strings.TrimSpace(parts[1]),
			Description: strings.TrimSpace(parts[2]),
			Author:      strings.TrimSpace(parts[3]),
			Timestamp:   strings.TrimSpace(parts[4]),
		})
	}

	return results, nil
}

// DependencyGraph builds a dependency graph for visualization.
type DependencyGraph struct {
	Nodes []GraphNode
	Edges []GraphEdge
}

type GraphNode struct {
	ChangeID string
	Label    string
	Status   string // ready, blocked, in_progress, completed
}

type GraphEdge struct {
	From string // change ID
	To   string // change ID
	Type string // blocks, parent-child
}

// BuildDependencyGraph creates a visual representation of task dependencies.
func (r *RevsetQueries) BuildDependencyGraph(ctx context.Context) (*DependencyGraph, error) {
	graph := &DependencyGraph{}

	// Get all tasks
	tasks, err := r.QueryTasks(ctx, `bookmarks(glob:"task-*")`)
	if err != nil {
		return nil, err
	}

	// Track statuses
	ready, _ := r.ReadyTasks(ctx)
	readySet := make(map[string]bool)
	for _, id := range ready {
		readySet[id] = true
	}

	inProgress, _ := r.InProgressTasks(ctx)
	inProgressSet := make(map[string]bool)
	for _, id := range inProgress {
		inProgressSet[id] = true
	}

	conflicts, _ := r.ConflictingTasks(ctx)
	conflictSet := make(map[string]bool)
	for _, id := range conflicts {
		conflictSet[id] = true
	}

	// Build nodes
	for _, task := range tasks {
		status := "blocked"
		if readySet[task.ChangeID] {
			status = "ready"
		}
		if inProgressSet[task.ChangeID] {
			status = "in_progress"
		}
		if conflictSet[task.ChangeID] {
			status = "conflict"
		}

		graph.Nodes = append(graph.Nodes, GraphNode{
			ChangeID: task.ChangeID,
			Label:    task.Description,
			Status:   status,
		})

		// Get dependencies for edges
		deps, err := r.TaskDependencies(ctx, task.ChangeID)
		if err == nil {
			for _, dep := range deps {
				graph.Edges = append(graph.Edges, GraphEdge{
					From: dep,
					To:   task.ChangeID,
					Type: "blocks",
				})
			}
		}
	}

	return graph, nil
}
