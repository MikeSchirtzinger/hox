// Package orchestrator provides jj-native task and agent orchestration.
//
// Instead of maintaining a separate dependency graph in SQLite, this package
// uses jj's native change DAG as the task graph. Tasks are changes, dependencies
// are ancestry, and assignments are bookmarks.
package orchestrator

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// Task represents a work item tracked as a jj change.
type Task struct {
	// ChangeID is the jj change ID (e.g., "xyzabc12")
	ChangeID string `json:"change_id"`

	// Title is the task title
	Title string `json:"title"`

	// Description is the full task description
	Description string `json:"description,omitempty"`

	// Priority: 0=critical, 1=high, 2=medium, 3=low, 4=backlog
	Priority int `json:"priority"`

	// Status: pending, in_progress, blocked, completed
	Status string `json:"status"`

	// Agent is the assigned agent ID (empty if unassigned)
	Agent string `json:"agent,omitempty"`

	// Labels for categorization
	Labels []string `json:"labels,omitempty"`

	// DueDate for time-sensitive tasks
	DueDate *time.Time `json:"due_date,omitempty"`

	// Context is the handoff context for agent continuity
	Context *HandoffContext `json:"context,omitempty"`

	// Bookmark is the jj bookmark name for this task
	Bookmark string `json:"bookmark,omitempty"`
}

// HandoffContext captures agent state for seamless handoffs.
type HandoffContext struct {
	// CurrentFocus describes what the agent was working on
	CurrentFocus string `json:"current_focus"`

	// Progress lists completed items
	Progress []string `json:"progress"`

	// NextSteps lists immediate next actions
	NextSteps []string `json:"next_steps"`

	// Blockers lists any blocking issues
	Blockers []string `json:"blockers,omitempty"`

	// OpenQuestions lists unresolved decisions
	OpenQuestions []string `json:"open_questions,omitempty"`

	// FilesTouched lists recently modified files
	FilesTouched []string `json:"files_touched,omitempty"`

	// UpdatedAt is when this context was last updated
	UpdatedAt time.Time `json:"updated_at"`
}

// TaskMetadata is stored in .tasks/metadata.jsonl for non-DAG data.
type TaskMetadata struct {
	ChangeID  string     `json:"change_id"`
	Priority  int        `json:"priority"`
	Labels    []string   `json:"labels,omitempty"`
	DueDate   *time.Time `json:"due_date,omitempty"`
	Agent     string     `json:"agent,omitempty"`
	CreatedAt time.Time  `json:"created_at"`
	UpdatedAt time.Time  `json:"updated_at"`
}

// DescriptionFormat is the structured format for task change descriptions.
const DescriptionFormat = `Task: %s
Priority: %d
Status: %s
Agent: %s

## Context
%s

## Progress
%s

## Next Steps
%s

## Blockers
%s

## Open Questions
%s

## Files Touched
%s
`

// FormatDescription creates a structured description for a task change.
func (t *Task) FormatDescription() string {
	agent := t.Agent
	if agent == "" {
		agent = "unassigned"
	}

	contextStr := ""
	progressStr := "None"
	nextStepsStr := "None"
	blockersStr := "None"
	questionsStr := "None"
	filesStr := "None"

	if t.Context != nil {
		contextStr = t.Context.CurrentFocus

		if len(t.Context.Progress) > 0 {
			var items []string
			for _, p := range t.Context.Progress {
				items = append(items, "- [x] "+p)
			}
			progressStr = strings.Join(items, "\n")
		}

		if len(t.Context.NextSteps) > 0 {
			var items []string
			for _, s := range t.Context.NextSteps {
				items = append(items, "- [ ] "+s)
			}
			nextStepsStr = strings.Join(items, "\n")
		}

		if len(t.Context.Blockers) > 0 {
			var items []string
			for _, b := range t.Context.Blockers {
				items = append(items, "- "+b)
			}
			blockersStr = strings.Join(items, "\n")
		}

		if len(t.Context.OpenQuestions) > 0 {
			var items []string
			for _, q := range t.Context.OpenQuestions {
				items = append(items, "- "+q)
			}
			questionsStr = strings.Join(items, "\n")
		}

		if len(t.Context.FilesTouched) > 0 {
			filesStr = strings.Join(t.Context.FilesTouched, "\n")
		}
	}

	return fmt.Sprintf(DescriptionFormat,
		t.Title,
		t.Priority,
		t.Status,
		agent,
		contextStr,
		progressStr,
		nextStepsStr,
		blockersStr,
		questionsStr,
		filesStr,
	)
}

