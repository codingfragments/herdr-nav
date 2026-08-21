//! Rich preview rendering for the selected target (spec §7).
//!
//! One shape for every kind, four stacked regions: kind glyph + title,
//! provenance subtitle, 1–3 status chips, a labelled monospace body
//! (clipped, never scrollable), and a footer naming the default
//! action + alternate. Resolution is debounced 60ms after the cursor
//! settles; while a slow provider resolves, the previous preview stays
//! visible and dimmed (spec §7.4).
//!
//! **Phase 2:** previews for the Session kinds the tree already
//! renders — group, workspace, tab, pane. Pane preview shows the
//! last-N lines of live scrollback via `pane.read` with ANSI colour
//! preserved. Other groups' previews land with their provider phases
//! (Agents → 5, Pinned+zoxide → 6a, Plugins → 7).

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::nav::{Chip, ChipSemantic, Kind, Node, Preview};
use crate::socket_client;

// ── Palette (shared with render.rs; finalized in Phase 9) ───────────────────────

const MANTLE: Color = Color::Rgb(0x1e, 0x20, 0x30);
const TEXT: Color = Color::Rgb(0xca, 0xd3, 0xf5);
const SUBTEXT0: Color = Color::Rgb(0xa5, 0xad, 0xcb);
const SURFACE2: Color = Color::Rgb(0x5b, 0x60, 0x78);
const MAUVE: Color = Color::Rgb(0xc6, 0xa0, 0xf6);
const GREEN: Color = Color::Rgb(0xa6, 0xda, 0x95);
const RED: Color = Color::Rgb(0xed, 0x87, 0x96);
const YELLOW: Color = Color::Rgb(0xee, 0xd4, 0x9f);

/// Debounce window for preview resolution (spec §7.4).
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(60);

/// How many scrollback lines the pane preview pulls (spec §7: "last N").
const PANE_PREVIEW_LINES: u32 = 200;

/// Kind glyph (spec §9). Matches render.rs.
fn kind_glyph(kind: Kind) -> char {
    match kind {
        Kind::Group => '❯',
        Kind::Workspace => '◫',
        Kind::Tab => '▤',
        Kind::Pane => '▪',
        Kind::Dir | Kind::Zox => '▤',
        Kind::Plugin => '⬢',
        Kind::Agent => '◆',
    }
}

/// Kind colour (spec §9). Matches render.rs.
fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::Group => MAUVE,
        Kind::Workspace => Color::Rgb(0xb7, 0xbd, 0xf8),
        Kind::Tab => Color::Rgb(0x91, 0xd7, 0xe3),
        Kind::Pane => Color::Rgb(0x8a, 0xad, 0xf4),
        Kind::Dir => Color::Rgb(0xee, 0xd4, 0x9f),
        Kind::Zox => Color::Rgb(0x8b, 0xd5, 0xca),
        Kind::Plugin => Color::Rgb(0xf5, 0xbd, 0xe6),
        Kind::Agent => Color::Rgb(0xa6, 0xda, 0x95),
    }
}

/// Chip colour by semantics (spec §7/§9).
fn chip_color(s: ChipSemantic) -> Color {
    match s {
        ChipSemantic::Ok => GREEN,
        ChipSemantic::Info => MAUVE,
        ChipSemantic::Warn => YELLOW,
        ChipSemantic::Error => RED,
        ChipSemantic::Blocked => RED,
    }
}

