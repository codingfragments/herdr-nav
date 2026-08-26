//! The capture wizard (Phase C4): a step-by-step ratatui form that
//! asks the user for template metadata and policies, then writes the
//! YAML. Reuses the theme stack (`theme::load`) and the popup geometry
//! (80%×80%) from the switcher.
//!
//! **Live preview:** the raw workspace data is fetched once (`fetch_raw`)
//! on wizard entry and cached. On every render, `build_template` (pure,
//! no socket calls) is called with the current form choices to produce a
//! live YAML preview in the right panel — so the user sees the template
//! evolve as they type the name, choose policies, etc.
//!
//! **Navigation:** `Esc` aborts (no write); `←` goes back one step;
//! `Enter` advances. On the Review step, `Enter` writes the file.
//! Clash handling and `$EDITOR` handoff land in C5.
//!
//! **Key discipline:** the popup PTY runs in legacy keyboard mode where
//! every key event arrives as `KeyEventKind::Press` (a single tap can
//! double-fire). We reuse the `KEY_DEBOUNCE` guard from `main.rs`.

use std::time::Duration;

use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Terminal;

use crate::capture::{self, CommandPolicy, CwdPolicy, RawCapture};
use crate::theme;

const KEY_DEBOUNCE: Duration = Duration::from_millis(40);
const TOTAL_STEPS: usize = 7;

// ── Steps ─────────────────────────────────────────────────────────

/// The wizard steps (spec §8). Steps 1-7 are the main flow; ClashPrompt
/// and EditorPrompt are interleaved after Review (Phase C5) and are not
/// counted in the progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    ScopeConfirm,
    Name,
    MatchGlobs,
    CommandPolicy,
    CwdPolicy,
    TabNames,
    Review,
    /// Shown after Review if the name clashes (Phase C5).
    ClashPrompt,
    /// Shown after a successful write (Phase C5).
    EditorPrompt,
}

impl Step {
    fn number(self) -> usize {
        match self {
            Self::ScopeConfirm => 1,
            Self::Name => 2,
            Self::MatchGlobs => 3,
            Self::CommandPolicy => 4,
            Self::CwdPolicy => 5,
            Self::TabNames => 6,
            Self::Review => 7,
            // C5 steps are not counted in the progress bar.
            Self::ClashPrompt | Self::EditorPrompt => 7,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::ScopeConfirm => "Confirm workspace",
            Self::Name => "Template name",
            Self::MatchGlobs => "Match globs",
            Self::CommandPolicy => "Command policy",
            Self::CwdPolicy => "cwd policy",
            Self::TabNames => "Tab names",
            Self::Review => "Review & write",
            Self::ClashPrompt => "Name clash",
            Self::EditorPrompt => "Open in editor?",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::ScopeConfirm => "Confirm the workspace to capture — this is the live structure from the herdr daemon.",
            Self::Name => "The filename stem and the `name:` field in the YAML. Required.",
            Self::MatchGlobs => "Glob patterns that auto-preselect this template (e.g. `**/Cargo.toml`). Space-separated. Tab toggles the default flag.",
            Self::CommandPolicy => "How to fill each pane's `command:` from the running process group.",
            Self::CwdPolicy => "How to handle each pane's working directory in the generated template.",
            Self::TabNames => "Tab names, pre-filled from live labels. ↑↓ to focus a tab, type to edit.",
            Self::Review => "Live YAML preview. Enter writes the template to ~/.config/herdr/templates/.",
            Self::ClashPrompt => "A template with this name already exists. Choose how to proceed.",
            Self::EditorPrompt => "Template written. Open it in your editor to fine-tune, or close.",
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::ScopeConfirm => None,
            Self::Name => Some(Self::ScopeConfirm),
            Self::MatchGlobs => Some(Self::Name),
            Self::CommandPolicy => Some(Self::MatchGlobs),
            Self::CwdPolicy => Some(Self::CommandPolicy),
            Self::TabNames => Some(Self::CwdPolicy),
            Self::Review => Some(Self::TabNames),
            Self::ClashPrompt => Some(Self::Review),
            Self::EditorPrompt => None, // no back — the write already happened
        }
    }
}

// ── Form state ───────────────────────────────────────────────────

