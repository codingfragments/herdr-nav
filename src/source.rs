//! Provider implementations: one per target group (spec §5).
//!
//! Each provider is a thin adapter over its data source that produces a
//! subtree and resolves previews for its own nodes. Providers are cheap
//! to enumerate and lazy to preview.
//!
//! **Status: scaffold only.** The real providers land one per phase in
//! PLANNING.md §17 (Session in Phase 1, Agents in Phase 5, Pinned+zoxide
//! in Phase 6a, Plugins in Phase 7). This module currently exposes only
//! the dispatch helper and stub providers that return empty subtrees.
//!
//! Data sources (spec §5):
//! - session: herdr daemon IPC — workspace/tab/pane graph, pane pids,
//!   cwd, last command, scrollback tail. Refresh on open + on daemon event.
//! - agents: agent-detect plugin (agent.start/agent.stop hooks), else a
//!   process-tree heuristic. Refresh on open + on hook fire.
//! - pinned: `~/.config/herdr/targets.toml`. Refresh on file mtime change.
//! - zoxide: `zoxide query --list --score`, top 50, existing paths only.
//!   Refresh on open (cache 30s).
//! - plugins: plugin registry — name, version, enabled, load error,
//!   declared actions. Refresh on open.

use crate::nav::{Group, Node, Provider};

/// Build the five group subtrees in spec §4 fixed order, using the
/// registered providers. A provider that fails leaves its group row in
/// place with a red "unavailable" meta and an error preview (spec §5/§11).
pub fn build_tree(socket_path: &str) -> Vec<Node> {
    let _ = socket_path;
    Group::ORDER.iter().map(|&g| stub_group(g)).collect()
}

/// Scaffold stub: a group node with no children. Replaced per-phase by
/// the real provider's `enumerate()` once that provider lands.
fn stub_group(group: Group) -> Node {
    Node {
        id: format!("group:{}", group.provider_id()),
        kind: crate::nav::Kind::Group,
        label: group.provider_id().to_string(),
        meta: String::new(),
        crumbs: None,
        children: Vec::new(),
        preview: crate::nav::Preview::default(),
        actions: crate::nav::Actions::default(),
    }
}

// Stub providers — replaced by real implementations in their phases.
// Kept here so the module compiles and the dispatch shape is visible.

pub struct SessionProvider;
impl Provider for SessionProvider {
    fn id(&self) -> &'static str {
        "session"
    }
    fn enumerate(&self) -> Result<Node, String> {
        // TODO Phase 1: herdr daemon IPC — workspace/tab/pane graph.
        Err("not implemented".to_string())
    }
    fn preview(&self, _id: &crate::nav::NodeId) -> crate::nav::Preview {
        crate::nav::Preview::default()
    }
    fn invoke(
        &self,
        _id: &crate::nav::NodeId,
        _act: crate::nav::Act,
    ) -> Result<crate::nav::Outcome, String> {
        // TODO Phase 3: jump to pane (switch workspace + tab + focus pane).
        Err("not implemented".to_string())
    }
}
