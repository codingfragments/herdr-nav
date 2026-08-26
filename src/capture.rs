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
            let tab_id = t.get("tab_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let label = t.get("label").and_then(|v| v.as_str()).unwrap_or(&tab_id).to_string();
            let number = t.get("number").and_then(|v| v.as_u64()).map(|n| n as u32).unwrap_or(0);
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
fn layout_export(
    socket_path: &str,
    tab_id: &str,
) -> Result<(usize, usize), String> {
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
            let first = node.get("first").map(|n| count_and_depth(n, pane_count)).unwrap_or(0);
            let second = node.get("second").map(|n| count_and_depth(n, pane_count)).unwrap_or(0);
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
            other => Err(format!("unknown cwd policy '{other}' (relative|absolute|inherit)")),
        }
    }
}

/// A captured tab's full export tree (the `layout.export` root) plus its
/// live label, ready to map to a `TemplateTab`.
struct CapturedTab {
    tab_label: String,
    root: serde_json::Value,
}

/// Capture the active workspace and emit a `Template` (spec §4/§7).
///
/// This is the C2 entry point: it reads the workspace, fetches the full
/// `layout.export` tree per tab, derives the base cwd, applies the cwd
/// policy, and assembles a `Template`. Command capture is C3 — every
/// pane's `command` is `None` here.
pub fn capture_template(
    socket_path: &str,
    name: &str,
    cwd_policy: CwdPolicy,
) -> Result<Template, String> {
    // 1. Focused workspace.
    let ws_resp = socket_client::request(socket_path, "workspace.list", serde_json::json!({}))
        .map_err(|e| format!("workspace.list failed: {e}"))?;
    let (workspace_id, _workspace_label) = parse_focused_workspace(&ws_resp)?;

    // 2. Tabs for that workspace, sorted by number.
    let tab_resp = socket_client::request(socket_path, "tab.list", serde_json::json!({}))
        .map_err(|e| format!("tab.list failed: {e}"))?;
    let tabs_meta = parse_tabs_for_workspace(&tab_resp, &workspace_id);

    // 3. Full layout.export tree per tab.
    let mut captured = Vec::with_capacity(tabs_meta.len());
    for (tab_id, tab_label, _number) in &tabs_meta {
        let root = fetch_layout_root(socket_path, tab_id)?;
        captured.push(CapturedTab {
            tab_label: tab_label.clone(),
            root,
        });
    }

    // 4. Derive the base cwd = first pane's cwd of the first tab
    //    (spec §6). The first tab is tabs_meta[0] (sorted by number);
    //    its first pane is the leftmost leaf of its export tree.
    let base_cwd = captured
        .first()
        .and_then(|t| first_leaf_cwd(&t.root))
        .unwrap_or_default();

    // 5. Map each tab → TemplateTab, applying the cwd policy.
    let template_tabs = captured
        .iter()
        .map(|t| map_tab(t, &base_cwd, cwd_policy))
        .collect();

    Ok(Template {
        name: name.to_string(),
        match_globs: Vec::new(),
        default: false,
        tabs: template_tabs,
    })
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
        "pane" => root
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "split" => root
            .get("first")
            .and_then(first_leaf_cwd)
            .or_else(|| root.get("second").and_then(first_leaf_cwd)),
        _ => None,
    }
}

/// Map a captured tab's export tree → a `TemplateTab` (spec §7).
///
/// The export root is either a `pane` (single-pane tab) or a `split`
/// (multi-pane). A tab's `layout` is always a `Layout` (a split), so:
/// - `pane` root → a one-pane `Layout` (direction "v", ratio 0).
/// - `split` root → `map_split` builds the `Layout` directly.
fn map_tab(tab: &CapturedTab, base_cwd: &str, policy: CwdPolicy) -> TemplateTab {
    let kind = tab.root.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let layout = match kind {
        "split" => map_split(&tab.root, base_cwd, policy),
        // Single-pane tab (or unknown): wrap the pane in a one-pane Layout.
        _ => Layout {
            direction: "v".to_string(),
            ratio: 0,
            panes: vec![map_pane(&tab.root, base_cwd, policy)],
        },
    };
    TemplateTab {
        name: tab.tab_label.clone(),
        cwd: None,
        layout,
    }
}

/// Map a `split` export node → a `Layout`. Its `first`/`second` children
/// are mapped via `map_child` (pane → leaf, split → nested split).
fn map_split(node: &serde_json::Value, base_cwd: &str, policy: CwdPolicy) -> Layout {
    let direction = node.get("direction").and_then(|v| v.as_str()).unwrap_or("right");
    let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let mut panes = Vec::new();
    if let Some(first) = node.get("first") {
        panes.push(map_child(first, base_cwd, policy));
    }
    if let Some(second) = node.get("second") {
        panes.push(map_child(second, base_cwd, policy));
    }
    Layout {
        direction: map_direction(direction),
        ratio: (ratio * 100.0).round() as u32,
        panes,
    }
}

/// Map a child of a split → a `PaneNode`. A `pane` → leaf; a `split` →
/// `Nested{layout}` (recursive).
fn map_child(node: &serde_json::Value, base_cwd: &str, policy: CwdPolicy) -> PaneNode {
    let kind = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "split" => PaneNode::Nested {
            layout: map_split(node, base_cwd, policy),
        },
        _ => map_pane(node, base_cwd, policy),
    }
}

/// Map a `pane` export node → a leaf `PaneNode::Pane`. `command` is
/// `None` in C2 (C3 fills it from `pane.process_info`).
fn map_pane(node: &serde_json::Value, base_cwd: &str, policy: CwdPolicy) -> PaneNode {
    let raw_cwd = node.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let label = node.get("label").and_then(|v| v.as_str());
    PaneNode::Pane {
        command: None,
        cwd: apply_cwd_policy(raw_cwd, base_cwd, policy),
        name: label.map(str::to_string),
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

/// Serialize a `Template` to YAML (spec §8 step 9).
pub fn template_to_yaml(template: &Template) -> Result<String, String> {
    serde_yaml::to_string(template).map_err(|e| format!("YAML serialize failed: {e}"))
}

/// The templates dir: `~/.config/herdr/templates/`.
fn templates_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(std::path::PathBuf::from(home).join(".config/herdr/templates"))
}

/// Write a template to `~/.config/herdr/templates/<name>.yaml`, creating
/// the dir if missing. **C2: silent overwrite on clash** (C5 adds the
/// prompt). Returns the written path.
pub fn write_template(name: &str, yaml: &str) -> Result<std::path::PathBuf, String> {
    let dir = templates_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, yaml).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
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

    fn captured_tab(root: serde_json::Value) -> CapturedTab {
        CapturedTab { tab_label: "main".to_string(), root }
    }

    #[test]
    fn map_tab_nested_split_preserves_structure() {
        let tab = captured_tab(export_nested_tree());
        let tt = map_tab(&tab, "/code/proj", CwdPolicy::Absolute);
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
        let tt = map_tab(&tab, "/x", CwdPolicy::Absolute);
        assert_eq!(tt.layout.direction, "v");
        assert_eq!(tt.layout.ratio, 0);
        assert_eq!(tt.layout.panes.len(), 1);
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
        let tt = map_tab(&tab, "/code/proj", CwdPolicy::Relative);
        let template = Template {
            name: "roundtrip".to_string(),
            match_globs: vec![],
            default: false,
            tabs: vec![tt],
        };
        let yaml = template_to_yaml(&template).unwrap();
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
}
