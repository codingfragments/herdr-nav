//! `herdr-nav` — fuzzy navigation between panes, agents, directories, and
//! plugins, with a rich preview pane.
//!
//! The popup opens a real PTY (Herdr popup placement). This binary reads
//! the available navigation targets via the Herdr socket API, renders a
//! fuzzy-filterable list + preview with `ratatui` driving a `crossterm`
//! backend directly, and runs an event loop until `Esc` closes the popup
//! or `Enter` switches to the selected target.
//!
//! **Status: Phase 1 — popup shell + Session tree browse.**
//! The event loop renders the real tree and handles browse keys. Search
//! mode (Phase 4), per-kind preview (Phase 2), and the switch action
//! (Phase 3) land in later phases.

mod config;
mod dirnav;
mod nav;
mod preview;
mod query;
mod render;
mod search;
mod socket_client;
pub mod source;
mod theme;

use std::time::Duration;

use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Debounce window for identical consecutive key events.
///
/// Herdr's popup PTY runs in legacy keyboard mode, where crossterm can't
/// distinguish a genuine press from an OS key-repeat or a flaky
/// double-delivery — every key event arrives as `KeyEventKind::Press`.
/// A single tap can therefore produce two `Press` events and fire the
/// bound action twice. We skip an identical key event that arrives
/// within this window of the previous one. Different keys always pass.
/// Tuned to catch near-instant duplicate delivery without eating
/// deliberate fast double-taps (a human double-tap is ~100ms+ apart).
/// (Inherited contract from the sister herdr-flash / herdr-zextract
/// ports — confirmed live against Herdr 0.8.0.)
const KEY_DEBOUNCE: Duration = Duration::from_millis(40);

// ── Name prompt (dir/zox workspace creation) ────────────────────────────────

/// An inline prompt for naming a new workspace (spec §8.2 amended).
/// Prefilled with a good default derived from the path; the user
/// confirms (Enter) to create + enter, or cancels (Esc) to abort
/// without creating and stay in the popup. Carries the template to
/// build from: for `Enter` it's the auto-resolved default, for `^t`
/// it's the user-selected template (spec §8.4 amended — ^t now asks
/// for a name after template selection, so both keys share one build
/// path).
struct NamePrompt {
    /// The node id being acted on (`pinned:<path>` / `zox:<path>`).
    node_id: String,
    /// The expanded path (for the workspace.create call).
    path: String,
    /// The editable name (prefilled with the default).
    name: String,
    /// The template to build the workspace from.
    template: source::Template,
}

/// Template picker state (spec §8.4): `^t` on a dir/zox
/// opens a selector listing templates; ↑↓ moves, Enter builds,
/// Esc returns to the switcher.
struct TemplatePicker {
    /// The node id being acted on (`pinned:<path>` / `zox:<path>`).
    node_id: String,
    /// The expanded path.
    path: String,
    /// The workspace name (prefilled default).
    name: String,
    /// The available templates.
    templates: Vec<source::Template>,
    /// Cursor into `templates`.
    cursor: usize,
}

/// Plugin action picker state (spec §8.3): `Enter` on a plugin
/// opens a selector listing its declared actions; ↑↓ move,
/// Enter runs the action + closes, Esc returns.
struct PluginActionPicker {
    /// The plugin id (e.g. `herdr-flash`).
    plugin_id: String,
    /// The plugin's declared actions as (id, title) pairs.
    actions: Vec<(String, String)>,
    /// Cursor into `actions`.
    cursor: usize,
}

// ── Launch context ────────────────────────────────────────────────────────────

/// Launch context: which pane this popup was opened relative to.
struct LaunchContext {
    focused_pane_id: String,
}

/// Reads the launch context from `HERDR_PLUGIN_CONTEXT_JSON` (set by Herdr
/// for a real plugin-pane invocation). Falls back to `HERDR_ACTIVE_PANE_ID`
/// for manual dev-testing. (Same contract as herdr-flash / herdr-zextract.)
fn launch_context() -> Result<LaunchContext, String> {
    if let Ok(context_json) = std::env::var("HERDR_PLUGIN_CONTEXT_JSON") {
        let context: serde_json::Value = serde_json::from_str(&context_json)
            .map_err(|e| format!("invalid context JSON: {e}"))?;
        let focused_pane_id = context
            .get("focused_pane_id")
            .and_then(|v| v.as_str())
            .ok_or(
                "context JSON has no focused_pane_id (nothing was focused before this popup opened)",
            )?
            .to_string();
        return Ok(LaunchContext { focused_pane_id });
    }
    let focused_pane_id = std::env::var("HERDR_ACTIVE_PANE_ID").map_err(|_| {
        "neither HERDR_PLUGIN_CONTEXT_JSON nor HERDR_ACTIVE_PANE_ID is set".to_string()
    })?;
    Ok(LaunchContext { focused_pane_id })
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    // Launch context and socket path are best-effort in Phase 1: if Herdr
    // isn't running, the Session provider degrades to an "unavailable"
    // stub and the popup still opens (useful for dev). The real plugin
    // always has both set (see doc/env-vars.md).
    let ctx = launch_context().ok();
    let socket_path = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
    let config = config::Config::load();
    let group_order = config.resolved_groups();
    // Auto-follow Herdr's theme (spec §9 amended): read
    // ~/.config/herdr/config.toml, resolve the theme name, apply
    // [theme.custom] overrides. Falls back to catppuccin (Herdr's
    // default) if the file is missing or malformed.
    let palette = theme::load();

    let mut tree = nav::Tree::new(source::build_tree(&socket_path, &group_order, &config));

    // Terminal setup — same contract as the sister ports: enter raw mode,
    // hide the cursor, enter the alternate screen, then restore on exit.
    enable_raw_mode().map_err(|e| format!("enable_raw_mode: {e}"))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| format!("EnterAlternateScreen: {e}"))?;
    execute!(stdout, cursor::Hide).map_err(|e| format!("Hide cursor: {e}"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("Terminal::new: {e}"))?;

    let result = event_loop(
        &mut terminal,
        &mut tree,
        &socket_path,
        &palette,
        &group_order,
        &config,
        ctx.as_ref(),
    );

    // Restore terminal regardless of how the loop exited.
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), cursor::Show).ok();
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )
    .ok();
    result
}

