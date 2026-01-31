//! Active agents table widget
//!
//! Displays a table of active agents with their progress and status.

use super::status_color_from_name;
use crate::{AgentNode, DashboardState};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};

pub struct AgentTableWidget;

impl AgentTableWidget {
    /// Render the agents table
    pub fn render(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" ACTIVE AGENTS ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if state.agents.is_empty() {
            let empty_msg = "No active agents";
            buf.set_string(
                inner.x + 1,
                inner.y,
                empty_msg,
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        // Define column widths
        let widths = [
            Constraint::Length(12), // Agent
            Constraint::Min(20),    // Task
            Constraint::Length(10), // Progress
            Constraint::Length(8),  // Status
            Constraint::Length(10), // Duration
        ];

        // Create header row
        let header = Row::new(vec![
            Cell::from("Agent"),
            Cell::from("Task"),
            Cell::from("Progress"),
            Cell::from("Status"),
            Cell::from("Duration"),
        ])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

        // Create data rows
        let rows: Vec<Row> = state
            .agents
            .iter()
            .map(|agent| Self::create_agent_row(agent))
            .collect();

        // Render table
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1);

        Widget::render(table, inner, buf);
    }

    /// Create a table row for an agent
    fn create_agent_row(agent: &AgentNode) -> Row<'static> {
        let agent_name = Self::truncate_text(&agent.name, 11);
        let task = Self::truncate_text(&agent.task, 19);
        let progress_bar = agent.progress_bar(8);
        let status = agent.status.indicator();
        let duration = Self::format_duration(agent.duration_ms);

        let status_color = status_color_from_name(agent.status.color_name());

        Row::new(vec![
            Cell::from(agent_name).style(Style::default().fg(Color::White)),
            Cell::from(task).style(Style::default().fg(Color::Gray)),
            Cell::from(progress_bar).style(Style::default().fg(Self::progress_color(agent.progress))),
            Cell::from(status).style(Style::default().fg(status_color)),
            Cell::from(duration).style(Style::default().fg(Color::Cyan)),
        ])
    }

    /// Truncate text to fit width
    fn truncate_text(text: &str, max_len: usize) -> String {
        if text.len() <= max_len {
            text.to_string()
        } else {
            format!("{}…", &text[..max_len.saturating_sub(1)])
        }
    }

    /// Format duration in milliseconds to human-readable
    fn format_duration(ms: u64) -> String {
        let seconds = ms / 1000;
        let minutes = seconds / 60;
        let remaining_seconds = seconds % 60;

        if minutes > 0 {
            format!("{}m {}s", minutes, remaining_seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    /// Get color for progress bar based on progress value
    fn progress_color(progress: f32) -> Color {
        if progress >= 0.8 {
            Color::Green
        } else if progress >= 0.4 {
            Color::Yellow
        } else {
            Color::Red
        }
    }


    /// Render with detailed view (more columns)
    pub fn render_detailed(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" ACTIVE AGENTS (DETAILED) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if state.agents.is_empty() {
            let empty_msg = "No active agents";
            buf.set_string(
                inner.x + 1,
                inner.y,
                empty_msg,
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        // Define column widths for detailed view
        let widths = [
            Constraint::Length(12), // Agent
            Constraint::Min(15),    // Task
            Constraint::Length(10), // Progress
            Constraint::Length(8),  // Status
            Constraint::Length(10), // Duration
            Constraint::Length(8),  // Calls
            Constraint::Length(8),  // Success
        ];

        // Create header row
        let header = Row::new(vec![
            Cell::from("Agent"),
            Cell::from("Task"),
            Cell::from("Progress"),
            Cell::from("Status"),
            Cell::from("Duration"),
            Cell::from("Calls"),
            Cell::from("Success"),
        ])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

        // Create data rows
        let rows: Vec<Row> = state
            .agents
            .iter()
            .map(|agent| Self::create_detailed_row(agent))
            .collect();

        // Render table
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1);

        Widget::render(table, inner, buf);
    }

    /// Create a detailed table row for an agent
    fn create_detailed_row(agent: &AgentNode) -> Row<'static> {
        let agent_name = Self::truncate_text(&agent.name, 11);
        let task = Self::truncate_text(&agent.task, 14);
        let progress_bar = agent.progress_bar(8);
        let status = agent.status.indicator();
        let duration = Self::format_duration(agent.duration_ms);
        let calls = agent.tool_calls.to_string();
        let success = format!("{:.0}%", agent.success_rate * 100.0);

        let status_color = status_color_from_name(agent.status.color_name());
        let success_color = if agent.success_rate >= 0.9 {
            Color::Green
        } else if agent.success_rate >= 0.7 {
            Color::Yellow
        } else {
            Color::Red
        };

        Row::new(vec![
            Cell::from(agent_name).style(Style::default().fg(Color::White)),
            Cell::from(task).style(Style::default().fg(Color::Gray)),
            Cell::from(progress_bar).style(Style::default().fg(Self::progress_color(agent.progress))),
            Cell::from(status).style(Style::default().fg(status_color)),
            Cell::from(duration).style(Style::default().fg(Color::Cyan)),
            Cell::from(calls).style(Style::default().fg(Color::Blue)),
            Cell::from(success).style(Style::default().fg(success_color)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentStatus;

    #[test]
    fn test_format_duration() {
        assert_eq!(AgentTableWidget::format_duration(0), "0s");
        assert_eq!(AgentTableWidget::format_duration(5000), "5s");
        assert_eq!(AgentTableWidget::format_duration(65000), "1m 5s");
        assert_eq!(AgentTableWidget::format_duration(135000), "2m 15s");
    }

    #[test]
    fn test_truncate_text() {
        assert_eq!(
            AgentTableWidget::truncate_text("Short", 10),
            "Short"
        );
        assert_eq!(
            AgentTableWidget::truncate_text("This is a very long text", 10),
            "This is a…"
        );
    }

    #[test]
    fn test_progress_color() {
        assert_eq!(AgentTableWidget::progress_color(0.9), Color::Green);
        assert_eq!(AgentTableWidget::progress_color(0.5), Color::Yellow);
        assert_eq!(AgentTableWidget::progress_color(0.2), Color::Red);
    }

    #[test]
    fn test_create_agent_row() {
        let agent = AgentNode {
            id: "agent-1".to_string(),
            name: "Test Agent".to_string(),
            phase: 1,
            status: AgentStatus::Running,
            progress: 0.5,
            change_id: Some("abc123".to_string()),
            tool_calls: 10,
            success_rate: 0.9,
            duration_ms: 125000,
            task: "Build widgets".to_string(),
        };

        let _row = AgentTableWidget::create_agent_row(&agent);
        // Should not panic and create valid row
    }

    #[test]
    fn test_render_with_empty_state() {
        let state = DashboardState::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let area = Rect::new(0, 0, 80, 10);

        AgentTableWidget::render(&state, area, &mut buf);
        // Should not panic with empty state
    }

    #[test]
    fn test_render_detailed() {
        let mut state = DashboardState::default();
        state.agents.push(AgentNode {
            id: "agent-1".to_string(),
            name: "Test Agent".to_string(),
            phase: 1,
            status: AgentStatus::Running,
            progress: 0.75,
            change_id: None,
            tool_calls: 25,
            success_rate: 0.96,
            duration_ms: 45000,
            task: "Complex task".to_string(),
        });

        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 24));
        let area = Rect::new(0, 0, 100, 10);

        AgentTableWidget::render_detailed(&state, area, &mut buf);
        // Should not panic with detailed rendering
    }
}
