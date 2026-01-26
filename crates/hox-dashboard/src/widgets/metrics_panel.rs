//! Global metrics panel widget
//!
//! Displays high-level orchestration metrics in a compact header format.

use crate::{DashboardState, GlobalMetrics};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders},
};

pub struct MetricsPanelWidget;

impl MetricsPanelWidget {
    /// Render the global metrics panel
    pub fn render(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" GLOBAL METRICS ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        let metrics = &state.global_metrics;
        let lines = Self::format_metrics(metrics);

        // Render metrics in two rows if space allows
        if inner.height >= 2 {
            // First row: Tool calls, failures, success rate
            buf.set_string(
                inner.x + 1,
                inner.y,
                &lines[0],
                Style::default().fg(Color::White),
            );

            // Second row: Total time, active agents
            if inner.height >= 2 {
                buf.set_string(
                    inner.x + 1,
                    inner.y + 1,
                    &lines[1],
                    Style::default().fg(Color::White),
                );
            }
        }

        // Add color highlights for key metrics
        Self::highlight_metrics(state, inner, buf);
    }

    /// Format metrics into display lines
    fn format_metrics(metrics: &GlobalMetrics) -> Vec<String> {
        let success_rate = metrics.success_rate();
        let duration = metrics.formatted_duration();

        vec![
            format!(
                "Tool Calls: {:>6}    Failures: {:>4}    Success: {:>5.1}%",
                metrics.total_tool_calls, metrics.total_failures, success_rate
            ),
            format!(
                "Total Time: {:>9}    Active Agents: {:>2}    Completed: {:>2}",
                duration, metrics.active_agents, metrics.completed_agents
            ),
        ]
    }

    /// Highlight specific metrics with colors
    fn highlight_metrics(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        let metrics = &state.global_metrics;

        // Highlight success rate with color based on value
        let success_rate = metrics.success_rate();
        let success_color = if success_rate >= 95.0 {
            Color::Green
        } else if success_rate >= 80.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        // Find "Success:" text position and color the percentage
        let success_text = format!("{:.1}%", success_rate);
        let line = Self::format_metrics(metrics)[0].clone();
        if let Some(pos) = line.find("Success:") {
            let value_pos = pos + 9; // "Success: " length
            if value_pos < line.len() {
                buf.set_string(
                    area.x + 1 + value_pos as u16,
                    area.y,
                    &success_text,
                    Style::default().fg(success_color).bold(),
                );
            }
        }

        // Highlight failures in red if any exist
        if metrics.total_failures > 0 {
            let failures_text = format!("{:>4}", metrics.total_failures);
            if let Some(pos) = line.find("Failures:") {
                let value_pos = pos + 10; // "Failures: " length
                buf.set_string(
                    area.x + 1 + value_pos as u16,
                    area.y,
                    &failures_text,
                    Style::default().fg(Color::Red).bold(),
                );
            }
        }

        // Highlight active agents in yellow
        if metrics.active_agents > 0 {
            let line2 = Self::format_metrics(metrics)[1].clone();
            let active_text = format!("{:>2}", metrics.active_agents);
            if let Some(pos) = line2.find("Active Agents:") {
                let value_pos = pos + 15; // "Active Agents: " length
                if area.height >= 2 {
                    buf.set_string(
                        area.x + 1 + value_pos as u16,
                        area.y + 1,
                        &active_text,
                        Style::default().fg(Color::Yellow).bold(),
                    );
                }
            }
        }
    }

    /// Render a compact single-line version for smaller areas
    pub fn render_compact(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        let metrics = &state.global_metrics;
        let compact = format!(
            "Calls: {} | Failures: {} | Success: {:.1}% | Time: {} | Active: {}",
            metrics.total_tool_calls,
            metrics.total_failures,
            metrics.success_rate(),
            metrics.formatted_duration(),
            metrics.active_agents
        );

        buf.set_string(
            area.x,
            area.y,
            &compact,
            Style::default().fg(Color::White),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_metrics() {
        let metrics = GlobalMetrics {
            total_tool_calls: 1234,
            total_failures: 12,
            total_time_ms: 2723000, // 45m 23s
            active_agents: 3,
            completed_agents: 5,
        };

        let lines = MetricsPanelWidget::format_metrics(&metrics);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Tool Calls:"));
        assert!(lines[0].contains("1234"));
        assert!(lines[0].contains("12"));
        assert!(lines[1].contains("45m 23s"));
        assert!(lines[1].contains("3"));
    }

    #[test]
    fn test_render_with_empty_metrics() {
        let state = DashboardState::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let area = Rect::new(0, 0, 80, 4);

        MetricsPanelWidget::render(&state, area, &mut buf);
        // Should not panic with default metrics
    }

    #[test]
    fn test_compact_render() {
        let state = DashboardState {
            global_metrics: GlobalMetrics {
                total_tool_calls: 100,
                total_failures: 5,
                total_time_ms: 60000,
                active_agents: 2,
                completed_agents: 1,
            },
            ..Default::default()
        };

        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let area = Rect::new(0, 0, 100, 1);

        MetricsPanelWidget::render_compact(&state, area, &mut buf);
        // Should not panic with compact rendering
    }
}
