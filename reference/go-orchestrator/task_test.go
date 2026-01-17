package orchestrator

import (
	"context"
	"strings"
	"testing"
	"time"
)

func TestFormatDescription(t *testing.T) {
	task := &Task{
		Title:    "Implement VCS abstraction",
		Priority: 1,
		Status:   "in_progress",
		Agent:    "agent-42",
		Context: &HandoffContext{
			CurrentFocus: "Working on jj backend implementation",
			Progress: []string{
				"Designed interface",
				"Implemented git backend",
			},
			NextSteps: []string{
				"Implement jj backend",
				"Add workspace support",
			},
			Blockers:      []string{},
			OpenQuestions: []string{"Should colocated default to jj?"},
			FilesTouched:  []string{"internal/vcs/jj/jj.go", "internal/vcs/vcs.go"},
		},
	}

	desc := task.FormatDescription()

	// Verify key sections exist
	if !strings.Contains(desc, "Task: Implement VCS abstraction") {
		t.Error("Missing task title")
	}
	if !strings.Contains(desc, "Priority: 1") {
		t.Error("Missing priority")
	}
	if !strings.Contains(desc, "Status: in_progress") {
		t.Error("Missing status")
	}
	if !strings.Contains(desc, "Agent: agent-42") {
		t.Error("Missing agent")
	}
	if !strings.Contains(desc, "Working on jj backend") {
		t.Error("Missing context")
	}
	if !strings.Contains(desc, "[x] Designed interface") {
		t.Error("Missing progress item")
	}
	if !strings.Contains(desc, "[ ] Implement jj backend") {
		t.Error("Missing next step")
	}
	if !strings.Contains(desc, "Should colocated default") {
		t.Error("Missing open question")
	}
	if !strings.Contains(desc, "internal/vcs/jj/jj.go") {
		t.Error("Missing file touched")
	}
}

func TestParseDescription(t *testing.T) {
	desc := `Task: Implement VCS abstraction
Priority: 1
Status: in_progress
Agent: agent-42

## Context
Working on jj backend implementation

## Progress
- [x] Designed interface
- [x] Implemented git backend

## Next Steps
- [ ] Implement jj backend
- [ ] Add workspace support

## Blockers
None

## Open Questions
- Should colocated default to jj?

## Files Touched
internal/vcs/jj/jj.go
internal/vcs/vcs.go
`

	task, err := ParseDescription(desc)
	if err != nil {
		t.Fatalf("ParseDescription failed: %v", err)
	}

	if task.Title != "Implement VCS abstraction" {
		t.Errorf("Expected title 'Implement VCS abstraction', got '%s'", task.Title)
	}
	if task.Priority != 1 {
		t.Errorf("Expected priority 1, got %d", task.Priority)
	}
	if task.Status != "in_progress" {
		t.Errorf("Expected status 'in_progress', got '%s'", task.Status)
	}
	if task.Agent != "agent-42" {
		t.Errorf("Expected agent 'agent-42', got '%s'", task.Agent)
	}

	if task.Context == nil {
		t.Fatal("Expected context to be parsed")
	}

	if !strings.Contains(task.Context.CurrentFocus, "Working on jj backend") {
		t.Errorf("Expected context to contain focus, got '%s'", task.Context.CurrentFocus)
	}

	if len(task.Context.Progress) != 2 {
		t.Errorf("Expected 2 progress items, got %d", len(task.Context.Progress))
	}

	if len(task.Context.NextSteps) != 2 {
		t.Errorf("Expected 2 next steps, got %d", len(task.Context.NextSteps))
	}

	if len(task.Context.OpenQuestions) != 1 {
		t.Errorf("Expected 1 open question, got %d", len(task.Context.OpenQuestions))
	}

	if len(task.Context.FilesTouched) != 2 {
		t.Errorf("Expected 2 files touched, got %d", len(task.Context.FilesTouched))
	}
}

func TestParseDescriptionRoundTrip(t *testing.T) {
	original := &Task{
		Title:    "Test Task",
		Priority: 2,
		Status:   "pending",
		Agent:    "agent-99",
		Context: &HandoffContext{
			CurrentFocus: "Testing round trip",
			Progress:     []string{"Step 1 done", "Step 2 done"},
			NextSteps:    []string{"Step 3", "Step 4"},
			Blockers:     []string{"Waiting for API"},
			OpenQuestions: []string{
				"Question 1?",
				"Question 2?",
			},
			FilesTouched: []string{"file1.go", "file2.go"},
		},
	}

	// Format then parse
	desc := original.FormatDescription()
	parsed, err := ParseDescription(desc)
	if err != nil {
		t.Fatalf("Round trip failed: %v", err)
	}

	// Verify fields match
	if parsed.Title != original.Title {
		t.Errorf("Title mismatch: got '%s', want '%s'", parsed.Title, original.Title)
	}
	if parsed.Priority != original.Priority {
		t.Errorf("Priority mismatch: got %d, want %d", parsed.Priority, original.Priority)
	}
	if parsed.Status != original.Status {
		t.Errorf("Status mismatch: got '%s', want '%s'", parsed.Status, original.Status)
	}
	if parsed.Agent != original.Agent {
		t.Errorf("Agent mismatch: got '%s', want '%s'", parsed.Agent, original.Agent)
	}

	if len(parsed.Context.Progress) != len(original.Context.Progress) {
		t.Errorf("Progress count mismatch: got %d, want %d",
			len(parsed.Context.Progress), len(original.Context.Progress))
	}
	if len(parsed.Context.NextSteps) != len(original.Context.NextSteps) {
		t.Errorf("NextSteps count mismatch: got %d, want %d",
			len(parsed.Context.NextSteps), len(original.Context.NextSteps))
	}
}

