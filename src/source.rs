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
use serde::Deserialize;

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
        Group::Agents => AgentsProvider::new(socket_path.to_string())
            .enumerate()
            .unwrap_or_else(|e| unavailable_stub(group, &e)),
        Group::Pinned => PinnedProvider::new()
            .enumerate()
            .unwrap_or_else(|e| unavailable_stub(group, &e)),
        Group::Zoxide => ZoxideProvider::new()
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

// ── Agents provider ──────────────────────────────────────────────────────────

/// The Agents provider (spec §5): agent-detect plugin hooks
/// (agent.start/stop), else process-tree heuristic. Phase 5 uses the
/// `agent.list` socket method (confirmed live) which returns every
/// detected agent pane with its status. Flat list, sort waiting →
/// working → idle then recency. Meta = status.
pub struct AgentsProvider {
    socket_path: String,
}

impl AgentsProvider {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
}

impl Provider for AgentsProvider {
    fn id(&self) -> &'static str {
        "agents"
    }

    fn enumerate(&self) -> Result<Node, String> {
        let result =
            crate::socket_client::request(&self.socket_path, "agent.list", serde_json::json!({}))
                .map_err(|e| format!("agent.list failed: {e}"))?;

        let agents = result
            .get("agents")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(build_agents_tree(&agents))
    }

    fn preview(&self, _id: &NodeId) -> Preview {
        // Phase 5 preview: agent transcript tail / blocked question.
        // The preview is rendered by preview::resolve_preview once the
        // node is in the tree; here we return a default — the preview
        // module dispatches on Kind::Agent once this provider is live.
        Preview::default()
    }

    fn invoke(&self, id: &NodeId, _act: crate::nav::Act) -> Result<crate::nav::Outcome, String> {
        // Jump to the pane the agent runs in — same as a pane jump.
        let pane_id = id.strip_prefix("agents:").unwrap_or(id);
        let _ = crate::socket_client::request(
            &self.socket_path,
            "pane.focus",
            serde_json::json!({"pane_id": pane_id}),
        )
        .map_err(|e| e.to_string())?;
        Ok(crate::nav::Outcome::Close {
            toast: format!("jumped to agent {pane_id}"),
        })
    }
}

