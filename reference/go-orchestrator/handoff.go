package orchestrator

import (
	"context"
	"fmt"
	"strings"
	"time"
)

// HandoffGenerator creates structured handoff context for agent transitions.
type HandoffGenerator struct {
	tm *TaskManager
}

// NewHandoffGenerator creates a handoff generator.
func NewHandoffGenerator(tm *TaskManager) *HandoffGenerator {
	return &HandoffGenerator{tm: tm}
}

// GenerateHandoff creates handoff context from a task's current state.
// This would typically be called by a summarization model (e.g., Haiku).
func (h *HandoffGenerator) GenerateHandoff(ctx context.Context, changeID string, summary HandoffSummary) error {
	handoff := &HandoffContext{
		CurrentFocus:  summary.CurrentFocus,
		Progress:      summary.Progress,
		NextSteps:     summary.NextSteps,
		Blockers:      summary.Blockers,
		OpenQuestions: summary.OpenQuestions,
		FilesTouched:  summary.FilesTouched,
		UpdatedAt:     time.Now(),
	}

	return h.tm.UpdateHandoff(ctx, changeID, handoff)
}

// HandoffSummary is the input for generating handoff context.
// This would be produced by a summarization model.
type HandoffSummary struct {
	CurrentFocus  string
	Progress      []string
	NextSteps     []string
	Blockers      []string
	OpenQuestions []string
	FilesTouched  []string
}

// LoadHandoff retrieves the handoff context for a task.
func (h *HandoffGenerator) LoadHandoff(ctx context.Context, changeID string) (*HandoffContext, error) {
	desc, err := h.tm.jj.Exec(ctx, "log", "-r", changeID, "-n", "1", "--no-graph", "-T", "description")
	if err != nil {
		return nil, fmt.Errorf("failed to get description: %w", err)
	}

	task, err := ParseDescription(string(desc))
	if err != nil {
		return nil, err
	}

	return task.Context, nil
}

// GetDiff returns the cumulative diff for a change (for new agent context).
func (h *HandoffGenerator) GetDiff(ctx context.Context, changeID string) (string, error) {
	output, err := h.tm.jj.Exec(ctx, "diff", "-r", fmt.Sprintf("root()..%s", changeID))
	if err != nil {
		return "", fmt.Errorf("failed to get diff: %w", err)
	}
	return string(output), nil
}

// GetChangeLog returns the change history for context.
func (h *HandoffGenerator) GetChangeLog(ctx context.Context, changeID string) ([]ChangeEntry, error) {
	output, err := h.tm.jj.Exec(ctx, "log",
		"-r", fmt.Sprintf("ancestors(%s)", changeID),
		"--no-graph",
		"-T", `change_id ++ "|" ++ description.first_line() ++ "\n"`,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to get change log: %w", err)
	}

	var entries []ChangeEntry
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.SplitN(line, "|", 2)
		if len(parts) < 2 {
			continue
		}

		entries = append(entries, ChangeEntry{
			ChangeID:    strings.TrimSpace(parts[0]),
			Description: strings.TrimSpace(parts[1]),
		})
	}

	return entries, nil
}

// ChangeEntry represents a single change in the log.
type ChangeEntry struct {
	ChangeID    string
	Description string
}

// AgentHandoff contains everything a new agent needs to continue work.
type AgentHandoff struct {
	// Task is the task being handed off
	Task *Task

	// Context is the structured handoff context
	Context *HandoffContext

	// Diff is the cumulative code changes
	Diff string

	// History is the change log
	History []ChangeEntry

	// Metadata is the task metadata
	Metadata *TaskMetadata
}

// PrepareHandoff gathers all context for a new agent taking over a task.
func (h *HandoffGenerator) PrepareHandoff(ctx context.Context, changeID string) (*AgentHandoff, error) {
	// Get task description
	desc, err := h.tm.jj.Exec(ctx, "log", "-r", changeID, "-n", "1", "--no-graph", "-T", "description")
	if err != nil {
		return nil, fmt.Errorf("failed to get description: %w", err)
	}

	task, err := ParseDescription(string(desc))
	if err != nil {
		return nil, err
	}
	task.ChangeID = changeID

	// Get diff
	diff, err := h.GetDiff(ctx, changeID)
	if err != nil {
		diff = "(failed to get diff)"
	}

	// Get history
	history, err := h.GetChangeLog(ctx, changeID)
	if err != nil {
		history = nil
	}

	// Get metadata
	allMeta, err := h.tm.metadata.Load()
	if err != nil {
		allMeta = make(map[string]*TaskMetadata)
	}
	metadata := allMeta[changeID]

	return &AgentHandoff{
		Task:     task,
		Context:  task.Context,
		Diff:     diff,
		History:  history,
		Metadata: metadata,
	}, nil
}