/// Phase 1 event loop: browse the Session tree (spec §3.1/§8).
///
/// Browse only — the query bar is empty and printable characters are
/// inert (search mode lands in Phase 4). `↑↓`/`^n`/`^p` move the
/// cursor (wraps); `→`/`Space`/`Tab` expand or step to the first child;
/// `←` collapses or jumps to the parent; `Enter` toggles a branch
/// (inert on a leaf — the leaf default action lands in Phase 3); `Esc`
/// closes the popup.
fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    tree: &mut nav::Tree,
    socket_path: &str,
    palette: &theme::Palette,
    group_order: &[String],
    cfg: &config::Config,
    ctx: Option<&LaunchContext>,
) -> Result<(), String> {
    let mut last_key: Option<(event::KeyEvent, std::time::Instant)> = None;
    let mut last_cursor_change: Option<std::time::Instant> = None;
    let mut flash_error: Option<(String, String)> = None;

    // Name prompt for dir/zox workspace creation (spec §8.2 amended:
    // prompt the user to name the new workspace, prefill a default,
    // confirm = create + enter; cancel = don't create, stay open).
    // None = no prompt active; Some = prompt visible, editing the name.
    let mut name_prompt: Option<NamePrompt> = None;
    // Template picker (spec §8.4): `^t` on a dir/zox opens a
    // selector listing templates; Enter builds, Esc returns.
    let mut template_picker: Option<TemplatePicker> = None;
    // Plugin action picker (spec §8.3): Enter on a plugin opens
    // a selector listing its actions; ↑↓ move, Enter runs,
    // Esc returns.
    let mut plugin_action_picker: Option<PluginActionPicker> = None;
    // Kill confirm (spec §8: `^d`): first press shows an inline
    // footer confirm; second `^d` confirms, any other key cancels.
    let mut kill_confirm: Option<(String, String)> = None;
    // Help dialog (spec §13): `?` opens a centered overlay with the
    // full keymap + query-filter syntax summary.
    let mut help_open = false;

    // Haystack built once per invocation (spec §6.1): DFS, leaves
    // only, group order. Stable for the whole popup (providers
    // don't refresh mid-invocation in Phase 4).
    let mut haystack = search::build_haystack(tree);
    // Whether any templates exist (for the ^t footer hint, spec §8.4).
    let templates_exist = !source::read_templates().is_empty();
    // Search view: None = browse mode, Some = search mode (query non-empty).
    let mut search_view: Option<search::SearchView> = None;
    // Phase 16 "extend zoxide": once the user presses `Tab` in search
    // mode to extend the zoxide list beyond `zoxide_limit`, this flag
    // sticks for the rest of the invocation so we don't re-run the
    // subprocess on every keystroke. The haystack is rebuilt once with
    // the extended zox leaves and stays extended.
    let mut extended_zox = false;
    // Phase 17 DirNav mode: `None` = not active (browse/search as
    // before); `Some` = the body shows the directory walker. The
    // switcher's `Tree` + `search_view` are preserved off-screen so Esc
    // restores them exactly (expansion intact).
    let mut dirnav: Option<dirnav::DirNavView> = None;

    loop {
        tree.ensure_cursor_valid();
        let is_dirnav = dirnav.is_some();
        // Phase 16: the `Tab extend` hint shows in search mode when the
        // match list has no Dir/Zox leaves and zoxide hasn't been extended
        // yet this invocation. Suppressed while DirNav is active.
        let extend_hint = !is_dirnav
            && search_view.as_ref().is_some_and(|v| {
                !extended_zox && search::has_no_dir_matches(&haystack, &v.matches)
            });
        terminal
            .draw(|frame| {
                render::draw(
                    frame,
                    tree,
                    &haystack,
                    search_view.as_ref(),
                    socket_path,
                    last_cursor_change,
                    flash_error.as_ref(),
                    name_prompt
                        .as_ref()
                        .map(|p| (p.node_id.as_str(), p.name.as_str())),
                    template_picker
                        .as_ref()
                        .map(|p| (p.templates.as_slice(), p.cursor)),
                    templates_exist,
                    plugin_action_picker
                        .as_ref()
                        .map(|p| (p.plugin_id.as_str(), p.actions.as_slice(), p.cursor)),
                    kill_confirm
                        .as_ref()
                        .map(|(id, label)| (id.as_str(), label.as_str())),
                    palette,
                    help_open,
                    extend_hint,
                    dirnav.as_ref(),
                )
            })
            .map_err(|e| format!("draw: {e}"))?;

        if !event::poll(Duration::from_millis(250)).map_err(|e| format!("poll: {e}"))? {
            continue;
        }

        let CtEvent::Key(key) = event::read().map_err(|e| format!("read: {e}"))? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        // Debounce identical consecutive presses (legacy keyboard mode).
        if let Some((prev, ts)) = &last_key {
            if prev == &key && ts.elapsed() < KEY_DEBOUNCE {
                continue;
            }
        }
        last_key = Some((key, std::time::Instant::now()));

        let cursor_before = if let Some(d) = dirnav.as_ref() {
            d.cursor
        } else if let Some(v) = search_view.as_ref() {
            v.cursor
        } else {
            tree.cursor
        };
        let is_search = search_view.is_some();
        // Shift-only (no Ctrl/Alt) is how uppercase letters arrive in most
        // terminals — accept it for printable chars (sister-port contract).
        let only_shift = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT)
            && !key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
            && !key.modifiers.contains(crossterm::event::KeyModifiers::ALT);

        use KeyCode::*;
        match key.code {
            Esc => {
                // Close the help dialog first.
                if help_open {
                    help_open = false;
                    continue;
                }
                // Cancel any pending kill confirm.
                kill_confirm = None;
                // Cancel the plugin action picker first (don't run, stay open).
                if plugin_action_picker.take().is_some() {
                    continue;
                }
                // Cancel the template picker first (don't build, stay open).
                if template_picker.take().is_some() {
                    continue;
                }
                // Cancel the name prompt first (don't create, stay open).
                if name_prompt.take().is_some() {
                    continue;
                }
                // Phase 17/18 DirNav: two-stage Esc — active in-level
                // query → clear it (full level re-shown); no query →
                // exit DirNav and restore the prior switcher state
                // (Tree + SearchView were preserved off-screen).
                if let Some(d) = dirnav.as_mut() {
                    if !d.query.is_empty() {
                        d.query.clear();
                        d.requery();
                    } else {
                        dirnav = None;
                    }
                    continue;
                }
                // Two-stage Esc (spec §3): search → clear query
                // (back to browse); browse → close.
                if is_search {
                    search_view = None;
                } else {
                    break;
                }
            }
            Down => {
                if let Some(d) = dirnav.as_mut() {
                    d.move_down();
                } else if let Some(p) = plugin_action_picker.as_mut() {
                    if p.cursor + 1 < p.actions.len() {
                        p.cursor += 1;
                    }
                } else if let Some(p) = template_picker.as_mut() {
                    if p.cursor + 1 < p.templates.len() {
                        p.cursor += 1;
                    }
                } else if let Some(v) = search_view.as_mut() {
                    v.move_down();
                } else {
                    tree.move_down();
                }
            }
            Up => {
                if let Some(d) = dirnav.as_mut() {
                    d.move_up();
                } else if let Some(p) = plugin_action_picker.as_mut() {
                    if p.cursor > 0 {
                        p.cursor -= 1;
                    }
                } else if let Some(p) = template_picker.as_mut() {
                    if p.cursor > 0 {
                        p.cursor -= 1;
                    }
                } else if let Some(v) = search_view.as_mut() {
                    v.move_up();
                } else {
                    tree.move_up();
                }
            }
            Char('n')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                if let Some(d) = dirnav.as_mut() {
                    d.move_down();
                } else if let Some(v) = search_view.as_mut() {
                    v.move_down();
                } else {
                    tree.move_down();
                }
            }
            Char('p')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // Phase 19 DirNav `^p`: pin the selected directory (or
                // cwd if the cursor is out of range) into Pinned dirs;
                // stay in DirNav (no toast — dropped per user request).
                if let Some(d) = dirnav.as_ref() {
                    let path = d
                        .cursor_entry()
                        .map(|e| e.path.clone())
                        .unwrap_or_else(|| d.cwd.clone());
                    let path_str = path.display().to_string();
                    match source::write_pin(&path_str) {
                        Ok(slot) => {
                            flash_error = Some((
                                format!("dirnav:{}", path_str),
                                format!("pinned → slot {slot}"),
                            ));
                        }
                        Err(e) => {
                            flash_error = Some((format!("dirnav:{}", path_str), e));
                        }
                    }
                    continue;
                }
                // `^p` pin (spec §8): pin the selected dir (or the
                // selected pane's cwd) into Pinned dirs; writes
                // `targets.toml`; stay open (no toast — dropped per
                // user request). Spec §8 amended: `^p` is pin, not
                // up-nav (up is ↑ arrow only; `^n` stays for down).
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if let Some(path) = pin_path_for(&node, socket_path) {
                        match source::write_pin(&path) {
                            Ok(slot) => {
                                flash_error =
                                    Some((node.id.clone(), format!("pinned → slot {slot}")));
                                // Rebuild the tree so the new pin
                                // appears in the Pinned group; keep the
                                // cursor on the same node.
                                refresh(
                                    tree,
                                    &mut search_view,
                                    &mut haystack,
                                    socket_path,
                                    group_order,
                                    &cfg.bias,
                                    cfg,
                                );
                            }
                            Err(e) => {
                                flash_error = Some((node.id.clone(), e));
                            }
                        }
                    }
                }
            }
            Char('u')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !is_dirnav =>
            {
                // `^u` unpin (spec §8 amended): on a pinned dir,
                // remove it from `targets.toml`; stay open. Inert on
                // non-pinned kinds (pane/agent/plugin/ws/tab).
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if node.kind == nav::Kind::Dir || node.kind == nav::Kind::Zox {
                        let path = node.id.split_once(':').map(|(_, p)| p).unwrap_or(&node.id);
                        match source::unpin(path) {
                            Ok(true) => {
                                flash_error = Some((node.id.clone(), "unpinned".to_string()));
                                refresh(
                                    tree,
                                    &mut search_view,
                                    &mut haystack,
                                    socket_path,
                                    group_order,
                                    &cfg.bias,
                                    cfg,
                                );
                            }
                            Ok(false) => {
                                flash_error = Some((node.id.clone(), "not pinned".to_string()));
                            }
                            Err(e) => {
                                flash_error = Some((node.id.clone(), e));
                            }
                        }
                    }
                }
            }
            Backspace => {
                // Name prompt: delete last char of the name.
                if let Some(prompt) = name_prompt.as_mut() {
                    prompt.name.pop();
                } else if let Some(d) = dirnav.as_mut() {
                    // Phase 18 DirNav: delete last query char; empty →
                    // clear the search (full level, stay in DirNav).
                    if !d.query.is_empty() {
                        d.query.pop();
                        d.requery();
                    }
                } else if let Some(v) = search_view.as_mut() {
                    // Search: delete last char; empty → browse (spec §3).
                    v.query.pop();
                    if v.query.is_empty() {
                        search_view = None;
                    } else {
                        v.requery(&haystack, &cfg.bias);
                    }
                }
            }
            Right | Tab | Char(' ') if key.modifiers.is_empty() => {
                // Phase 17 DirNav: `→` descends into the cursor dir;
                // `Tab`/`Space` are inert here (Phase 18 wires typing).
                if let Some(d) = dirnav.as_mut() {
                    if key.code == Right {
                        if let Some(child) = d.child() {
                            if let Some(next) = dirnav::DirNavView::at(child) {
                                *d = next; // came_from cleared, cursor 0
                            }
                        }
                    }
                    continue;
                }
                // Name prompt: Space types a space into the name.
                // →/Tab are inert while the prompt is open.
                if let Some(prompt) = name_prompt.as_mut() {
                    if key.code == Char(' ') {
                        prompt.name.push(' ');
                    }
                    continue;
                }
                // In search mode, →/Tab are inert; Space types a
                // space (spec §8). In browse, expand/step.
                // Phase 16: `Tab` in search mode extends the zoxide list
                // when the match list has no Dir/Zox leaves and zoxide
                // hasn't been extended yet this invocation.
                if is_search {
                    if let Some(v) = search_view.as_mut() {
                        if key.code == Char(' ') {
                            v.query.push(' ');
                            v.requery(&haystack, &cfg.bias);
                        } else if key.code == Tab && extend_hint {
                            if !source::zoxide_available() {
                                flash_error = Some((
                                    "zoxide".to_string(),
                                    "zoxide not installed".to_string(),
                                ));
                            } else {
                                // Rebuild only the zoxide group with the
                                // extended limit, then rebuild the haystack
                                // from the updated tree. Sticky: set the flag
                                // so we don't re-run the subprocess.
                                let zox_group = source::zoxide_group_with_limit(
                                    cfg.zoxide_extend_limit as usize,
                                );
                                if let Some(slot) =
                                    tree.root.iter_mut().find(|n| n.id == "group:zoxide")
                                {
                                    *slot = zox_group;
                                }
                                haystack = search::build_haystack(tree);
                                v.requery(&haystack, &cfg.bias);
                                extended_zox = true;
                            }
                        }
                    }
                } else {
                    tree.expand_or_step();
                }
            }
            Left => {
                // Phase 17 DirNav: `←` ascends to the parent, landing
                // the cursor on the entry we came from (if any).
                if let Some(d) = dirnav.as_mut() {
                    if let Some(parent) = d.parent() {
                        let came_from = d.cwd.clone();
                        if let Some(mut next) = dirnav::DirNavView::at(parent) {
                            next.came_from = Some(came_from.clone());
                            if let Some(pos) = next.entries.iter().position(|e| e.path == came_from)
                            {
                                next.cursor = pos;
                            }
                            *d = next;
                        }
                    }
                    continue;
                }
                // Inert in search (spec §8).
                if !is_search {
                    tree.collapse_or_parent();
                }
            }
            Char('t')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // Phase 19 DirNav `^t`: open the template picker for the
                // selected directory (or cwd), mirroring dir/zox `^t`.
                // Inert if no templates are configured.
                if let Some(d) = dirnav.as_ref() {
                    if template_picker.is_none() && name_prompt.is_none() {
                        let templates = source::read_templates();
                        if !templates.is_empty() {
                            let path = d
                                .cursor_entry()
                                .map(|e| e.path.clone())
                                .unwrap_or_else(|| d.cwd.clone());
                            let path_str = path.display().to_string();
                            let cursor = source::preselect_template(&templates, &path_str);
                            template_picker = Some(TemplatePicker {
                                node_id: format!("dirnav:{}", path_str),
                                path: source::expand_path(&path_str),
                                name: source::workspace_name_default(&path_str),
                                templates,
                                cursor,
                            });
                        }
                    }
                    continue;
                }
                // `^t` on a dir/zox: open the template picker (spec §8.4).
                // Inert on non-dir leaves and in search mode (no group
                // context for the path). Unbound if no templates.toml.
                let is_dir = match &search_view {
                    Some(v) => v
                        .cursor_leaf(&haystack)
                        .is_some_and(|l| l.kind == nav::Kind::Dir || l.kind == nav::Kind::Zox),
                    None => tree
                        .cursor_row()
                        .is_some_and(|r| r.kind == nav::Kind::Dir || r.kind == nav::Kind::Zox),
                };
                if is_dir && template_picker.is_none() && name_prompt.is_none() {
                    let templates = source::read_templates();
                    if !templates.is_empty() {
                        let leaf_id = search_view
                            .as_ref()
                            .and_then(|v| v.cursor_leaf(&haystack))
                            .map(|l| l.id.clone())
                            .or_else(|| tree.cursor_row().map(|r| r.id.clone()));
                        if let Some(id) = leaf_id {
                            let path = id
                                .split_once(':')
                                .map(|(_, p)| p)
                                .unwrap_or(&id)
                                .to_string();
                            let cursor = source::preselect_template(&templates, &path);
                            template_picker = Some(TemplatePicker {
                                node_id: id.clone(),
                                path: source::expand_path(&path),
                                name: source::workspace_name_default(&path),
                                templates,
                                cursor,
                            });
                        }
                    }
                }
            }
            Char('d')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !is_dirnav =>
            {
                // `^d` kill (spec §8): kill the selected pane / tab /
                // workspace. First press shows an inline footer confirm;
                // second `^d` confirms + kills; any other key cancels.
                // Stay open. Inert on non-killable kinds (dir/zox/plugin).
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if let Some((target_id, label)) = kill_target(&node) {
                        if let Some((confirm_id, _)) = &kill_confirm {
                            if confirm_id == &target_id {
                                // Confirm: execute the kill.
                                match do_kill(socket_path, &node) {
                                    Ok(()) => {
                                        flash_error = Some((node.id.clone(), "killed".to_string()));
                                        // Rebuild the tree: the killed
                                        // node is gone; the cursor
                                        // clamps to the nearest valid row.
                                        refresh(
                                            tree,
                                            &mut search_view,
                                            &mut haystack,
                                            socket_path,
                                            group_order,
                                            &cfg.bias,
                                            cfg,
                                        );
                                    }
                                    Err(e) => {
                                        flash_error = Some((node.id.clone(), e));
                                    }
                                }
                                kill_confirm = None;
                            }
                        } else {
                            kill_confirm = Some((target_id, label));
                        }
                    }
                }
            }
            Char('r')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !is_dirnav =>
            {
                // `^r` restart command (spec §8.2): on a pane, send
                // Ctrl+C to interrupt the foreground process. Stay open.
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if node.kind == nav::Kind::Pane {
                        if let Some(pid) = node.id.strip_prefix("session:pane:") {
                            let _ = crate::socket_client::request(
                                socket_path,
                                "pane.send_keys",
                                serde_json::json!({
                                    "pane_id": pid,
                                    "keys": ["ctrl+c"],
                                }),
                            );
                            flash_error = Some((node.id.clone(), "interrupted".to_string()));
                            // Refresh so the pane's status meta
                            // updates (e.g. foreground process).
                            refresh(
                                tree,
                                &mut search_view,
                                &mut haystack,
                                socket_path,
                                group_order,
                                &cfg.bias,
                                cfg,
                            );
                        }
                    }
                }
            }
            Char('c')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !is_dirnav =>
            {
                // `^c` interrupt agent (spec §8.2): on an agent, send
                // Ctrl+C to the agent's pane. Stay open.
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if node.kind == nav::Kind::Agent {
                        if let Some(pid) = agent_pane_id(&node, socket_path) {
                            let _ = crate::socket_client::request(
                                socket_path,
                                "agent.send_keys",
                                serde_json::json!({
                                    "target": pid,
                                    "keys": ["ctrl+c"],
                                }),
                            );
                            flash_error = Some((node.id.clone(), "interrupted".to_string()));
                            // Refresh so the agent's status meta
                            // updates (waiting → running, etc.).
                            refresh(
                                tree,
                                &mut search_view,
                                &mut haystack,
                                socket_path,
                                group_order,
                                &cfg.bias,
                                cfg,
                            );
                        }
                    }
                }
            }
            Char('x')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !is_dirnav =>
            {
                // `^x` detach agent (spec §8.2): on an agent, release
                // the agent from its pane. Stay open.
                if let Some(node) = current_cursor(tree, &search_view, &haystack) {
                    if node.kind == nav::Kind::Agent {
                        if let Some(pid) = agent_pane_id(&node, socket_path) {
                            let _ = crate::socket_client::request(
                                socket_path,
                                "pane.release_agent",
                                serde_json::json!({"pane_id": pid}),
                            );
                            flash_error = Some((node.id.clone(), "detached".to_string()));
                            // Rebuild: the detached agent leaves
                            // the Agents list; cursor clamps.
                            refresh(
                                tree,
                                &mut search_view,
                                &mut haystack,
                                socket_path,
                                group_order,
                                &cfg.bias,
                                cfg,
                            );
                        }
                    }
                }
            }
            Char('?') if key.modifiers.is_empty() => {
                // `?` opens the in-popup help dialog (spec §13).
                help_open = !help_open;
            }
            Char('f')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // `^f` enters DirNav mode (Phase 17): a filesystem
                // directory walker starting at the focused pane's cwd.
                // Inert if a dialog/prompt is open or DirNav is already
                // active. The in-level search (Phase 18) and commit
                // verb (Phase 19) land later.
                if dirnav.is_none()
                    && name_prompt.is_none()
                    && template_picker.is_none()
                    && plugin_action_picker.is_none()
                    && !help_open
                {
                    let cwd = focused_cwd(ctx, socket_path);
                    // Fall back to $HOME if the pane cwd is missing or
                    // unreadable (Phase 17 edge case).
                    dirnav = cwd.and_then(dirnav::DirNavView::at).or_else(|| {
                        std::env::var("HOME")
                            .ok()
                            .map(std::path::PathBuf::from)
                            .and_then(dirnav::DirNavView::at)
                    });
                }
            }
            Enter => {
                // Phase 19 DirNav commit: Enter opens a new workspace at
                // the selected directory (or cwd if the cursor is out of
                // range / on a non-dir), reusing the existing template-
                // build path + name prompt — identical to dir/zox Enter.
                if let Some(d) = dirnav.as_ref() {
                    let path = d
                        .cursor_entry()
                        .map(|e| e.path.clone())
                        .unwrap_or_else(|| d.cwd.clone());
                    let expanded = source::expand_path(&path.display().to_string());
                    name_prompt = Some(NamePrompt {
                        node_id: format!("dirnav:{}", path.display()),
                        path: expanded.clone(),
                        name: source::workspace_name_default(&path.display().to_string()),
                        template: source::default_template_for(&expanded),
                    });
                    continue;
                }
                // If a plugin action picker is active, confirm: run the action
                // (spec §8.3) via plugin.action.invoke, then close.
                if let Some(picker) = plugin_action_picker.take() {
                    let action = &picker.actions[picker.cursor.min(picker.actions.len() - 1)];
                    let _ = crate::socket_client::request(
                        socket_path,
                        "plugin.action.invoke",
                        serde_json::json!({
                            "plugin": &picker.plugin_id,
                            "action_id": &action.0,
                        }),
                    );
                    break;
                }
                // If a template picker is active, confirm: open a name
                // prompt carrying the selected template (spec §8.4 amended
                // — ^t now asks for a name after template selection, so it
                // shares the one build path with Enter).
                if let Some(picker) = template_picker.take() {
                    let template =
                        picker.templates[picker.cursor.min(picker.templates.len() - 1)].clone();
                    name_prompt = Some(NamePrompt {
                        node_id: picker.node_id,
                        path: picker.path,
                        name: picker.name,
                        template,
                    });
                    continue;
                }
                // If a name prompt is active, confirm: build the workspace
                // from the prompt's template (Enter = auto-resolved default,
                // ^t = user-selected) and close (spec §8.2/§8.4 amended).
                if let Some(prompt) = name_prompt.take() {
                    match source::build_workspace_from_template(
                        &prompt.path,
                        &prompt.name,
                        &prompt.template,
                    ) {
                        Ok(Some(pane_id)) => {
                            // Focus the first pane so the user lands in it.
                            let _ = socket_client::request(
                                socket_path,
                                "pane.focus",
                                serde_json::json!({"pane_id": pane_id}),
                            );
                            break;
                        }
                        Ok(None) => break,
                        Err(e) => {
                            flash_error = Some((prompt.node_id, e));
                        }
                    }
                    continue;
                }
                // The node id to invoke on: in search, the cursor
                // leaf; in browse, the cursor row.
                let leaf_id = search_view
                    .as_ref()
                    .and_then(|v| v.cursor_leaf(&haystack))
                    .map(|l| l.id.clone())
                    .or_else(|| tree.cursor_row().map(|r| r.id.clone()));
                let is_leaf = match &search_view {
                    Some(v) => v.cursor_leaf(&haystack).is_some(),
                    None => tree
                        .cursor_row()
                        .is_some_and(|r| r.is_leaf && !r.id.ends_with(":hint")),
                };
                if is_leaf {
                    if let Some(id) = leaf_id {
                        // Dir/zox: prompt for a workspace name (spec §8.2
                        // amended). Enter builds from the auto-resolved
                        // default template (match-glob → default → hardcoded
                        // 1-tab/1-pane); ^t lets the user pick the template.
                        if id.starts_with("pinned:") || id.starts_with("zox:") {
                            let path = id.split_once(':').map(|(_, p)| p).unwrap_or(&id);
                            let expanded = source::expand_path(path);
                            name_prompt = Some(NamePrompt {
                                node_id: id.clone(),
                                path: expanded.clone(),
                                name: source::workspace_name_default(path),
                                template: source::default_template_for(&expanded),
                            });
                        } else if id.starts_with("plugin:") {
                            // Plugin: open the action picker (spec §8.3),
                            // unless the plugin has no actions (inert).
                            let pid = id.strip_prefix("plugin:").unwrap_or(&id);
                            if let Some(picker) = build_plugin_action_picker(socket_path, pid) {
                                plugin_action_picker = Some(picker);
                            } else {
                                flash_error = Some((id, "no actions".to_string()));
                            }
                        } else {
                            match invoke_action(socket_path, &id) {
                                Ok(nav::Outcome::Close { .. }) => break,
                                Ok(nav::Outcome::Stay { .. }) => {}
                                Err(e) => {
                                    flash_error = Some((id, e));
                                }
                            }
                        }
                    }
                } else if !is_search {
                    // Branch in browse → step into it.
                    tree.expand_or_step();
                }
            }
            Char('.') if key.modifiers.is_empty() && is_dirnav => {
                // Phase 19 DirNav: `.` toggles hidden entries (dotfiles).
                // Refresh the listing in place; the cursor clamps and the
                // in-level search re-runs against the new entry set.
                if let Some(d) = dirnav.as_mut() {
                    d.show_hidden = !d.show_hidden;
                    d.refresh_entries();
                }
            }
            Char(c)
                if c.is_ascii_graphic()
                    && (key.modifiers.is_empty() || only_shift)
                    && is_dirnav =>
            {
                // Phase 18 DirNav: typing fuzzy-filters the current
                // level's entry names and lands on the first match.
                if let Some(d) = dirnav.as_mut() {
                    d.query.push(c);
                    d.requery();
                }
            }
            Char(c)
                if c.is_ascii_graphic()
                    && (key.modifiers.is_empty() || only_shift)
                    && !is_dirnav =>
            {
                // Name prompt: append to the name.
                if let Some(prompt) = name_prompt.as_mut() {
                    prompt.name.push(c);
                } else {
                    // Printable char → enter search (or append), re-rank,
                    // cursor → 0 (spec §3).
                    match search_view.as_mut() {
                        Some(v) => {
                            v.query.push(c);
                            v.requery(&haystack, &cfg.bias);
                        }
                        None => {
                            let mut v = search::view(&haystack, c.to_string(), &cfg.bias);
                            v.cursor = 0;
                            search_view = Some(v);
                        }
                    }
                }
            }
            _ => {
                // Any unhandled key cancels a pending kill confirm.
                kill_confirm = None;
            }
        }

        let cursor_after = if let Some(d) = dirnav.as_ref() {
            d.cursor
        } else if let Some(v) = search_view.as_ref() {
            v.cursor
        } else {
            tree.cursor
        };
        if cursor_after != cursor_before {
            last_cursor_change = Some(std::time::Instant::now());
            flash_error = None;
        }
    }

    Ok(())
}

