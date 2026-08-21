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
mod nav;
mod preview;
mod render;
mod socket_client;
mod source;

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

// ── Launch context ────────────────────────────────────────────────────────────

/// Launch context: which pane this popup was opened relative to.
#[allow(dead_code)] // focused_pane_id used in Phase 3 (pin-cwd / active ordering)
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
    let _ctx = launch_context().ok();
    let socket_path = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
    let _config = config::Config::load();

    let mut tree = nav::Tree::new(source::build_tree(&socket_path));

    // Terminal setup — same contract as the sister ports: enter raw mode,
    // hide the cursor, enter the alternate screen, then restore on exit.
    enable_raw_mode().map_err(|e| format!("enable_raw_mode: {e}"))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| format!("EnterAlternateScreen: {e}"))?;
    execute!(stdout, cursor::Hide).map_err(|e| format!("Hide cursor: {e}"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("Terminal::new: {e}"))?;

    let result = event_loop(&mut terminal, &mut tree, &socket_path);

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
) -> Result<(), String> {
    let mut last_key: Option<(event::KeyEvent, std::time::Instant)> = None;
    // Tracks when the cursor last moved, for the 60ms preview debounce
    // (spec §7.4). Any cursor-moving key updates this; the preview
    // stays stale-and-dimmed until the debounce window elapses.
    let mut last_cursor_change: Option<std::time::Instant> = None;

    loop {
        tree.ensure_cursor_valid();
        terminal
            .draw(|frame| render::draw(frame, tree, socket_path, last_cursor_change))
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

        use KeyCode::*;
        let before = tree.cursor;
        match key.code {
            Esc => break,
            // Bare arrows move the cursor (no modifier guard — the
            // guard below would reject a plain Down/Up).
            Down => tree.move_down(),
            Up => tree.move_up(),
            // ^n / ^p are the same motion (spec §8).
            Char('n')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                tree.move_down()
            }
            Char('p')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                tree.move_up()
            }
            Right | Tab | Char(' ') => tree.expand_or_step(),
            Left => tree.collapse_or_parent(),
            Enter => tree.toggle(),
            // Printable characters are inert in Phase 1 (search is Phase 4).
            _ => {}
        }
        if tree.cursor != before {
            last_cursor_change = Some(std::time::Instant::now());
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("herdr-nav: {e}");
        std::process::exit(1);
    }
}