// FormatForAgent formats the handoff as a prompt for the new agent.
func (ah *AgentHandoff) FormatForAgent() string {
	var sb strings.Builder

	sb.WriteString("# Agent Handoff Context\n\n")

	// Task info
	sb.WriteString("## Task\n")
	sb.WriteString(fmt.Sprintf("**Title:** %s\n", ah.Task.Title))
	sb.WriteString(fmt.Sprintf("**Priority:** %d\n", ah.Task.Priority))
	sb.WriteString(fmt.Sprintf("**Status:** %s\n", ah.Task.Status))
	if ah.Task.Agent != "" {
		sb.WriteString(fmt.Sprintf("**Previous Agent:** %s\n", ah.Task.Agent))
	}
	sb.WriteString("\n")

	// Context
	if ah.Context != nil {
		sb.WriteString("## Where We Left Off\n")
		if ah.Context.CurrentFocus != "" {
			sb.WriteString(fmt.Sprintf("%s\n\n", ah.Context.CurrentFocus))
		}

		if len(ah.Context.Progress) > 0 {
			sb.WriteString("### Completed\n")
			for _, p := range ah.Context.Progress {
				sb.WriteString(fmt.Sprintf("- [x] %s\n", p))
			}
			sb.WriteString("\n")
		}

		if len(ah.Context.NextSteps) > 0 {
			sb.WriteString("### Next Steps\n")
			for _, s := range ah.Context.NextSteps {
				sb.WriteString(fmt.Sprintf("- [ ] %s\n", s))
			}
			sb.WriteString("\n")
		}

		if len(ah.Context.Blockers) > 0 {
			sb.WriteString("### Blockers\n")
			for _, b := range ah.Context.Blockers {
				sb.WriteString(fmt.Sprintf("- %s\n", b))
			}
			sb.WriteString("\n")
		}

		if len(ah.Context.OpenQuestions) > 0 {
			sb.WriteString("### Open Questions\n")
			for _, q := range ah.Context.OpenQuestions {
				sb.WriteString(fmt.Sprintf("- %s\n", q))
			}
			sb.WriteString("\n")
		}

		if len(ah.Context.FilesTouched) > 0 {
			sb.WriteString("### Files Modified\n")
			for _, f := range ah.Context.FilesTouched {
				sb.WriteString(fmt.Sprintf("- %s\n", f))
			}
			sb.WriteString("\n")
		}
	}

	// History summary
	if len(ah.History) > 0 {
		sb.WriteString("## Change History\n")
		maxHistory := 10
		if len(ah.History) < maxHistory {
			maxHistory = len(ah.History)
		}
		for i := 0; i < maxHistory; i++ {
			entry := ah.History[i]
			sb.WriteString(fmt.Sprintf("- `%s`: %s\n", entry.ChangeID[:8], entry.Description))
		}
		if len(ah.History) > 10 {
			sb.WriteString(fmt.Sprintf("- ... and %d more changes\n", len(ah.History)-10))
		}
		sb.WriteString("\n")
	}

	// Diff summary (truncated for prompt)
	if ah.Diff != "" {
		sb.WriteString("## Code Changes\n")
		lines := strings.Split(ah.Diff, "\n")
		if len(lines) > 100 {
			sb.WriteString("```diff\n")
			sb.WriteString(strings.Join(lines[:100], "\n"))
			sb.WriteString(fmt.Sprintf("\n... (%d more lines)\n", len(lines)-100))
			sb.WriteString("```\n")
		} else {
			sb.WriteString("```diff\n")
			sb.WriteString(ah.Diff)
			sb.WriteString("\n```\n")
		}
	}

	return sb.String()
}
