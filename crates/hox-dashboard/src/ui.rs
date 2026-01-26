//! Main UI layout and rendering
//!
//! Defines the overall dashboard layout and delegates to individual widgets.

use crate::{
    app::{App, TabSelection},
    widgets::{AgentGraphWidget, AgentTableWidget, EventLogWidget, MetricsPanelWidget},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

/// Draw the entire dashboard UI
pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Main layout: header + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header (title + keybindings)
            Constraint::Min(0),    // Content area
        ])
        .split(size);

    // Render header
    render_header(frame, chunks[0], app);

    // Render content based on selected tab
    match app.selected_tab {
        TabSelection::Overview => render_overview(frame, chunks[1], app),
        TabSelection::Agents => render_agents(frame, chunks[1], app),
        TabSelection::Oplog => render_oplog(frame, chunks[1], app),
    }
}

/// Render the header with title and keybindings
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    // Title
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("HOX DASHBOARD", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(
                format!("Session: {}", app.state.session.id),
                Style::default().fg(Color::Gray),
            ),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, header_chunks[0]);

    // Keybindings
    let keybindings = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[q]", Style::default().fg(Color::Yellow)),
            Span::raw("uit "),
            Span::styled("[r]", Style::default().fg(Color::Yellow)),
            Span::raw("efresh "),
            Span::styled("[Tab]", Style::default().fg(Color::Yellow)),
            Span::raw(" switch"),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Right);
    frame.render_widget(keybindings, header_chunks[1]);
}

/// Render the overview tab
fn render_overview(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Agent graph
            Constraint::Length(5),  // Metrics panel
            Constraint::Min(10),    // Agents table
            Constraint::Length(10), // Event log (bottom)
        ])
        .split(area);

    // Agent orchestration graph
    frame.render_widget(
        WidgetAdapter::new(|area, buf| AgentGraphWidget::render(&app.state, area, buf)),
        chunks[0],
    );

    // Global metrics
    frame.render_widget(
        WidgetAdapter::new(|area, buf| MetricsPanelWidget::render(&app.state, area, buf)),
        chunks[1],
    );

    // Agents table
    frame.render_widget(
        WidgetAdapter::new(|area, buf| AgentTableWidget::render(&app.state, area, buf)),
        chunks[2],
    );

    // Event log (recent oplog)
    frame.render_widget(
        WidgetAdapter::new(|area, buf| EventLogWidget::render(&app.state, area, buf)),
        chunks[3],
    );
}

/// Render the detailed agents tab
fn render_agents(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab header
            Constraint::Min(0),     // Full agents table
        ])
        .split(area);

    // Tab header
    let tab_titles = vec!["Overview", "Agents", "Oplog"];
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title("View"))
        .select(1) // Agents is index 1
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[0]);

    // Full agents table with more detail
    frame.render_widget(
        WidgetAdapter::new(|area, buf| AgentTableWidget::render_detailed(&app.state, area, buf)),
        chunks[1],
    );
}

/// Render the oplog tab
fn render_oplog(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab header
            Constraint::Min(0),     // Full event log
        ])
        .split(area);

    // Tab header
    let tab_titles = vec!["Overview", "Agents", "Oplog"];
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title("View"))
        .select(2) // Oplog is index 2
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[0]);

    // Full event log
    frame.render_widget(
        WidgetAdapter::new(|area, buf| EventLogWidget::render(&app.state, area, buf)),
        chunks[1],
    );
}

/// Widget adapter to bridge static render methods to ratatui's Widget trait
struct WidgetAdapter<F>
where
    F: Fn(Rect, &mut Buffer),
{
    render_fn: F,
}

impl<F> WidgetAdapter<F>
where
    F: Fn(Rect, &mut Buffer),
{
    fn new(render_fn: F) -> Self {
        Self { render_fn }
    }
}

impl<F> Widget for WidgetAdapter<F>
where
    F: Fn(Rect, &mut Buffer),
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        (self.render_fn)(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DashboardState;

    #[test]
    fn test_ui_layout_creation() {
        // Test that layout chunks are created correctly
        let rect = Rect::new(0, 0, 80, 24);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(rect);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].height, 3);
        assert!(chunks[1].height > 0);
    }
}
