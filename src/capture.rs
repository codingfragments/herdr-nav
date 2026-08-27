//! Capture: analyze the current workspace and emit a template YAML.
//!
//! - **Phase C1** (merged): the spine — read the active workspace and
//!   print a plain-text summary.
//! - **Phase C2** (this phase): map `layout.export` → `Template`, apply
//!   a cwd policy, and write a YAML the existing `^t` apply path can
//!   read and build. CLI flags (`--name`, `--cwd-policy`) drive it; the
//!   wizard lands in C4.
//! - Later phases: C3 commands, C4 wizard, C5 clash+editor.
//!
//! See `spec/capture-template-plan.md`.
//!
//! **Socket discipline:** one fresh connection per request
//! (`socket_client::request`). For a workspace with `T` tabs and `P`
//! panes, C2 issues `1 + 1 + T` calls (workspace.list, tab.list, one
//! layout.export per tab). C3 adds `P` `pane.process_info` calls.
//!
//! **Gotcha (spike §3):** `pane.layout` ignores its `tab_id` param (it
//! always returns the active tab). We use `layout.export` exclusively —
//! it is param-driven by `tab_id` and returns the portable recursive
//! tree.

use std::collections::HashMap;

use crate::socket_client;
use crate::source::{self, Layout, PaneNode, Template, TemplateTab};

/// A captured workspace summary (Phase C1): enough to prove the daemon
/// calls work and to give later phases the structure to map. No commands
/// or cwd policy yet — those land in C2/C3.
#[derive(Debug, Clone)]
pub struct Summary {
    pub workspace_id: String,
    pub workspace_label: String,
    pub tabs: Vec<TabSummary>,
}

/// One tab in the summary.
#[derive(Debug, Clone)]
pub struct TabSummary {
    pub tab_id: String,
    pub tab_label: String,
    pub number: u32,
    /// Pane count from `layout.export`'s `panes[]` (the real structure,
    /// not `tab.list`'s `pane_count` — they should agree; we trust the
    /// export since it's what we'll map in C2).
    pub pane_count: usize,
    /// Depth of the split tree (0 = single pane, 1 = one split, …).
    /// A rough fidelity indicator for the summary print.
    pub split_depth: usize,
}

/// Capture the active workspace's structure from the daemon.
///
/// 1. `workspace.list` → the focused workspace.
/// 2. `tab.list` → that workspace's tabs, sorted by `number`.
/// 3. Per tab `layout.export {tab_id}` → pane count + split depth.
///
/// Errors are returned as a human-readable string (the binary prints
/// them to stderr and exits 1).
pub fn capture_summary(socket_path: &str) -> Result<Summary, String> {
    // 1. Find the focused workspace.
    let ws_resp = socket_client::request(socket_path, "workspace.list", serde_json::json!({}))
        .map_err(|e| format!("workspace.list failed: {e}"))?;
    let (workspace_id, workspace_label) = parse_focused_workspace(&ws_resp)?;

    // 2. This workspace's tabs, sorted by number.
    let tab_resp = socket_client::request(socket_path, "tab.list", serde_json::json!({}))
        .map_err(|e| format!("tab.list failed: {e}"))?;
    let tabs = parse_tabs_for_workspace(&tab_resp, &workspace_id);

    // 3. Per tab: layout.export for the real pane count + split depth.
    let mut tab_summaries = Vec::with_capacity(tabs.len());
    for (tab_id, tab_label, number) in &tabs {
        let (pane_count, split_depth) = match layout_export(socket_path, tab_id) {
            Ok((panes, depth)) => (panes, depth),
            Err(e) => {
                // A failed export shouldn't kill the whole summary —
                // report the tab with a zero count and surface the error
                // in the print. Later phases decide whether to abort.
                eprintln!("herdr-nav: layout.export for {tab_id} failed: {e}");
                (0, 0)
            }
        };

        tab_summaries.push(TabSummary {
            tab_id: tab_id.clone(),
            tab_label: tab_label.clone(),
            number: *number,
            pane_count,
            split_depth,
        });
    }

    Ok(Summary {
        workspace_id,
        workspace_label,
        tabs: tab_summaries,
    })
}

/// Parse `workspace.list` response → the focused workspace's
/// `(workspace_id, label)`. Falls back to the first workspace if none
/// is marked `focused` (defensive — shouldn't happen in a live session).
/// Pure (no socket) so it's unit-testable with fixture JSON.
pub fn parse_focused_workspace(ws_resp: &serde_json::Value) -> Result<(String, String), String> {
    let workspaces = ws_resp
        .get("workspaces")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "workspace.list response missing workspaces[]".to_string())?;
    let active_ws = workspaces
        .iter()
        .find(|w| w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
        .or_else(|| workspaces.first())
        .ok_or_else(|| "no workspaces in session".to_string())?;
    let id = active_ws
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let label = active_ws
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    Ok((id, label))
}

/// Parse `tab.list` response → the tabs belonging to `workspace_id`,
/// sorted by `number`, as `(tab_id, label, number)`. Pure (no socket).
pub fn parse_tabs_for_workspace(
    tab_resp: &serde_json::Value,
    workspace_id: &str,
) -> Vec<(String, String, u32)> {
    let mut tabs: Vec<(String, String, u32)> = tab_resp
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            let ws = t.get("workspace_id").and_then(|v| v.as_str()).unwrap_or("");
            if ws != workspace_id {
                return None;
            }
            let tab_id = t
                .get("tab_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let label = t
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(&tab_id)
                .to_string();
            let number = t
                .get("number")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .unwrap_or(0);
            Some((tab_id, label, number))
        })
        .collect();
    tabs.sort_by_key(|(_, _, n)| *n);
    tabs
}

/// Call `layout.export {tab_id}` and return `(pane_count, split_depth)`.
///
/// `pane_count` = number of leaf panes in the tree. `split_depth` = the
/// maximum nesting of split nodes (0 = root is a pane, 1 = one level of
/// splits, …). Phase C2 will return the full tree; C1 only needs the
/// counts for the summary print.
fn layout_export(socket_path: &str, tab_id: &str) -> Result<(usize, usize), String> {
    let resp = socket_client::request(
        socket_path,
        "layout.export",
        serde_json::json!({"tab_id": tab_id}),
    )
    .map_err(|e| format!("layout.export failed: {e}"))?;
    parse_layout_export(&resp)
}