#[allow(dead_code)]
struct CaptureForm {
    step: Step,
    /// Cached raw capture (fetched once on wizard entry).
    raw: RawCapture,
    /// Tab labels (live, pre-filled) — editable on TabNames.
    tab_labels: Vec<String>,
    /// Template name (editable).
    name: String,
    /// match globs (one line, space-separated).
    match_globs: String,
    /// `default: true` toggle.
    default_flag: bool,
    /// Command policy (cursor index).
    command_policy_idx: usize,
    /// cwd policy (cursor index).
    cwd_policy_idx: usize,
    /// Which tab is focused for editing on the TabNames step.
    focused_tab_idx: usize,
    /// Scroll offset for the YAML preview.
    preview_scroll: usize,
    /// The written template path (set after a successful write, used by
    /// the EditorPrompt to exec $EDITOR). Phase C5.
    written_path: Option<std::path::PathBuf>,
}

const COMMAND_CHOICES: &[(&str, &str)] = &[
    ("keep", "Best-effort capture from pane.process_info, with # best-effort: comments on guesses"),
    ("blank", "Force every pane to a plain shell — no command, no process_info calls"),
];
const CWD_CHOICES: &[(&str, &str)] = &[
    ("relative", "Relativize under the workspace base cwd; keep absolute when distant"),
    ("absolute", "Keep every pane cwd absolute as captured (machine-specific)"),
    ("inherit", "Blank every pane cwd — each inherits the new workspace's cwd"),
];

impl CaptureForm {
    fn command_policy(&self) -> CommandPolicy {
        match self.command_policy_idx {
            1 => CommandPolicy::Blank,
            _ => CommandPolicy::Keep,
        }
    }

    fn cwd_policy(&self) -> CwdPolicy {
        match self.cwd_policy_idx {
            1 => CwdPolicy::Absolute,
            2 => CwdPolicy::Inherit,
            _ => CwdPolicy::Relative,
        }
    }

    fn match_globs_vec(&self) -> Vec<String> {
        self.match_globs
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// Build the live YAML preview from the current form state + cached
    /// raw capture. Pure (no socket calls). Returns None if the name
    /// is empty (can't build a template without it).
    fn build_preview_yaml(&self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        let (template, annotations) = capture::build_template(
            &self.raw,
            name,
            self.cwd_policy(),
            self.command_policy(),
            &self.tab_labels,
            self.match_globs_vec(),
            self.default_flag,
        );
        capture::template_to_yaml(&template, &annotations).ok()
    }
}

// ── Entry point ──────────────────────────────────────────────────

pub fn run(socket_path: &str) -> Result<(), String> {
    // Fetch all raw data once (all socket calls happen here).
    let raw = capture::fetch_raw(socket_path)?;
    let tab_labels: Vec<String> = raw.tabs.iter().map(|t| t.tab_label.clone()).collect();

    let mut form = CaptureForm {
        step: Step::ScopeConfirm,
        name: raw.workspace_label.clone(),
        tab_labels,
        match_globs: String::new(),
        default_flag: false,
        command_policy_idx: 0,
        cwd_policy_idx: 0,
        focused_tab_idx: 0,
        preview_scroll: 0,
        written_path: None,
        raw,
    };

    // Terminal setup — same contract as the switcher.
    enable_raw_mode().map_err(|e| format!("enable_raw_mode: {e}"))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| format!("EnterAlternateScreen: {e}"))?;
    execute!(stdout, cursor::Hide).map_err(|e| format!("Hide cursor: {e}"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("Terminal::new: {e}"))?;

    let palette = theme::load();
    let result = wizard_loop(&mut terminal, &mut form, socket_path, &palette);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), cursor::Show).ok();
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )
    .ok();

    result
}

// ── Event loop ──────────────────────────────────────────────────