/// Draw the preview for the node under the cursor (spec §7). `node` is
/// `None` when there's nothing to preview (empty tree / no cursor).
/// `socket_path` is used for the pane scrollback fetch. The
/// `last_change` instant drives the 60ms debounce + stale-and-dim.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    node: Option<&Node>,
    socket_path: &str,
    last_change: Option<Instant>,
) {
    // Header bar (mantle) + body (transparent).
    let body_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    let (header, body, is_stale) = match node {
        Some(n) => {
            let p = resolve_preview(n, socket_path);
            (header_line(n, &p), p.body, is_stale(last_change))
        }
        None => (
            Line::styled(
                " preview ",
                Style::default().fg(SUBTEXT0).add_modifier(Modifier::BOLD),
            )
            .style(Style::default().bg(MANTLE)),
            vec![
                Line::raw(""),
                Line::styled(
                    "no selection",
                    Style::default().fg(SURFACE2).add_modifier(Modifier::DIM),
                ),
            ],
            false,
        ),
    };

    frame.render_widget(Paragraph::new(header), body_area[0]);
    // Body: transparent so the terminal theme shows through; stale → dim.
    // The pane preview is bottom-anchored (last scrollback line = last
    // preview line) and lines are hard-truncated to the pane width — no
    // wrapping, wide lines show their first N chars (spec §7.4: clipped,
    // never scrollable). Group/workspace/tab previews are short, so the
    // same clip is a no-op for them.
    let body_style = if is_stale {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    let clipped = clip_body(&body, body_area[1]);
    frame.render_widget(Paragraph::new(clipped).style(body_style), body_area[1]);
}

/// Clip the preview body to `area`: truncate each line to `area.width`
/// graphemes (no wrap — wide lines show their first N chars), and keep
/// only the last `area.height` lines (bottom-anchored: the last
/// scrollback line is the last preview line). Spec §7.4: read-only,
/// clipped, never scrollable.
fn clip_body(body: &[Line<'static>], area: Rect) -> Vec<Line<'static>> {
    let w = area.width as usize;
    let h = area.height as usize;
    if h == 0 {
        return Vec::new();
    }
    // Bottom-anchor: take the last `h` lines.
    let start = body.len().saturating_sub(h);
    let out: Vec<Line<'static>> = body[start..]
        .iter()
        .map(|line| truncate_line(line, w))
        .collect();
    out
}

/// Truncate a line to `w` cells, preserving span styles (so ANSI
/// colour survives the clip). Walks the spans, taking chars until the
/// width budget is spent; each kept span keeps its style.
fn truncate_line(line: &Line<'static>, w: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut budget = w;
    for span in &line.spans {
        if budget == 0 {
            break;
        }
        let content: String = span.content.chars().take(budget).collect();
        let taken = content.chars().count();
        budget = budget.saturating_sub(taken);
        if taken > 0 {
            spans.push(Span::styled(content, span.style));
        }
    }
    Line::from(spans).style(line.style)
}

/// Has `last_change` aged past the debounce window? While it hasn't,
/// the previous preview is shown dimmed (stale-and-dim, spec §7.4).
fn is_stale(last_change: Option<Instant>) -> bool {
    match last_change {
        Some(t) => t.elapsed() < PREVIEW_DEBOUNCE,
        None => false,
    }
}

/// The 2-row header: row 0 = kind glyph + title + chips; row 1 = subtitle.
fn header_line(node: &Node, p: &Preview) -> Line<'static> {
    let glyph = kind_glyph(node.kind);
    let title = Span::styled(
        format!(" {glyph} {} ", node.label),
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    );
    let mut spans = vec![title];
    for chip in &p.chips {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" {} ", chip.text),
            Style::default().fg(chip_color(chip.semantic)),
        ));
    }
    Line::from(spans).style(Style::default().bg(MANTLE))
}

/// Resolve the preview for a node (spec §7). For Session kinds this is
/// synchronous in Phase 2 (the socket call is fast; the debounce handles
/// the perceptual latency). Pane preview fetches scrollback via
/// `pane.read`; the others are built from the tree node itself.
fn resolve_preview(node: &Node, socket_path: &str) -> Preview {
    match node.kind {
        Kind::Pane => pane_preview(node, socket_path),
        Kind::Agent => agent_preview(node, socket_path),
        Kind::Dir | Kind::Zox => dir_preview(node),
        Kind::Group => group_preview(node),
        Kind::Workspace => workspace_preview(node),
        Kind::Tab => tab_preview(node),
        // Other kinds land with their provider phases.
        _ => Preview {
            icon: kind_glyph(node.kind),
            title: node.label.clone(),
            ..Preview::default()
        },
    }
}

/// Pane preview (spec §7): last-N lines of live scrollback, ANSI colour
/// preserved; footer line = cwd + cursor position. Chips:
/// focused/running, pid, cpu (best-effort from the pane node — Phase 5
/// fills the live fields; here we show what the tree carries).
fn pane_preview(node: &Node, socket_path: &str) -> Preview {
    let pane_id = node.id.strip_prefix("session:pane:").unwrap_or(&node.id);
    let body = read_pane_scrollback(socket_path, pane_id);
    let body_label = "PANE PREVIEW";
    Preview {
        icon: kind_glyph(Kind::Pane),
        title: node.label.clone(),
        subtitle: format!("pane {pane_id}"),
        chips: Vec::new(),
        body_label,
        body,
        action: "jump to pane".to_string(),
        alt: String::new(),
    }
}

