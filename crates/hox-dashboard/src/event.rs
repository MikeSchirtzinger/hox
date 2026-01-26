//! Event handling for crossterm terminal events
//!
//! Polls for keyboard, resize, and tick events.

use crate::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Application events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    /// Key press event
    Key(KeyEvent),
    /// Timer tick for refresh checks
    Tick,
    /// Terminal resize event
    Resize(u16, u16),
}

/// Poll for the next event with timeout
pub fn poll_event(timeout: Duration) -> Result<Option<AppEvent>> {
    if event::poll(timeout)? {
        match event::read()? {
            Event::Key(key) => Ok(Some(AppEvent::Key(key))),
            Event::Resize(width, height) => Ok(Some(AppEvent::Resize(width, height))),
            _ => Ok(Some(AppEvent::Tick)),
        }
    } else {
        Ok(Some(AppEvent::Tick))
    }
}

/// Check if a key event is a quit command (q or Ctrl+C)
pub fn is_quit_event(key: KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char('q') | KeyCode::Char('Q')
    ) || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

/// Check if a key event is a refresh command (r or F5)
pub fn is_refresh_event(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R') | KeyCode::F(5))
}

/// Check if a key event is a tab forward command (Tab)
pub fn is_next_tab_event(key: KeyEvent) -> bool {
    key.code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::SHIFT)
}

/// Check if a key event is a tab backward command (Shift+Tab)
pub fn is_prev_tab_event(key: KeyEvent) -> bool {
    key.code == KeyCode::BackTab
        || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_quit_event() {
        let quit_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(is_quit_event(quit_q));

        let quit_ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(is_quit_event(quit_ctrl_c));

        let not_quit = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(!is_quit_event(not_quit));
    }

    #[test]
    fn test_is_refresh_event() {
        let refresh_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(is_refresh_event(refresh_r));

        let refresh_f5 = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
        assert!(is_refresh_event(refresh_f5));

        let not_refresh = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(!is_refresh_event(not_refresh));
    }

    #[test]
    fn test_tab_navigation() {
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert!(is_next_tab_event(tab));

        let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        assert!(is_prev_tab_event(shift_tab));

        let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        assert!(is_prev_tab_event(backtab));
    }
}
