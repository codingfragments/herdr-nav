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

use std::time::Instant;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::dirnav::DirNavView;
use crate::nav::{Kind, Tree, Twisty};
use crate::search::{Leaf, SearchView};
use crate::source;
use crate::theme::Palette;

// ── Resolved colours (from the active Palette, spec §9 amended) ──────────────
//
// The palette is auto-followed from Herdr's `[theme]` setting (see
// `theme.rs`). These aliases are derived once per draw from the
// palette so the rest of the module reads like the original consts.

/// Resolved colour set — built from the active `Palette` once per
/// `draw` call, passed by reference to the helpers.
struct Colors {
    mantle: Color,
    surface0: Color,
    surface2: Color,
    text: Color,
    subtext0: Color,
    mauve: Color,
    red: Color,
    peach: Color,
}

impl Colors {
    fn from(p: &Palette) -> Self {
        Self {
            mantle: p.panel_bg,
            surface0: p.selection_bg,
            surface2: p.overlay0,
            text: p.text,
            subtext0: p.subtext0,
            mauve: p.mauve,
            red: p.red,
            peach: p.peach,
        }
    }
}

/// Kind glyph (spec §9).
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

/// Kind colour (spec §9), derived from the active palette.
fn kind_color(p: &Palette, kind: Kind) -> Color {
    crate::theme::kind_color(p, kind)
}

/// Draw the whole popup (spec §2): four bands inside a bordered frame,
/// with surface2 horizontal rules separating the bars from the body.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn draw(
    frame: &mut Frame,
    tree: &Tree,
    haystack: &[Leaf],
    search: Option<&SearchView>,
    socket_path: &str,
    last_change: Option<Instant>,
    flash_error: Option<&(String, String)>,
    name_prompt: Option<(&str, &str)>,
    template_picker: Option<(&[source::Template], usize)>,
    templates_exist: bool,
    plugin_action_picker: Option<(&str, &[(String, String)], usize)>,
    kill_confirm: Option<(&str, &str)>,
    palette: &Palette,
    help_open: bool,
    extend_hint: bool,
    dirnav: Option<&DirNavView>,
) {
    let area = frame.area();
    let c = Colors::from(palette);

    // The cursor node's kind, for the dynamic footer (spec §8: the Enter
    // hint names the action Enter will perform). In browse, the tree
    // cursor row; in search, the search cursor leaf; in DirNav, always
    // a directory (Phase 17 — the listing is dirs + dir-symlinks only).
    let cursor_kind = if dirnav.is_some() {
        Some(Kind::Dir)
    } else {
        match search {
            Some(v) => v.cursor_leaf(haystack).map(|l| l.kind),
            None => tree
                .cursor_row()
                .and_then(|r| tree.node_at(&r.path))
                .map(|n| n.kind),
        }
    };

    let bands = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search bar
            Constraint::Length(1), // ─ rule
            Constraint::Min(1),    // body
            Constraint::Length(1), // ─ rule
            Constraint::Length(1), // footer
        ])
        .split(area);

    draw_search_bar(frame, bands[0], search, dirnav, name_prompt, &c);
    draw_rule(frame, bands[1], &c);
    draw_body(
        frame,
        bands[2],
        tree,
        haystack,
        search,
        dirnav,
        socket_path,
        last_change,
        flash_error,
        palette,
        &c,
    );
    draw_rule(frame, bands[3], &c);
    // When the name-prompt dialog is active, suppress the footer —
    // the dialog carries its own ⏎/esc hints, so the overall footer
    // would duplicate them.
    if name_prompt.is_none()
        && template_picker.is_none()
        && plugin_action_picker.is_none()
        && !help_open
    {
        draw_footer(
            frame,
            bands[4],
            search,
            cursor_kind,
            name_prompt,
            templates_exist,
            kill_confirm,
            extend_hint,
            dirnav.is_some(),
            &c,
        );
    }

    // Name-prompt dialog overlay (spec §8.2 amended): a centered
    // bordered dialog on top of everything, asking for the workspace
    // name. Rendered last so it sits above all bands.
    if let Some((label, name)) = name_prompt {
        draw_name_prompt(frame, area, label, name, &c);
    }

    // Template-picker overlay (spec §8.4): a centered bordered
    // dialog listing templates. Rendered last so it sits above all.
    if let Some((templates, cursor)) = template_picker {
        draw_template_picker(frame, area, templates, cursor, &c);
    }

    // Plugin-action-picker overlay (spec §8.3): a centered
    // bordered dialog listing a plugin's declared actions.
    // Rendered last so it sits above all.
    if let Some((plugin_id, actions, cursor)) = plugin_action_picker {
        draw_plugin_action_picker(frame, area, plugin_id, actions, cursor, &c);
    }

    // Help dialog overlay (spec §13): `?` opens a centered
    // overlay with the full keymap + query-filter syntax.
    if help_open {
        draw_help_dialog(frame, area, &c);
    }
}