fn wizard_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    form: &mut CaptureForm,
    socket_path: &str,
    palette: &theme::Palette,
) -> Result<(), String> {
    let mut last_key: Option<(KeyCode, std::time::Instant)> = None;
    loop {
        terminal
            .draw(|f| draw_wizard(f, form, palette))
            .map_err(|e| format!("draw: {e}"))?;

        if !event::poll(Duration::from_millis(100)).map_err(|e| format!("poll: {e}"))? {
            continue;
        }
        let ev = event::read().map_err(|e| format!("read: {e}"))?;
        let CtEvent::Key(key) = ev else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if let Some((last_code, last_time)) = last_key {
            if last_code == key.code && last_time.elapsed() < KEY_DEBOUNCE {
                continue;
            }
        }
        last_key = Some((key.code, std::time::Instant::now()));

        match handle_key(form, key.code, key.modifiers, socket_path)? {
            Action::Continue => {}
            Action::Abort => return Ok(()),
            Action::Done => return Ok(()),
        }
    }
}

enum Action {
    Continue,
    Abort,
    Done,
}

/// Build the YAML from the current form state and write it (Phase C5).
/// Stores the written path in `form.written_path` for the editor prompt.
fn do_write(form: &mut CaptureForm) -> Result<(), String> {
    let yaml = match form.build_preview_yaml() {
        Some(y) => y,
        None => return Err("cannot build template (name empty?)".to_string()),
    };
    let name = form.name.trim().to_string();
    let path = capture::write_template(&name, &yaml)?;
    form.written_path = Some(path);
    Ok(())
}

/// Exec the user's editor on the written path (Phase C5, spec §9).
///
/// `$VISUAL` → `$EDITOR` → `vi`. Uses `exec` (not spawn) — the popup
/// pane process is **replaced** by the editor. When the editor exits,
/// the pane process ends and the popup closes. No post-edit validation:
/// the plugin isn't alive after `exec`. A malformed YAML is surfaced by
/// `read_templates` on the next `^t` use (it already logs parse errors
/// to stderr).
fn exec_editor(path: &std::path::Path) {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let cmd = parts.next().unwrap_or("vi");
    let args: Vec<String> = parts.map(str::to_string).collect();
    // Restore the terminal before exec so the editor gets a clean TTY.
    disable_raw_mode().ok();
    let _ = execute!(std::io::stdout(), cursor::Show);
    let _ = execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(cmd);
    command.args(&args);
    command.arg(path);
    let err = command.exec();
    eprintln!("herdr-nav: exec {cmd} failed: {err}");
}