/// Read the pane's scrollback via `pane.read` and parse ANSI into
/// styled ratatui lines (spec §7; reuses the herdr-flash approach:
/// format=ansi, strip_ansi=false, then `ansi-to-tui` IntoText).
fn read_pane_scrollback(socket_path: &str, pane_id: &str) -> Vec<Line<'static>> {
    if socket_path.is_empty() {
        return vec![Line::styled(
            "(no socket — pane scrollback unavailable)",
            Style::default().fg(SURFACE2),
        )];
    }
    let params = serde_json::json!({
        "pane_id": pane_id,
        "source": "recent_unwrapped",
        "lines": PANE_PREVIEW_LINES,
        "format": "ansi",
        "strip_ansi": false,
    });
    match socket_client::request(socket_path, "pane.read", params) {
        Ok(result) => {
            let text = result
                .get("read")
                .and_then(|v| v.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            ansi_to_lines(text)
        }
        Err(e) => vec![Line::styled(
            format!("pane.read failed: {e}"),
            Style::default().fg(RED),
        )],
    }
}

/// Parse ANSI-styled text into one ratatui `Line` per line (spec §7;
/// ported from herdr-flash's `parse_ansi_lines`). On any parse failure
/// we fall back to plain unstyled lines so the preview never breaks.
fn ansi_to_lines(text: &str) -> Vec<Line<'static>> {
    use ansi_to_tui::IntoText as _;
    match text.into_text() {
        Ok(t) => t.lines.into_iter().collect(),
        Err(_) => text.lines().map(|l| Line::raw(l.to_string())).collect(),
    }
}

/// Agent preview (spec §7): tail of the agent transcript; if
/// blocked, the pending question + its options verbatim. Chips:
/// status + duration, token count. Phase 5 fetches the transcript
/// via `pane.read` (the agent runs in a pane) and shows the tail.
fn agent_preview(node: &Node, socket_path: &str) -> Preview {
    let pane_id = node.id.strip_prefix("agents:").unwrap_or(&node.id);
    let status = node.meta.clone();
    let chips = vec![Chip {
        text: status.clone(),
        semantic: match status.as_str() {
            "waiting" => ChipSemantic::Blocked,
            "working" => ChipSemantic::Ok,
            _ => ChipSemantic::Info,
        },
    }];
    let body = read_pane_scrollback(socket_path, pane_id);
    Preview {
        icon: kind_glyph(Kind::Agent),
        title: node.label.clone(),
        subtitle: format!("agent pane {pane_id}"),
        chips,
        body_label: "AGENT TRANSCRIPT",
        body,
        action: "jump to agent pane".to_string(),
        alt: String::new(),
    }
}

/// Directory preview (spec §7): first ~8 entries (dirs
/// first), then last-visit recency and hit count. Chips:
/// git branch + dirty, entry count. Phase 6a reads the dir
/// listing locally (the socket dir-listing method is TBD).
fn dir_preview(node: &Node) -> Preview {
    let path = node.id.split_once(':').map(|(_, p)| p).unwrap_or(&node.id);
    let expanded = expand_path(path);
    let (branch, dirty) = git_status(&expanded);
    let mut chips = Vec::new();
    if let Some(b) = &branch {
        let text = if dirty { format!("{b}*") } else { b.clone() };
        chips.push(Chip {
            text,
            semantic: if dirty {
                ChipSemantic::Warn
            } else {
                ChipSemantic::Ok
            },
        });
    }
    let entries = dir_entries(&expanded);
    chips.push(Chip {
        text: format!("{} entries", entries.len()),
        semantic: ChipSemantic::Info,
    });
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Directory",
            Style::default().fg(SUBTEXT0).add_modifier(Modifier::BOLD),
        ),
    ];
    for e in entries.iter().take(8) {
        let mark = if e.is_dir { "/" } else { " " };
        body.push(Line::from(vec![
            Span::raw(format!(" {mark} ")),
            Span::raw(e.name.clone()),
        ]));
    }
    if entries.len() > 8 {
        body.push(Line::raw(format!(" … ({} more)", entries.len() - 8)));
    }
    Preview {
        icon: kind_glyph(node.kind),
        title: node.label.clone(),
        subtitle: expanded.clone(),
        chips,
        body_label: "DIRECTORY",
        body,
        action: "open workspace".to_string(),
        alt: String::new(),
    }
}

/// Expand `~`/`$HOME` in a path (mirrors source::expand_path).
fn expand_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