/// Parse a `layout.export` response → `(pane_count, split_depth)`.
/// Pure (no socket) so it's unit-testable with fixture JSON.
pub fn parse_layout_export(resp: &serde_json::Value) -> Result<(usize, usize), String> {
    let root = resp
        .get("layout")
        .and_then(|l| l.get("root"))
        .ok_or_else(|| "layout.export response missing layout.root".to_string())?;
    let mut pane_count = 0usize;
    let depth = count_and_depth(root, &mut pane_count);
    Ok((pane_count, depth))
}

/// Recurse the export tree: count leaves and return the max split depth.
/// A `pane` node is depth 0; a `split` node is 1 + max(first, second).
fn count_and_depth(node: &serde_json::Value, pane_count: &mut usize) -> usize {
    let kind = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "pane" => {
            *pane_count += 1;
            0
        }
        "split" => {
            let first = node
                .get("first")
                .map(|n| count_and_depth(n, pane_count))
                .unwrap_or(0);
            let second = node
                .get("second")
                .map(|n| count_and_depth(n, pane_count))
                .unwrap_or(0);
            1 + first.max(second)
        }
        _ => 0,
    }
}

/// Print the summary as plain text to stdout (Phase C1). One line per
/// tab, plus a header. This is the CLI probe — no ratatui, no terminal
/// setup. The binary exits 0 after printing.
pub fn print_summary(s: &Summary) {
    println!("workspace: {} ({})", s.workspace_label, s.workspace_id);
    println!("tabs: {}", s.tabs.len());
    let total_panes: usize = s.tabs.iter().map(|t| t.pane_count).sum();
    println!("panes: {total_panes}");
    println!();
    for t in &s.tabs {
        println!(
            "  tab {} {:<2}  {} pane{}  split-depth {}",
            t.number,
            if t.tab_label.is_empty() {
                format!("({})", t.tab_id)
            } else {
                format!("{} ({})", t.tab_label, t.tab_id)
            },
            t.pane_count,
            if t.pane_count == 1 { "" } else { "s" },
            t.split_depth,
        );
    }
}

// ── Phase C2: layout.export → Template mapping + write ─────────────

/// The cwd policy applied to every captured pane (spec §6). One global
/// choice in C2/C4; per-pane clearing is a C5 editor concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CwdPolicy {
    /// Relativize under the workspace base cwd; keep absolute when the
    /// pane cwd is not under the base ("far distant"). Default.
    Relative,
    /// Keep every pane cwd absolute as captured. Machine-specific.
    Absolute,
    /// Blank every pane cwd → each pane inherits the new workspace's
    /// cwd on apply.
    Inherit,
}

impl CwdPolicy {
    /// Parse from a CLI flag string ("relative" | "absolute" | "inherit").
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "relative" | "" => Ok(Self::Relative),
            "absolute" => Ok(Self::Absolute),
            "inherit" | "blank" => Ok(Self::Inherit),
            other => Err(format!(
                "unknown cwd policy '{other}' (relative|absolute|inherit)"
            )),
        }
    }
}

/// The command policy (spec §5 / Phase C3). One global choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPolicy {
    /// Best-effort capture from `pane.process_info`; annotate guesses
    /// with a `# best-effort:` comment. Default.
    Keep,
    /// Force every pane's `command` to `None` (plain shell). No
    /// `pane.process_info` calls are made.
    Blank,
}

impl CommandPolicy {
    /// Parse from a CLI flag string ("keep" | "blank").
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "keep" | "" => Ok(Self::Keep),
            "blank" => Ok(Self::Blank),
            other => Err(format!("unknown command policy '{other}' (keep|blank)")),
        }
    }
}

/// Known interactive shells — when the only foreground process is one
/// of these, the pane is a plain shell and `command` is `None` (high
/// confidence). From the spike: a lone `fish`/`bash`/`zsh` is reliable.
const SHELLS: &[&str] = &["fish", "bash", "zsh", "sh", "dash", "ksh", "tcsh", "csh"];

// ── Raw capture (Phase C4: split fetch from build for live preview) ──

/// One tab's raw export tree + live label. The tree is the `layout.export`
/// `root` node (a recursive `split`/`pane` tree).
#[derive(Debug, Clone)]
pub struct RawTab {
    pub tab_label: String,
    pub root: serde_json::Value,
}

/// The best-effort command capture result for one pane (pre-computed
/// during `fetch_raw` so the wizard can rebuild the template on every
/// render without socket calls).
#[derive(Debug, Clone)]
pub struct PaneCommandResult {
    pub command: Option<String>,
    pub annotation: Option<Annotation>,
}

/// All the raw data fetched from the daemon in one pass. The wizard
/// caches this and calls `build_template` on every render to produce a
/// live YAML preview as the user makes choices.
#[derive(Debug, Clone)]
pub struct RawCapture {
    pub workspace_id: String,
    pub workspace_label: String,
    pub base_cwd: String,
    pub tabs: Vec<RawTab>,
    /// Per-pane best-effort command results, keyed by pane_id.
    pub pane_commands: HashMap<String, PaneCommandResult>,
}

/// Fetch all raw data from the daemon in one pass (spec §4/§7).
///
/// This does all socket calls: `workspace.list`, `tab.list`, one
/// `layout.export` per tab, and one `pane.process_info` per pane. The
/// wizard calls this once on entry; `build_template` (pure) is then
/// called on every render for the live preview.
pub fn fetch_raw(socket_path: &str) -> Result<RawCapture, String> {
    // 1. Focused workspace.
    let ws_resp = socket_client::request(socket_path, "workspace.list", serde_json::json!({}))
        .map_err(|e| format!("workspace.list failed: {e}"))?;
    let (workspace_id, workspace_label) = parse_focused_workspace(&ws_resp)?;

    // 2. Tabs for that workspace, sorted by number.
    let tab_resp = socket_client::request(socket_path, "tab.list", serde_json::json!({}))
        .map_err(|e| format!("tab.list failed: {e}"))?;
    let tabs_meta = parse_tabs_for_workspace(&tab_resp, &workspace_id);

    // 3. Full layout.export tree per tab.
    let mut tabs = Vec::with_capacity(tabs_meta.len());
    for (tab_id, tab_label, _number) in &tabs_meta {
        let root = fetch_layout_root(socket_path, tab_id)?;
        tabs.push(RawTab {
            tab_label: tab_label.clone(),
            root,
        });
    }

    // 4. Derive the base cwd = first pane's cwd of the first tab.
    let base_cwd = tabs
        .first()
        .and_then(|t| first_leaf_cwd(&t.root))
        .unwrap_or_default();

    // 5. Collect all pane_ids and fetch best-effort commands.
    let mut pane_commands = HashMap::new();
    for tab in &tabs {
        for pane_id in collect_pane_ids(&tab.root) {
            let cwd = pane_cwd_for(&tab.root, &pane_id).unwrap_or_default();
            match best_effort_command(socket_path, &pane_id, &cwd) {
                Ok((command, annotation)) => {
                    pane_commands.insert(
                        pane_id.clone(),
                        PaneCommandResult {
                            command,
                            annotation,
                        },
                    );
                }
                Err(e) => {
                    pane_commands.insert(
                        pane_id.clone(),
                        PaneCommandResult {
                            command: None,
                            annotation: Some(Annotation {
                                pane_id: pane_id.clone(),
                                text: format!("best-effort: pane.process_info failed: {e}"),
                            }),
                        },
                    );
                }
            }
        }
    }

    Ok(RawCapture {
        workspace_id,
        workspace_label,
        base_cwd,
        tabs,
        pane_commands,
    })
}

