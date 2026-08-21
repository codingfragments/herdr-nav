//! Provider implementations: one per target group (spec §5).
//!
//! Each provider is a thin adapter over its data source that produces a
//! subtree and resolves previews for its own nodes. Providers are cheap
//! to enumerate and lazy to preview.
//!
//! **Phase 1:** only `SessionProvider` is real (herdr daemon IPC via
//! `pane.list`); the other four groups render as red "unavailable"
//! stubs (spec §5/§11). Their providers land in later phases
//! (Agents → 5, Pinned+zoxide → 6a, Plugins → 7).

use crate::nav::{Group, Kind, Node, NodeId, Preview, Provider};

/// Build the five group subtrees in spec §4 fixed order, using the
/// registered providers. A provider that fails leaves its group row in
/// place with a red "unavailable" meta and an error preview (spec §5/§11).
pub fn build_tree(socket_path: &str) -> Vec<Node> {
    Group::ORDER
        .iter()
        .map(|&g| group_node(socket_path, g))
        .collect()
}

/// Produce one root group node. For Session, run the real provider; for
/// every other group, render an "unavailable" stub until its phase lands.
fn group_node(socket_path: &str, group: Group) -> Node {
    match group {
        Group::Session => SessionProvider::new(socket_path.to_string())
            .enumerate()
            .unwrap_or_else(|e| unavailable_stub(group, &e)),
        _ => unavailable_stub(group, "not implemented (later phase)"),
    }
}

/// A group row whose provider failed (spec §11): the row stays, meta is
/// red "unavailable", and the preview shows the error text.
fn unavailable_stub(group: Group, reason: &str) -> Node {
    Node {
        id: format!("group:{}", group.provider_id()),
        kind: Kind::Group,
        label: group_label(group),
        meta: "unavailable".to_string(),
        crumbs: None,
        children: Vec::new(),
        preview: Preview {
            icon: group_glyph(Group::Session),
            title: group_label(group),
            subtitle: reason.to_string(),
            chips: Vec::new(),
            body_label: "SUMMARY",
            body: vec![format!("provider unavailable: {reason}").into()],
            action: String::new(),
            alt: String::new(),
        },
        actions: crate::nav::Actions::default(),
    }
}

/// Human-readable group label (spec §4).
fn group_label(group: Group) -> String {
    match group {
        Group::Session => "Session",
        Group::Agents => "Agents",
        Group::Pinned => "Pinned dirs",
        Group::Zoxide => "zoxide",
        Group::Plugins => "Plugins",
    }
    .to_string()
}

/// Kind glyph for a group row (spec §9). Group glyph is `❯`.
fn group_glyph(_: Group) -> char {
    '❯'
}

// ── Session provider ─────────────────────────────────────────────────────────

/// The Session provider (spec §5): herdr daemon IPC — the
/// workspace/tab/pane graph. Phase 1 reconstructs the tree from the flat
/// `pane.list` response (the only confirmed session-graph method; see
/// PLANNING.md §5). Panes carry `workspace_id`/`tab_id`, so the
/// workspace → tab → pane tree is derived by grouping.
pub struct SessionProvider {
    socket_path: String,
}

impl SessionProvider {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
}