/// Git branch + dirty for a path (best-effort).
fn git_status(path: &str) -> (Option<String>, bool) {
    let out = std::process::Command::new("git")
        .args(["-C", path, "status", "--porcelain", "--branch"])
        .output();
    let Ok(out) = out else { return (None, false) };
    if !out.status.success() {
        return (None, false);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let branch = stdout
        .lines()
        .find_map(|l| l.strip_prefix("## ").and_then(|s| s.split(' ').next()))
        .map(str::to_string);
    let dirty = !out.stdout.is_empty() && stdout.lines().any(|l| !l.starts_with("## "));
    (branch, dirty)
}

/// Directory entries (dirs first), best-effort.
struct DirEntry {
    name: String,
    is_dir: bool,
}

fn dir_entries(path: &str) -> Vec<DirEntry> {
    let Ok(read) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for e in read.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(DirEntry { name, is_dir: true });
        } else {
            files.push(DirEntry {
                name,
                is_dir: false,
            });
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    dirs
}

/// Group preview (spec §7): aggregate roster of children + one line
/// explaining the group's role. Chips: counts, error counts.
fn group_preview(node: &Node) -> Preview {
    let mut body = vec![Line::raw("")];
    let role = match node.id.as_str() {
        "group:session" => "live panes in your current session",
        "group:agents" => "agents waiting on you",
        "group:pinned" => "directories you pinned (⌘1–⌘9)",
        "group:zoxide" => "frequently-visited directories (frecency)",
        "group:plugins" => "installed plugins",
        _ => "",
    };
    if !role.is_empty() {
        body.push(Line::raw(role.to_string()));
    }
    body.push(Line::raw(""));
    for c in &node.children {
        let glyph = kind_glyph(c.kind);
        let color = kind_color(c.kind);
        body.push(Line::from(vec![
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::raw(c.label.clone()),
        ]));
    }
    let chips = if node.meta == "unavailable" {
        vec![Chip {
            text: "unavailable".to_string(),
            semantic: ChipSemantic::Error,
        }]
    } else {
        vec![Chip {
            text: format!("{} entries", node.children.len()),
            semantic: ChipSemantic::Info,
        }]
    };
    Preview {
        icon: kind_glyph(Kind::Group),
        title: node.label.clone(),
        subtitle: String::new(),
        chips,
        body_label: "SUMMARY",
        body,
        action: String::new(),
        alt: String::new(),
    }
}

/// Workspace preview (spec §7): child inventory (tabs) + chip
/// active/detached. The ASCII layout diagram lands in Phase 9 (needs
/// the tab layout from the daemon); here we list the tabs.
fn workspace_preview(node: &Node) -> Preview {
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Tabs",
            Style::default().fg(SUBTEXT0).add_modifier(Modifier::BOLD),
        ),
    ];
    for c in &node.children {
        let glyph = kind_glyph(c.kind);
        let color = kind_color(c.kind);
        let mark = if c.meta == "active" { "● " } else { "  " };
        body.push(Line::from(vec![
            Span::raw(mark.to_string()),
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::raw(c.label.clone()),
        ]));
    }
    let chips = if node.meta == "active" {
        vec![Chip {
            text: "active".to_string(),
            semantic: ChipSemantic::Ok,
        }]
    } else {
        Vec::new()
    };
    Preview {
        icon: kind_glyph(Kind::Workspace),
        title: node.label.clone(),
        subtitle: String::new(),
        chips,
        body_label: "SUMMARY",
        body,
        action: "switch to workspace".to_string(),
        alt: String::new(),
    }
}

