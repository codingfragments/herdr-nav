//! Top-level rendering: the four bands (title/search/body/footer), the
//! body split (list + preview), the list status strip, and browse-mode
//! row rendering (spec §2/§3.1/§10).
//!
//! **Phase 1:** browse only. The search bar is empty (typing is inert;
//! search mode lands in Phase 4). The preview pane is a placeholder
//! (per-kind previews land in Phase 2). Theme colours and the full row
//! grammar (§9/§10) are applied in Phase 9; here we use a minimal
//! Catppuccin-Macchiato-flavoured palette sufficient to render.
//!
//! **Background decision (2026-08-21):** the popup body (list + preview
//! content) is **transparent** — no explicit bg — so the host terminal's
//! themed background shows through, instead of painting a fixed dark
//! `base` that reads as pure black against a lighter terminal theme. The
//! bars (title/search/footer/status-strip) keep `mantle` per spec §2
//! ("bars = mantle"), and surface2 horizontal rules separate the bands
//! for crisp separation. This amends spec §9's `base = popup body` to
//! "body = terminal-themed (transparent)"; PLANNING.md wins per the
//! standing precedence rule.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::nav::{Kind, Tree, Twisty};

// ── Minimal palette (spec §9, finalized in Phase 9) ───────────────────────────

// BASE is intentionally unused for fills: the body is transparent so the
// terminal theme shows through. Kept only for reference / future use.
#[allow(dead_code)]
const BASE: Color = Color::Rgb(0x24, 0x27, 0x3a);
const MANTLE: Color = Color::Rgb(0x1e, 0x20, 0x30);
const SURFACE0: Color = Color::Rgb(0x36, 0x3a, 0x4f);
const TEXT: Color = Color::Rgb(0xca, 0xd3, 0xf5);
const SUBTEXT0: Color = Color::Rgb(0xa5, 0xad, 0xcb);
const SURFACE2: Color = Color::Rgb(0x5b, 0x60, 0x78);
const MAUVE: Color = Color::Rgb(0xc6, 0xa0, 0xf6);
const SAPPHIRE: Color = Color::Rgb(0x7d, 0xc4, 0xe4);
const RED: Color = Color::Rgb(0xed, 0x87, 0x96);
const PEACH: Color = Color::Rgb(0xf5, 0xa9, 0x7f);

/// Kind glyph (spec §9). Finalized in Phase 9; here only the kinds
/// Phase 1 renders (Group/Workspace/Tab/Pane) are exercised.
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

/// Kind colour (spec §9).
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

/// Draw the whole popup (spec §2): four bands inside a bordered frame,
/// with surface2 horizontal rules separating the bars from the body.
pub fn draw(frame: &mut Frame, tree: &Tree) {
    let area = frame.area();

    // No outer border — the Herdr popup window itself renders one
    // (confirmed in the sister plugins: herdr-flash draws no outer
    // border around the full area). Bands render directly into `area`.
    //
    // title / search / rule / body / rule / footer. The rules are
    // 1-row `Block::default().borders(Borders::TOP)` bands — a
    // thin `─` line in surface2, not a bulky filled band.
    let bands = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Length(1), // search bar
            Constraint::Length(1), // ─ rule
            Constraint::Min(1),    // body
            Constraint::Length(1), // ─ rule
            Constraint::Length(1), // footer
        ])
        .split(area);

    draw_title_bar(frame, bands[0], tree);
    draw_search_bar(frame, bands[1]);
    draw_rule(frame, bands[2]);
    draw_body(frame, bands[3], tree);
    draw_rule(frame, bands[4]);
    draw_footer(frame, bands[5]);
}

/// A thin full-width surface2 horizontal rule via a ratatui top
/// border (spec §2 band separation). A 1-row `Block` with
/// `Borders::TOP` — the border line occupies the row, no fill.
fn draw_rule(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(SURFACE2)),
        area,
    );
}

fn draw_title_bar(frame: &mut Frame, area: Rect, tree: &Tree) {
    let rows = tree.visible_rows();
    let badge = Span::styled(
        " BROWSE ",
        Style::default()
            .bg(SAPPHIRE)
            .fg(MANTLE)
            .add_modifier(Modifier::BOLD),
    );
    let counts = Span::styled(format!(" {} ", rows.len()), Style::default().fg(SUBTEXT0));
    let title = Span::styled(
        " herdr switch ",
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    );
    let line = Line::from(vec![title, badge, counts]).style(Style::default().bg(MANTLE));
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_search_bar(frame: &mut Frame, area: Rect) {
    // Phase 1: query is empty; typing is inert (search mode is Phase 4).
    let prompt = Span::styled(
        "❯ ",
        Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
    );
    let placeholder = Span::styled("type to search…", Style::default().fg(SURFACE2));
    let line = Line::from(vec![prompt, placeholder]).style(Style::default().bg(MANTLE));
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_body(frame: &mut Frame, area: Rect, tree: &Tree) {
    // Body split: list 44% · vertical rule · preview 56% (spec §2).
    // Below 60 cols the preview is dropped and the list takes the full
    // width (spec §2; the toggle key lands in Phase 9).
    if area.width < 60 {
        draw_list(frame, area, tree);
        return;
    }
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(44),
            Constraint::Length(1),
            Constraint::Percentage(56),
        ])
        .split(area);

    draw_list(frame, split[0], tree);
    // Vertical rule — a 1-cell surface2 column (spec §2), no border.
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(SURFACE2)),
        split[1],
    );
    draw_preview_placeholder(frame, split[2]);
}