impl Provider for SessionProvider {
    fn id(&self) -> &'static str {
        "session"
    }

    fn enumerate(&self) -> Result<Node, String> {
        let result =
            crate::socket_client::request(&self.socket_path, "pane.list", serde_json::json!({}))
                .map_err(|e| format!("pane.list failed: {e}"))?;

        let panes = result
            .get("panes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Workspace/tab labels come from workspace.list + tab.list, not
        // from pane.list (which only carries ids). Fall back to the id
        // if either call fails — the tree still renders.
        let ws_names = name_map(
            &self.socket_path,
            "workspace.list",
            "workspaces",
            "workspace_id",
        );
        let tab_names = name_map(&self.socket_path, "tab.list", "tabs", "tab_id");

        let (active_workspace, active_tab) = active_ids_from_env();
        Ok(build_session_tree(
            &panes,
            &active_workspace,
            &active_tab,
            &ws_names,
            &tab_names,
        ))
    }

    fn preview(&self, _id: &NodeId) -> Preview {
        // Phase 2: per-kind preview (pane scrollback, workspace/tab inventory).
        Preview::default()
    }

    fn invoke(&self, id: &NodeId, _act: crate::nav::Act) -> Result<crate::nav::Outcome, String> {
        // Phase 3: jump to the target. For a pane, `pane.focus` does the
        // whole switch (workspace + tab + focus) in one call. For a
        // workspace/tab node we focus the first pane under it (or the
        // active one) — same invoke path.
        let pane_id = id.strip_prefix("session:pane:").map(str::to_string);
        match pane_id {
            Some(pid) => {
                let _ = crate::socket_client::request(
                    &self.socket_path,
                    "pane.focus",
                    serde_json::json!({"pane_id": pid}),
                )
                .map_err(|e| e.to_string())?;
                Ok(crate::nav::Outcome::Close {
                    toast: format!("jumped to pane {pid}"),
                })
            }
            None => {
                // Workspace/tab: focus the first pane under this node.
                // Re-enumerate to find the node, then walk for its first pane.
                let first_pane = self
                    .enumerate()
                    .ok()
                    .and_then(|n| node_for(id, &n).and_then(first_pane_under));
                match first_pane {
                    Some(pid) => {
                        let _ = crate::socket_client::request(
                            &self.socket_path,
                            "pane.focus",
                            serde_json::json!({"pane_id": pid}),
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(crate::nav::Outcome::Close {
                            toast: format!("switched to {id}"),
                        })
                    }
                    None => Err(format!("no pane under {id}")),
                }
            }
        }
    }
}

/// Reconstruct the Session tree (workspace → tab → pane) from the flat
/// `pane.list` panes array (spec §4/§5). Panes are grouped by
/// `workspace_id` then `tab_id`; workspaces and tabs are synthesised as
/// interior `Workspace`/`Tab` nodes, panes as `Pane` leaves. The active
/// workspace/tab (from the launch context) is marked `meta = "active"` so
/// `Tree::new` pre-expands to it.
///
/// Defensive: missing `workspace_id`/`tab_id` fields degrade to a flat
/// list under a single implicit workspace, so the tree never crashes on a
/// partial daemon response.
fn build_session_tree(
    panes: &[serde_json::Value],
    active_workspace: &str,
    active_tab: &str,
    ws_names: &std::collections::HashMap<String, String>,
    tab_names: &std::collections::HashMap<String, String>,
) -> Node {
    let mut workspaces: Vec<(String, String, Vec<PaneRow>)> = Vec::new();
    for pane in panes {
        let Some(pane_id) = pane_str(pane, "pane_id") else {
            continue;
        };
        let label = pane_label(pane, &pane_id);
        let ws_id = pane_str(pane, "workspace_id").unwrap_or_default();
        let tab_id = pane_str(pane, "tab_id").unwrap_or_default();
        // Real labels from workspace.list / tab.list; fall back to the id.
        let ws_name = ws_names
            .get(&ws_id)
            .cloned()
            .unwrap_or_else(|| ws_id.clone());
        let tab_name = tab_names
            .get(&tab_id)
            .cloned()
            .unwrap_or_else(|| tab_id.clone());

        let entry = workspaces
            .iter_mut()
            .find(|(id, _, _)| id == &ws_id)
            .map(|(_, _, rows)| rows);
        match entry {
            Some(rows) => rows.push(PaneRow {
                pane_id,
                label,
                tab_id,
                tab_name,
            }),
            None => workspaces.push((
                ws_id,
                ws_name,
                vec![PaneRow {
                    pane_id,
                    label,
                    tab_id,
                    tab_name,
                }],
            )),
        }
    }

    // Active workspace first, then the rest in encounter order.
    workspaces.sort_by_key(|(id, _, _)| id.as_str() != active_workspace);

    let ws_nodes: Vec<Node> = workspaces
        .into_iter()
        .map(|(ws_id, ws_name, panes)| {
            workspace_node(&ws_id, &ws_name, &panes, active_workspace, active_tab)
        })
        .collect();

    Node {
        id: "group:session".to_string(),
        kind: Kind::Group,
        label: "Session".to_string(),
        meta: format!("{} panes", panes.len()),
        crumbs: None,
        children: ws_nodes,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// One pane row extracted from `pane.list`, grouped for tree building.
struct PaneRow {
    pane_id: String,
    label: String,
    tab_id: String,
    tab_name: String,
}

/// Build a `Workspace` node with `Tab` children, panes grouped by tab.
/// Active tab first within the workspace. The workspace is marked active
/// when `ws_id == active_workspace`.
fn workspace_node(
    ws_id: &str,
    ws_name: &str,
    panes: &[PaneRow],
    active_workspace: &str,
    active_tab: &str,
) -> Node {
    // Group panes by tab_id.
    let mut tabs: Vec<(String, String, Vec<&PaneRow>)> = Vec::new();
    for p in panes {
        let entry = tabs
            .iter_mut()
            .find(|(id, _, _)| id == &p.tab_id)
            .map(|(_, _, rows)| rows);
        match entry {
            Some(rows) => rows.push(p),
            None => tabs.push((p.tab_id.clone(), p.tab_name.clone(), vec![p])),
        }
    }
    tabs.sort_by_key(|(id, _, _)| id.as_str() != active_tab);

    let tab_nodes: Vec<Node> = tabs
        .into_iter()
        .map(|(tab_id, tab_name, rows)| tab_node(&tab_id, &tab_name, &rows, active_tab))
        .collect();

    Node {
        id: format!("session:ws:{ws_id}"),
        kind: Kind::Workspace,
        label: ws_name.to_string(),
        meta: if ws_id == active_workspace {
            "active".to_string()
        } else {
            String::new()
        },
        crumbs: None,
        children: tab_nodes,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// Build a `Tab` node with `Pane` leaves. Marked active when
/// `tab_id == active_tab`.
fn tab_node(tab_id: &str, tab_name: &str, panes: &[&PaneRow], active_tab: &str) -> Node {
    let pane_nodes: Vec<Node> = panes
        .iter()
        .map(|p| pane_leaf(&p.pane_id, &p.label))
        .collect();
    Node {
        id: format!("session:tab:{tab_id}"),
        kind: Kind::Tab,
        label: tab_name.to_string(),
        meta: if tab_id == active_tab {
            "active".to_string()
        } else {
            String::new()
        },
        crumbs: None,
        children: pane_nodes,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// Resolve a NodeId to its node by re-enumerating. (The tree is
/// cheap to rebuild; this is only called on Enter, not per keystroke.)
fn node_for<'a>(id: &str, root: &'a Node) -> Option<&'a Node> {
    if root.id == id {
        return Some(root);
    }
    for c in &root.children {
        if let Some(n) = node_for(id, c) {
            return Some(n);
        }
    }
    None
}

/// First pane id under a node (depth-first), or None. Returns an
/// owned id so the caller doesn't borrow the transient tree.
fn first_pane_under(node: &Node) -> Option<String> {
    if node.kind == Kind::Pane {
        return node.id.strip_prefix("session:pane:").map(str::to_string);
    }
    for c in &node.children {
        if let Some(p) = first_pane_under(c) {
            return Some(p);
        }
    }
    None
}

/// Build a `Pane` leaf.
fn pane_leaf(pane_id: &str, label: &str) -> Node {
    Node {
        id: format!("session:pane:{pane_id}"),
        kind: Kind::Pane,
        label: label.to_string(),
        meta: String::new(),
        crumbs: None,
        children: Vec::new(),
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// Fetch a list method and build an `{id -> label}` map. Used to
/// get the real workspace/tab labels (pane.list only carries ids).
/// Returns an empty map on any socket/parse failure — the caller
/// falls back to the id, so the tree still renders.
fn name_map(
    socket_path: &str,
    method: &str,
    array_key: &str,
    id_key: &str,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if socket_path.is_empty() {
        return map;
    }
    let Ok(result) = crate::socket_client::request(socket_path, method, serde_json::json!({}))
    else {
        return map;
    };
    if let Some(arr) = result.get(array_key).and_then(|v| v.as_array()) {
        for item in arr {
            if let (Some(id), Some(label)) = (
                item.get(id_key).and_then(|v| v.as_str()),
                item.get("label").and_then(|v| v.as_str()),
            ) {
                map.insert(id.to_string(), label.to_string());
            }
        }
    }
    map
}

/// Read the active workspace/tab ids from `HERDR_PLUGIN_CONTEXT_JSON`
/// (set by Herdr for a real plugin-pane invocation). Falls back to empty
/// strings (no active marker) when unavailable.
fn active_ids_from_env() -> (String, String) {
    if let Ok(json) = std::env::var("HERDR_PLUGIN_CONTEXT_JSON") {
        if let Ok(ctx) = serde_json::from_str::<serde_json::Value>(&json) {
            let ws = ctx
                .get("workspace_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tab = ctx
                .get("tab_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            return (ws, tab);
        }
    }
    (String::new(), String::new())
}

/// Extract a string field from a pane object.
fn pane_str(pane: &serde_json::Value, key: &str) -> Option<String> {
    pane.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// A pane's display label (spec §4.1): `label` → `terminal_title_stripped`
/// → `pane {id}`. Mirrors herdr-zextract's `pane_title`.
fn pane_label(pane: &serde_json::Value, pane_id: &str) -> String {
    pane_str(pane, "label")
        .filter(|s| !s.is_empty())
        .or_else(|| pane_str(pane, "terminal_title_stripped").filter(|s| !s.is_empty()))
        .unwrap_or_else(|| format!("pane {pane_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn pane(id: &str, tab: &str, ws: &str, label: &str) -> serde_json::Value {
        serde_json::json!({
            "pane_id": id,
            "tab_id": tab,
            "workspace_id": ws,
            "label": label,
        })
    }

    #[test]
    fn builds_workspace_tab_pane_tree() {
        let panes = vec![
            pane("p1", "t1", "w1", "nvim"),
            pane("p2", "t1", "w1", "cargo"),
            pane("p3", "t2", "w1", "zsh"),
            pane("p4", "t1", "w2", "editor"),
        ];
        let session = build_session_tree(
            &panes,
            "",
            "",
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        );
        assert_eq!(session.kind, Kind::Group);
        assert_eq!(session.label, "Session");
        // Two workspaces.
        assert_eq!(session.children.len(), 2);
        let w1 = &session.children[0];
        assert_eq!(w1.kind, Kind::Workspace);
        // w1 has two tabs.
        assert_eq!(w1.children.len(), 2);
        let t1 = &w1.children[0];
        assert_eq!(t1.kind, Kind::Tab);
        // t1 has two panes.
        assert_eq!(t1.children.len(), 2);
        assert_eq!(t1.children[0].kind, Kind::Pane);
        assert_eq!(t1.children[0].label, "nvim");
    }

    #[test]
    fn active_workspace_first() {
        let panes = vec![pane("p1", "t1", "w1", "a"), pane("p2", "t1", "w2", "b")];
        let session = build_session_tree(
            &panes,
            "w2",
            "t1",
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        );
        // w2 (active) first.
        assert_eq!(session.children[0].id, "session:ws:w2");
        assert_eq!(session.children[0].meta, "active");
    }

    #[test]
    fn missing_workspace_id_degrades_to_flat() {
        // No workspace_id → all panes under one implicit workspace ("").
        let panes = vec![
            serde_json::json!({"pane_id": "p1", "tab_id": "t1", "label": "nvim"}),
            serde_json::json!({"pane_id": "p2", "tab_id": "t1", "label": "sh"}),
        ];
        let session = build_session_tree(
            &panes,
            "",
            "",
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        );
        assert_eq!(session.children.len(), 1); // one implicit workspace
        assert_eq!(session.children[0].children.len(), 1); // one tab
        assert_eq!(session.children[0].children[0].children.len(), 2); // 2 panes
    }

    #[test]
    fn pane_label_falls_back_to_id() {
        let p = serde_json::json!({"pane_id": "w1:p1"});
        assert_eq!(pane_label(&p, "w1:p1"), "pane w1:p1");
        let p = serde_json::json!({"pane_id": "p1", "terminal_title_stripped": "nvim"});
        assert_eq!(pane_label(&p, "p1"), "nvim");
    }

    #[test]
    fn unavailable_stub_marks_meta_red() {
        let n = unavailable_stub(Group::Agents, "nope");
        assert_eq!(n.meta, "unavailable");
        assert_eq!(n.preview.body_label, "SUMMARY");
        assert!(n.preview.body[0].to_string().contains("nope"));
    }

    #[test]
    fn build_tree_five_groups_in_spec_order() {
        // No socket → every provider fails → 5 unavailable stubs, but
        // the root structure (5 groups, spec §4 order, Session first) is
        // intact. This is the dev/no-Herdr path.
        let root = build_tree("");
        assert_eq!(root.len(), 5);
        assert_eq!(root[0].kind, Kind::Group);
        assert_eq!(root[0].label, "Session");
        assert_eq!(root[1].label, "Agents");
        assert_eq!(root[2].label, "Pinned dirs");
        assert_eq!(root[3].label, "zoxide");
        assert_eq!(root[4].label, "Plugins");
        // Every group is unavailable without a socket.
        assert!(root.iter().all(|n| n.meta == "unavailable"));
    }

    #[test]
    fn first_pane_under_finds_pane() {
        // A workspace → tab → pane tree; first_pane_under walks depth-first.
        let pane = pane_leaf("w1:p1", "nvim");
        let tab = Node {
            id: "session:tab:t1".into(),
            kind: Kind::Tab,
            label: "t1".into(),
            meta: String::new(),
            crumbs: None,
            children: vec![pane],
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        let ws = Node {
            id: "session:ws:w1".into(),
            kind: Kind::Workspace,
            label: "w1".into(),
            meta: String::new(),
            crumbs: None,
            children: vec![tab],
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        assert_eq!(first_pane_under(&ws), Some("w1:p1".to_string()));
        // A workspace with no panes → None.
        let empty_ws = Node {
            id: "session:ws:w2".into(),
            kind: Kind::Workspace,
            label: "w2".into(),
            meta: String::new(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        assert_eq!(first_pane_under(&empty_ws), None);
    }

    #[test]
    fn node_for_finds_by_id() {
        let pane = pane_leaf("w1:p1", "nvim");
        let session = Node {
            id: "group:session".into(),
            kind: Kind::Group,
            label: "S".into(),
            meta: String::new(),
            crumbs: None,
            children: vec![Node {
                id: "session:ws:w1".into(),
                kind: Kind::Workspace,
                label: "w1".into(),
                meta: String::new(),
                crumbs: None,
                children: vec![pane],
                preview: Preview::default(),
                actions: crate::nav::Actions::default(),
            }],
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        };
        assert!(node_for("session:ws:w1", &session).is_some());
        assert!(node_for("session:pane:w1:p1", &session).is_some());
        assert!(node_for("nope", &session).is_none());
    }
}

#[cfg(test)]
mod name_map_tests {
    use super::*;

    fn pane(id: &str, tab: &str, ws: &str, label: &str) -> serde_json::Value {
        serde_json::json!({
            "pane_id": id,
            "tab_id": tab,
            "workspace_id": ws,
            "label": label,
        })
    }

    #[test]
    fn build_session_tree_uses_name_maps() {
        let panes = vec![
            pane("w9:p1", "w9:t1", "w9", "nvim"),
            pane("w9:p2", "w9:t1", "w9", "sh"),
            pane("wA:p3", "wA:tA", "wA", "cargo"),
        ];
        let mut ws_names = std::collections::HashMap::new();
        ws_names.insert("w9".to_string(), "claude-chats".to_string());
        ws_names.insert("wA".to_string(), "herdr-nav".to_string());
        let mut tab_names = std::collections::HashMap::new();
        tab_names.insert("w9:t1".to_string(), "main".to_string());
        tab_names.insert("wA:tA".to_string(), "dev".to_string());
        let session = build_session_tree(&panes, "w9", "w9:t1", &ws_names, &tab_names);
        // Workspace labels come from the map, not the raw id.
        assert_eq!(session.children[0].label, "claude-chats");
        assert_eq!(session.children[1].label, "herdr-nav");
        // Tab label comes from the map too.
        assert_eq!(session.children[0].children[0].label, "main");
    }

    #[test]
    fn name_map_falls_back_to_id_on_empty() {
        let panes = vec![pane("p1", "t1", "w9", "nvim")];
        // Empty maps → fall back to the raw id (no crash).
        let session = build_session_tree(
            &panes,
            "w9",
            "t1",
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        );
        assert_eq!(session.children[0].label, "w9");
        assert_eq!(session.children[0].children[0].label, "t1");
    }
}