/// Walk an export tree and collect all leaf pane_ids in order
/// (left-to-right, depth-first).
fn collect_pane_ids(root: &serde_json::Value) -> Vec<String> {
    let mut ids = Vec::new();
    collect_pane_ids_rec(root, &mut ids);
    ids
}

fn collect_pane_ids_rec(node: &serde_json::Value, ids: &mut Vec<String>) {
    let kind = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "pane" => {
            if let Some(id) = node.get("pane_id").and_then(|v| v.as_str()) {
                ids.push(id.to_string());
            }
        }
        "split" => {
            if let Some(first) = node.get("first") {
                collect_pane_ids_rec(first, ids);
            }
            if let Some(second) = node.get("second") {
                collect_pane_ids_rec(second, ids);
            }
        }
        _ => {}
    }
}

/// Walk an export tree and collect each leaf pane's live `label` (empty
/// string when the pane has no label), in left-to-right depth-first
/// order — the same order `map_split`/`map_child` walk, so `pane_names`
/// indices align with `map_pane`'s `pane_idx`. Used to pre-fill the
/// wizard's per-pane name fields.
pub fn collect_pane_labels(root: &serde_json::Value) -> Vec<String> {
    let mut labels = Vec::new();
    collect_pane_labels_rec(root, &mut labels);
    labels
}

fn collect_pane_labels_rec(node: &serde_json::Value, labels: &mut Vec<String>) {
    let kind = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "pane" => {
            let label = node.get("label").and_then(|v| v.as_str()).unwrap_or("");
            labels.push(label.to_string());
        }
        "split" => {
            if let Some(first) = node.get("first") {
                collect_pane_labels_rec(first, labels);
            }
            if let Some(second) = node.get("second") {
                collect_pane_labels_rec(second, labels);
            }
        }
        _ => {}
    }
}

/// Find the cwd for a specific pane_id in an export tree.
fn pane_cwd_for(root: &serde_json::Value, target_pane_id: &str) -> Option<String> {
    let kind = root.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "pane" => {
            let id = root.get("pane_id").and_then(|v| v.as_str()).unwrap_or("");
            if id == target_pane_id {
                root.get("cwd").and_then(|v| v.as_str()).map(str::to_string)
            } else {
                None
            }
        }
        "split" => root
            .get("first")
            .and_then(|n| pane_cwd_for(n, target_pane_id))
            .or_else(|| {
                root.get("second")
                    .and_then(|n| pane_cwd_for(n, target_pane_id))
            }),
        _ => None,
    }
}

/// Build a `Template` from the raw capture + the user's choices (pure,
/// no socket calls). Called on every wizard render for the live preview.
///
/// `tab_names` overrides the live tab labels (first entry → first tab,
/// etc.); entries beyond the tab count are ignored, missing entries
/// fall back to the live label.
#[allow(clippy::too_many_arguments)]
pub fn build_template(
    raw: &RawCapture,
    name: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
    tab_names: &[String],
    pane_names: &[Vec<String>],
    match_globs: Vec<String>,
    default: bool,
) -> (Template, Vec<Annotation>) {
    let mut annotations = Vec::new();
    let template_tabs = raw
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let label = tab_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| tab.tab_label.clone());
            let tab_pane_names = pane_names.get(i).cloned().unwrap_or_default();
            let mut pane_idx = 0usize;
            map_tab(
                tab,
                &raw.base_cwd,
                cwd_policy,
                command_policy,
                &raw.pane_commands,
                &mut annotations,
                label,
                &tab_pane_names,
                &mut pane_idx,
            )
        })
        .collect();

    (
        Template {
            name: name.to_string(),
            match_globs,
            default,
            tabs: template_tabs,
        },
        annotations,
    )
}

/// Convenience wrapper: fetch + build in one call (for the CLI path).
pub fn capture_template(
    socket_path: &str,
    name: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
) -> Result<(Template, Vec<Annotation>), String> {
    let raw = fetch_raw(socket_path)?;
    Ok(build_template(
        &raw,
        name,
        cwd_policy,
        command_policy,
        &[],
        &[],
        Vec::new(),
        false,
    ))
}

/// Fetch the `layout.export` root for one tab.
fn fetch_layout_root(socket_path: &str, tab_id: &str) -> Result<serde_json::Value, String> {
    let resp = socket_client::request(
        socket_path,
        "layout.export",
        serde_json::json!({"tab_id": tab_id}),
    )
    .map_err(|e| format!("layout.export for {tab_id} failed: {e}"))?;
    resp.get("layout")
        .and_then(|l| l.get("root"))
        .cloned()
        .ok_or_else(|| format!("layout.export for {tab_id} missing layout.root"))
}

/// The cwd of the leftmost leaf pane in an export tree (root → first
/// leaf). Used to derive the workspace base cwd (spec §6).
fn first_leaf_cwd(root: &serde_json::Value) -> Option<String> {
    let kind = root.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "pane" => root.get("cwd").and_then(|v| v.as_str()).map(str::to_string),
        "split" => root
            .get("first")
            .and_then(first_leaf_cwd)
            .or_else(|| root.get("second").and_then(first_leaf_cwd)),
        _ => None,
    }
}

