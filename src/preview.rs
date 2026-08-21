//! Rich preview rendering for the selected target.
//!
//! The preview pane sits to the right of (or below) the fuzzy list and
//! shows source-specific context for the highlighted target.
//!
//! **Status: Phase 1.** A placeholder preview is rendered by `render.rs`;
//! the per-kind preview renderers land in Phase 2 (PLANNING.md §17),
//! one per `Kind`.
#![allow(dead_code)]

use crate::nav::{Node, NodeId};

/// Render the preview for the node under the cursor into the frame's
/// given area.
///
/// In the scaffold this is a placeholder; the real implementation
/// dispatches on `node.kind` and fetches/renders the appropriate
/// content via the socket client (spec §7).
pub fn render(
    _frame: &mut ratatui::Frame<'_>,
    _area: ratatui::layout::Rect,
    _node: Option<&Node>,
    _socket_path: &str,
) {
    let _ = NodeId::from("");
    // TODO Phase 2.
}