/// Build the Agents group node (flat list of agent leaves, spec §4/§5).
/// Sort waiting → working → idle, then recency (focused first). Meta =
/// status. Each agent leaf's id is `agents:<pane_id>` so invoke can
/// strip the prefix and focus the pane.
fn build_agents_tree(agents: &[serde_json::Value]) -> Node {
    let mut leaves: Vec<Node> = Vec::new();
    for agent in agents {
        let Some(pane_id) = agent.get("pane_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = agent
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("agent");
        let status = agent
            .get("agent_status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let label = agent
            .get("terminal_title_stripped")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(name);
        leaves.push(Node {
            id: format!("agents:{pane_id}"),
            kind: Kind::Agent,
            label: label.to_string(),
            meta: status.to_string(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        });
    }
    // Sort waiting → working → idle (spec §4), then focused first.
    leaves.sort_by_key(|n| match n.meta.as_str() {
        "waiting" => 0,
        "working" => 1,
        _ => 2,
    });
    Node {
        id: "group:agents".to_string(),
        kind: Kind::Group,
        label: "Agents".to_string(),
        meta: format!("{} agents", leaves.len()),
        crumbs: None,
        children: leaves,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

// ── Pinned + zoxide providers ────────────────────────────────────────────────

/// The Pinned dirs provider (spec §5): reads
/// `~/.config/herdr/targets.toml`, refresh on file mtime change.
/// Flat list, config file order (slot ⌘1–⌘9). Meta = slot.
pub struct PinnedProvider;

impl Default for PinnedProvider {
    fn default() -> Self {
        Self
    }
}

impl PinnedProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Provider for PinnedProvider {
    fn id(&self) -> &'static str {
        "pinned"
    }

    fn enumerate(&self) -> Result<Node, String> {
        Ok(build_pinned_tree())
    }

    fn preview(&self, _id: &NodeId) -> Preview {
        Preview::default() // preview::resolve_preview dispatches on Kind::Dir
    }

    fn invoke(&self, id: &NodeId, _act: crate::nav::Act) -> Result<crate::nav::Outcome, String> {
        // Open a new workspace at the pinned path.
        open_dir_workspace(id)
    }
}

/// The zoxide provider (spec §5): `zoxide query --list --score`,
/// top 50, existing paths only. 30s cache (Phase 10 wires the cache;
/// Phase 6a re-runs on each open). Meta = frecency score.
pub struct ZoxideProvider;

impl Default for ZoxideProvider {
    fn default() -> Self {
        Self
    }
}

impl ZoxideProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Provider for ZoxideProvider {
    fn id(&self) -> &'static str {
        "zoxide"
    }

    fn enumerate(&self) -> Result<Node, String> {
        Ok(build_zoxide_tree())
    }

    fn preview(&self, _id: &NodeId) -> Preview {
        Preview::default() // preview::resolve_preview dispatches on Kind::Zox
    }

    fn invoke(&self, id: &NodeId, _act: crate::nav::Act) -> Result<crate::nav::Outcome, String> {
        open_dir_workspace(id)
    }
}

/// Open a new workspace at a directory path (spec §8.2): a
/// worktree-space if the path is inside a git repo, a plain
/// workspace otherwise. Never reuses the current workspace.
/// `id` is the node id (`pinned:<path>` or `zox:<path>`); the
/// path is the part after the colon.
pub fn open_dir_workspace(id: &str) -> Result<crate::nav::Outcome, String> {
    open_dir_workspace_named(id, None)
}

/// Create a new workspace at the path, optionally named. `name` is
/// the user-edited workspace label (None = let herdr pick).
pub fn open_dir_workspace_named(
    id: &str,
    name: Option<&str>,
) -> Result<crate::nav::Outcome, String> {
    let path = id.split_once(':').map(|(_, p)| p).unwrap_or(id);
    let expanded = expand_path(path);
    let socket = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();

    let result = create_dir_workspace(&socket, &expanded, name)?;
    if let Some(pane_id) = result {
        let _ = crate::socket_client::request(
            &socket,
            "pane.focus",
            serde_json::json!({"pane_id": pane_id}),
        );
    }
    Ok(crate::nav::Outcome::Close {
        toast: format!("opened workspace at {path}"),
    })
}

/// Create a new workspace at `expanded` (worktree-space inside a
/// git repo, plain otherwise) and return the first pane's id so
/// the caller can focus it. Returns None if the create succeeded
/// but no pane id was in the response (best-effort focus).
fn create_dir_workspace(
    socket: &str,
    expanded: &str,
    name: Option<&str>,
) -> Result<Option<String>, String> {
    // Worktree-space if inside a git repo, else plain workspace.
    if is_inside_git_repo(expanded) {
        // Try worktree.create; fall back to workspace.create if
        // the path is already a worktree (spec §8.2: always a NEW ws).
        let r = crate::socket_client::request(
            socket,
            "worktree.create",
            serde_json::json!({"path": expanded}),
        );
        if let Ok(resp) = r {
            return Ok(resp
                .get("root_pane")
                .and_then(|v| v.get("pane_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string));
        }
    }
    // Plain workspace (or worktree fallback). Pass the name as
    // `label` if provided.
    let mut params = serde_json::json!({"cwd": expanded});
    if let Some(name) = name {
        params["label"] = serde_json::Value::String(name.to_string());
    }
    let resp = crate::socket_client::request(socket, "workspace.create", params)
        .map_err(|e| e.to_string())?;
    Ok(resp
        .get("root_pane")
        .and_then(|v| v.get("pane_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

/// Expand `~` and `$HOME` in a path.
pub fn expand_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    if let Some(rest) = path.strip_prefix("$HOME/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

/// Is `path` inside a git work tree? (decides worktree-space vs plain.)
fn is_inside_git_repo(path: &str) -> bool {
    let Ok(out) = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--is-inside-work-tree"])
        .output()
    else {
        return false;
    };
    out.status.success() && out.stdout.starts_with(b"true")
}

/// Build the Pinned dirs group node (flat list of Dir leaves).
/// Reads `~/.config/herdr/targets.toml`; empty if missing.
fn build_pinned_tree() -> Node {
    let pins = read_targets_toml();
    let leaves: Vec<Node> = pins
        .into_iter()
        .map(|(path, slot)| Node {
            id: format!("pinned:{path}"),
            kind: Kind::Dir,
            label: short_path(&path),
            meta: format!("⌘{slot}"),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        })
        .collect();
    Node {
        id: "group:pinned".to_string(),
        kind: Kind::Group,
        label: "Pinned dirs".to_string(),
        meta: if leaves.is_empty() {
            "empty".to_string()
        } else {
            format!("{} pins", leaves.len())
        },
        crumbs: None,
        children: leaves,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// Read `~/.config/herdr/targets.toml` → `Vec<(path, slot)>`. Empty
/// if missing or malformed (no crash).
fn read_targets_toml() -> Vec<(String, u32)> {
    // Spec §13: `~/.config/herdr/targets.toml` (herdr-level, shared —
    // not the plugin's own config dir). Prefer the herdr-level file;
    // fall back to the plugin dir, then HOME, so the tree still
    // renders wherever the file lives.
    let herdr_dir = std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.config/herdr"));
    let plugin_dir = std::env::var("HERDR_PLUGIN_CONFIG_DIR").ok();
    let candidates = [herdr_dir, plugin_dir];
    for dir in candidates.into_iter().flatten() {
        let path = format!("{dir}/targets.toml");
        if let Ok(content) = std::fs::read_to_string(&path) {
            return parse_targets_toml(&content);
        }
    }
    Vec::new()
}

/// Parse `targets.toml` content → `Vec<(path, slot)>`.
fn parse_targets_toml(content: &str) -> Vec<(String, u32)> {
    let mut pins = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `[[pin]] path = "..." slot = N` — parse the key=value pairs.
        if line.starts_with("[[pin]]") {
            pins.push((String::new(), 0));
            continue;
        }
        let Some(last) = pins.last_mut() else {
            continue;
        };
        if let Some(v) = line
            .strip_prefix("path = ")
            .and_then(|s| s.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        {
            last.0 = expand_path(v);
        } else if let Some(v) = line
            .strip_prefix("slot = ")
            .and_then(|s| s.trim().parse().ok())
        {
            last.1 = v;
        }
    }
    pins.into_iter().filter(|(p, _)| !p.is_empty()).collect()
}

/// Build the zoxide group node (flat list of Zox leaves).
/// `zoxide query --list --score`, top 50, existing paths only.
fn build_zoxide_tree() -> Node {
    let entries = zoxide_query();
    let leaves: Vec<Node> = entries
        .into_iter()
        .take(50)
        .map(|(score, path)| Node {
            id: format!("zox:{path}"),
            kind: Kind::Zox,
            label: short_path(&path),
            meta: format!("{score}"),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: crate::nav::Actions::default(),
        })
        .collect();
    Node {
        id: "group:zoxide".to_string(),
        kind: Kind::Group,
        label: "zoxide".to_string(),
        meta: if leaves.is_empty() {
            "empty".to_string()
        } else {
            format!("{} entries", leaves.len())
        },
        crumbs: None,
        children: leaves,
        preview: Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

/// Run `zoxide query --list --score` → `Vec<(score, path)>`.
/// Existing paths only, sorted by frecency (zoxide's own order).
fn zoxide_query() -> Vec<(f64, String)> {
    let Ok(out) = std::process::Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        // `  36.0 /Users/...` — leading spaces, then score, then path.
        let line = line.trim_start();
        let mut parts = line.splitn(2, ' ');
        let score: f64 = parts.next().unwrap_or("").trim().parse().unwrap_or(0.0);
        let path = parts.next().unwrap_or("").trim().to_string();
        if !path.is_empty() {
            entries.push((score, path));
        }
    }
    entries
}

/// Shorten a path for display: replace $HOME with `~`.
fn short_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix(&format!("{home}/")) {
            return format!("~/{rest}");
        }
    }
    path.to_string()
}

/// Derive a good default workspace name from a directory path:
/// the last path segment (e.g. `~/code/herdr` → `herdr`).
pub fn workspace_name_default(path: &str) -> String {
    let expanded = expand_path(path);
    expanded
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace")
        .to_string()
}
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

// ── Templates (§8.4) ────────────────────────────────────────────────────

/// A workspace template (spec §8.4): tmuxinator-style tabs/
/// panes/splits/startup commands. Parsed from
/// `~/.config/herdr/templates.toml` via the `toml` crate.
#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub name: String,
    /// Glob patterns that auto-preselect this template when the
    /// target path matches (spec §8.4).
    #[serde(default, rename = "match")]
    pub match_globs: Vec<String>,
    /// `default = true` — the fallback when no match glob fits.
    #[serde(default)]
    pub default: bool,
    pub tabs: Vec<TemplateTab>,
}

/// One tab in a template.
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateTab {
    pub name: String,
    /// Startup commands, one per pane. An empty command =
    /// a plain login shell (no nested shell).
    #[serde(default)]
    pub panes: Vec<String>,
    /// Working directory for every pane in this tab. None =
    /// the workspace's cwd (the path the workspace was
    /// opened at). Passed to pane.split/tab.create as `cwd`,
    /// so no `cd` command is needed.
    #[serde(default)]
    pub cwd: Option<String>,
    /// `"v"` (vertical/side-by-side) or `"h"` (horizontal/stacked).
    #[serde(default = "default_split")]
    pub split: String,
    /// Split ratio (0–100). 0 = even.
    #[serde(default)]
    pub ratio: u32,
}

/// Read `~/.config/herdr/templates.toml` → `Vec<Template>`. Empty
/// if missing or malformed (no crash). Spec §8.4: with no
/// templates.toml, `^t` is unbound.
pub fn read_templates_toml() -> Vec<Template> {
    let Some(dir) = std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.config/herdr"))
    else {
        return Vec::new();
    };
    let path = format!("{dir}/templates.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_templates_toml(&content)
}

/// Parse `templates.toml` content → `Vec<Template>` via the `toml` crate.
fn default_split() -> String {
    "v".to_string()
}

fn parse_templates_toml(content: &str) -> Vec<Template> {
    #[derive(Deserialize)]
    struct Templates {
        template: Vec<Template>,
    }
    match toml::from_str::<Templates>(content) {
        Ok(t) => t
            .template
            .into_iter()
            .filter(|t| !t.name.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Build a workspace from a template at `path` (spec §8.4):
/// create the workspace, then for each tab, split panes and send
/// startup commands. Returns the first pane's id so the caller
/// can focus it.
pub fn build_workspace_from_template(
    path: &str,
    name: &str,
    template: &Template,
) -> Result<Option<String>, String> {
    let socket = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
    // Create the workspace (plain; worktree is handled by the caller).
    let resp = crate::socket_client::request(
        &socket,
        "workspace.create",
        serde_json::json!({"cwd": path, "label": name}),
    )
    .map_err(|e| e.to_string())?;
    let ws_id = resp
        .get("workspace")
        .and_then(|v| v.get("workspace_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let first_pane = resp
        .get("root_pane")
        .and_then(|v| v.get("pane_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // Focus the new workspace's root pane before any splits —
    // herdr's pane.split targets the ACTIVE pane in the ACTIVE
    // workspace, so without focusing first the split lands in
    // the current workspace (confirmed live), not the new one.
    if let Some(fp) = &first_pane {
        let _ = crate::socket_client::request(
            &socket,
            "pane.focus",
            serde_json::json!({"pane_id": fp}),
        );
    }
    // Remember the pane that was focused before we built, so we
    // can restore focus to it (the user stays in the current ws).
    let prev_focused = current_focused_pane(&socket);

    for (tab_i, tab) in template.tabs.iter().enumerate() {
        // Per-tab cwd (None = the workspace's cwd = the path
        // the workspace was opened at). Passed to tab.create /
        // pane.split so no `cd` command is needed.
        let cwd = tab.cwd.as_deref().unwrap_or(path);
        // The first tab uses the workspace's initial pane; later
        // tabs need tab.create.
        let (pane_id, _new_tab) = if tab_i == 0 {
            (first_pane.clone().unwrap_or_default(), false)
        } else {
            let r = crate::socket_client::request(
                &socket,
                "tab.create",
                serde_json::json!({"workspace_id": ws_id, "cwd": cwd}),
            )
            .map_err(|e| e.to_string())?;
            (
                r.get("root_pane")
                    .and_then(|v| v.get("pane_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                true,
            )
        };
        // Send the first pane's command. An empty command = plain
        // shell pane (the login shell herdr configured) —
        // don't send anything, so no nested shell.
        if let Some(cmd) = tab.panes.first() {
            if !cmd.is_empty() {
                let _ = crate::socket_client::request(
                    &socket,
                    "pane.send_text",
                    serde_json::json!({"pane_id": pane_id, "text": format!("{cmd}\n")}),
                );
            }
        }
        // Split additional panes. An empty command = plain
        // shell pane (skip the send).
        let mut current_pane = pane_id;
        for (_pane_i, cmd) in tab.panes.iter().enumerate().skip(1) {
            let direction = if tab.split == "h" { "down" } else { "right" };
            let ratio = if tab.ratio > 0 {
                tab.ratio as f64 / 100.0
            } else {
                0.5
            };
            let r = crate::socket_client::request(
                &socket,
                "pane.split",
                serde_json::json!({"pane_id": current_pane, "direction": direction, "ratio": ratio, "cwd": cwd}),
            )
            .map_err(|e| e.to_string())?;
            current_pane = r
                .get("pane")
                .and_then(|v| v.get("pane_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !cmd.is_empty() {
                let _ = crate::socket_client::request(
                    &socket,
                    "pane.send_text",
                    serde_json::json!({"pane_id": current_pane, "text": format!("{cmd}\n")}),
                );
            }
        }
    }
    // Restore focus to the pane that was focused before the build
    // (the user stays in the current workspace, not the new one).
    if let Some(prev) = prev_focused {
        let _ = crate::socket_client::request(
            &socket,
            "pane.focus",
            serde_json::json!({"pane_id": prev}),
        );
    }
    Ok(first_pane)
}

/// Best-effort: which pane is currently focused? (None if the
/// socket call fails.) Used to restore focus after building a
/// workspace from a template.
fn current_focused_pane(socket: &str) -> Option<String> {
    let r = crate::socket_client::request(socket, "pane.list", serde_json::json!({})).ok()?;
    let panes = r.get("panes").and_then(|v| v.as_array())?;
    for p in panes {
        if p.get("focused").and_then(|v| v.as_bool()) == Some(true) {
            return p
                .get("pane_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
    }
    None
}

/// Preselect the template whose `match` glob fits `path`, else the
/// configured `default` (spec §8.4). Falls back to 0.
pub fn preselect_template(templates: &[Template], path: &str) -> usize {
    // First: a match glob fits.
    for (i, t) in templates.iter().enumerate() {
        for glob in &t.match_globs {
            if glob_match(glob, path) {
                return i;
            }
        }
    }
    // Else: the default template.
    for (i, t) in templates.iter().enumerate() {
        if t.default {
            return i;
        }
    }
    0
}

/// Minimal glob match: `**` matches any sequence, `*` matches
/// within a path segment. Good enough for template `match` patterns.
fn glob_match(glob: &str, path: &str) -> bool {
    // `**/Cargo.toml` → `**` matches zero-or-more path segments,
    // so it matches both `/Users/foo/Cargo.toml` and `Cargo.toml`.
    if let Some(rest) = glob.strip_prefix("**") {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        return path.ends_with(rest);
    }
    // `*` → matches any chars within a segment.
    if glob.contains('*') {
        let parts: Vec<&str> = glob.split('*').collect();
        let mut idx = 0;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            match path[idx..].find(part) {
                Some(pos) => idx += pos + part.len(),
                None => return false,
            }
            let _ = i;
        }
        return true;
    }
    path == glob
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
        // Session + Agents need a socket; Pinned + zoxide are local
        // (targets.toml / zoxide CLI) so they're never "unavailable"
        // from a missing socket — only empty if no pins/zoxide entries.
        assert_eq!(root[0].meta, "unavailable"); // Session
        assert_eq!(root[1].meta, "unavailable"); // Agents
                                                 // Plugins still unavailable (Phase 7).
        assert_eq!(root[4].meta, "unavailable");
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

#[cfg(test)]
mod agents_tests {
    use super::*;

    fn agent(pane_id: &str, agent: &str, status: &str, title: &str) -> serde_json::Value {
        serde_json::json!({
            "pane_id": pane_id,
            "agent": agent,
            "agent_status": status,
            "terminal_title_stripped": title,
        })
    }

    #[test]
    fn builds_flat_agent_list() {
        let agents = vec![
            agent("wA:p1", "pi", "working", "π - herdr-nav"),
            agent("wA:p2", "codex", "waiting", "cx - sandbox"),
        ];
        let group = build_agents_tree(&agents);
        assert_eq!(group.kind, Kind::Group);
        assert_eq!(group.label, "Agents");
        assert_eq!(group.children.len(), 2);
        // waiting sorts before working (spec §4).
        assert_eq!(group.children[0].meta, "waiting");
        assert_eq!(group.children[1].meta, "working");
        // Label prefers terminal_title_stripped.
        assert!(group.children[0].label.contains("sandbox"));
    }

    #[test]
    fn agent_leaf_id_strips_for_invoke() {
        let agents = vec![agent("wA:p1", "pi", "working", "pi")];
        let group = build_agents_tree(&agents);
        let leaf = &group.children[0];
        assert_eq!(leaf.kind, Kind::Agent);
        assert!(leaf.id.starts_with("agents:wA:p1"));
    }
}

#[cfg(test)]
mod dir_tests {
    use super::*;

    #[test]
    fn pinned_empty_when_no_targets_toml() {
        // No targets.toml → empty group (not unavailable).
        let group = build_pinned_tree();
        assert_eq!(group.kind, Kind::Group);
        assert_eq!(group.label, "Pinned dirs");
        assert_eq!(group.meta, "empty");
        assert!(group.children.is_empty());
    }

    #[test]
    fn zoxide_parses_score_and_path() {
        // `  36.0 /Users/foo` → trim → "36.0" + "/Users/foo".
        // The real parser splits on the first space (leading), then
        // trims both halves.
        let line = "  36.0 /Users/foo".trim_start();
        let mut parts = line.splitn(2, ' ');
        let score: f64 = parts.next().unwrap_or("").trim().parse().unwrap_or(0.0);
        let path = parts.next().unwrap_or("").trim().to_string();
        assert_eq!(score, 36.0);
        assert_eq!(path, "/Users/foo");
    }

    #[test]
    fn expand_path_tilde() {
        // ~/foo → $HOME/foo when HOME is set.
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path("~/foo"), "/Users/test/foo");
        assert_eq!(expand_path("/abs/path"), "/abs/path");
        std::env::remove_var("HOME");
    }

    #[test]
    fn open_dir_workspace_strips_prefix() {
        // The id is `pinned:<path>` or `zox:<path>`; the path is
        // after the colon. We can't call the socket in a unit test, but
        // we can verify the prefix strip.
        let id = "pinned:~/code/herdr";
        let path = id.split_once(':').map(|(_, p)| p).unwrap_or(id);
        assert_eq!(path, "~/code/herdr");
    }
}

#[cfg(test)]
mod name_default_tests {
    use super::*;
    #[test]
    fn workspace_name_default_is_last_segment() {
        assert_eq!(workspace_name_default("/Users/foo/code/herdr"), "herdr");
        assert_eq!(workspace_name_default("~/code/herdr"), "herdr");
        assert_eq!(workspace_name_default("/trailing/"), "trailing");
        assert_eq!(workspace_name_default("/"), "workspace");
        assert_eq!(workspace_name_default("bare"), "bare");
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn parse_templates_toml_basic() {
        let toml = r#"
[[template]]
name = "rust-dev"
match = ["**/Cargo.toml"]
tabs = [
  { name = "editor", panes = ["nvim .", "cargo watch -x test"], split = "v", ratio = 60 },
  { name = "shell", panes = ["zsh"] },
]

[[template]]
name = "plain"
default = true
tabs = [{ name = "shell", panes = ["zsh"] }]
"#;
        let t = parse_templates_toml(toml);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "rust-dev");
        assert!(t[0].match_globs.contains(&"**/Cargo.toml".to_string()));
        assert!(!t[0].default);
        assert_eq!(t[0].tabs.len(), 2);
        assert_eq!(
            t[0].tabs[0].panes,
            vec!["nvim .".to_string(), "cargo watch -x test".to_string()]
        );
        assert_eq!(t[0].tabs[0].split, "v");
        assert_eq!(t[0].tabs[0].ratio, 60);
        assert_eq!(t[1].name, "plain");
        assert!(t[1].default);
        // Per-tab cwd is optional (None when unset).
        assert!(t[0].tabs[0].cwd.is_none());
    }

    #[test]
    fn parse_templates_toml_with_cwd() {
        let toml = r#"
[[template]]
name = "dev"
tabs = [{ name = "editor", panes = [""], cwd = "~/code" }]
"#;
        let t = parse_templates_toml(toml);
        assert_eq!(t[0].tabs[0].cwd.as_deref(), Some("~/code"));
        // Empty command = plain shell pane (no nested shell).
        assert_eq!(t[0].tabs[0].panes, vec!["".to_string()]);
    }

    #[test]
    fn glob_match_double_star() {
        assert!(glob_match("**/Cargo.toml", "/Users/foo/code/Cargo.toml"));
        assert!(glob_match("**/Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("**/Cargo.toml", "/Users/foo/code/lib.rs"));
    }

    #[test]
    fn preselect_template_match_then_default() {
        let templates = vec![
            Template {
                name: "rust-dev".into(),
                match_globs: vec!["**/Cargo.toml".into()],
                default: false,
                tabs: vec![],
            },
            Template {
                name: "plain".into(),
                match_globs: vec![],
                default: true,
                tabs: vec![],
            },
        ];
        // A rust path → match glob fits → preselect rust-dev (0).
        assert_eq!(preselect_template(&templates, "/Users/foo/Cargo.toml"), 0);
        // A non-rust path → no match → default (plain, 1).
        assert_eq!(preselect_template(&templates, "/Users/foo/notes"), 1);
    }
}