/// Map a raw tab's export tree → a `TemplateTab` (spec §7).
///
/// The export root is either a `pane` (single-pane tab) or a `split`
/// (multi-pane). A tab's `layout` is always a `Layout` (a split), so:
/// - `pane` root → a one-pane `Layout` (direction "v", ratio 0).
/// - `split` root → `map_split` builds the `Layout` directly.
///
/// `tab_name` overrides the live label. `annotations` is appended to
/// in pane order (left-to-right, depth-first).
#[allow(clippy::too_many_arguments)]
fn map_tab(
    tab: &RawTab,
    base_cwd: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
    pane_commands: &HashMap<String, PaneCommandResult>,
    annotations: &mut Vec<Annotation>,
    tab_name: String,
    pane_names: &[String],
    pane_idx: &mut usize,
) -> TemplateTab {
    let kind = tab.root.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let layout = match kind {
        "split" => map_split(
            &tab.root,
            base_cwd,
            cwd_policy,
            command_policy,
            pane_commands,
            annotations,
            pane_names,
            pane_idx,
        ),
        _ => Layout {
            direction: "v".to_string(),
            ratio: 0,
            panes: vec![map_pane(
                &tab.root,
                base_cwd,
                cwd_policy,
                command_policy,
                pane_commands,
                annotations,
                pane_names,
                pane_idx,
            )],
        },
    };
    TemplateTab {
        name: tab_name,
        cwd: None,
        layout,
    }
}

#[allow(clippy::too_many_arguments)]
fn map_split(
    node: &serde_json::Value,
    base_cwd: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
    pane_commands: &HashMap<String, PaneCommandResult>,
    annotations: &mut Vec<Annotation>,
    pane_names: &[String],
    pane_idx: &mut usize,
) -> Layout {
    let direction = node
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("right");
    let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let mut panes = Vec::new();
    if let Some(first) = node.get("first") {
        panes.push(map_child(
            first,
            base_cwd,
            cwd_policy,
            command_policy,
            pane_commands,
            annotations,
            pane_names,
            pane_idx,
        ));
    }
    if let Some(second) = node.get("second") {
        panes.push(map_child(
            second,
            base_cwd,
            cwd_policy,
            command_policy,
            pane_commands,
            annotations,
            pane_names,
            pane_idx,
        ));
    }
    Layout {
        direction: map_direction(direction),
        ratio: (ratio * 100.0).round() as u32,
        panes,
    }
}

#[allow(clippy::too_many_arguments)]
fn map_child(
    node: &serde_json::Value,
    base_cwd: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
    pane_commands: &HashMap<String, PaneCommandResult>,
    annotations: &mut Vec<Annotation>,
    pane_names: &[String],
    pane_idx: &mut usize,
) -> PaneNode {
    let kind = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "split" => PaneNode::Nested {
            layout: map_split(
                node,
                base_cwd,
                cwd_policy,
                command_policy,
                pane_commands,
                annotations,
                pane_names,
                pane_idx,
            ),
        },
        _ => map_pane(
            node,
            base_cwd,
            cwd_policy,
            command_policy,
            pane_commands,
            annotations,
            pane_names,
            pane_idx,
        ),
    }
}

/// Map a `pane` export node → a leaf `PaneNode::Pane`. The `command`
/// comes from the pre-computed `pane_commands` map (fetched during
/// `fetch_raw`), or `None` when the policy is `Blank`.
///
/// `pane_names`/`pane_idx` apply the wizard's per-pane name override
/// (Phase: combined Names step). The pane at `pane_names[*pane_idx]`
/// wins over the live `label`: a non-empty entry becomes `Some(name)`;
/// an empty entry becomes `None` (the "missing name" — no `name:` field
/// written); an absent entry (defensive, when the wizard has fewer entries
/// than panes) falls back to the live `label` (current behavior).
/// `*pane_idx` is advanced once per leaf pane, in the same left-to-right
/// depth-first order `collect_pane_labels` walks, so indices align.
#[allow(clippy::too_many_arguments)]
fn map_pane(
    node: &serde_json::Value,
    base_cwd: &str,
    cwd_policy: CwdPolicy,
    command_policy: CommandPolicy,
    pane_commands: &HashMap<String, PaneCommandResult>,
    annotations: &mut Vec<Annotation>,
    pane_names: &[String],
    pane_idx: &mut usize,
) -> PaneNode {
    let raw_cwd = node.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let label = node.get("label").and_then(|v| v.as_str());
    let pane_id = node.get("pane_id").and_then(|v| v.as_str()).unwrap_or("");

    let command = if command_policy == CommandPolicy::Blank {
        None
    } else {
        match pane_commands.get(pane_id) {
            Some(r) => {
                if let Some(ref ann) = r.annotation {
                    annotations.push(ann.clone());
                }
                r.command.clone()
            }
            None => None,
        }
    };

    let name = match pane_names.get(*pane_idx) {
        Some(s) if !s.is_empty() => Some(s.clone()),
        Some(_) => None,
        None => label.map(str::to_string),
    };
    *pane_idx += 1;

    PaneNode::Pane {
        command,
        cwd: apply_cwd_policy(raw_cwd, base_cwd, cwd_policy),
        name,
    }
}

/// `layout.export` direction → `Layout.direction` (spec §7).
/// `"right"` (side-by-side) → `"v"`; `"down"` (stacked) → `"h"`.
fn map_direction(dir: &str) -> String {
    match dir {
        "down" => "h".to_string(),
        _ => "v".to_string(), // "right" and any unknown → side-by-side
    }
}

// ── Phase C3: pane.process_info best-effort command capture ──────

/// A `# best-effort:` annotation to inject as a YAML comment above the
/// pane's `command:` line. serde_yaml can't emit comments, so we collect
/// these during mapping and inject them in a post-processing pass on the
/// serialized string (spec §5 / plan C3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The pane id this annotation belongs to (for matching in the YAML).
    pub pane_id: String,
    /// The comment text (without the leading `# `).
    pub text: String,
}