/// A thin full-width surface2 horizontal rule via a ratatui top
/// border (spec §2 band separation). A 1-row `Block` with
/// `Borders::TOP` — the border line occupies the row, no fill.
fn draw_rule(frame: &mut Frame, area: Rect, c: &Colors) {
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(c.surface2)),
        area,
    );
}

/// Split a display path into `(prefix, direct_parent)` for the
/// DirNav search bar: the last segment (direct parent) is rendered
/// brighter than the prefix. `prefix` includes the trailing slash so
/// it concatenates cleanly. For a bare segment (no slash) the prefix is
/// empty.
fn split_direct_parent(path: &str) -> (String, String) {
    match path.rfind('/') {
        Some(i) => (path[..=i].to_string(), path[i + 1..].to_string()),
        None => (String::new(), path.to_string()),
    }
}

fn draw_search_bar(
    frame: &mut Frame,
    area: Rect,
    search: Option<&SearchView>,
    dirnav: Option<&DirNavView>,
    _name_prompt: Option<(&str, &str)>,
    c: &Colors,
) {
    let prompt = Span::styled(
        "❯ ",
        Style::default().fg(c.mauve).add_modifier(Modifier::BOLD),
    );
    // Phase 18 DirNav: the search bar shows the cwd as a breadcrumb
    // path (direct parent kept full, earlier segments shortened) plus
    // the in-level query. The path is the "where am I" context; the
    // query is the filter.
    if let Some(d) = dirnav {
        let width = area.width as usize;
        let query_len = d.query.chars().count();
        // Budget: "❯ " (2) + path + ("  " gap + query + " ▮" caret) when
        // searching, else path + " ▮".
        let path_budget = if d.query.is_empty() {
            width.saturating_sub(3)
        } else {
            width.saturating_sub(2 + 2 + query_len + 1)
        };
        let path_disp = crate::dirnav::display_path(&d.cwd, path_budget);
        // Split the display path into the direct parent (last segment,
        // brighter) and the prefix (dimmed).
        let (prefix, parent) = split_direct_parent(&path_disp);
        let mut spans = vec![
            prompt,
            Span::styled(prefix, Style::default().fg(c.surface2)),
            Span::styled(parent, Style::default().fg(c.subtext0)),
        ];
        if d.query.is_empty() {
            spans.push(Span::styled(" ▮", Style::default().fg(c.mauve)));
        } else {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(d.query.clone(), Style::default().fg(c.text)));
            spans.push(Span::styled("▮", Style::default().fg(c.mauve)));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans).style(Style::default().bg(c.mantle))),
            area,
        );
        return;
    }
    let line = match search {
        Some(v) => Line::from(vec![
            prompt,
            Span::raw(v.query.clone()),
            Span::styled("▮", Style::default().fg(c.mauve)),
        ])
        .style(Style::default().bg(c.mantle)),
        None => Line::from(vec![
            prompt,
            Span::styled("type to search…", Style::default().fg(c.surface2)),
        ])
        .style(Style::default().bg(c.mantle)),
    };
    frame.render_widget(Paragraph::new(line), area);
}

#[allow(clippy::too_many_arguments)]
fn draw_body(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    haystack: &[Leaf],
    search: Option<&SearchView>,
    dirnav: Option<&DirNavView>,
    socket_path: &str,
    last_change: Option<Instant>,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) {
    // Phase 17 DirNav: the body becomes a single-column directory walker.
    // The preview pane reuses the existing dir preview for the selected
    // entry (built from a synthetic Kind::Dir node).
    if let Some(d) = dirnav {
        draw_dirnav_body(frame, area, d, socket_path, last_change, palette, c);
        return;
    }
    // Body split: list 44% · vertical rule · preview 56% (spec §2).
    // Below 60 cols the preview is dropped and the list takes the full
    // width (spec §2; the toggle key lands in Phase 9).
    if area.width < 60 {
        draw_list_or_search(frame, area, tree, haystack, search, flash_error, palette, c);
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

    draw_list_or_search(
        frame,
        split[0],
        tree,
        haystack,
        search,
        flash_error,
        palette,
        c,
    );
    // Vertical rule — a thin `│` line via a ratatui LEFT border on
    // the 1-cell column (spec §2), not a filled surface2 bar.
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(c.surface2)),
        split[1],
    );
    // Preview for the cursor node (spec §7). In browse, the tree
    // cursor's path; in search, the search cursor leaf's path.
    let node = match search {
        Some(v) => v
            .cursor_leaf(haystack)
            .and_then(|l| tree.node_at(&l.path))
            .map(std::borrow::Cow::Borrowed),
        None => tree
            .cursor_row()
            .and_then(|r| tree.node_at(&r.path))
            .map(std::borrow::Cow::Borrowed),
    };
    crate::preview::draw(
        frame,
        split[2],
        node.as_deref(),
        socket_path,
        last_change,
        palette,
    );
}

/// Dispatch to the browse tree list or the search flat list.
#[allow(clippy::too_many_arguments)]
fn draw_list_or_search(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    haystack: &[Leaf],
    search: Option<&SearchView>,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) {
    match search {
        Some(v) => draw_search_list(frame, area, haystack, v, flash_error, palette, c),
        None => draw_list(frame, area, tree, flash_error, palette, c),
    }
}