// ParseDescription extracts task info from a structured description.
func ParseDescription(desc string) (*Task, error) {
	task := &Task{
		Context: &HandoffContext{},
	}

	lines := strings.Split(desc, "\n")
	currentSection := ""

	for _, line := range lines {
		line = strings.TrimSpace(line)

		// Parse header fields
		if strings.HasPrefix(line, "Task: ") {
			task.Title = strings.TrimPrefix(line, "Task: ")
			continue
		}
		if strings.HasPrefix(line, "Priority: ") {
			fmt.Sscanf(strings.TrimPrefix(line, "Priority: "), "%d", &task.Priority)
			continue
		}
		if strings.HasPrefix(line, "Status: ") {
			task.Status = strings.TrimPrefix(line, "Status: ")
			continue
		}
		if strings.HasPrefix(line, "Agent: ") {
			agent := strings.TrimPrefix(line, "Agent: ")
			if agent != "unassigned" {
				task.Agent = agent
			}
			continue
		}

		// Track sections
		if strings.HasPrefix(line, "## ") {
			currentSection = strings.TrimPrefix(line, "## ")
			continue
		}

		// Parse section content
		if line == "" || line == "None" {
			continue
		}

		switch currentSection {
		case "Context":
			if task.Context.CurrentFocus == "" {
				task.Context.CurrentFocus = line
			} else {
				task.Context.CurrentFocus += "\n" + line
			}

		case "Progress":
			if strings.HasPrefix(line, "- [x] ") {
				task.Context.Progress = append(task.Context.Progress,
					strings.TrimPrefix(line, "- [x] "))
			}

		case "Next Steps":
			if strings.HasPrefix(line, "- [ ] ") {
				task.Context.NextSteps = append(task.Context.NextSteps,
					strings.TrimPrefix(line, "- [ ] "))
			}

		case "Blockers":
			if strings.HasPrefix(line, "- ") {
				task.Context.Blockers = append(task.Context.Blockers,
					strings.TrimPrefix(line, "- "))
			}

		case "Open Questions":
			if strings.HasPrefix(line, "- ") {
				task.Context.OpenQuestions = append(task.Context.OpenQuestions,
					strings.TrimPrefix(line, "- "))
			}

		case "Files Touched":
			if line != "" {
				task.Context.FilesTouched = append(task.Context.FilesTouched, line)
			}
		}
	}

	return task, nil
}

// MetadataStore manages .tasks/metadata.jsonl
type MetadataStore struct {
	repoRoot string
	path     string
}

// NewMetadataStore creates a metadata store for the given repo.
func NewMetadataStore(repoRoot string) *MetadataStore {
	return &MetadataStore{
		repoRoot: repoRoot,
		path:     filepath.Join(repoRoot, ".tasks", "metadata.jsonl"),
	}
}

// EnsureDir creates the .tasks directory if needed.
func (m *MetadataStore) EnsureDir() error {
	return os.MkdirAll(filepath.Dir(m.path), 0755)
}

// Load reads all metadata from the JSONL file.
func (m *MetadataStore) Load() (map[string]*TaskMetadata, error) {
	result := make(map[string]*TaskMetadata)

	file, err := os.Open(m.path)
	if os.IsNotExist(err) {
		return result, nil
	}
	if err != nil {
		return nil, fmt.Errorf("failed to open metadata: %w", err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}

		var meta TaskMetadata
		if err := json.Unmarshal([]byte(line), &meta); err != nil {
			continue // Skip malformed lines
		}

		result[meta.ChangeID] = &meta
	}

	return result, scanner.Err()
}

// Save writes a single metadata entry (appends to JSONL).
func (m *MetadataStore) Save(meta *TaskMetadata) error {
	if err := m.EnsureDir(); err != nil {
		return err
	}

	file, err := os.OpenFile(m.path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return fmt.Errorf("failed to open metadata file: %w", err)
	}
	defer file.Close()

	data, err := json.Marshal(meta)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %w", err)
	}

	_, err = file.WriteString(string(data) + "\n")
	return err
}

// Compact rewrites the JSONL file, deduplicating by change_id (last wins).
func (m *MetadataStore) Compact() error {
	all, err := m.Load()
	if err != nil {
		return err
	}

	if err := m.EnsureDir(); err != nil {
		return err
	}

	// Write to temp file, then rename
	tmpPath := m.path + ".tmp"
	file, err := os.Create(tmpPath)
	if err != nil {
		return fmt.Errorf("failed to create temp file: %w", err)
	}

	for _, meta := range all {
		data, err := json.Marshal(meta)
		if err != nil {
			file.Close()
			os.Remove(tmpPath)
			return err
		}
		file.WriteString(string(data) + "\n")
	}

	if err := file.Close(); err != nil {
		os.Remove(tmpPath)
		return err
	}

	return os.Rename(tmpPath, m.path)
}

// TaskManager orchestrates tasks using jj changes.
type TaskManager struct {
	repoRoot string
	jj       JJExecutor
	metadata *MetadataStore
}

// JJExecutor is an interface for running jj commands.
type JJExecutor interface {
	Exec(ctx context.Context, args ...string) ([]byte, error)
}

// NewTaskManager creates a task manager for the given repo.
func NewTaskManager(repoRoot string, jj JJExecutor) *TaskManager {
	return &TaskManager{
		repoRoot: repoRoot,
		jj:       jj,
		metadata: NewMetadataStore(repoRoot),
	}
}