fn draw_list(frame: &mut Frame, area: Rect, tree: &Tree) {
    let rows = tree.visible_rows();
    let h = area.height as usize;

    // Reserve a 1-row status strip at the bottom (spec §2). The strip is
    // a bar (mantle); the list content above it is transparent.
    let (list_area, strip_area) = if h > 1 {
        let s = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        (s[0], s[1])
    } else {
        (area, Rect::ZERO)
    };

    let list_h = list_area.height as usize;
    // Keep the cursor in view: clamp the scroll so the cursor row is visible.
    let mut start = tree.scroll;
    if list_h > 0 {
        if tree.cursor < start {
            start = tree.cursor;
        } else if tree.cursor >= start + list_h {
            start = tree.cursor + 1 - list_h;
        }
    }
    let start = start.min(rows.len().saturating_sub(1));

    let visible: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(start)
        .take(list_h)
        .map(|(i, row)| draw_row(i, row, i == tree.cursor))
        .collect();

    // No bg on the list paragraph — transparent so the terminal theme
    // shows through; selected rows carry their own SURFACE0 bg.
    frame.render_widget(Paragraph::new(visible), list_area);

    // Status strip: scope left, position right (spec §2). A bar → mantle.
    let scope = Span::styled(" tree · target groups ", Style::default().fg(SURFACE2));
    let pos = Span::styled(
        format!(
            " {}/{} ",
            tree.cursor.saturating_add(1).min(rows.len()),
            rows.len()
        ),
        Style::default().fg(SUBTEXT0),
    );
    frame.render_widget(
        Paragraph::new(
            Line::from(vec![scope, Span::raw(""), pos]).style(Style::default().bg(MANTLE)),
        ),
        strip_area,
    );
}

fn draw_row(_i: usize, row: &crate::nav::VisibleRow, selected: bool) -> Line<'static> {
    let indent = "  ".repeat(row.depth);
    let twisty = match row.twisty {
        Twisty::Expanded => "▾ ",
        Twisty::Closed => "▸ ",
        Twisty::Leaf => "  ",
    };
    let glyph = kind_glyph(row.kind);
    let glyph_color = kind_color(row.kind);

    let label_color = if selected { TEXT } else { SUBTEXT0 };

    // Selection: a 2px left bar in the row's kind colour (spec §9). In a
    // cell TUI we render the first cell as a kind-coloured background
    // (a space with bg=kind_color), which reads as a solid left bar.
    let bar = if selected {
        Span::styled(" ", Style::default().bg(glyph_color))
    } else {
        Span::raw(" ")
    };

    let mut spans = vec![bar, Span::raw(indent)];
    spans.push(Span::styled(
        format!("{twisty}{glyph} "),
        Style::default()
            .fg(glyph_color)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        row.label.clone(),
        Style::default().fg(label_color),
    ));
    if !row.meta.is_empty() {
        spans.push(Span::raw("  "));
        let meta_color = if row.meta == "unavailable" || row.meta == "empty" {
            RED
        } else {
            SURFACE2
        };
        spans.push(Span::styled(
            row.meta.clone(),
            Style::default().fg(meta_color),
        ));
    }
    // Line-level bg only when selected (SURFACE0); non-selected rows are
    // transparent so the terminal theme shows through.
    let line_style = if selected {
        Style::default().bg(SURFACE0)
    } else {
        Style::default()
    };
    Line::from(spans).style(line_style)
}

fn draw_preview_placeholder(frame: &mut Frame, area: Rect) {
    // Phase 2: per-kind preview (spec §7). Here a placeholder — no
    // bordered box (spec §2 separates panes with a vertical rule only).
    // Header line is a bar (mantle); body is transparent.
    let header = Line::styled(
        " preview ",
        Style::default().fg(SUBTEXT0).add_modifier(Modifier::BOLD),
    )
    .style(Style::default().bg(MANTLE));
    let body = vec![
        Line::raw(""),
        Line::styled(
            "preview pane — per-kind content lands in Phase 2",
            Style::default().fg(SURFACE2).add_modifier(Modifier::DIM),
        ),
    ];
    let body_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    frame.render_widget(Paragraph::new(header), body_area[0]);
    // Body transparent — terminal theme shows through.
    frame.render_widget(Paragraph::new(body), body_area[1]);
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    // Mode-aware hints (spec §8). Browse: ⏎ open/expand, esc close.
    let hints = vec![
        Span::styled(
            " ⏎ ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled("open/expand", Style::default().fg(SUBTEXT0)),
        Span::styled(
            "   ↑↓ ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled("move", Style::default().fg(SUBTEXT0)),
        Span::styled(
            "   →← ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled("expand/collapse", Style::default().fg(SUBTEXT0)),
        Span::styled(
            "   esc ",
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled("close", Style::default().fg(SUBTEXT0)),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(hints).style(Style::default().bg(MANTLE))),
        area,
    );
}
