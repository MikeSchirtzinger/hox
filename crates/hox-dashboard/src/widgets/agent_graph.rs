//! Agent orchestration graph widget
//!
//! Renders a visual DAG showing phases and agents with progress bars.

use crate::{DashboardState, PhaseProgress, PhaseStatus};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub struct AgentGraphWidget;

impl AgentGraphWidget {
    /// Render the orchestration graph showing phases and agents
    pub fn render(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        // Create outer block
        let block = Block::default()
            .title(" ORCHESTRATION PROGRESS ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if state.phases.is_empty() {
            let empty_msg = Paragraph::new("No orchestration phases available")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            empty_msg.render(inner, buf);
            return;
        }

        // Calculate layout for phases
        let phase_count = state.phases.len();
        let phase_width = if phase_count > 0 {
            let connector_space = (phase_count.saturating_sub(1) * 4) as u16;
            inner.width.saturating_sub(connector_space) / phase_count as u16
        } else {
            inner.width
        };

        // Render each phase
        for (idx, phase) in state.phases.iter().enumerate() {
            let x_offset = idx as u16 * (phase_width + 4);
            if x_offset >= inner.width {
                break;
            }

            let phase_area = Rect {
                x: inner.x + x_offset,
                y: inner.y,
                width: phase_width.min(inner.width.saturating_sub(x_offset)),
                height: inner.height,
            };

            Self::render_phase(state, phase, phase_area, buf);

            // Render arrow connector (except for last phase)
            if idx < phase_count - 1 {
                let arrow_x = inner.x + x_offset + phase_width + 1;
                if arrow_x + 2 < inner.x + inner.width {
                    let arrow_y = inner.y + 1;
                    buf.set_string(
                        arrow_x,
                        arrow_y,
                        "──▶",
                        Style::default().fg(Color::DarkGray),
                    );
                }
            }
        }

        // Render overall progress bar at bottom
        if inner.height > 2 {
            let overall_progress = Self::calculate_overall_progress(&state.phases);
            let progress_y = inner.y + inner.height - 2;
            let progress_width = inner.width.saturating_sub(2);

            let label = format!("Overall: {:.0}%", overall_progress * 100.0);
            buf.set_string(
                inner.x,
                progress_y,
                &label,
                Style::default().fg(Color::White),
            );

            let bar = Self::create_progress_bar(overall_progress, progress_width as usize);
            buf.set_string(
                inner.x,
                progress_y + 1,
                &bar,
                Style::default().fg(Color::Green),
            );
        }
    }