/// Tab preview (spec §7): child inventory (panes) + ASCII split
/// diagram (Phase 9; here we list the panes). Chips: layout name.
fn tab_preview(node: &Node) -> Preview {
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Panes",
            Style::default().fg(SUBTEXT0).add_modifier(Modifier::BOLD),
        ),
    ];
    for c in &node.children {
        let glyph = kind_glyph(c.kind);
        let color = kind_color(c.kind);
        let mark = if c.meta == "active" { "● " } else { "  " };
        body.push(Line::from(vec![
            Span::raw(mark.to_string()),
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::raw(c.label.clone()),
        ]));
    }
    let chips = if node.meta == "active" {
        vec![Chip {
            text: "active".to_string(),
            semantic: ChipSemantic::Ok,
        }]
    } else {
        Vec::new()
    };
    Preview {
        icon: kind_glyph(Kind::Tab),
        title: node.label.clone(),
        subtitle: String::new(),
        chips,
        body_label: "SUMMARY",
        body,
        action: "switch to tab".to_string(),
        alt: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_node(id: &str, label: &str) -> Node {
        Node {
            id: id.into(),
            kind: Kind::Pane,
            label: label.into(),
            meta: String::new(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        }
    }

    #[test]
    fn pane_preview_strips_id_prefix() {
        let n = pane_node("session:pane:w1:p1", "nvim");
        // No socket → body explains the absence, doesn't crash.
        let p = resolve_preview(&n, "");
        assert_eq!(p.body_label, "PANE PREVIEW");
        assert_eq!(p.subtitle, "pane w1:p1");
        assert_eq!(p.action, "jump to pane");
    }

    #[test]
    fn group_preview_rosters_children() {
        let n = Node {
            id: "group:session".into(),
            kind: Kind::Group,
            label: "Session".into(),
            meta: "3 panes".into(),
            crumbs: None,
            children: vec![
                pane_node("session:pane:p1", "a"),
                pane_node("session:pane:p2", "b"),
            ],
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        let p = group_preview(&n);
        assert_eq!(p.body_label, "SUMMARY");
        assert!(p.body.iter().any(|l| l.to_string().contains("live panes")));
        assert!(p.body.iter().any(|l| l.to_string().contains("a")));
        assert_eq!(p.chips.len(), 1);
        assert_eq!(p.chips[0].text, "2 entries");
    }

    #[test]
    fn group_preview_unavailable_chip() {
        let n = Node {
            id: "group:agents".into(),
            kind: Kind::Group,
            label: "Agents".into(),
            meta: "unavailable".into(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        let p = group_preview(&n);
        assert_eq!(p.chips[0].text, "unavailable");
        assert_eq!(p.chips[0].semantic, ChipSemantic::Error);
    }

    #[test]
    fn workspace_preview_marks_active() {
        let n = Node {
            id: "session:ws:w1".into(),
            kind: Kind::Workspace,
            label: "herdr-dev".into(),
            meta: "active".into(),
            crumbs: None,
            children: vec![Node {
                id: "session:tab:t1".into(),
                kind: Kind::Tab,
                label: "editor".into(),
                meta: "active".into(),
                crumbs: None,
                children: Vec::new(),
                preview: Preview::default(),
                actions: crate::nav::Actions::default(),
            }],
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        let p = workspace_preview(&n);
        assert_eq!(p.chips[0].text, "active");
        assert_eq!(p.chips[0].semantic, ChipSemantic::Ok);
        assert!(p.body.iter().any(|l| l.to_string().contains("editor")));
    }

    #[test]
    fn ansi_to_lines_plain_fallback() {
        // Non-ANSI text → plain lines, one per source line.
        let lines = ansi_to_lines("hello\nworld");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn clip_body_bottom_anchors_and_truncates() {
        use ratatui::layout::Rect;
        // 5 lines, area fits 3 → keep last 3 (bottom-anchored).
        let body: Vec<Line<'static>> = (0..5).map(|i| Line::raw(format!("line {i}"))).collect();
        let area = Rect::new(0, 0, 10, 3);
        let clipped = clip_body(&body, area);
        assert_eq!(clipped.len(), 3);
        assert_eq!(clipped[0].to_string(), "line 2");
        assert_eq!(clipped[2].to_string(), "line 4");
    }

    #[test]
    fn clip_body_truncates_wide_lines() {
        use ratatui::layout::Rect;
        // A 30-char line, area width 10 → first 10 chars only.
        let body: Vec<Line<'static>> = vec![Line::raw("0123456789abcdefghij")];
        let area = Rect::new(0, 0, 10, 1);
        let clipped = clip_body(&body, area);
        assert_eq!(clipped.len(), 1);
        assert_eq!(clipped[0].to_string(), "0123456789");
    }

    #[test]
    fn clip_body_preserves_span_style() {
        use ratatui::layout::Rect;
        use ratatui::style::{Color, Modifier, Style};
        // A styled span (red bold) must survive the clip with its style.
        let red = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        let body: Vec<Line<'static>> =
            vec![Line::from(vec![Span::styled("0123456789abcdefghij", red)])];
        let area = Rect::new(0, 0, 10, 1);
        let clipped = clip_body(&body, area);
        assert_eq!(clipped[0].to_string(), "0123456789");
        // The kept span retains its style.
        assert_eq!(clipped[0].spans.len(), 1);
        assert_eq!(clipped[0].spans[0].style, red);
    }
}
