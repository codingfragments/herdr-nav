//! Top-level rendering: the two-pane layout (list + preview), the query
//! bar, the footer, and the keybinding help overlay.
//!
//! **Status: scaffold only.** The real popup shell + tree layout lands in
//! Phase 1 (PLANNING.md §17). This module currently exposes only
//! [`draw_placeholder`], used by the scaffold event loop.

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Scaffold-only placeholder draw: a bordered box with the plugin name
/// and an "Esc to close" hint. Replaced by the real two-pane layout in
/// Phase 3.
pub fn draw_placeholder(frame: &mut ratatui::Frame<'_>) {
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" herdr-nav — scaffold ");
    let text = vec![
        Line::raw(""),
        Line::styled(
            "navigation scaffold — no functionality yet",
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::raw(""),
        Line::raw("Esc to close"),
    ];
    Paragraph::new(text)
        .block(block)
        .render(area, frame.buffer_mut());
}