fn handle_key(
    form: &mut CaptureForm,
    code: KeyCode,
    mods: KeyModifiers,
    _socket_path: &str,
) -> Result<Action, String> {
    if code == KeyCode::Esc {
        return Ok(Action::Abort);
    }
    if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
        return Ok(Action::Abort);
    }

    // ← goes back one step (visible "← back" hint).
    if code == KeyCode::Left && form.step != Step::ScopeConfirm {
        if let Some(prev) = form.step.prev() {
            form.step = prev;
            form.preview_scroll = 0;
        }
        return Ok(Action::Continue);
    }

    match form.step {
        Step::ScopeConfirm => {
            if code == KeyCode::Enter {
                form.step = Step::Name;
            }
            Ok(Action::Continue)
        }
        Step::Name => {
            match code {
                KeyCode::Enter => {
                    if !form.name.trim().is_empty() {
                        form.step = Step::MatchGlobs;
                    }
                }
                KeyCode::Backspace => {
                    form.name.pop();
                }
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                    form.name.push(c);
                }
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::MatchGlobs => {
            match code {
                KeyCode::Enter => form.step = Step::CommandPolicy,
                KeyCode::Backspace => {
                    form.match_globs.pop();
                }
                KeyCode::Char(' ') => form.match_globs.push(' '),
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                    form.match_globs.push(c);
                }
                KeyCode::Tab => form.default_flag = !form.default_flag,
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::CommandPolicy => {
            match code {
                KeyCode::Up if form.command_policy_idx > 0 => form.command_policy_idx -= 1,
                KeyCode::Down if form.command_policy_idx < COMMAND_CHOICES.len() - 1 => {
                    form.command_policy_idx += 1
                }
                KeyCode::Enter => form.step = Step::CwdPolicy,
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::CwdPolicy => {
            match code {
                KeyCode::Up if form.cwd_policy_idx > 0 => form.cwd_policy_idx -= 1,
                KeyCode::Down if form.cwd_policy_idx < CWD_CHOICES.len() - 1 => {
                    form.cwd_policy_idx += 1
                }
                KeyCode::Enter => form.step = Step::TabNames,
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::TabNames => {
            match code {
                KeyCode::Enter => form.step = Step::Review,
                // Up/Down move focus between tabs.
                KeyCode::Up if form.focused_tab_idx > 0 => form.focused_tab_idx -= 1,
                KeyCode::Down if form.focused_tab_idx < form.tab_labels.len().saturating_sub(1) => {
                    form.focused_tab_idx += 1
                }
                KeyCode::Backspace => {
                    if let Some(tab) = form.tab_labels.get_mut(form.focused_tab_idx) {
                        tab.pop();
                    }
                }
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                    if let Some(tab) = form.tab_labels.get_mut(form.focused_tab_idx) {
                        tab.push(c);
                    }
                }
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::Review => {
            match code {
                KeyCode::Up => {
                    if form.preview_scroll > 0 {
                        form.preview_scroll -= 1;
                    }
                }
                KeyCode::Down => {
                    form.preview_scroll = form.preview_scroll.saturating_add(1);
                }
                KeyCode::Enter => {
                    let name = form.name.trim().to_string();
                    if name.is_empty() {
                        return Ok(Action::Continue);
                    }
                    // Phase C5: clash check before write.
                    if capture::template_exists(&name) {
                        form.step = Step::ClashPrompt;
                        return Ok(Action::Continue);
                    }
                    // No clash — write and go to the editor prompt.
                    do_write(form)?;
                    form.step = Step::EditorPrompt;
                }
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::ClashPrompt => {
            match code {
                // o = overwrite, c = cancel, r = rename
                KeyCode::Char('o') => {
                    do_write(form)?;
                    form.step = Step::EditorPrompt;
                }
                KeyCode::Char('c') | KeyCode::Esc => {
                    return Ok(Action::Abort);
                }
                KeyCode::Char('r') => {
                    form.step = Step::Name;
                }
                _ => {}
            }
            Ok(Action::Continue)
        }
        Step::EditorPrompt => {
            match code {
                // y = yes, open $EDITOR; n = no, close
                KeyCode::Char('y') => {
                    if let Some(ref path) = form.written_path {
                        exec_editor(path);
                        // exec replaces the process; if it returns,
                        // it failed.
                        return Ok(Action::Done);
                    }
                    return Ok(Action::Done);
                }
                KeyCode::Char('n') | KeyCode::Enter => {
                    if let Some(ref path) = form.written_path {
                        println!("wrote {}", path.display());
                    }
                    return Ok(Action::Done);
                }
                _ => {}
            }
            Ok(Action::Continue)
        }
    }
}

// ── Rendering ───────────────────────────────────────────────────

fn draw_wizard(f: &mut ratatui::Frame, form: &mut CaptureForm, palette: &theme::Palette) {
    let area = f.area();

    // Outer popup: rounded border, accent title with step indicator.
    let title = format!(
        " {} · {} ",
        "herdr capture",
        form.step.title()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.surface1))
        .title(Span::styled(
            title,
            Style::default().fg(palette.accent).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(palette.panel_bg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layout: progress bar (1) · hint (2) · separator (1) · body (flex) · separator (1) · footer (1).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // progress dots
            Constraint::Length(2),  // hint
            Constraint::Length(1),  // separator
            Constraint::Min(3),    // body
            Constraint::Length(1),  // separator
            Constraint::Length(1),  // footer
        ])
        .split(inner);

    // Progress dots: ●●●○○○○ for step 3/7.
    draw_progress(f, chunks[0], form, palette);

    // Hint line.
    let hint = Paragraph::new(Line::from(Span::styled(
        form.step.hint(),
        Style::default().fg(palette.subtext0),
    )))
    .wrap(Wrap { trim: true });
    f.render_widget(hint, chunks[1]);

    // Separators.
    draw_separator(f, chunks[2], palette);
    draw_separator(f, chunks[4], palette);

    // Body: two-column (input | preview) for most steps; full-width
    // for ScopeConfirm.
    let body = chunks[3];
    match form.step {
        Step::ScopeConfirm => draw_scope_confirm(f, body, form, palette),
        _ => draw_two_column(f, body, form, palette),
    }

    draw_footer(f, chunks[5], form, palette);
}

/// Step progress indicator: ● for completed/current, ○ for upcoming.
fn draw_progress(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let current = form.step.number();
    let mut spans = vec![Span::styled("  ", Style::default())];
    for i in 1..=TOTAL_STEPS {
        let (dot, style) = if i < current {
            ("●", Style::default().fg(palette.surface1))
        } else if i == current {
            ("●", Style::default().fg(palette.accent).add_modifier(Modifier::BOLD))
        } else {
            ("○", Style::default().fg(palette.overlay0))
        };
        spans.push(Span::styled(dot, style));
        if i < TOTAL_STEPS {
            spans.push(Span::styled(" ─ ", Style::default().fg(palette.overlay0)));
        }
    }
    spans.push(Span::styled(
        format!("  {}/{}", current, TOTAL_STEPS),
        Style::default().fg(palette.overlay0),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A thin horizontal rule.
fn draw_separator(f: &mut ratatui::Frame, area: Rect, palette: &theme::Palette) {
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(palette.surface0),
        ))),
        area,
    );
}

/// Two-column layout: left = step input, right = live YAML preview.
/// The input column is 45% so text fields and descriptions have room.
fn draw_two_column(f: &mut ratatui::Frame, area: Rect, form: &mut CaptureForm, palette: &theme::Palette) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Left: step input (no border — the content itself provides structure).
    let input_area = chunks[0];
    // 1-cell inner padding via a margin block.
    let pad = Block::default().borders(Borders::NONE).style(Style::default().bg(palette.panel_bg));
    let input_inner = pad.inner(input_area);
    f.render_widget(pad, input_area);

    match form.step {
        Step::Name => draw_name(f, input_inner, form, palette),
        Step::MatchGlobs => draw_match_globs(f, input_inner, form, palette),
        Step::CommandPolicy => draw_command_policy(f, input_inner, form, palette),
        Step::CwdPolicy => draw_cwd_policy(f, input_inner, form, palette),
        Step::TabNames => draw_tab_names(f, input_inner, form, palette),
        Step::Review => draw_review_input(f, input_inner, form, palette),
        Step::ClashPrompt => draw_clash_prompt(f, input_inner, form, palette),
        Step::EditorPrompt => draw_editor_prompt(f, input_inner, form, palette),
        _ => {}
    }

    // Right: live YAML preview (on every step).
    draw_preview(f, chunks[1], form, palette);
}

// ── Per-step input rendering ─────────────────────────────────────

fn draw_scope_confirm(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Workspace summary",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "This is the live structure from the herdr daemon.",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  workspace   ", Style::default().fg(palette.overlay0)),
            Span::styled(
                format!("{} ({})", form.raw.workspace_label, form.raw.workspace_id),
                Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  tabs        ", Style::default().fg(palette.overlay0)),
            Span::styled(format!("{}", form.tab_labels.len()), Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled("  panes       ", Style::default().fg(palette.overlay0)),
            Span::styled(
                format!("{}", form.raw.pane_commands.len()),
                Style::default().fg(palette.text),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled("  per-tab breakdown:", Style::default().fg(palette.subtext0))),
    ];

    for (i, label) in form.tab_labels.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(format!("    tab {}  ", i + 1), Style::default().fg(palette.accent)),
            Span::styled(
                format!("{:<16}", if label.is_empty() { "(unnamed)" } else { label }),
                Style::default().fg(palette.text),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Enter to continue · Esc to abort",
        Style::default().fg(palette.overlay0),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_name(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    // Label.
    let mut lines = vec![
        Line::from(Span::styled(
            "Template name",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "The filename stem and the `name:` field in the YAML.",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
    ];

    // Framed input field — occupies rows 3,4,5 (y+3, height 3).
    let field_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if form.name.trim().is_empty() {
            palette.red
        } else {
            palette.surface1
        }))
        .style(Style::default().bg(palette.surface0));
    let field_area = Rect {
        x: area.x,
        y: area.y + 3,
        width: area.width,
        height: 3,
    };
    f.render_widget(field_block, field_area);
    let field_inner = Rect {
        x: field_area.x + 1,
        y: field_area.y + 1,
        width: field_area.width.saturating_sub(2),
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{}▍", form.name),
                Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
            ),
        ])),
        field_inner,
    );

    // Pad 3 blank lines for the field height (rows 3,4,5) + 1 blank
    // for visual separation before the hint (row 6).
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));

    // Validation hint (row 7).
    if form.name.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "  ⚠ name is required to continue",
            Style::default().fg(palette.peach),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  → writes to ~/.config/herdr/templates/{}.yaml", form.name.trim()),
            Style::default().fg(palette.overlay0),
        )));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_match_globs(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Match globs",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Glob patterns that auto-preselect this template (e.g. `**/Cargo.toml`).",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
    ];

    // Framed input field — occupies rows 3,4,5 (y+3, height 3).
    let field_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.surface1))
        .style(Style::default().bg(palette.surface0));
    let field_area = Rect {
        x: area.x,
        y: area.y + 3,
        width: area.width,
        height: 3,
    };
    f.render_widget(field_block, field_area);
    let field_inner = Rect {
        x: field_area.x + 1,
        y: field_area.y + 1,
        width: field_area.width.saturating_sub(2),
        height: 1,
    };
    let globs_display = if form.match_globs.is_empty() {
        "(none — leave empty for no auto-match)"
    } else {
        &form.match_globs
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{}▍", globs_display),
                Style::default().fg(if form.match_globs.is_empty() {
                    palette.overlay0
                } else {
                    palette.text
                }),
            ),
        ])),
        field_inner,
    );

    // Pad 3 blank lines for the field height (rows 3,4,5) + 1 blank
    // for visual separation before the toggle (row 6).
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));

    // Default flag toggle (row 7).
    let default_marker = if form.default_flag { "✔" } else { "○" };
    let default_style = if form.default_flag {
        Style::default().fg(palette.green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.subtext0)
    };
    lines.push(Line::from(vec![
        Span::styled(format!("  {default_marker}  "), default_style),
        Span::styled("default: true", default_style),
        Span::styled(
            "  (fallback template when no match glob fits)",
            Style::default().fg(palette.overlay0),
        ),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Tab toggles the default flag",
        Style::default().fg(palette.overlay0),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_command_policy(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Command policy",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "How to fill each pane's `command:` from the running process group.",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
    ];

    for (i, (label, desc)) in COMMAND_CHOICES.iter().enumerate() {
        let is_selected = i == form.command_policy_idx;
        let marker = if is_selected { "▶" } else { " " };
        let label_style = if is_selected {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
                .bg(palette.selection_bg)
        } else {
            Style::default().fg(palette.subtext0)
        };
        let desc_style = if is_selected {
            Style::default().fg(palette.text).bg(palette.selection_bg)
        } else {
            Style::default().fg(palette.overlay0)
        };
        // Label line with selection background.
        let label_line = format!(" {} {:<6} ", marker, label);
        lines.push(Line::from(Span::styled(label_line, label_style)));
        // Description line, indented.
        lines.push(Line::from(vec![
            Span::styled("   ", Style::default().bg(if is_selected { palette.selection_bg } else { palette.panel_bg })),
            Span::styled(*desc, desc_style),
        ]));
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled(
        "  ↑↓ move · Enter to continue",
        Style::default().fg(palette.overlay0),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_cwd_policy(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "cwd policy",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "How to handle each pane's working directory in the generated template.",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
    ];

    for (i, (label, desc)) in CWD_CHOICES.iter().enumerate() {
        let is_selected = i == form.cwd_policy_idx;
        let marker = if is_selected { "▶" } else { " " };
        let label_style = if is_selected {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
                .bg(palette.selection_bg)
        } else {
            Style::default().fg(palette.subtext0)
        };
        let desc_style = if is_selected {
            Style::default().fg(palette.text).bg(palette.selection_bg)
        } else {
            Style::default().fg(palette.overlay0)
        };
        let label_line = format!(" {} {:<10} ", marker, label);
        lines.push(Line::from(Span::styled(label_line, label_style)));
        lines.push(Line::from(vec![
            Span::styled("   ", Style::default().bg(if is_selected { palette.selection_bg } else { palette.panel_bg })),
            Span::styled(*desc, desc_style),
        ]));
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled(
        "  ↑↓ move · Enter to continue",
        Style::default().fg(palette.overlay0),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_tab_names(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Tab names",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Pre-filled from live labels. ↑↓ to focus a tab, type to edit.",
            Style::default().fg(palette.overlay0),
        )),
        Line::raw(""),
    ];

    for (i, label) in form.tab_labels.iter().enumerate() {
        let is_focused = i == form.focused_tab_idx;
        let marker = if is_focused { "▶" } else { " " };
        let style = if is_focused {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
                .bg(palette.selection_bg)
        } else {
            Style::default().fg(palette.subtext0)
        };
        let cursor = if is_focused { "▍" } else { "" };
        let display = if label.is_empty() { "(unnamed)" } else { label.as_str() };
        lines.push(Line::from(Span::styled(
            format!(" {} tab {}: {}{} ", marker, i + 1, display, cursor),
            style,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  ↑↓ focus · type to edit · Enter to continue",
        Style::default().fg(palette.overlay0),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_review_input(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Ready to write",
            Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
    ];

    // Summary as a definition list with aligned keys.
    let summary = vec![
        ("name", form.name.trim()),
        ("default", if form.default_flag { "true" } else { "false" }),
        ("command", COMMAND_CHOICES[form.command_policy_idx].0),
        ("cwd", CWD_CHOICES[form.cwd_policy_idx].0),
    ];
    for (key, val) in &summary {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<10} ", key), Style::default().fg(palette.overlay0)),
            Span::styled(*val, Style::default().fg(palette.text)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  ↑↓ scroll the preview →",
        Style::default().fg(palette.overlay0),
    )));
    lines.push(Line::from(Span::styled(
        "  Enter writes the file",
        Style::default().fg(palette.green).add_modifier(Modifier::BOLD),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

// ── Phase C5: clash + editor prompt rendering ──────────────────

fn draw_clash_prompt(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let mut lines = vec![
        Line::from(Span::styled(
            "⚠ Name clash",
            Style::default().fg(palette.red).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            format!(
                "A template named \"{}\" already exists at:",
                form.name.trim()
            ),
            Style::default().fg(palette.text),
        )),
        Line::from(Span::styled(
            format!("  ~/.config/herdr/templates/{}.yaml", form.name.trim()),
            Style::default().fg(palette.subtext0),
        )),
        Line::raw(""),
        Line::raw(""),
    ];

    let choices = [
        ("o", "overwrite", "replace the existing file", palette.peach),
        ("r", "rename", "go back and pick a new name", palette.accent),
        ("c", "cancel", "abort without writing", palette.subtext0),
    ];
    for (key, label, desc, color) in &choices {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", key), Style::default().fg(*color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<10} ", label), Style::default().fg(palette.text)),
            Span::styled(*desc, Style::default().fg(palette.overlay0)),
        ]));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        "  Esc also cancels",
        Style::default().fg(palette.overlay0),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_editor_prompt(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let path = form.written_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let mut lines = vec![
        Line::from(Span::styled(
            "✓ Template written",
            Style::default().fg(palette.green).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            format!("  {}", path),
            Style::default().fg(palette.subtext0),
        )),
        Line::raw(""),
        Line::raw(""),
        Line::from(Span::styled(
            "Open it in your editor to fine-tune?",
            Style::default().fg(palette.text),
        )),
        Line::raw(""),
    ];

    let choices = [
        ("y", "yes", format!("open in {} (replaces this pane)", editor), palette.green),
        ("n", "no", "close the popup".to_string(), palette.subtext0),
    ];
    for (key, label, desc, color) in &choices {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", key), Style::default().fg(*color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<6} ", label), Style::default().fg(palette.text)),
            Span::styled(desc, Style::default().fg(palette.overlay0)),
        ]));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        "  Enter also closes",
        Style::default().fg(palette.overlay0),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_preview(f: &mut ratatui::Frame, area: Rect, form: &mut CaptureForm, palette: &theme::Palette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.surface1))
        .title(Span::styled(" Live preview ", Style::default().fg(palette.subtext0)))
        .style(Style::default().bg(palette.surface0));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build the live YAML from the current form state.
    let yaml = match form.build_preview_yaml() {
        Some(y) => y,
        None => {
            let lines = vec![
                Line::from(Span::styled(
                    "(enter a name to see the preview)",
                    Style::default().fg(palette.overlay0),
                )),
            ];
            f.render_widget(Paragraph::new(lines), inner);
            return;
        }
    };

    // Syntax-highlight + scroll.
    let all_lines: Vec<Line> = yaml
        .lines()
        .map(|line| highlight_yaml_line(line, palette))
        .collect();

    let visible_height = inner.height as usize;
    let max_scroll = all_lines.len().saturating_sub(visible_height);
    form.preview_scroll = form.preview_scroll.min(max_scroll);
    let start = form.preview_scroll;
    let end = (start + visible_height).min(all_lines.len());
    let visible: Vec<Line> = all_lines[start..end].to_vec();

    f.render_widget(Paragraph::new(visible), inner);
}

/// Simple YAML syntax highlighting for one line.
fn highlight_yaml_line(line: &str, palette: &theme::Palette) -> Line<'static> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    // Comment line.
    if trimmed.starts_with('#') {
        return Line::from(vec![
            Span::styled(" ".repeat(indent), Style::default()),
            Span::styled(trimmed.to_string(), Style::default().fg(palette.green)),
        ]);
    }

    // List item: `- value` or `- key: value`.
    if let Some(rest) = trimmed.strip_prefix("- ") {
        let dash_style = Style::default().fg(palette.subtext0);
        if let Some((key, val)) = rest.split_once(':') {
            let val = val.trim_start();
            return Line::from(vec![
                Span::styled(" ".repeat(indent), Style::default()),
                Span::styled("- ", dash_style),
                Span::styled(format!("{}:", key), Style::default().fg(palette.accent)),
                Span::styled(" ", Style::default()),
                Span::styled(val.to_string(), Style::default().fg(palette.text)),
            ]);
        }
        return Line::from(vec![
            Span::styled(" ".repeat(indent), Style::default()),
            Span::styled("- ", dash_style),
            Span::styled(rest.to_string(), Style::default().fg(palette.text)),
        ]);
    }

    // Key: value line.
    if let Some((key, val)) = trimmed.split_once(':') {
        let val = val.trim_start();
        let mut spans = vec![
            Span::styled(" ".repeat(indent), Style::default()),
            Span::styled(format!("{}:", key), Style::default().fg(palette.accent)),
        ];
        if !val.is_empty() {
            spans.push(Span::styled(" ", Style::default()));
            spans.push(Span::styled(val.to_string(), Style::default().fg(palette.text)));
        }
        return Line::from(spans);
    }

    Line::from(Span::styled(line.to_string(), Style::default().fg(palette.text)))
}

// ── Footer ──────────────────────────────────────────────────────

fn draw_footer(f: &mut ratatui::Frame, area: Rect, form: &CaptureForm, palette: &theme::Palette) {
    let (left, right) = match form.step {
        Step::ScopeConfirm => (
            vec![Span::styled("esc abort", Style::default().fg(palette.overlay0))],
            vec![Span::styled(
                "⏎ continue",
                Style::default().fg(palette.subtext0),
            )],
        ),
        Step::Review => (
            vec![
                Span::styled("← back   ", Style::default().fg(palette.overlay0)),
                Span::styled("esc abort", Style::default().fg(palette.overlay0)),
            ],
            vec![
                Span::styled("↑↓ scroll   ", Style::default().fg(palette.overlay0)),
                Span::styled(
                    "⏎ write",
                    Style::default().fg(palette.green).add_modifier(Modifier::BOLD),
                ),
            ],
        ),
        Step::ClashPrompt => (
            vec![Span::styled("esc cancel", Style::default().fg(palette.overlay0))],
            vec![Span::styled(
                "o overwrite   r rename   c cancel",
                Style::default().fg(palette.overlay0),
            )],
        ),
        Step::EditorPrompt => (
            vec![Span::styled("", Style::default())],
            vec![Span::styled(
                "y open editor   n close",
                Style::default().fg(palette.overlay0),
            )],
        ),
        _ => (
            vec![
                Span::styled("← back   ", Style::default().fg(palette.overlay0)),
                Span::styled("esc abort", Style::default().fg(palette.overlay0)),
            ],
            vec![Span::styled(
                "⏎ continue",
                Style::default().fg(palette.subtext0),
            )],
        ),
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    f.render_widget(
        Paragraph::new(Line::from(left)).alignment(Alignment::Left),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        chunks[1],
    );
}