/// Phase 17 DirNav body: a single-column directory listing (dirs +
/// dir-symlinks only) with a status strip, plus the preview pane showing
/// the selected directory's contents (reusing the existing dir preview
/// via a synthetic `Kind::Dir` node). Below 60 cols the preview is
/// dropped, mirroring `draw_body`.
#[allow(clippy::too_many_arguments)]
fn draw_dirnav_body(
    frame: &mut Frame,
    area: Rect,
    d: &DirNavView,
    socket_path: &str,
    last_change: Option<Instant>,
    palette: &Palette,
    c: &Colors,
) {
    if area.width < 60 {
        draw_dirnav_list(frame, area, d, palette, c);
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
    draw_dirnav_list(frame, split[0], d, palette, c);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(c.surface2)),
        split[1],
    );
    // Preview for the selected dir: build a synthetic Kind::Dir node so
    // the existing `dir_preview` resolver (which reads the path from
    // `node.id` after the colon) works unchanged.
    let node = d.cursor_entry().map(|e| crate::nav::Node {
        id: format!("dirnav:{}", e.path.display()),
        kind: Kind::Dir,
        label: e.name.clone(),
        meta: String::new(),
        crumbs: None,
        children: Vec::new(),
        preview: crate::nav::Preview::default(),
        actions: crate::nav::Actions::default(),
    });
    crate::preview::draw(
        frame,
        split[2],
        node.as_ref(),
        socket_path,
        last_change,
        palette,
    );
}

