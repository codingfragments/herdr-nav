//! `herdr-nav` — fuzzy navigation between panes, agents, directories, and
//! plugins, with a rich preview pane.
//!
//! The popup opens a real PTY (Herdr popup placement). This binary reads
//! the available navigation targets via the Herdr socket API, renders a
//! fuzzy-filterable list + preview with `ratatui` driving a `crossterm`
//! backend directly, and runs an event loop until `Esc` closes the popup
//! or `Enter` switches to the selected target.
//!
//! **Status: scaffold only.** Module bodies are stubs; the event loop
//! renders a placeholder and exits on `Esc`. Real source gathering,
//! fuzzy matching, preview rendering, and target-switching land in the
//! phase sequence in PLANNING.md §17.

// Scaffold only: stub modules are not yet wired into the event loop.
// Remove this allow as each phase in PLANNING.md §17 lands real use.
#![allow(dead_code)]

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
    let _ctx = launch_context()?;
    let _socket_path = std::env::var("HERDR_SOCKET_PATH")
        .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;
    let _config = config::Config::load();

    // Terminal setup — same contract as the sister ports: enter raw mode,
    // hide the cursor, enter the alternate screen, then restore on exit.
    enable_raw_mode().map_err(|e| format!("enable_raw_mode: {e}"))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| format!("EnterAlternateScreen: {e}"))?;
    execute!(stdout, cursor::Hide).map_err(|e| format!("Hide cursor: {e}"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("Terminal::new: {e}"))?;

    let result = event_loop(&mut terminal);

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

/// Placeholder event loop: render a scaffold banner, exit on `Esc`.
///
/// Replaced by the real tree-browse loop in Phase 1 (PLANNING.md §17).
fn event_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<(), String> {
    let mut last_key: Option<(event::KeyEvent, std::time::Instant)> = None;

    loop {
        terminal
            .draw(render::draw_placeholder)
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

        if key.code == KeyCode::Esc {
            break;
        }
        // All other keys are no-ops in the scaffold.
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("herdr-nav: {e}");
        std::process::exit(1);
    }
}
