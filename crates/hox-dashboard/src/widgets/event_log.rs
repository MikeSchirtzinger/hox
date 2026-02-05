//! JJ operation log widget
//!
//! Displays recent JJ operations in a scrollable log format.

use crate::{DashboardState, JjOpType, JjOplogEntry};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

pub struct EventLogWidget;

impl EventLogWidget {
    /// Render the JJ operation log
    pub fn render(state: &DashboardState, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" JJ OPLOG ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if state.oplog.is_empty() {
            let empty_msg = "No JJ operations recorded";
            buf.set_string(
                inner.x + 1,
                inner.y,
                empty_msg,
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        // Create list items from oplog entries
        let items: Vec<ListItem> = state
            .oplog
            .iter()
            .take(inner.height as usize)
            .map(|entry| Self::format_entry(entry))
            .collect();

        // Render as a list
        let list = List::new(items);
        Widget::render(list, inner, buf);
    }

    /// Format a single oplog entry
    fn format_entry(entry: &JjOplogEntry) -> ListItem<'static> {
        let time = entry.formatted_time();
        let icon = entry.op_type.icon();
        let desc = Self::truncate_description(&entry.description, 60);

        let color = Self::op_type_color(entry.op_type);

        // Format: "HH:MM:SS  icon description"
        let line = format!("{}  {} {}", time, icon, desc);

        ListItem::new(line).style(Style::default().fg(color))
    }

    /// Truncate description to fit width
    fn truncate_description(desc: &str, max_len: usize) -> String {
        if desc.chars().count() <= max_len {
            desc.to_string()
        } else {
            let truncated: String = desc.chars().take(max_len.saturating_sub(1)).collect();
            format!("{}…", truncated)
        }
    }

    /// Get color for operation type
    fn op_type_color(op_type: JjOpType) -> Color {
        match op_type {
            JjOpType::New => Color::Green,
            JjOpType::Describe => Color::Cyan,
            JjOpType::Squash => Color::Magenta,
            JjOpType::Bookmark => Color::Yellow,
            JjOpType::Commit => Color::Blue,
            JjOpType::Rebase => Color::LightRed,
            JjOpType::Workspace => Color::LightMagenta,
            JjOpType::Other => Color::DarkGray,
        }
    }

    /// Render with scrolling support
    pub fn render_scrollable(
        state: &DashboardState,
        area: Rect,
        buf: &mut Buffer,
        scroll_offset: usize,
    ) {
        let block = Block::default()
            .title(" JJ OPLOG ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        if state.oplog.is_empty() {
            let empty_msg = "No JJ operations recorded";
            buf.set_string(
                inner.x + 1,
                inner.y,
                empty_msg,
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        // Calculate visible range with scroll offset
        let total_entries = state.oplog.len();
        let visible_count = inner.height as usize;
        let start = scroll_offset.min(total_entries.saturating_sub(visible_count));
        let end = (start + visible_count).min(total_entries);

        // Render visible entries
        for (idx, entry) in state.oplog[start..end].iter().enumerate() {
            if idx >= inner.height as usize {
                break;
            }

            let time = entry.formatted_time();
            let icon = entry.op_type.icon();
            let max_desc_len = inner.width.saturating_sub(15) as usize; // Account for time + icon + spacing
            let desc = Self::truncate_description(&entry.description, max_desc_len);

            let color = Self::op_type_color(entry.op_type);

            let line = format!("{}  {} {}", time, icon, desc);
            buf.set_string(
                inner.x,
                inner.y + idx as u16,
                &line,
                Style::default().fg(color),
            );

            // If agent ID is available, show it in gray at the end
            if let Some(ref agent_id) = entry.agent_id {
                let agent_text = format!(" [{}]", agent_id);
                let agent_x = inner
                    .x
                    .saturating_add(line.len() as u16)
                    .min(inner.x + inner.width.saturating_sub(agent_text.len() as u16 + 1));

                if agent_x + agent_text.len() as u16 <= inner.x + inner.width {
                    buf.set_string(
                        agent_x,
                        inner.y + idx as u16,
                        &agent_text,
                        Style::default().fg(Color::DarkGray),
                    );
                }
            }
        }

        // Show scroll indicator if there are more entries
        if total_entries > visible_count {
            let scroll_info = format!("{}-{}/{}", start + 1, end, total_entries);
            buf.set_string(
                inner.x + inner.width.saturating_sub(scroll_info.len() as u16 + 1),
                inner.y + inner.height - 1,
                &scroll_info,
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_format_entry() {
        let entry = JjOplogEntry {
            id: "op123".to_string(),
            timestamp: Utc::now(),
            description: "new empty commit".to_string(),
            agent_id: Some("agent-1".to_string()),
            op_type: JjOpType::New,
            tags: HashMap::new(),
        };

        let _item = EventLogWidget::format_entry(&entry);
        // Should not panic and create valid list item
    }

    #[test]
    fn test_truncate_description() {
        let long_desc = "This is a very long description that should be truncated";
        let truncated = EventLogWidget::truncate_description(long_desc, 20);
        assert!(truncated.chars().count() <= 20);
        assert!(truncated.ends_with('…'));

        let short_desc = "Short";
        let not_truncated = EventLogWidget::truncate_description(short_desc, 20);
        assert_eq!(not_truncated, "Short");
    }

    #[test]
    fn test_op_type_colors() {
        assert_eq!(EventLogWidget::op_type_color(JjOpType::New), Color::Green);
        assert_eq!(
            EventLogWidget::op_type_color(JjOpType::Describe),
            Color::Cyan
        );
        assert_eq!(
            EventLogWidget::op_type_color(JjOpType::Bookmark),
            Color::Yellow
        );
    }

    #[test]
    fn test_render_with_empty_log() {
        let state = DashboardState::default();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let area = Rect::new(0, 0, 80, 10);

        EventLogWidget::render(&state, area, &mut buf);
        // Should not panic with empty log
    }

    #[test]
    fn test_scrollable_render() {
        let mut state = DashboardState::default();

        // Add multiple entries
        for i in 0..20 {
            state.oplog.push(JjOplogEntry {
                id: format!("op{}", i),
                timestamp: Utc::now(),
                description: format!("Operation {}", i),
                agent_id: None,
                op_type: JjOpType::New,
                tags: HashMap::new(),
            });
        }

        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let area = Rect::new(0, 0, 80, 10);

        // Test with different scroll offsets
        EventLogWidget::render_scrollable(&state, area, &mut buf, 0);
        EventLogWidget::render_scrollable(&state, area, &mut buf, 5);
        EventLogWidget::render_scrollable(&state, area, &mut buf, 15);
        // Should not panic with scrolling
    }
}
