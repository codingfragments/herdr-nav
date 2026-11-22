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

use crate::host_ext::NavFull;
use crate::nav::{Chip, ChipSemantic, Kind, Node, Preview};

// ── Palette (shared with render.rs; finalized in Phase 9) ───────────────────────

use crate::theme::Palette;

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
fn kind_color(p: &Palette, kind: Kind) -> Color {
    crate::theme::kind_color(p, kind)
}

/// Chip colour by semantics (spec §7/§9).
fn chip_color(s: ChipSemantic, palette: &Palette) -> Color {
    match s {
        ChipSemantic::Ok => palette.green,
        ChipSemantic::Info => palette.mauve,
        ChipSemantic::Warn => palette.yellow,
        ChipSemantic::Error => palette.red,
        ChipSemantic::Blocked => palette.red,
    }
}

/// Draw the preview for the node under the cursor (spec §7). `node` is
/// `None` when there's nothing to preview (empty tree / no cursor).
/// The host is used for the pane scrollback fetch. The
/// `last_change` instant drives the 60ms debounce + stale-and-dim.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    node: Option<&Node>,
    host: &dyn NavFull,
    last_change: Option<Instant>,
    palette: &Palette,
) {
    // Header bar (mantle) + body (transparent).
    let body_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    let (header, body, is_stale) = match node {
        Some(n) => {
            let p = resolve_preview(n, host, palette);
            (header_line(n, &p, palette), p.body, is_stale(last_change))
        }
        None => (
            Line::styled(
                " preview ",
                Style::default()
                    .fg(palette.subtext0)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().bg(palette.panel_bg)),
            vec![
                Line::raw(""),
                Line::styled(
                    "no selection",
                    Style::default()
                        .fg(palette.overlay0)
                        .add_modifier(Modifier::DIM),
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
fn header_line(node: &Node, p: &Preview, palette: &Palette) -> Line<'static> {
    let glyph = kind_glyph(node.kind);
    let title = Span::styled(
        format!(" {glyph} {} ", node.label),
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
    );
    let mut spans = vec![title];
    for chip in &p.chips {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" {} ", chip.text),
            Style::default().fg(chip_color(chip.semantic, palette)),
        ));
    }
    Line::from(spans).style(Style::default().bg(palette.panel_bg))
}

/// Resolve the preview for a node (spec §7). For Session kinds this is
/// synchronous in Phase 2 (the socket call is fast; the debounce handles
/// the perceptual latency). Pane preview fetches scrollback via
/// `pane.read`; the others are built from the tree node itself.
#[allow(unreachable_patterns)]
fn resolve_preview(node: &Node, host: &dyn NavFull, palette: &Palette) -> Preview {
    match node.kind {
        Kind::Pane => pane_preview(node, host, palette),
        Kind::Agent => agent_preview(node, host, palette),
        Kind::Dir | Kind::Zox => dir_preview(node, palette),
        Kind::Plugin => plugin_preview(node, host, palette),
        Kind::Group => group_preview(node, palette),
        Kind::Workspace => workspace_preview(node, palette),
        Kind::Tab => tab_preview(node, palette),
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
fn pane_preview(node: &Node, host: &dyn NavFull, palette: &Palette) -> Preview {
    let pane_id = node.id.strip_prefix("session:pane:").unwrap_or(&node.id);
    let body = read_pane_scrollback(host, pane_id, palette);
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
fn read_pane_scrollback(
    host: &dyn NavFull,
    pane_id: &str,
    palette: &Palette,
) -> Vec<Line<'static>> {
    if host.name() == "none" {
        return vec![Line::styled(
            "(no socket — pane scrollback unavailable)",
            Style::default().fg(palette.overlay0),
        )];
    }
    // Same contract as before the migration: the scrollback, raw ANSI,
    // capped at PANE_PREVIEW_LINES (the host maps this to herdr's
    // `recent_unwrapped` + `format=ansi`).
    match host.read_pane(
        pane_id,
        muxhost::ReadSource::Scrollback,
        muxhost::ReadFormat::Ansi,
        Some(PANE_PREVIEW_LINES),
    ) {
        Ok(text) => ansi_to_lines(&text),
        Err(e) => vec![Line::styled(
            format!("pane.read failed: {e}"),
            Style::default().fg(palette.red),
        )],
    }
}

/// Expand/strip the non-SGR escape sequences a multiplexer capture can
/// carry, before `ansi-to-tui` parses (it handles SGR but *drops*
/// cursor-forward, which would shift columns):
/// - `CSI n C` (cursor forward — Rex's `--format=vt` uses it as
///   run-length spaces) becomes n spaces;
/// - SGR (`CSI … m`) is kept verbatim, for ansi-to-tui to style;
/// - every other escape (OSC 8 hyperlinks, charset selection, other
///   CSI) is dropped, so styles survive and columns line up.
fn vt_prepare(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            // copy the (UTF-8) char
            let ch_len = utf8_len(bytes[i]);
            out.push_str(&text[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        i += 1;
        match bytes.get(i) {
            Some(b'[') => {
                // CSI: params until a letter final
                i += 1;
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let final_byte = bytes.get(i).copied().unwrap_or(b'm');
                i += 1;
                match final_byte {
                    b'C' => {
                        let n: usize = text[start..i - 1]
                            .split(';')
                            .next()
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(1);
                        for _ in 0..n.max(1) {
                            out.push(' ');
                        }
                    }
                    b'm' => {
                        out.push_str(&text[start - 2..i]); // keep "ESC[…m"
                    }
                    _ => {} // every other CSI: dropped
                }
            }
            Some(b']') => {
                // OSC: drop until BEL or ST (ESC \)
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            Some(b'(') | Some(b')') => i += 2, // charset designation + 1 char
            _ => i += 1,                       // other single-char escapes
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}
/// Parse ANSI-styled text into one ratatui `Line` per line (spec §7;
/// ported from herdr-flash's `parse_ansi_lines`; `vt_prepare` is
/// flash's fix for Rex's `--format=vt`, which uses `CSI n C` as
/// run-length spaces that ansi-to-tui would drop — columns then line
/// up wrong). On any parse failure we fall back to plain unstyled
/// lines so the preview never breaks.
fn ansi_to_lines(text: &str) -> Vec<Line<'static>> {
    use ansi_to_tui::IntoText as _;
    let text = vt_prepare(text);
    match text.into_text() {
        Ok(t) => t.lines.into_iter().collect(),
        Err(_) => text.lines().map(|l| Line::raw(l.to_string())).collect(),
    }
}

/// Agent preview (spec §7): tail of the agent transcript; if
/// blocked, the pending question + its options verbatim. Chips:
/// status + duration, token count. Phase 5 fetches the transcript
/// via `pane.read` (the agent runs in a pane) and shows the tail.
fn agent_preview(node: &Node, host: &dyn NavFull, palette: &Palette) -> Preview {
    let pane_id = node.id.strip_prefix("agents:").unwrap_or(&node.id);
    let status = node.meta.clone();
    let chips = vec![Chip {
        text: status.clone(),
        semantic: match status.as_str() {
            "waiting" => ChipSemantic::Blocked,
            "working" => ChipSemantic::Ok,
            "error" => ChipSemantic::Error,
            "done" => ChipSemantic::Info,
            _ => ChipSemantic::Info,
        },
    }];
    let body = read_pane_scrollback(host, pane_id, palette);
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
fn dir_preview(node: &Node, palette: &Palette) -> Preview {
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
            Style::default()
                .fg(palette.subtext0)
                .add_modifier(Modifier::BOLD),
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

/// Plugin preview (spec §7): one-line description,
/// declared actions, chips: enabled/version. Fetches
/// the plugin's actions from `plugin.list`.
fn plugin_preview(node: &Node, host: &dyn NavFull, palette: &Palette) -> Preview {
    let pid = node.id.strip_prefix("plugin:").unwrap_or(&node.id);
    let chips = vec![Chip {
        text: node.meta.clone(),
        semantic: if node.meta == "disabled" {
            ChipSemantic::Error
        } else {
            ChipSemantic::Ok
        },
    }];
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Actions",
            Style::default()
                .fg(palette.subtext0)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    // Fetch the plugin's actions from the host's plugin list.
    if host.name() != "none" {
        if let Ok(plugins) = host.list_plugins() {
            for plugin in &plugins {
                if plugin.get("plugin_id").and_then(|v| v.as_str()) == Some(pid) {
                    let actions = plugin
                        .get("actions")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if actions.is_empty() {
                        body.push(Line::raw("  (no actions declared)"));
                    } else {
                        for a in &actions {
                            let aid = a.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let title = a.get("title").and_then(|v| v.as_str()).unwrap_or(aid);
                            body.push(Line::from(vec![
                                Span::raw("  ▸ "),
                                Span::raw(title.to_string()),
                            ]));
                        }
                    }
                    break;
                }
            }
        }
    }
    Preview {
        icon: kind_glyph(Kind::Plugin),
        title: node.label.clone(),
        subtitle: format!("plugin {pid}"),
        chips,
        body_label: "PLUGIN",
        body,
        action: if node.meta == "no actions" || node.meta == "disabled" {
            String::new()
        } else {
            "open actions".to_string()
        },
        alt: String::new(),
    }
}

/// Group preview (spec §7): aggregate roster of children + one line
/// explaining the group's role. Chips: counts, error counts.
fn group_preview(node: &Node, palette: &Palette) -> Preview {
    let mut body = vec![Line::raw("")];
    let role = match node.id.as_str() {
        "group:session" => "live panes in your current session",
        "group:agents" => "agent panes · red = waiting on you",
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
        let color = kind_color(palette, c.kind);
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
fn workspace_preview(node: &Node, palette: &Palette) -> Preview {
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Tabs",
            Style::default()
                .fg(palette.subtext0)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    for c in &node.children {
        let glyph = kind_glyph(c.kind);
        let color = kind_color(palette, c.kind);
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
fn tab_preview(node: &Node, palette: &Palette) -> Preview {
    let mut body = vec![
        Line::raw(""),
        Line::styled(
            "Panes",
            Style::default()
                .fg(palette.subtext0)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    for c in &node.children {
        let glyph = kind_glyph(c.kind);
        let color = kind_color(palette, c.kind);
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
        // No host → body explains the absence, doesn't crash.
        let p = resolve_preview(&n, &crate::host_ext::NoHost, &Palette::default());
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
        let p = group_preview(&n, &Palette::default());
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
        let p = group_preview(&n, &Palette::default());
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
        let p = workspace_preview(&n, &Palette::default());
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

#[cfg(test)]
mod vt_fixture_tests {
    use super::*;

    /// The recorded Rex vt capture (tests/fixtures/rex/capture-vt.txt):
    /// vt_prepare must expand CSI-n-C runs into spaces and ansi-to-tui
    /// must turn the result into styled lines — the same fix
    /// herdr-flash made for Rex's `--format=vt`.
    #[test]
    fn vt_fixture_parses_into_lines() {
        let raw = include_str!("../tests/fixtures/rex/capture-vt.txt");
        let lines = ansi_to_lines(raw);
        assert!(!lines.is_empty(), "no lines parsed from the fixture");
        let text: String = lines
            .iter()
            .flat_map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(
            text.contains("fish"),
            "fixture text not rendered: {:?}",
            &text[..text.len().min(80)]
        );
        // CSI n C expanded into spaces, not dropped: no stray ESC in output.
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn vt_prepare_expands_cursor_forward() {
        // "a" + ESC[10C + "b" -> 10 spaces between them, SGR kept.
        let prepared = vt_prepare("a\u{1b}[10Cb\u{1b}[1;32m x \u{1b}[0m");
        assert!(
            prepared.contains("a          b"),
            "cursor-forward not expanded: {prepared:?}"
        );
        assert!(
            prepared.contains("\u{1b}[1;32m"),
            "SGR must survive verbatim"
        );
    }
}