/// The DirNav single-column listing + status strip (Phase 17 + 18).
/// One row per visible entry: dir glyph (or link glyph for symlinks),
/// name (with matched chars highlighted peach+bold when searching), and
/// a right-aligned meta (`<dir>` or `→ target`). The selected row gets
/// the surface0 background + a 2px kind-colour left bar, matching the
/// main list's selection grammar (spec §10).
///
/// Phase 18: when `query` is non-empty, the list narrows to the ranked
/// matches (`d.matches`); `↑↓` wrap within that set. The status strip
/// shows `matches/total` instead of `cursor/entries`.
fn draw_dirnav_list(frame: &mut Frame, area: Rect, d: &DirNavView, palette: &Palette, c: &Colors) {
    let h = area.height as usize;
    let (list_area, strip_area) = if h > 1 {
        let s = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        (s[0], s[1])
    } else {
        (area, Rect::ZERO)
    };

    // Build the visible rows: when searching, the match set (ranked);
    // otherwise all entries. Each row carries its entry index, the
    // entry, optional match positions, and whether it's the cursor.
    let searching = !d.query.is_empty();
    let total = if searching {
        d.matches.len()
    } else {
        d.entries.len()
    };

    let list_h = list_area.height as usize;
    let mut start = d.scroll;
    if list_h > 0 {
        if d.cursor < start {
            start = d.cursor;
        } else if d.cursor >= start + list_h {
            start = d.cursor + 1 - list_h;
        }
    }
    let start = start.min(total.saturating_sub(1));

    let dir_color = crate::theme::kind_color(palette, Kind::Dir);
    let visible: Vec<Line> = (start..total)
        .take(list_h)
        .filter_map(|row| {
            let (entry_idx, indices) = if searching {
                let m = d.matches.get(row)?;
                (m.entry_idx, Some(m.indices.as_slice()))
            } else {
                (row, None)
            };
            let entry = d.entries.get(entry_idx)?;
            Some(draw_dirnav_row(
                entry,
                row == d.cursor,
                indices,
                dir_color,
                c,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(visible), list_area);

    // Status strip: `dirnav · <basename(cwd)>` left, `matches/total` right
    // (Phase 18: when searching, show match count vs entry count).
    let cwd_label = d
        .cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| d.cwd.display().to_string());
    let scope = Span::styled(
        format!(" dirnav · {cwd_label} "),
        Style::default().fg(c.surface2),
    );
    let pos = if searching {
        Span::styled(
            format!(" {}/{} ", d.matches.len(), d.entries.len()),
            Style::default().fg(c.subtext0),
        )
    } else {
        Span::styled(
            format!(
                " {}/{} ",
                d.cursor.saturating_add(1).min(d.entries.len()),
                d.entries.len()
            ),
            Style::default().fg(c.subtext0),
        )
    };
    frame.render_widget(
        Paragraph::new(
            Line::from(vec![scope, Span::raw(""), pos]).style(Style::default().bg(c.mantle)),
        ),
        strip_area,
    );
}

/// One DirNav row (Phase 17 + 18): glyph + name (left, with matched
/// chars peach+bold when `indices` is Some), meta (right).
fn draw_dirnav_row(
    e: &crate::dirnav::DirEntry,
    selected: bool,
    indices: Option<&[u32]>,
    dir_color: Color,
    c: &Colors,
) -> Line<'static> {
    let glyph = if e.is_symlink { '↪' } else { '▤' };
    let label_color = if selected { c.text } else { c.subtext0 };
    let bar = if selected {
        Span::styled(" ", Style::default().bg(dir_color))
    } else {
        Span::raw(" ")
    };
    let meta = if e.is_symlink {
        // Show the resolved target for symlinks.
        let target = std::fs::canonicalize(&e.path)
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        format!("→ {} ", target)
    } else {
        "<dir> ".to_string()
    };

    // Build the name spans: matched chars peach+bold, the rest in the
    // label colour. Coalesce adjacent chars of the same style into runs.
    let matched: std::collections::HashSet<u32> = indices
        .map(|idx| idx.iter().copied().collect())
        .unwrap_or_default();
    let name_spans = build_name_spans(&e.name, &matched, label_color, c.peach);

    let mut spans = vec![
        bar,
        Span::styled(
            format!("{glyph} "),
            Style::default().fg(dir_color).add_modifier(Modifier::BOLD),
        ),
    ];
    spans.extend(name_spans);
    spans.push(Span::raw(" "));
    spans.push(Span::styled(meta, Style::default().fg(c.surface2)));
    Line::from(spans)
}

/// Coalesce a name into runs: matched chars peach+bold, unmatched chars
/// in `label_color`. Mirrors the main search row's run coalescing
/// (spec §6.4) but over a bare name (no crumb prefix).
fn build_name_spans(
    name: &str,
    matched: &std::collections::HashSet<u32>,
    label_color: Color,
    match_color: Color,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut run = String::new();
    let mut run_matched = chars.first().is_some_and(|_| matched.contains(&0));
    for (i, ch) in chars.iter().enumerate() {
        let is_m = matched.contains(&(i as u32));
        if i == 0 {
            run.push(*ch);
            run_matched = is_m;
            continue;
        }
        if is_m == run_matched {
            run.push(*ch);
        } else {
            out.push(Span::styled(
                std::mem::take(&mut run),
                if run_matched {
                    Style::default()
                        .fg(match_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(label_color)
                },
            ));
            run.push(*ch);
            run_matched = is_m;
        }
    }
    if !run.is_empty() {
        out.push(Span::styled(
            run,
            if run_matched {
                Style::default()
                    .fg(match_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(label_color)
            },
        ));
    }
    out
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) {
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
        .map(|(i, row)| draw_row(i, row, i == tree.cursor, flash_error, palette, c))
        .collect();

    // No bg on the list paragraph — transparent so the terminal theme
    // shows through; selected rows carry their own c.surface0 bg.
    frame.render_widget(Paragraph::new(visible), list_area);

    // Status strip: scope left, position right (spec §2). A bar → mantle.
    let scope = Span::styled(" tree · target groups ", Style::default().fg(c.surface2));
    let pos = Span::styled(
        format!(
            " {}/{} ",
            tree.cursor.saturating_add(1).min(rows.len()),
            rows.len()
        ),
        Style::default().fg(c.subtext0),
    );
    frame.render_widget(
        Paragraph::new(
            Line::from(vec![scope, Span::raw(""), pos]).style(Style::default().bg(c.mantle)),
        ),
        strip_area,
    );
}

/// Search-mode flat list (spec §3.2/§6.4): one row per matched leaf,
/// breadcrumb prefix dimmed, matched chars peach+bold, label subtext0
/// (or text on the selected row). Status strip: `flat leaves ·
/// fuzzy` + `matches/total`.
fn draw_search_list(
    frame: &mut Frame,
    area: Rect,
    haystack: &[Leaf],
    view: &SearchView,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) {
    let h = area.height as usize;
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
    let mut start = 0;
    if list_h > 0 && view.cursor >= list_h {
        start = view.cursor + 1 - list_h;
    }

    let visible: Vec<Line> = if view.matches.is_empty() {
        // No matches (spec §11): one dim centred line.
        vec![Line::styled(
            format!(" no targets match \"{}\" ", view.parsed.needle),
            Style::default().fg(c.surface2),
        )]
    } else {
        view.matches
            .iter()
            .enumerate()
            .skip(start)
            .take(list_h)
            .map(|(i, m)| {
                let leaf = &haystack[m.index];
                draw_search_row(leaf, m, i == view.cursor, flash_error, palette, c)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(visible), list_area);

    // Status strip: active filters left, matches/total right (spec §2/§15).
    // Phase 11: shows the parsed filter label (e.g. "agents · pane · !zox")
    // or "flat leaves · fuzzy" when no filters are active.
    let scope_text = if view.parsed.has_filters() {
        format!(" {} · fuzzy ", view.parsed.status_label())
    } else {
        " flat leaves · fuzzy ".to_string()
    };
    let scope = Span::styled(scope_text, Style::default().fg(c.surface2));
    let pos = Span::styled(
        format!(" {}/{} ", view.matches.len(), haystack.len()),
        Style::default().fg(c.subtext0),
    );
    frame.render_widget(
        Paragraph::new(
            Line::from(vec![scope, Span::raw(""), pos]).style(Style::default().bg(c.mantle)),
        ),
        strip_area,
    );
}

/// One search row (spec §3.2/§6.4/§10): breadcrumb prefix dimmed,
/// matched chars peach+bold, label subtext0 (or text on selected).
/// Match indices are character positions in `leaf.match_text`.
fn draw_search_row(
    leaf: &Leaf,
    m: &crate::search::ScoredMatch,
    selected: bool,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) -> Line<'static> {
    let is_error = flash_error.is_some_and(|(id, _)| id == &leaf.id);
    let glyph = kind_glyph(leaf.kind);
    let glyph_color = kind_color(palette, leaf.kind);

    let label_color = if is_error {
        c.red
    } else if selected {
        c.text
    } else {
        c.subtext0
    };
    let bar = if selected {
        Span::styled(" ", Style::default().bg(glyph_color))
    } else {
        Span::raw(" ")
    };

    // Build the match-text runs: crumb prefix (dimmed surface2) +
    // label (subtext0/text), with matched chars overlaid peach+bold.
    let matched: std::collections::HashSet<u32> = m.indices.iter().copied().collect();
    let chars: Vec<char> = leaf.match_text.chars().collect();
    let crumb_end = leaf.crumb_prefix_len;

    let mut spans: Vec<Span<'static>> = vec![
        bar,
        Span::styled(
            format!("{glyph} "),
            Style::default()
                .fg(glyph_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let mut run = String::new();
    let mut run_style = Style::default();
    let flush = |run: &mut String, style: Style, spans: &mut Vec<Span<'static>>| {
        if !run.is_empty() {
            spans.push(Span::styled(std::mem::take(run), style));
        }
    };
    for (i, ch) in chars.iter().enumerate() {
        let is_matched = matched.contains(&(i as u32));
        let is_crumb = i < crumb_end;
        let style = if is_matched {
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD)
        } else if is_crumb {
            Style::default().fg(c.surface2)
        } else {
            Style::default().fg(label_color)
        };
        // Coalesce: if same style as the current run, append; else flush.
        if run_style != style {
            flush(&mut run, run_style, &mut spans);
            run_style = style;
        }
        run.push(*ch);
    }
    flush(&mut run, run_style, &mut spans);

    let line_style = if selected {
        Style::default().bg(c.surface0)
    } else {
        Style::default()
    };
    Line::from(spans).style(line_style)
}

fn draw_row(
    _i: usize,
    row: &crate::nav::VisibleRow,
    selected: bool,
    flash_error: Option<&(String, String)>,
    palette: &Palette,
    c: &Colors,
) -> Line<'static> {
    let indent = "  ".repeat(row.depth);
    let twisty = match row.twisty {
        Twisty::Expanded => "▾ ",
        Twisty::Closed => "▸ ",
        Twisty::Leaf => "  ",
    };
    let glyph = kind_glyph(row.kind);
    let glyph_color = kind_color(palette, row.kind);

    // Error flash (spec §11): a row whose Enter failed flashes red.
    let is_error = flash_error.is_some_and(|(id, _)| id == &row.id);
    let label_color = if is_error {
        c.red
    } else if selected {
        c.text
    } else {
        c.subtext0
    };

    // Selection: a 2px left bar in the row's kind colour (spec §9). In a
    // cell TUI we render the first cell as a kind-coloured background
    // (a space with bg=kind_color), which reads as a solid left bar.
    let is_hint = row.id.ends_with(":hint");
    let dim_style = if is_hint {
        Style::default().fg(c.surface2)
    } else {
        Style::default()
    };
    let bar = if selected {
        Span::styled(" ", Style::default().bg(glyph_color))
    } else {
        Span::raw(" ")
    };

    let mut spans = vec![bar, Span::raw(indent)];
    spans.push(Span::styled(
        format!("{twisty}{glyph} "),
        if is_hint {
            dim_style
        } else {
            Style::default()
                .fg(glyph_color)
                .add_modifier(Modifier::BOLD)
        },
    ));
    spans.push(Span::styled(
        row.label.clone(),
        if is_hint {
            dim_style
        } else {
            Style::default().fg(label_color)
        },
    ));
    if !row.meta.is_empty() {
        spans.push(Span::raw("  "));
        let meta_color = if row.meta == "unavailable" || row.meta == "empty" {
            c.red
        } else {
            c.surface2
        };
        spans.push(Span::styled(
            row.meta.clone(),
            Style::default().fg(meta_color),
        ));
    }
    // Line-level bg only when selected (c.surface0); non-selected rows are
    // transparent so the terminal theme shows through.
    let line_style = if selected {
        Style::default().bg(c.surface0)
    } else {
        Style::default()
    };
    Line::from(spans).style(line_style)
}

#[allow(clippy::too_many_arguments)]
fn draw_footer(
    frame: &mut Frame,
    area: Rect,
    search: Option<&SearchView>,
    cursor_kind: Option<Kind>,
    name_prompt: Option<(&str, &str)>,
    templates_exist: bool,
    kill_confirm: Option<(&str, &str)>,
    extend_hint: bool,
    is_dirnav: bool,
    c: &Colors,
) {
    // Kill confirm active: show the inline confirm prompt (spec §8).
    if let Some((_id, label)) = kill_confirm {
        let line = Line::from(vec![
            Span::styled(
                " ⏎ ",
                Style::default().fg(c.red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("kill {label}? ^d confirm · any key cancel"),
                Style::default().fg(c.peach),
            ),
        ]);
        frame.render_widget(line, area);
        return;
    }
    // Phase 17 DirNav footer: a dedicated hint row for the directory
    // walker. Enter/^t/^p land in Phase 19; here we show navigation +
    // help + back.
    if is_dirnav {
        let ks = Style::default().fg(c.peach).add_modifier(Modifier::BOLD);
        let ds = Style::default().fg(c.subtext0);
        let line = Line::from(vec![
            Span::styled(" ↑↓ ", ks),
            Span::styled("move   ", ds),
            Span::styled("← ", ks),
            Span::styled("up   ", ds),
            Span::styled("→ ", ks),
            Span::styled("in   ", ds),
            Span::styled("? ", ks),
            Span::styled("help   ", ds),
            Span::styled("esc ", ks),
            Span::styled("back", ds),
        ])
        .style(Style::default().bg(c.mantle));
        frame.render_widget(line, area);
        return;
    }
    // Name prompt active: confirm/cancel hints.
    let (enter_hint, esc_hint) = if name_prompt.is_some() {
        ("create workspace", "cancel")
    } else {
        match search {
            Some(_) => (enter_action_label(cursor_kind, true), "clear"),
            None => (enter_action_label(cursor_kind, false), "close"),
        }
    };
    let mut hints = vec![
        Span::styled(
            " ⏎ ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ),
        Span::styled(enter_hint, Style::default().fg(c.subtext0)),
        Span::styled(
            "   ↑↓ ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ),
        Span::styled("move", Style::default().fg(c.subtext0)),
        Span::styled(
            "   esc ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ),
        Span::styled(esc_hint, Style::default().fg(c.subtext0)),
    ];
    // ^t hint (spec §8.4): show only when templates exist AND the
    // cursor is on a dir/zox (the kinds that support templates).
    // Omitted when no templates/ dir — the key is unbound.
    let is_dir = matches!(cursor_kind, Some(Kind::Dir) | Some(Kind::Zox));
    if templates_exist && is_dir && name_prompt.is_none() {
        hints.push(Span::styled(
            "   ^t ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::styled("template", Style::default().fg(c.subtext0)));
    }
    // Side-action hints (spec §8): ^p pin, ^d kill, ^r/^c/^x alternates.
    // Shown per kind, only in browse mode (search mode keeps the footer
    // minimal — the query is the focus).
    if search.is_none() && name_prompt.is_none() {
        match cursor_kind {
            Some(Kind::Dir) => {
                hints.push(Span::styled(
                    "   ^p ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("pin   ", Style::default().fg(c.subtext0)));
                hints.push(Span::styled(
                    "^u ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("unpin", Style::default().fg(c.subtext0)));
            }
            Some(Kind::Zox) => {
                hints.push(Span::styled(
                    "   ^p ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("pin", Style::default().fg(c.subtext0)));
            }
            Some(Kind::Pane) => {
                hints.push(Span::styled(
                    "   ^p ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("pin   ", Style::default().fg(c.subtext0)));
                hints.push(Span::styled(
                    "^d ",
                    Style::default().fg(c.red).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("kill   ", Style::default().fg(c.subtext0)));
                hints.push(Span::styled(
                    "^r ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("interrupt", Style::default().fg(c.subtext0)));
            }
            Some(Kind::Agent) => {
                hints.push(Span::styled(
                    "   ^c ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled(
                    "interrupt   ",
                    Style::default().fg(c.subtext0),
                ));
                hints.push(Span::styled(
                    "^x ",
                    Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("detach", Style::default().fg(c.subtext0)));
            }
            Some(Kind::Workspace) | Some(Kind::Tab) => {
                hints.push(Span::styled(
                    "   ^d ",
                    Style::default().fg(c.red).add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled("kill", Style::default().fg(c.subtext0)));
            }
            _ => {}
        }
    }
    // `Tab extend` hint (Phase 16): shown only in search mode when the
    // match list has no Dir/Zox leaves and zoxide has not yet been
    // extended this invocation. Pressing `Tab` re-runs zoxide against a
    // much larger limit so deeper frecency dirs surface.
    if extend_hint && search.is_some() && name_prompt.is_none() {
        hints.push(Span::styled(
            "   ⇥ ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ));
        hints.push(Span::styled(
            "extend zoxide",
            Style::default().fg(c.subtext0),
        ));
    }
    // `?` help hint (spec §13): always shown, both modes.
    hints.push(Span::styled(
        "   ? ",
        Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
    ));
    hints.push(Span::styled("help", Style::default().fg(c.subtext0)));
    frame.render_widget(
        Paragraph::new(Line::from(hints).style(Style::default().bg(c.mantle))),
        area,
    );
}

/// The Enter action label for a kind (spec §8.2). In browse, branches
/// expand/step; leaves run their default action. In search, every
/// row is a leaf so it's always the default action.
fn enter_action_label(kind: Option<Kind>, is_search: bool) -> &'static str {
    match kind {
        Some(Kind::Pane) => "jump to pane",
        Some(Kind::Agent) => "jump to agent",
        Some(Kind::Dir) | Some(Kind::Zox) => "open workspace",
        Some(Kind::Plugin) => "open actions",
        // Branches (group/workspace/tab) in browse → expand/step.
        // In search there are no branches, so None falls through to
        // the generic leaf action.
        _ if !is_search => "expand",
        _ => "run action",
    }
}

/// Centered name-prompt dialog (spec §8.2 amended). A bordered
/// dialog on top of the popup, asking for the new workspace's
/// name. Prefilled with the default; Enter confirms, Esc cancels.
fn draw_name_prompt(frame: &mut Frame, area: Rect, _label: &str, name: &str, c: &Colors) {
    // Center a ~50-wide, 5-row dialog: title border, label, name,
    // hints, bottom border. Don't clear the whole popup — render
    // the dialog on top of the bands so the popup stays visible
    // behind it; the dialog's solid mantle bg makes it readable.
    let w = 50u16;
    let h = 5u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w.min(area.width), h.min(area.height));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c.surface2))
        .title(Span::styled(
            " Open workspace ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(c.mantle));
    let inner = block.inner(dialog);
    // Clear only the dialog rect (so the bands behind the dialog
    // don't bleed through the dialog's content), not the whole
    // popup — the popup stays visible around the dialog.
    Clear.render(dialog, frame.buffer_mut());
    block.render(dialog, frame.buffer_mut());

    // Row 0: explanatory label.
    let label_line = Line::styled(" Name the new workspace:", Style::default().fg(c.subtext0))
        .style(Style::default().bg(c.mantle));

    // Row 1: the editable name + caret.
    let name_line = Line::from(vec![
        Span::styled(
            " ❯ ",
            Style::default().fg(c.mauve).add_modifier(Modifier::BOLD),
        ),
        Span::raw(name.to_string()),
        Span::styled("▮", Style::default().fg(c.mauve)),
    ])
    .style(Style::default().bg(c.mantle));

    // Row 2: hints inside the dialog.
    let hint = Line::from(vec![
        Span::styled(
            " ⏎ ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ),
        Span::styled("create   ", Style::default().fg(c.subtext0)),
        Span::styled(
            "esc ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ),
        Span::styled("cancel", Style::default().fg(c.subtext0)),
    ])
    .style(Style::default().bg(c.mantle));

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(Paragraph::new(label_line), body[0]);
    frame.render_widget(Paragraph::new(name_line), body[1]);
    frame.render_widget(Paragraph::new(hint), body[2]);
}

/// Centered template-picker dialog (spec §8.4). Lists the
/// configured templates with the cursor highlighted; Enter builds,
/// Esc returns. Sized to fit the template count.
fn draw_template_picker(
    frame: &mut Frame,
    area: Rect,
    templates: &[source::Template],
    cursor: usize,
    c: &Colors,
) {
    let n = templates.len() as u16;
    // title border + one row per template + hint row + bottom border.
    let h = (n + 3).min(area.height);
    let w = 44u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w.min(area.width), h.min(area.height));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c.surface2))
        .title(Span::styled(
            " Open with template ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(c.mantle));
    let inner = block.inner(dialog);
    Clear.render(dialog, frame.buffer_mut());
    block.render(dialog, frame.buffer_mut());

    let mut rows: Vec<Line> = Vec::new();
    for (i, t) in templates.iter().enumerate() {
        let selected = i == cursor;
        let mark = if selected { "▸ " } else { "  " };
        let default_tag = if t.default { " (default)" } else { "" };
        let style = if selected {
            Style::default().fg(c.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(c.subtext0)
        };
        rows.push(
            Line::from(vec![
                Span::styled(mark.to_string(), style),
                Span::styled(format!("{}{}", t.name, default_tag), style),
            ])
            .style(if selected {
                Style::default().bg(c.surface0)
            } else {
                Style::default().bg(c.mantle)
            }),
        );
    }
    rows.push(Line::raw(""));
    rows.push(
        Line::from(vec![
            Span::styled(
                " ⏎ ",
                Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
            ),
            Span::styled("build   ", Style::default().fg(c.subtext0)),
            Span::styled(
                "esc ",
                Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
            ),
            Span::styled("back", Style::default().fg(c.subtext0)),
        ])
        .style(Style::default().bg(c.mantle)),
    );

    frame.render_widget(Paragraph::new(rows), inner);
}

/// Centered help dialog (spec §13): `?` opens an overlay with the
/// full keymap + query-filter syntax summary. Esc closes.
fn draw_help_dialog(frame: &mut Frame, area: Rect, c: &Colors) {
    let w = 56u16;
    let h = 21u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w.min(area.width), h.min(area.height));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c.surface2))
        .title(Span::styled(
            " herdr-nav · help ",
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(c.mantle));
    let inner = block.inner(dialog);
    Clear.render(dialog, frame.buffer_mut());
    block.render(dialog, frame.buffer_mut());

    let ks = Style::default().fg(c.peach).add_modifier(Modifier::BOLD);
    let ds = Style::default().fg(c.subtext0);
    let sep = Span::raw("  ");

    let rows = vec![
        Line::from(vec![
            Span::styled("↑↓", ks),
            sep.clone(),
            Span::styled("move cursor (wraps)", ds),
        ]),
        Line::from(vec![
            Span::styled("→/Space/Tab", ks),
            sep.clone(),
            Span::styled("expand / step into", ds),
        ]),
        Line::from(vec![
            Span::styled("←", ks),
            sep.clone(),
            Span::styled("collapse / jump to parent", ds),
        ]),
        Line::from(vec![
            Span::styled("Enter", ks),
            sep.clone(),
            Span::styled("default action (jump / open / pick)", ds),
        ]),
        Line::from(vec![
            Span::styled("Esc", ks),
            sep.clone(),
            Span::styled("close / clear query (two-stage)", ds),
        ]),
        Line::from(vec![
            Span::styled("Tab", ks),
            sep.clone(),
            Span::styled("extend zoxide (search, no dir hits)", ds),
        ]),
        Line::from(vec![
            Span::styled("^f", ks),
            sep.clone(),
            Span::styled("directory navigation mode (DirNav)", ds),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("in DirNav:", ds),
            Span::raw("  "),
            Span::styled("type", ks),
            Span::raw(" "),
            Span::styled("fuzzy-search this level", ds),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("^p", ks),
            sep.clone(),
            Span::styled("pin selected dir or pane cwd", ds),
        ]),
        Line::from(vec![
            Span::styled("^u", ks),
            sep.clone(),
            Span::styled("unpin selected pinned dir", ds),
        ]),
        Line::from(vec![
            Span::styled("^d", ks),
            sep.clone(),
            Span::styled("kill pane / tab / workspace (confirm)", ds),
        ]),
        Line::from(vec![
            Span::styled("^t", ks),
            sep.clone(),
            Span::styled("open with template (dir/zox)", ds),
        ]),
        Line::from(vec![
            Span::styled("^r ^c ^x", ks),
            sep.clone(),
            Span::styled("interrupt / detach (pane/agent)", ds),
        ]),
        Line::raw(""),
        Line::from(vec![Span::styled("Query filters:", ds), Span::raw("  ")]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("agents nvim", ks),
            sep.clone(),
            Span::styled("group scope (leading)", ds),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("@pane", ks),
            sep.clone(),
            Span::styled("kind filter (kind:pane = @pane)", ds),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("@dir", ks),
            sep.clone(),
            Span::styled("union: pinned + zoxide", ds),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("!plugin", ks),
            sep.clone(),
            Span::styled("negation: exclude kind/group", ds),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("See doc/query-filters.md for full syntax.", ds),
            Span::raw("  "),
        ]),
    ];

    frame.render_widget(Paragraph::new(rows), inner);
}

/// Centered plugin-action-picker dialog (spec §8.3). Lists a
/// plugin's declared actions with the cursor highlighted; Enter runs,
/// Esc returns. Sized to fit the action count.
fn draw_plugin_action_picker(
    frame: &mut Frame,
    area: Rect,
    plugin_id: &str,
    actions: &[(String, String)],
    cursor: usize,
    c: &Colors,
) {
    let n = actions.len() as u16;
    let h = (n + 3).min(area.height);
    let w = 44u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w.min(area.width), h.min(area.height));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c.surface2))
        .title(Span::styled(
            format!(" {plugin_id} ▸ ACTION "),
            Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(c.mantle));
    let inner = block.inner(dialog);
    Clear.render(dialog, frame.buffer_mut());
    block.render(dialog, frame.buffer_mut());

    let mut rows: Vec<Line> = Vec::new();
    for (i, (aid, title)) in actions.iter().enumerate() {
        let selected = i == cursor;
        let mark = if selected { "▸ " } else { "  " };
        let style = if selected {
            Style::default().fg(c.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(c.subtext0)
        };
        let label = if selected { title.clone() } else { aid.clone() };
        rows.push(
            Line::from(vec![
                Span::styled(mark.to_string(), style),
                Span::styled(label, style),
            ])
            .style(if selected {
                Style::default().bg(c.surface0)
            } else {
                Style::default().bg(c.mantle)
            }),
        );
    }
    rows.push(Line::raw(""));
    rows.push(
        Line::from(vec![
            Span::styled(
                " ⏎ ",
                Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
            ),
            Span::styled("run   ", Style::default().fg(c.subtext0)),
            Span::styled(
                "esc ",
                Style::default().fg(c.peach).add_modifier(Modifier::BOLD),
            ),
            Span::styled("back", Style::default().fg(c.subtext0)),
        ])
        .style(Style::default().bg(c.mantle)),
    );

    frame.render_widget(Paragraph::new(rows), inner);
}
