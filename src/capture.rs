//! Capture: analyze the current workspace and (in later phases) emit a
//! template. **Phase C1** is the spine only — read the active workspace
//! from the daemon and print a plain-text summary. No UI, no YAML.
//!
//! This module is the home for the whole capture feature; later phases
//! (C2 mapping+write, C3 commands, C4 wizard, C5 clash+editor) grow it.
//! See `spec/capture-template-plan.md`.
//!
//! **Socket discipline:** one fresh connection per request
//! (`socket_client::request`). For a workspace with `T` tabs this phase
//! issues `1 + 1 + T` calls (workspace.list, tab.list, one layout.export
//! per tab). Acceptable for a one-shot capture.
//!
//! **Gotcha (spike §3):** `pane.layout` ignores its `tab_id` param (it
//! always returns the active tab). We use `layout.export` exclusively —
//! it is param-driven by `tab_id` and returns the portable recursive
//! tree.

use crate::socket_client;

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
}