// CreateTask creates a new task as a jj change.
func (tm *TaskManager) CreateTask(ctx context.Context, task *Task) error {
	// Create a new change
	_, err := tm.jj.Exec(ctx, "new", "-m", task.FormatDescription())
	if err != nil {
		return fmt.Errorf("failed to create change: %w", err)
	}

	// Get the new change ID
	output, err := tm.jj.Exec(ctx, "log", "-r", "@", "-n", "1", "--no-graph", "-T", "change_id")
	if err != nil {
		return fmt.Errorf("failed to get change ID: %w", err)
	}
	task.ChangeID = strings.TrimSpace(string(output))

	// Create bookmark for the task
	bookmarkName := fmt.Sprintf("task-%s", task.ChangeID[:8])
	task.Bookmark = bookmarkName
	_, err = tm.jj.Exec(ctx, "bookmark", "create", bookmarkName)
	if err != nil {
		return fmt.Errorf("failed to create bookmark: %w", err)
	}

	// Save metadata
	meta := &TaskMetadata{
		ChangeID:  task.ChangeID,
		Priority:  task.Priority,
		Labels:    task.Labels,
		DueDate:   task.DueDate,
		Agent:     task.Agent,
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
	}

	return tm.metadata.Save(meta)
}

// AssignTask assigns a task to an agent.
func (tm *TaskManager) AssignTask(ctx context.Context, changeID, agentID string) error {
	// Create agent bookmark
	bookmarkName := fmt.Sprintf("agent-%s/%s", agentID, changeID[:8])
	_, err := tm.jj.Exec(ctx, "bookmark", "create", bookmarkName, "-r", changeID)
	if err != nil {
		return fmt.Errorf("failed to create agent bookmark: %w", err)
	}

	// Update metadata
	meta := &TaskMetadata{
		ChangeID:  changeID,
		Agent:     agentID,
		UpdatedAt: time.Now(),
	}

	return tm.metadata.Save(meta)
}

// GetReadyTasks returns tasks that have no blocking dependencies.
func (tm *TaskManager) GetReadyTasks(ctx context.Context) ([]*Task, error) {
	// Use revset to find leaf task bookmarks
	output, err := tm.jj.Exec(ctx, "log",
		"-r", `heads(bookmarks(glob:"task-*")) - conflicts()`,
		"--no-graph",
		"-T", `change_id ++ "\n"`,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to query ready tasks: %w", err)
	}

	var tasks []*Task
	changeIDs := strings.Split(strings.TrimSpace(string(output)), "\n")

	metadata, err := tm.metadata.Load()
	if err != nil {
		return nil, err
	}

	for _, id := range changeIDs {
		id = strings.TrimSpace(id)
		if id == "" {
			continue
		}

		// Get the change description
		desc, err := tm.jj.Exec(ctx, "log", "-r", id, "-n", "1", "--no-graph", "-T", "description")
		if err != nil {
			continue
		}

		task, err := ParseDescription(string(desc))
		if err != nil {
			continue
		}

		task.ChangeID = id

		// Merge metadata
		if meta, ok := metadata[id]; ok {
			task.Priority = meta.Priority
			task.Labels = meta.Labels
			task.DueDate = meta.DueDate
			task.Agent = meta.Agent
		}

		tasks = append(tasks, task)
	}

	return tasks, nil
}

// GetAgentTasks returns all tasks assigned to an agent.
func (tm *TaskManager) GetAgentTasks(ctx context.Context, agentID string) ([]*Task, error) {
	pattern := fmt.Sprintf(`bookmarks(glob:"agent-%s/*")`, agentID)
	output, err := tm.jj.Exec(ctx, "log",
		"-r", pattern,
		"--no-graph",
		"-T", `change_id ++ "\n"`,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to query agent tasks: %w", err)
	}

	var tasks []*Task
	changeIDs := strings.Split(strings.TrimSpace(string(output)), "\n")

	for _, id := range changeIDs {
		id = strings.TrimSpace(id)
		if id == "" {
			continue
		}

		desc, err := tm.jj.Exec(ctx, "log", "-r", id, "-n", "1", "--no-graph", "-T", "description")
		if err != nil {
			continue
		}

		task, err := ParseDescription(string(desc))
		if err != nil {
			continue
		}

		task.ChangeID = id
		tasks = append(tasks, task)
	}

	return tasks, nil
}

// UpdateHandoff updates the task's handoff context (for agent continuity).
func (tm *TaskManager) UpdateHandoff(ctx context.Context, changeID string, handoff *HandoffContext) error {
	// Get current description
	desc, err := tm.jj.Exec(ctx, "log", "-r", changeID, "-n", "1", "--no-graph", "-T", "description")
	if err != nil {
		return fmt.Errorf("failed to get description: %w", err)
	}

	// Parse and update
	task, err := ParseDescription(string(desc))
	if err != nil {
		return err
	}

	handoff.UpdatedAt = time.Now()
	task.Context = handoff

	// Update the change description
	_, err = tm.jj.Exec(ctx, "describe", "-r", changeID, "-m", task.FormatDescription())
	return err
}