func TestHandoffContextFormat(t *testing.T) {
	ctx := &HandoffContext{
		CurrentFocus: "Implementing feature X",
		Progress:     []string{"Research done", "API designed"},
		NextSteps:    []string{"Write tests", "Implement"},
		Blockers:     []string{"Need DB access"},
		OpenQuestions: []string{
			"Which auth method?",
		},
		FilesTouched: []string{"api.go", "handler.go"},
		UpdatedAt:    time.Now(),
	}

	task := &Task{
		Title:    "Feature X",
		Priority: 1,
		Status:   "in_progress",
		Context:  ctx,
	}

	desc := task.FormatDescription()

	// Should have all sections
	sections := []string{
		"## Context",
		"## Progress",
		"## Next Steps",
		"## Blockers",
		"## Open Questions",
		"## Files Touched",
	}

	for _, section := range sections {
		if !strings.Contains(desc, section) {
			t.Errorf("Missing section: %s", section)
		}
	}
}

// mockJJ implements JJExecutor for testing
type mockJJ struct {
	responses map[string]string
	calls     [][]string
}

func newMockJJ() *mockJJ {
	return &mockJJ{
		responses: make(map[string]string),
		calls:     make([][]string, 0),
	}
}

func (m *mockJJ) Exec(ctx context.Context, args ...string) ([]byte, error) {
	m.calls = append(m.calls, args)

	// Build a key from the args
	key := strings.Join(args, " ")
	if resp, ok := m.responses[key]; ok {
		return []byte(resp), nil
	}

	// Default responses
	if len(args) > 0 {
		switch args[0] {
		case "log":
			return []byte("abc12345\n"), nil
		case "bookmark":
			return []byte(""), nil
		case "new":
			return []byte("Created new change\n"), nil
		case "describe":
			return []byte(""), nil
		}
	}

	return []byte(""), nil
}

func (m *mockJJ) setResponse(args string, response string) {
	m.responses[args] = response
}

func TestTaskManagerCreateTask(t *testing.T) {
	mock := newMockJJ()
	tm := NewTaskManager("/tmp/test-repo", mock)

	task := &Task{
		Title:    "New Task",
		Priority: 1,
		Status:   "pending",
	}

	ctx := context.Background()
	err := tm.CreateTask(ctx, task)
	if err != nil {
		t.Fatalf("CreateTask failed: %v", err)
	}

	// Verify jj commands were called
	if len(mock.calls) < 3 {
		t.Errorf("Expected at least 3 jj calls, got %d", len(mock.calls))
	}

	// Should have called: new, log, bookmark create
	foundNew := false
	foundLog := false
	foundBookmark := false

	for _, call := range mock.calls {
		if len(call) > 0 {
			switch call[0] {
			case "new":
				foundNew = true
			case "log":
				foundLog = true
			case "bookmark":
				foundBookmark = true
			}
		}
	}

	if !foundNew {
		t.Error("Expected 'jj new' to be called")
	}
	if !foundLog {
		t.Error("Expected 'jj log' to be called")
	}
	if !foundBookmark {
		t.Error("Expected 'jj bookmark' to be called")
	}
}

func TestAgentHandoffFormat(t *testing.T) {
	handoff := &AgentHandoff{
		Task: &Task{
			Title:    "Important Task",
			Priority: 1,
			Status:   "in_progress",
			Agent:    "agent-42",
		},
		Context: &HandoffContext{
			CurrentFocus: "Working on the thing",
			Progress:     []string{"Did step 1", "Did step 2"},
			NextSteps:    []string{"Do step 3"},
			Blockers:     []string{"Waiting for API"},
			OpenQuestions: []string{
				"Which approach?",
			},
			FilesTouched: []string{"main.go"},
		},
		History: []ChangeEntry{
			{ChangeID: "abc12345", Description: "Initial work"},
			{ChangeID: "def67890", Description: "More progress"},
		},
		Diff: "+new line\n-old line\n",
	}

	formatted := handoff.FormatForAgent()

	// Check structure
	if !strings.Contains(formatted, "# Agent Handoff Context") {
		t.Error("Missing main header")
	}
	if !strings.Contains(formatted, "## Task") {
		t.Error("Missing Task section")
	}
	if !strings.Contains(formatted, "## Where We Left Off") {
		t.Error("Missing context section")
	}
	if !strings.Contains(formatted, "## Change History") {
		t.Error("Missing history section")
	}
	if !strings.Contains(formatted, "## Code Changes") {
		t.Error("Missing diff section")
	}

	// Check content
	if !strings.Contains(formatted, "Important Task") {
		t.Error("Missing task title")
	}
	if !strings.Contains(formatted, "agent-42") {
		t.Error("Missing previous agent")
	}
	if !strings.Contains(formatted, "Working on the thing") {
		t.Error("Missing context focus")
	}
	if !strings.Contains(formatted, "abc12345") {
		t.Error("Missing history entry")
	}
}