/// Cursor info (owned, works for both browse and search modes).
struct CursorInfo {
    id: String,
    kind: nav::Kind,
    label: String,
}

/// Refresh the tree + haystack after a side action mutates session
/// state (spec §8: pin / kill / detach). Rebuilds the tree from the
/// socket, preserves the cursor on the same object if it still
/// exists, and re-runs the search query if search mode is active.
fn refresh(
    tree: &mut nav::Tree,
    search_view: &mut Option<search::SearchView>,
    haystack: &mut Vec<search::Leaf>,
    socket_path: &str,
    groups: &[String],
    bias_cfg: &config::BiasCfg,
    cfg: &config::Config,
) {
    let new_root = source::build_tree(socket_path, groups, cfg);
    tree.reload(new_root);
    *haystack = search::build_haystack(tree);
    if let Some(v) = search_view.as_mut() {
        v.requery(haystack, bias_cfg);
    }
}

/// Get the current cursor (browse or search mode) as owned info.
fn current_cursor(
    tree: &nav::Tree,
    search: &Option<search::SearchView>,
    haystack: &[search::Leaf],
) -> Option<CursorInfo> {
    if let Some(v) = search.as_ref() {
        v.cursor_leaf(haystack).map(|l| CursorInfo {
            id: l.id.clone(),
            kind: l.kind,
            label: l.label.clone(),
        })
    } else {
        tree.cursor_row().map(|r| CursorInfo {
            id: r.id.clone(),
            kind: r.kind,
            label: r.label.clone(),
        })
    }
}