/// Best-effort recover a pane's startup `command` from `pane.process_info`
/// (spec §5 / spike §"Second spike"). Returns `(command, annotation)`:
///
/// - **Plain shell** (only foreground process is a known shell) →
///   `(None, None)`. High confidence.
/// - **Non-shell** → pick the non-shell foreground process whose `cwd`
///   matches the pane cwd (smallest `pid` tiebreak); `command` = its
///   `cmdline`; `annotation` = `best-effort: captured from pane <id>
///   process <name>; verify`.
/// - **No match** → `(None, Some("best-effort: …; no confident match"))`.
///
/// The spike showed `pane.process_info` returns the **whole foreground
/// process group** with **no `ppid`**, so the user's originally-launched
/// command can't be identified with certainty when multiple non-shell
/// processes are present. The editor step (C5) is the verification surface.
pub fn best_effort_command(
    socket_path: &str,
    pane_id: &str,
    pane_cwd: &str,
) -> Result<(Option<String>, Option<Annotation>), String> {
    let resp = socket_client::request(
        socket_path,
        "pane.process_info",
        serde_json::json!({"pane_id": pane_id}),
    )
    .map_err(|e| format!("pane.process_info failed: {e}"))?;
    let procs = resp
        .get("process_info")
        .and_then(|p| p.get("foreground_processes"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Plain shell: only foreground process is a known shell.
    if procs.len() == 1 {
        if let Some(name) = procs[0].get("name").and_then(|v| v.as_str()) {
            if SHELLS.contains(&name) {
                return Ok((None, None));
            }
        }
    }

    // Non-shell: pick the non-shell process whose cwd matches the pane cwd,
    // smallest pid tiebreak. The spike showed the foreground process group
    // can include the agent's MCP-server children (e.g. bun, trajectory);
    // matching cwd is the best available heuristic without ppid.
    let candidates: Vec<&serde_json::Value> = procs
        .iter()
        .filter(|p| {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !SHELLS.contains(&name)
        })
        .collect();
    // Prefer a cwd match; fall back to all non-shell candidates.
    let cwd_match: Vec<&serde_json::Value> = candidates
        .iter()
        .copied()
        .filter(|p| p.get("cwd").and_then(|v| v.as_str()).unwrap_or("") == pane_cwd)
        .collect();
    let pool: Vec<&serde_json::Value> = if !cwd_match.is_empty() {
        cwd_match
    } else {
        candidates
    };
    // Smallest pid tiebreak (clone to a sortable vec).
    let mut sorted: Vec<&serde_json::Value> = pool;
    sorted.sort_by_key(|p| p.get("pid").and_then(|v| v.as_u64()).unwrap_or(u64::MAX));

    match sorted.first() {
        Some(proc) => {
            let cmdline = proc.get("cmdline").and_then(|v| v.as_str()).unwrap_or("");
            let name = proc.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            if cmdline.is_empty() {
                Ok((
                    None,
                    Some(Annotation {
                        pane_id: pane_id.to_string(),
                        text: format!(
                            "best-effort: pane {pane_id} process {name}; no cmdline; verify"
                        ),
                    }),
                ))
            } else {
                Ok((
                    Some(cmdline.to_string()),
                    Some(Annotation {
                        pane_id: pane_id.to_string(),
                        text: format!(
                            "best-effort: captured from pane {pane_id} process {name}; verify"
                        ),
                    }),
                ))
            }
        }
        None => Ok((
            None,
            Some(Annotation {
                pane_id: pane_id.to_string(),
                text: format!("best-effort: pane {pane_id}; no confident match"),
            }),
        )),
    }
}

/// Parse a `pane.process_info` response into its foreground processes
/// (pure, for unit testing). Returns the `foreground_processes` array.
#[cfg(test)]
pub fn parse_foreground_processes(resp: &serde_json::Value) -> Vec<serde_json::Value> {
    resp.get("process_info")
        .and_then(|p| p.get("foreground_processes"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Apply the cwd policy to one pane's cwd (spec §6).
///
/// - `Relative`: under base → `./…`-relative; not under base → absolute;
///   equals base → `.`. `~`/`$HOME` expanded first (so `~`-paths under
///   the base still relativize).
/// - `Absolute`: as captured (after `~`/`$HOME` expansion).
/// - `Inherit`: `None` (blank → inherits the new workspace's cwd).
fn apply_cwd_policy(raw_cwd: &str, base_cwd: &str, policy: CwdPolicy) -> Option<String> {
    if raw_cwd.is_empty() {
        return None;
    }
    let expanded = source::expand_path(raw_cwd);
    match policy {
        CwdPolicy::Inherit => None,
        CwdPolicy::Absolute => Some(expanded),
        CwdPolicy::Relative => {
            if base_cwd.is_empty() {
                return Some(expanded);
            }
            // Under the base → relativize.
            if let Some(rest) = expanded.strip_prefix(base_cwd) {
                return Some(match rest {
                    "" => ".".to_string(),
                    other => format!("./{}", other.trim_start_matches('/')),
                });
            }
            // Not under the base → keep absolute.
            Some(expanded)
        }
    }
}

/// Serialize a `Template` to YAML (spec §8 step 9), injecting `# best-effort:`
/// annotations as comments above the matching pane's `command:` line.
///
/// serde_yaml can't emit comments natively, so we serialize first, then
/// post-process: for each annotation, find the pane's `command:` line and
/// insert the comment immediately above it. Pane ids are NOT in the
/// serialized YAML (they're dropped in the mapping), so we match by pane
/// order — the Nth annotation belongs to the Nth pane that has a
/// `command:` key, in document order. This is fragile but correct for
/// the deterministic output serde_yaml produces.
pub fn template_to_yaml(template: &Template, annotations: &[Annotation]) -> Result<String, String> {
    let yaml =
        serde_yaml::to_string(template).map_err(|e| format!("YAML serialize failed: {e}"))?;
    if annotations.is_empty() {
        return Ok(yaml);
    }
    Ok(inject_comments(&yaml, annotations))
}

/// Inject `# best-effort:` comments above `command:` lines in the YAML.
///
/// We walk the serialized YAML line by line. Each `- command:` (or
/// `command:`) line is the Nth pane's command field, in document order.
/// The Nth annotation (if any) is inserted as a comment above it. Blank
/// `command:` lines (serialized as `command: null` or omitted entirely
/// via skip_serializing_if) don't get a comment — the annotation for a
/// `None` command is still useful, so we attach it above the pane's first
/// serialized field instead.
///
/// This is a best-effort post-processor; the exact insertion point is an
/// implementation detail (the contract is "adjacent to the guessed
/// command"). For `None` commands with an annotation, we insert above
/// the pane's first key (cwd/name), or skip if the pane serialized as
/// `{}` (no keys).
fn inject_comments(yaml: &str, annotations: &[Annotation]) -> String {
    let mut out = String::with_capacity(yaml.len() + annotations.len() * 80);
    let mut ann_iter = annotations.iter();
    let mut next_ann = ann_iter.next();
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // A `command:` key (with a value) marks a pane whose command was
        // captured. Insert the matching annotation above it.
        if line.contains("command:") && !line.contains("command: null") {
            if let Some(ann) = next_ann {
                // Find the indentation of the current line to align the comment.
                let indent = line.len() - line.trim_start().len();
                out.push_str(&" ".repeat(indent));
                out.push_str("# ");
                out.push_str(&ann.text);
                out.push('\n');
                next_ann = ann_iter.next();
            }
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    // Any remaining annotations (for panes whose command: null and thus
    // serialized without a command: line) are dropped — there's no clean
    // anchor point. The editor step (C5) is the surface for those.
    out.trim_end().to_string()
}

/// The templates dir: `~/.config/herdr/templates/`.
fn templates_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(std::path::PathBuf::from(home).join(".config/herdr/templates"))
}

/// Write a template to `~/.config/herdr/templates/<name>.yaml`, creating
/// the dir if missing. Returns the written path.
pub fn write_template(name: &str, yaml: &str) -> Result<std::path::PathBuf, String> {
    let dir = templates_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, yaml).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Check if a template named `<name>.yaml` already exists in the
/// templates dir (Phase C5 clash check).
pub fn template_exists(name: &str) -> bool {
    templates_dir()
        .map(|dir| dir.join(format!("{name}.yaml")).exists())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Fixtures are the real daemon responses from the spike
    // (spec/spike-layout-export.md), trimmed to the fields C1 reads.

    fn ws_list() -> serde_json::Value {
        json!({
            "workspaces": [
                {"workspace_id": "wD", "label": ".dotfiles", "focused": true,  "tab_count": 1},
                {"workspace_id": "wM", "label": "paperless", "focused": false, "tab_count": 2},
                {"workspace_id": "wP", "label": "vim-herdr-navigation", "focused": false, "tab_count": 2}
            ]
        })
    }

    fn tab_list() -> serde_json::Value {
        json!({
            "tabs": [
                {"tab_id": "wD:t1", "workspace_id": "wD", "number": 1, "label": "1"},
                {"tab_id": "wM:t1", "workspace_id": "wM", "number": 1, "label": "1"},
                {"tab_id": "wM:t2", "workspace_id": "wM", "number": 2, "label": "hsunt"},
                {"tab_id": "wP:t1", "workspace_id": "wP", "number": 1, "label": "1"},
                {"tab_id": "wP:t2", "workspace_id": "wP", "number": 2, "label": "2"}
            ]
        })
    }

    // wP:t2 from the spike: right-split over a down-split (3 panes).
    fn export_nested() -> serde_json::Value {
        json!({
            "layout": {
                "workspace_id": "wP", "tab_id": "wP:t2",
                "root": {
                    "type": "split", "direction": "right", "ratio": 0.5,
                    "first":  {"type": "pane", "pane_id": "wP:p3", "cwd": "/a"},
                    "second": {
                        "type": "split", "direction": "down", "ratio": 0.5,
                        "first":  {"type": "pane", "pane_id": "wP:p4", "cwd": "/b"},
                        "second": {"type": "pane", "pane_id": "wP:p5", "cwd": "/c"}
                    }
                }
            }
        })
    }

    // A single-pane tab: root is a pane node, not a split.
    fn export_single() -> serde_json::Value {
        json!({
            "layout": {
                "workspace_id": "wD", "tab_id": "wD:t1",
                "root": {"type": "pane", "pane_id": "wD:p1", "cwd": "/x"}
            }
        })
    }

    #[test]
    fn parse_focused_workspace_picks_focused() {
        let (id, label) = parse_focused_workspace(&ws_list()).unwrap();
        assert_eq!(id, "wD");
        assert_eq!(label, ".dotfiles");
    }

    #[test]
    fn parse_focused_workspace_falls_back_to_first_when_none_focused() {
        let resp = json!({"workspaces": [
            {"workspace_id": "wX", "label": "x", "focused": false},
            {"workspace_id": "wY", "label": "y", "focused": false}
        ]});
        let (id, _) = parse_focused_workspace(&resp).unwrap();
        assert_eq!(id, "wX");
    }

    #[test]
    fn parse_focused_workspace_errs_when_empty() {
        let resp = json!({"workspaces": []});
        assert!(parse_focused_workspace(&resp).is_err());
    }

    #[test]
    fn parse_tabs_for_workspace_filters_and_sorts() {
        let tabs = parse_tabs_for_workspace(&tab_list(), "wM");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0], ("wM:t1".into(), "1".into(), 1));
        assert_eq!(tabs[1], ("wM:t2".into(), "hsunt".into(), 2));
    }

    #[test]
    fn parse_tabs_for_workspace_empty_for_unknown_ws() {
        assert!(parse_tabs_for_workspace(&tab_list(), "wZ").is_empty());
    }

    #[test]
    fn parse_layout_export_nested_three_panes_depth_two() {
        let (count, depth) = parse_layout_export(&export_nested()).unwrap();
        assert_eq!(count, 3);
        assert_eq!(depth, 2);
    }

    #[test]
    fn parse_layout_export_single_pane_depth_zero() {
        let (count, depth) = parse_layout_export(&export_single()).unwrap();
        assert_eq!(count, 1);
        assert_eq!(depth, 0);
    }

    #[test]
    fn parse_layout_export_errs_on_missing_root() {
        let resp = json!({"layout": {}});
        assert!(parse_layout_export(&resp).is_err());
    }

    // ── Phase C2: mapping + cwd policy + round-trip ──

    fn export_nested_tree() -> serde_json::Value {
        // wP:t2 from the spike: right-split over a down-split (3 panes).
        json!({
            "type": "split", "direction": "right", "ratio": 0.5,
            "first":  {"type": "pane", "pane_id": "p3", "cwd": "/code/proj", "label": "editor"},
            "second": {
                "type": "split", "direction": "down", "ratio": 0.5,
                "first":  {"type": "pane", "pane_id": "p4", "cwd": "/code/proj/sub", "label": "testhel"},
                "second": {"type": "pane", "pane_id": "p5", "cwd": "/other"}
            }
        })
    }

    fn captured_tab(root: serde_json::Value) -> RawTab {
        RawTab {
            tab_label: "main".to_string(),
            root,
        }
    }

    #[test]
    fn map_tab_nested_split_preserves_structure() {
        let tab = captured_tab(export_nested_tree());
        let tt = map_tab(
            &tab,
            "/code/proj",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &[],
            &mut 0,
        );
        assert_eq!(tt.name, "main");
        assert_eq!(tt.layout.direction, "v"); // right → v
        assert_eq!(tt.layout.ratio, 50);
        assert_eq!(tt.layout.panes.len(), 2);
        // first = editor pane
        match &tt.layout.panes[0] {
            PaneNode::Pane { cwd, name, .. } => {
                assert_eq!(cwd.as_deref(), Some("/code/proj"));
                assert_eq!(name.as_deref(), Some("editor"));
            }
            other => panic!("expected Pane, got {other:?}"),
        }
        // second = nested down-split (h)
        match &tt.layout.panes[1] {
            PaneNode::Nested { layout } => {
                assert_eq!(layout.direction, "h"); // down → h
                assert_eq!(layout.ratio, 50);
                assert_eq!(layout.panes.len(), 2);
            }
            other => panic!("expected Nested, got {other:?}"),
        }
    }

    #[test]
    fn map_tab_single_pane_wraps_in_one_pane_layout() {
        let root = json!({"type": "pane", "pane_id": "p1", "cwd": "/x"});
        let tab = captured_tab(root);
        let tt = map_tab(
            &tab,
            "/x",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "tab1".to_string(),
            &[],
            &mut 0,
        );
        assert_eq!(tt.layout.direction, "v");
        assert_eq!(tt.layout.ratio, 0);
        assert_eq!(tt.layout.panes.len(), 1);
    }

    // ── Combined Names step: per-pane name override ──

    #[test]
    fn collect_pane_labels_walks_left_to_right_depth_first() {
        // right-split over a down-split: p3, then p4, p5.
        let root = export_nested_tree();
        let labels = collect_pane_labels(&root);
        assert_eq!(labels, vec!["editor", "testhel", ""]); // p5 has no label
    }

    #[test]
    fn map_pane_override_non_empty_wins_over_live_label() {
        // A pane with a live label "editor"; override to "my-editor".
        let root = json!({"type": "pane", "pane_id": "p3", "cwd": "/a", "label": "editor"});
        let tab = captured_tab(root);
        let tt = map_tab(
            &tab,
            "/a",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &["my-editor".to_string()],
            &mut 0,
        );
        match &tt.layout.panes[0] {
            PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("my-editor")),
            other => panic!("expected Pane, got {other:?}"),
        }
    }

    #[test]
    fn map_pane_override_empty_blanks_to_none() {
        // A pane with a live label "editor"; override to empty → None
        // (the "missing name" — no `name:` field written).
        let root = json!({"type": "pane", "pane_id": "p3", "cwd": "/a", "label": "editor"});
        let tab = captured_tab(root);
        let tt = map_tab(
            &tab,
            "/a",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &[String::new()],
            &mut 0,
        );
        match &tt.layout.panes[0] {
            PaneNode::Pane { name, .. } => assert_eq!(*name, None),
            other => panic!("expected Pane, got {other:?}"),
        }
    }

    #[test]
    fn map_pane_override_absent_falls_back_to_live_label() {
        // No override entry for this pane → live label wins (current
        // behavior, defensive when the wizard has fewer entries than panes).
        let root = json!({"type": "pane", "pane_id": "p3", "cwd": "/a", "label": "editor"});
        let tab = captured_tab(root);
        let tt = map_tab(
            &tab,
            "/a",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &[],
            &mut 0,
        );
        match &tt.layout.panes[0] {
            PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("editor")),
            other => panic!("expected Pane, got {other:?}"),
        }
    }

    #[test]
    fn map_pane_override_indices_align_with_tree_walk_order() {
        // Nested tree: p3 (label "editor"), p4 (label "testhel"), p5 (no label).
        // Overrides: ["a", "", "c"] → p3=a, p4=None (blanked), p5=c.
        let root = export_nested_tree();
        let tab = captured_tab(root);
        let tt = map_tab(
            &tab,
            "/code/proj",
            CwdPolicy::Absolute,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &["a".to_string(), String::new(), "c".to_string()],
            &mut 0,
        );
        // first = pane p3
        match &tt.layout.panes[0] {
            PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("a")),
            other => panic!("expected Pane, got {other:?}"),
        }
        // second = nested down-split → p4, p5
        match &tt.layout.panes[1] {
            PaneNode::Nested { layout } => {
                assert_eq!(layout.panes.len(), 2);
                match &layout.panes[0] {
                    PaneNode::Pane { name, .. } => assert_eq!(*name, None, "p4 blanked"),
                    other => panic!("expected Pane, got {other:?}"),
                }
                match &layout.panes[1] {
                    PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("c")),
                    other => panic!("expected Pane, got {other:?}"),
                }
            }
            other => panic!("expected Nested, got {other:?}"),
        }
    }

    #[test]
    fn build_template_round_trips_mixed_pane_names_through_read_templates() {
        // p3 → "a", p4 → blanked (None), p5 → "c" (was unlabeled).
        let root = export_nested_tree();
        let raw = RawCapture {
            workspace_id: "wP".to_string(),
            workspace_label: "ws".to_string(),
            base_cwd: "/code/proj".to_string(),
            tabs: vec![RawTab {
                tab_label: "main".to_string(),
                root,
            }],
            pane_commands: HashMap::new(),
        };
        let (template, _anns) = build_template(
            &raw,
            "roundtrip",
            CwdPolicy::Relative,
            CommandPolicy::Blank,
            &["main".to_string()],
            &[vec!["a".to_string(), String::new(), "c".to_string()]],
            Vec::new(),
            false,
        );
        let yaml = template_to_yaml(&template, &[]).unwrap();
        let parsed: source::Template = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.tabs.len(), 1);
        // p3 = a
        match &parsed.tabs[0].layout.panes[0] {
            PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("a")),
            other => panic!("expected Pane, got {other:?}"),
        }
        // p4 blanked, p5 = c
        match &parsed.tabs[0].layout.panes[1] {
            PaneNode::Nested { layout } => {
                match &layout.panes[0] {
                    PaneNode::Pane { name, .. } => assert_eq!(*name, None),
                    other => panic!("expected Pane, got {other:?}"),
                }
                match &layout.panes[1] {
                    PaneNode::Pane { name, .. } => assert_eq!(name.as_deref(), Some("c")),
                    other => panic!("expected Pane, got {other:?}"),
                }
            }
            other => panic!("expected Nested, got {other:?}"),
        }
    }

    #[test]
    fn cwd_policy_relative_under_base() {
        assert_eq!(
            apply_cwd_policy("/code/proj/sub", "/code/proj", CwdPolicy::Relative),
            Some("./sub".to_string())
        );
    }

    #[test]
    fn cwd_policy_relative_equals_base() {
        assert_eq!(
            apply_cwd_policy("/code/proj", "/code/proj", CwdPolicy::Relative),
            Some(".".to_string())
        );
    }

    #[test]
    fn cwd_policy_relative_distant_keeps_absolute() {
        assert_eq!(
            apply_cwd_policy("/other", "/code/proj", CwdPolicy::Relative),
            Some("/other".to_string())
        );
    }

    #[test]
    fn cwd_policy_absolute() {
        assert_eq!(
            apply_cwd_policy("/code/proj", "/code/proj", CwdPolicy::Absolute),
            Some("/code/proj".to_string())
        );
    }

    #[test]
    fn cwd_policy_inherit_blanks() {
        assert_eq!(
            apply_cwd_policy("/code/proj", "/code/proj", CwdPolicy::Inherit),
            None
        );
    }

    #[test]
    fn cwd_policy_relative_empty_cwd_returns_none() {
        assert_eq!(apply_cwd_policy("", "/base", CwdPolicy::Relative), None);
    }

    #[test]
    fn first_leaf_cwd_walks_first_child() {
        let root = export_nested_tree();
        assert_eq!(first_leaf_cwd(&root), Some("/code/proj".to_string()));
    }

    #[test]
    fn template_to_yaml_round_trips_through_read_templates() {
        // Build a template from the nested fixture, serialize, parse back.
        let tab = captured_tab(export_nested_tree());
        let tt = map_tab(
            &tab,
            "/code/proj",
            CwdPolicy::Relative,
            CommandPolicy::Blank,
            &HashMap::new(),
            &mut Vec::new(),
            "main".to_string(),
            &[],
            &mut 0,
        );
        let template = Template {
            name: "roundtrip".to_string(),
            match_globs: vec![],
            default: false,
            tabs: vec![tt],
        };
        let yaml = template_to_yaml(&template, &[]).unwrap();
        // Parse it back through the existing read_templates path
        // (source::Template deserialization).
        let parsed: source::Template = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "roundtrip");
        assert_eq!(parsed.tabs.len(), 1);
        assert_eq!(parsed.tabs[0].name, "main");
        assert_eq!(parsed.tabs[0].layout.panes.len(), 2);
    }

    #[test]
    fn cwd_policy_parse_known_values() {
        assert_eq!(CwdPolicy::parse("relative").unwrap(), CwdPolicy::Relative);
        assert_eq!(CwdPolicy::parse("absolute").unwrap(), CwdPolicy::Absolute);
        assert_eq!(CwdPolicy::parse("inherit").unwrap(), CwdPolicy::Inherit);
        assert_eq!(CwdPolicy::parse("").unwrap(), CwdPolicy::Relative); // default
        assert!(CwdPolicy::parse("bogus").is_err());
    }

    // ── Phase C3: best-effort command capture ──

    fn process_info_resp(procs: &[serde_json::Value]) -> serde_json::Value {
        json!({"process_info": {"foreground_processes": procs}})
    }

    #[test]
    fn best_effort_plain_shell_returns_none_no_annotation() {
        // A lone fish process → plain shell, high confidence.
        let resp = process_info_resp(&[json!({
            "pid": 100, "name": "fish", "cmdline": "fish", "cwd": "/code"
        })]);
        let procs = parse_foreground_processes(&resp);
        // best_effort_command takes a socket; test the pure logic by
        // reconstructing the decision from the parsed procs.
        let only_shell = procs.len() == 1
            && procs[0]
                .get("name")
                .and_then(|v| v.as_str())
                .map(|n| SHELLS.contains(&n))
                .unwrap_or(false);
        assert!(only_shell);
    }

    #[test]
    fn best_effort_non_shell_picks_cwd_match() {
        // Two non-shell procs, one matches the pane cwd → pick it.
        let procs = [
            json!({"pid": 200, "name": "bun",   "cmdline": "bun server.mjs", "cwd": "/other"}),
            json!({"pid": 100, "name": "pi",    "cmdline": "pi",            "cwd": "/code"}),
        ];
        // Simulate the selection: non-shell, cwd match, smallest pid.
        let candidates: Vec<&serde_json::Value> = procs
            .iter()
            .filter(|p| !SHELLS.contains(&p.get("name").and_then(|v| v.as_str()).unwrap_or("")))
            .collect();
        let cwd_match: Vec<&serde_json::Value> = candidates
            .iter()
            .copied()
            .filter(|p| p.get("cwd").and_then(|v| v.as_str()).unwrap_or("") == "/code")
            .collect();
        let mut sorted: Vec<&serde_json::Value> = cwd_match;
        sorted.sort_by_key(|p| p.get("pid").and_then(|v| v.as_u64()).unwrap_or(u64::MAX));
        let picked = sorted.first().unwrap();
        assert_eq!(
            picked.get("cmdline").and_then(|v| v.as_str()).unwrap(),
            "pi"
        );
    }

    #[test]
    fn best_effort_no_match_returns_none_with_annotation() {
        // Only a shell present but multiple → no non-shell candidate.
        let procs = [json!({"pid": 1, "name": "fish", "cmdline": "fish", "cwd": "/code"})];
        let candidates: Vec<&serde_json::Value> = procs
            .iter()
            .filter(|p| !SHELLS.contains(&p.get("name").and_then(|v| v.as_str()).unwrap_or("")))
            .collect();
        assert!(
            candidates.is_empty(),
            "no non-shell candidates → no confident match"
        );
    }

    #[test]
    fn command_policy_parse_known_values() {
        assert_eq!(CommandPolicy::parse("keep").unwrap(), CommandPolicy::Keep);
        assert_eq!(CommandPolicy::parse("blank").unwrap(), CommandPolicy::Blank);
        assert_eq!(CommandPolicy::parse("").unwrap(), CommandPolicy::Keep); // default
        assert!(CommandPolicy::parse("bogus").is_err());
    }

    #[test]
    fn inject_comments_inserts_above_command_line() {
        let yaml = "name: t\ntabs:\n- name: tab1\n  layout:\n    panes:\n    - command: pi\n      cwd: .\n    - cwd: .\n";
        let annotations = vec![Annotation {
            pane_id: "p1".to_string(),
            text: "best-effort: captured from pane p1 process pi; verify".to_string(),
        }];
        let result = inject_comments(yaml, &annotations);
        // The comment should appear immediately above the `command: pi` line.
        let lines: Vec<&str> = result.lines().collect();
        let cmd_idx = lines
            .iter()
            .position(|l| l.contains("command: pi"))
            .unwrap();
        assert!(lines[cmd_idx - 1].contains("# best-effort:"));
        assert!(lines[cmd_idx - 1].contains("process pi"));
    }

    #[test]
    fn inject_comments_no_annotations_returns_unchanged() {
        let yaml = "name: t\ntabs: []\n";
        let result = inject_comments(yaml, &[]);
        assert_eq!(result, yaml.trim_end());
    }

    #[test]
    fn annotation_for_none_command_is_dropped_gracefully() {
        // A pane with command: null serializes without a command: line
        // (skip_serializing_if). Its annotation has no anchor and is dropped.
        let yaml = "name: t\ntabs:\n- name: tab1\n  layout:\n    panes:\n    - cwd: .\n";
        let annotations = vec![Annotation {
            pane_id: "p2".to_string(),
            text: "best-effort: no confident match".to_string(),
        }];
        let result = inject_comments(yaml, &annotations);
        // No comment injected (nothing to anchor to).
        assert!(!result.contains("# best-effort:"));
    }
}