    /// Render a single phase box with agents
    fn render_phase(
        state: &DashboardState,
        phase: &PhaseProgress,
        area: Rect,
        buf: &mut Buffer,
    ) {
        if area.width < 8 || area.height < 3 {
            return;
        }

        // Phase status color
        let color = match phase.status {
            PhaseStatus::Pending => Color::DarkGray,
            PhaseStatus::Active => Color::Yellow,
            PhaseStatus::Completed => Color::Green,
            PhaseStatus::Failed => Color::Red,
        };

        // Draw phase box border
        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, "─", Style::default().fg(color));
            if area.height > 2 {
                buf.set_string(x, area.y + area.height - 1, "─", Style::default().fg(color));
            }
        }
        for y in area.y..area.y + area.height {
            buf.set_string(area.x, y, "│", Style::default().fg(color));
            if area.width > 1 {
                buf.set_string(
                    area.x + area.width - 1,
                    y,
                    "│",
                    Style::default().fg(color),
                );
            }
        }

        // Corners
        buf.set_string(area.x, area.y, "┌", Style::default().fg(color));
        buf.set_string(
            area.x + area.width - 1,
            area.y,
            "┐",
            Style::default().fg(color),
        );
        buf.set_string(
            area.x,
            area.y + area.height - 1,
            "└",
            Style::default().fg(color),
        );
        buf.set_string(
            area.x + area.width - 1,
            area.y + area.height - 1,
            "┘",
            Style::default().fg(color),
        );

        // Phase name (truncated if needed)
        let name = if phase.name.len() > area.width.saturating_sub(2) as usize {
            format!("{}…", &phase.name[..area.width.saturating_sub(3) as usize])
        } else {
            phase.name.clone()
        };

        buf.set_string(
            area.x + 1,
            area.y + 1,
            &name,
            Style::default().fg(Color::White).bold(),
        );

        // Progress bar
        if area.height > 3 {
            let bar_width = area.width.saturating_sub(2) as usize;
            let bar = phase.progress_bar(bar_width);
            buf.set_string(area.x + 1, area.y + 2, &bar, Style::default().fg(color));
        }

        // Progress percentage
        if area.height > 4 {
            let pct = format!("{:.0}%", phase.progress * 100.0);
            let x_pos = area.x + (area.width / 2).saturating_sub(pct.len() as u16 / 2);
            buf.set_string(x_pos, area.y + 3, &pct, Style::default().fg(Color::White));
        }

        // Render agents in this phase
        if area.height > 5 {
            let agents_in_phase: Vec<_> = state
                .agents
                .iter()
                .filter(|a| a.phase == phase.number)
                .collect();

            for (idx, agent) in agents_in_phase.iter().enumerate() {
                let y = area.y + 5 + idx as u16;
                if y >= area.y + area.height - 1 {
                    break;
                }

                let agent_line = if area.width > 12 {
                    format!(
                        "{} {}",
                        agent.status.indicator(),
                        agent.name.chars().take(area.width.saturating_sub(4) as usize).collect::<String>()
                    )
                } else {
                    agent.status.indicator().to_string()
                };

                let agent_color = Self::status_to_color(agent.status.color_name());
                buf.set_string(
                    area.x + 1,
                    y,
                    &agent_line,
                    Style::default().fg(agent_color),
                );
            }
        }
    }

    /// Calculate overall progress across all phases
    fn calculate_overall_progress(phases: &[PhaseProgress]) -> f32 {
        if phases.is_empty() {
            return 0.0;
        }
        let total: f32 = phases.iter().map(|p| p.progress).sum();
        total / phases.len() as f32
    }

    /// Create progress bar string
    fn create_progress_bar(progress: f32, width: usize) -> String {
        let filled = (progress * width as f32) as usize;
        let empty = width.saturating_sub(filled);
        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }

    /// Convert color name string to ratatui Color
    fn status_to_color(color_name: &str) -> Color {
        match color_name {
            "gray" => Color::DarkGray,
            "yellow" => Color::Yellow,
            "green" => Color::Green,
            "red" => Color::Red,
            "magenta" => Color::Magenta,
            _ => Color::White,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overall_progress_calculation() {
        let phases = vec![
            PhaseProgress {
                progress: 1.0,
                ..PhaseProgress::new(0, "Phase 0")
            },
            PhaseProgress {
                progress: 0.5,
                ..PhaseProgress::new(1, "Phase 1")
            },
            PhaseProgress {
                progress: 0.0,
                ..PhaseProgress::new(2, "Phase 2")
            },
        ];

        let progress = AgentGraphWidget::calculate_overall_progress(&phases);
        assert!((progress - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_progress_bar_creation() {
        let bar = AgentGraphWidget::create_progress_bar(0.5, 10);
        assert_eq!(bar, "█████░░░░░");

        let bar = AgentGraphWidget::create_progress_bar(1.0, 10);
        assert_eq!(bar, "██████████");

        let bar = AgentGraphWidget::create_progress_bar(0.0, 10);
        assert_eq!(bar, "░░░░░░░░░░");
    }

    #[test]
    fn test_render_with_empty_state() {
        let state = DashboardState::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let area = Rect::new(0, 0, 80, 10);

        AgentGraphWidget::render(&state, area, &mut buf);
        // Should not panic with empty state
    }
}