/// Resolve a pin path for `^p` (spec §8): for a dir/zox leaf, the
/// path; for a pane, the pane's cwd (fetched via `pane.get`); for
/// an agent, the agent's cwd. None for non-pinnable kinds.
/// Fetch the focused pane's cwd (Phase 17 DirNav entry): reads
/// `pane.get` for the launch context's `focused_pane_id`. Returns None
/// if the context is missing, the socket call fails, or the pane has
/// no cwd — the caller falls back to `$HOME`.
fn focused_cwd(ctx: Option<&LaunchContext>, socket_path: &str) -> Option<std::path::PathBuf> {
    let pane_id = ctx?.focused_pane_id.as_str();
    let r = crate::socket_client::request(
        socket_path,
        "pane.get",
        serde_json::json!({"pane_id": pane_id}),
    )
    .ok()?;
    let cwd = r
        .get("pane")
        .and_then(|p| p.get("cwd"))
        .and_then(|v| v.as_str())?;
    Some(std::path::PathBuf::from(cwd))
}

fn pin_path_for(node: &CursorInfo, socket_path: &str) -> Option<String> {
    match node.kind {
        nav::Kind::Dir | nav::Kind::Zox => node.id.split_once(':').map(|(_, p)| p.to_string()),
        nav::Kind::Pane => node.id.strip_prefix("session:pane:").and_then(|pid| {
            crate::socket_client::request(
                socket_path,
                "pane.get",
                serde_json::json!({"pane_id": pid}),
            )
            .ok()
            .and_then(|r| {
                r.get("pane")
                    .and_then(|p| p.get("cwd"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        }),
        nav::Kind::Agent => agent_pane_id(node, socket_path).and_then(|pid| {
            crate::socket_client::request(
                socket_path,
                "pane.get",
                serde_json::json!({"pane_id": pid}),
            )
            .ok()
            .and_then(|r| {
                r.get("pane")
                    .and_then(|p| p.get("cwd"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        }),
        _ => None,
    }
}

/// Resolve the kill target (id + label) for `^d` (spec §8):
/// pane → `pane.close`, tab → `tab.close`, workspace →
/// `workspace.close`. None for non-killable kinds.
fn kill_target(node: &CursorInfo) -> Option<(String, String)> {
    match node.kind {
        nav::Kind::Pane => node
            .id
            .strip_prefix("session:pane:")
            .map(|pid| (pid.to_string(), format!("pane {}", node.label))),
        nav::Kind::Tab => node
            .id
            .strip_prefix("session:tab:")
            .map(|tid| (tid.to_string(), format!("tab {}", node.label))),
        nav::Kind::Workspace => node
            .id
            .strip_prefix("session:ws:")
            .map(|wid| (wid.to_string(), format!("workspace {}", node.label))),
        _ => None,
    }
}

/// Execute the kill via the right socket method.
fn do_kill(socket_path: &str, node: &CursorInfo) -> Result<(), String> {
    let (method, id_field, id) = match node.kind {
        nav::Kind::Pane => (
            "pane.close",
            "pane_id",
            node.id.strip_prefix("session:pane:").unwrap_or(&node.id),
        ),
        nav::Kind::Tab => (
            "tab.close",
            "tab_id",
            node.id.strip_prefix("session:tab:").unwrap_or(&node.id),
        ),
        nav::Kind::Workspace => (
            "workspace.close",
            "workspace_id",
            node.id.strip_prefix("session:ws:").unwrap_or(&node.id),
        ),
        _ => return Err("not killable".to_string()),
    };
    crate::socket_client::request(socket_path, method, serde_json::json!({ id_field: id }))
        .map(|_| ())
        .map_err(|e| format!("{method} failed: {e}"))
}

/// Resolve the pane id for an agent (via `agent.list` → pane_id).
fn agent_pane_id(node: &CursorInfo, socket_path: &str) -> Option<String> {
    let terminal_id = node.id.strip_prefix("agent:")?;
    let Ok(r) = crate::socket_client::request(socket_path, "agent.list", serde_json::json!({}))
    else {
        return None;
    };
    r.get("agents")
        .and_then(|v| v.as_array())
        .and_then(|agents| {
            agents.iter().find_map(|a| {
                if a.get("terminal_id").and_then(|v| v.as_str()) == Some(terminal_id) {
                    a.get("pane_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
}

/// Build a plugin action picker for `plugin_id` (spec §8.3):
/// fetches the plugin's declared actions from `plugin.list`.
/// Returns None if the plugin has no actions (not selectable).
fn build_plugin_action_picker(socket_path: &str, plugin_id: &str) -> Option<PluginActionPicker> {
    let Ok(r) = crate::socket_client::request(socket_path, "plugin.list", serde_json::json!({}))
    else {
        return None;
    };
    let plugins = r.get("plugins").and_then(|v| v.as_array());
    let plugin = plugins.and_then(|ps| {
        ps.iter()
            .find(|p| p.get("plugin_id").and_then(|v| v.as_str()) == Some(plugin_id))
    });
    let actions: Vec<(String, String)> = plugin
        .and_then(|p| p.get("actions").and_then(|v| v.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            (
                a.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
                a.get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
            )
        })
        .collect();
    if actions.is_empty() {
        None
    } else {
        Some(PluginActionPicker {
            plugin_id: plugin_id.to_string(),
            actions,
            cursor: 0,
        })
    }
}

/// Invoke the default action on a node via its provider (Phase 3).
/// Only the Session provider is real here; other groups' providers
/// land in later phases and return "not implemented".
fn invoke_action(socket_path: &str, id: &str) -> Result<nav::Outcome, String> {
    use crate::nav::Provider;
    // Dispatch to the right provider based on the node-id prefix.
    if id.starts_with("agents:") {
        let provider = source::AgentsProvider::new(socket_path.to_string());
        provider.invoke(&id.to_string(), nav::Act::Default)
    } else if id.starts_with("pinned:") || id.starts_with("zox:") {
        // Both dir groups share the same action: open a new workspace
        // at the path (spec §8.2). Dispatch by prefix to the right
        // provider so each owns its own nodes.
        if id.starts_with("pinned:") {
            let provider = source::PinnedProvider::new();
            provider.invoke(&id.to_string(), nav::Act::Default)
        } else {
            let provider = source::ZoxideProvider::new();
            provider.invoke(&id.to_string(), nav::Act::Default)
        }
    } else {
        let provider = source::SessionProvider::new(socket_path.to_string());
        provider.invoke(&id.to_string(), nav::Act::Default)
    }
}

/// Show a one-line toast in the host terminal via `notification.show`.
#[allow(dead_code)] // toast dropped (2026-08-21) — the jump is the
                    // important part; the toast was a spec nicety the user doesn't want.
fn show_toast(socket_path: &str, message: &str) {
    let _ = socket_client::request(
        socket_path,
        "notification.show",
        serde_json::json!({"title": "herdr-nav", "message": message, "duration_ms": 1500}),
    );
}

fn main() {
    if let Err(e) = run() {
        eprintln!("herdr-nav: {e}");
        std::process::exit(1);
    }
}
