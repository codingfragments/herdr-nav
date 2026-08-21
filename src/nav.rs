//! Node model, provider trait, and the five target groups.
//!
//! Mirrors the Herdr Switcher Spec §4 (node model) and §5 (providers).
//! Five groups at root, in fixed order: Session (3-level tree
//! workspace/tab/pane), Agents (flat), Pinned dirs (flat), zoxide (flat),
//! Plugins (flat). Leaf-ness is structural (`children.is_empty()`), so
//! search mode is one recursive walk with no per-group special-casing.
//! Crumbs are precomputed at tree-build time so search-mode rows need no
//! upward traversal during keystroke handling.
//!
//! **Status: Phase 1.** The tree state + browse logic is live; the
//! forward-looking types (Preview/Chip/Act/Outcome, the Dir/Zox/Plugin/
//! Agent kinds, config-shaped fields) are defined upfront per the spec
//! model and are wired in by later phases (2, 3, 5, 6, 7, 10).
#![allow(dead_code)]

use std::collections::BTreeSet;

/// Stable, provider-scoped node id, e.g. `"session:%2"`, `"zox:/home/dd/dotfiles"`.
pub type NodeId = String;

/// Every kind of node the switcher renders. Drives the glyph, the colour,
/// the preview body, and the default/alternate actions (spec §4.1, §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A root target group (Session, Agents, …). Branch, never a leaf.
    Group,
    /// Session-tree interior nodes.
    Workspace,
    Tab,
    /// Session-tree leaf.
    Pane,
    /// Pinned-dirs leaf.
    Dir,
    /// zoxide leaf.
    Zox,
    /// Plugins leaf.
    Plugin,
    /// Agents leaf.
    Agent,
}

/// The five root groups, in the spec's fixed display order (§4): the two
/// live/volatile groups first, the three stable groups after, ordered by
/// how explicitly the user chose them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Session,
    Agents,
    Pinned,
    Zoxide,
    Plugins,
}

impl Group {
    /// Spec §4 fixed order.
    pub const ORDER: [Group; 5] = [
        Group::Session,
        Group::Agents,
        Group::Pinned,
        Group::Zoxide,
        Group::Plugins,
    ];

    /// Provider id as used in config and the `Provider` trait.
    pub fn provider_id(self) -> &'static str {
        match self {
            Group::Session => "session",
            Group::Agents => "agents",
            Group::Pinned => "pinned",
            Group::Zoxide => "zoxide",
            Group::Plugins => "plugins",
        }
    }
}

/// One node in the switcher tree (spec §4.1).
///
/// `crumbs` is set on **leaves only** ("herdr-dev › editor"), for search
/// mode. `children` empty ⇒ leaf ⇒ appears in search mode. `preview` and
/// `actions` are resolved lazily / per-kind.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub kind: Kind,
    pub label: String,
    /// Right-aligned meta column: "%2", "902", "waiting", "v0.3.0".
    pub meta: String,
    /// "herdr-dev › editor" — set on leaves only, for search mode.
    pub crumbs: Option<String>,
    pub children: Vec<Node>,
    pub preview: Preview,
    pub actions: Actions,
}

impl Node {
    /// Leaf-ness is structural (spec §4.1).
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// Preview payload (spec §7): one shape for every kind, four stacked
/// regions. Resolved lazily, cached per id, debounced 60ms.
#[derive(Debug, Clone, Default)]
pub struct Preview {
    pub icon: char,
    pub title: String,
    pub subtitle: String,
    pub chips: Vec<Chip>,
    pub body_label: &'static str,
    /// Pre-rendered monospace lines, ANSI-styled for pane scrollback.
    /// Clipped at the pane height — never scrollable (spec §7.4).
    pub body: Vec<ratatui::text::Line<'static>>,
    pub action: String,
    pub alt: String,
}

/// One status pill in the preview header (spec §7).
#[derive(Debug, Clone)]
pub struct Chip {
    pub text: String,
    pub semantic: ChipSemantic,
}

/// Chip colour semantics (spec §7 / §9). Colour never carries meaning
/// alone — every coloured state also has a word in the meta column or a
/// chip label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipSemantic {
    Ok,
    Info,
    Warn,
    Error,
    Blocked,
}

/// Default + alternate actions for a node (spec §8.2). Named in the
/// preview footer so Enter is never a guess.
#[derive(Debug, Clone, Default)]
pub struct Actions {
    pub default: String,
    pub alt: String,
}

/// Which action to invoke (spec §8.2). `Default` runs the node's default
/// action; the alternates are the `^r ^c ^x` context actions, named per
/// item in the preview footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Default,
    AltRestart,
    AltInterrupt,
    AltDetach,
}

/// What a provider returns from `invoke` (spec §5). `Close` ends the
/// popup (and toasts what happened); `Stay` keeps it open (side actions
/// like pin).
#[derive(Debug, Clone)]
pub enum Outcome {
    Close { toast: String },
    Stay { toast: String },
}

/// One provider per group (spec §5). Cheap to enumerate, lazy to preview.
///
/// Providers are trait objects behind a `match Group` dispatch in
/// `source.rs`. A provider that fails must not break the popup: its group
/// row stays, its meta becomes red "unavailable", and its preview shows
/// the error text.
pub trait Provider {
    fn id(&self) -> &'static str;
    /// Must return in < 30ms (cached data OK). Produces the group's subtree.
    fn enumerate(&self) -> Result<Node, String>;
    /// May block up to 80ms; render stale + spinner past that.
    fn preview(&self, id: &NodeId) -> Preview;
    /// Run an action on a node. Errors surface in the footer, not a dialog.
    fn invoke(&self, id: &NodeId, act: Act) -> Result<Outcome, String>;
}

/// Expansion state for browse mode (spec §3). Owned by the browse view
/// alone — untouched by searching. Restoring it across invocations is
/// optional and expires after 10 minutes.
#[derive(Debug, Clone, Default)]
pub struct Expansion {
    pub expanded: BTreeSet<NodeId>,
}

/// Twisty glyph state for a visible row (spec §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Twisty {
    /// Branch, currently expanded — `▾`.
    Expanded,
    /// Branch, currently collapsed — `▸`.
    Closed,
    /// Leaf — no twisty, blank.
    Leaf,
}

/// One flattened visible row in browse mode (spec §3.1/§10). Carries a
/// child-index path from root so actions can navigate back to the node,
/// plus a snapshot of the display fields so rendering needs no borrow.
#[derive(Debug, Clone)]
pub struct VisibleRow {
    /// Child indices from root to this node, e.g. `[0, 1, 2]`.
    pub path: Vec<usize>,
    /// Indent depth (0 = root group).
    pub depth: usize,
    pub kind: Kind,
    pub id: NodeId,
    pub label: String,
    pub meta: String,
    pub twisty: Twisty,
    pub is_leaf: bool,
}

/// The browse view's mutable state: the root tree, which branches are
/// expanded, the cursor (row index into the flattened visible rows), and
/// the vertical scroll. Search mode (Phase 4) adds a query; here it is
/// empty and browse is the only mode.
#[derive(Debug, Clone)]
pub struct Tree {
    pub root: Vec<Node>,
    pub expanded: BTreeSet<NodeId>,
    pub cursor: usize,
    pub scroll: usize,
}

impl Tree {
    /// Build a tree from root nodes with Session pre-expanded to its
    /// active workspace + tab (spec §4) and the cursor on row 0.
    pub fn new(root: Vec<Node>) -> Self {
        let mut t = Self {
            root,
            expanded: BTreeSet::new(),
            cursor: 0,
            scroll: 0,
        };
        t.expand_session_default();
        t
    }

    /// Pre-expand Session to its active workspace and that workspace's
    /// active tab (spec §4). "Active" is marked by `meta = "active"` on
    /// the workspace/tab node (set by the Session provider from the launch
    /// context). Falls back to expanding Session + its first workspace +
    /// first tab if no active marker is present. Only acts when root[0]
    /// is the Session group (`id == "group:session"`).
    fn expand_session_default(&mut self) {
        // Session is root[0], id "group:session" (see source::stub_group).
        let Some(session) = self.root.first() else {
            return;
        };
        if session.id != "group:session" {
            return;
        }
        self.expanded.insert(session.id.clone());
        // Find the active workspace (or first).
        let ws_idx = session
            .children
            .iter()
            .position(|c| c.meta == "active")
            .unwrap_or(0);
        if let Some(ws) = session.children.get(ws_idx) {
            self.expanded.insert(ws.id.clone());
            // Find the active tab (or first).
            let tab_idx = ws
                .children
                .iter()
                .position(|c| c.meta == "active")
                .unwrap_or(0);
            if let Some(tab) = ws.children.get(tab_idx) {
                self.expanded.insert(tab.id.clone());
            }
        }
    }

    /// Borrow the node at a child-index path from root.
    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let mut node = self.root.get(*path.first()?)?;
        for &idx in &path[1..] {
            node = node.children.get(idx)?;
        }
        Some(node)
    }

    /// Borrow the node at a path, mutably.
    #[allow(dead_code)] // used in Phase 3 (invoke path)
    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut node = self.root.get_mut(*path.first()?)?;
        for &idx in &path[1..] {
            node = node.children.get_mut(idx)?;
        }
        Some(node)
    }

    /// Flatten the tree to visible rows, descending only into expanded
    /// branches (spec §3.1). Leaves and collapsed branches appear as
    /// single rows; expanded branches emit their children below.
    pub fn visible_rows(&self) -> Vec<VisibleRow> {
        let mut out = Vec::new();
        for (i, node) in self.root.iter().enumerate() {
            self.flatten_into(node, vec![i], 0, &mut out);
        }
        out
    }

    fn flatten_into(&self, node: &Node, path: Vec<usize>, depth: usize, out: &mut Vec<VisibleRow>) {
        let is_leaf = node.is_leaf();
        let is_expanded = self.expanded.contains(&node.id);
        let twisty = if is_leaf {
            Twisty::Leaf
        } else if is_expanded {
            Twisty::Expanded
        } else {
            Twisty::Closed
        };
        out.push(VisibleRow {
            path: path.clone(),
            depth,
            kind: node.kind,
            id: node.id.clone(),
            label: node.label.clone(),
            meta: node.meta.clone(),
            twisty,
            is_leaf,
        });
        if is_expanded && !is_leaf {
            for (i, child) in node.children.iter().enumerate() {
                // Child path = THIS node's path + child index. Do NOT
                // read `out.last().path`: that is the most recently
                // pushed row, which after recursing into an earlier
                // sibling's subtree is a *descendant*, not this node —
                // so a later sibling would inherit the descendant's path.
                let mut child_path = path.clone();
                child_path.push(i);
                self.flatten_into(child, child_path, depth + 1, out);
            }
        }
    }

    /// The visible row under the cursor, if any.
    pub fn cursor_row(&self) -> Option<VisibleRow> {
        self.visible_rows().get(self.cursor).cloned()
    }

    /// Move the cursor down one visible row, wrapping (spec §8).
    pub fn move_down(&mut self) {
        let n = self.visible_rows().len();
        if n > 0 {
            self.cursor = (self.cursor + 1) % n;
        }
    }

    /// Move the cursor up one visible row, wrapping (spec §8).
    pub fn move_up(&mut self) {
        let n = self.visible_rows().len();
        if n > 0 {
            self.cursor = (self.cursor + n - 1) % n;
        }
    }

    /// `→` / `Space` / `Tab`: expand a collapsed branch; if already open,
    /// step the cursor to its first child (spec §8).
    pub fn expand_or_step(&mut self) {
        let Some(row) = self.cursor_row() else {
            return;
        };
        if row.is_leaf {
            return; // leaves don't expand
        }
        if self.expanded.contains(&row.id) {
            // Already open → step to first child (if it has one).
            let child_count = self
                .node_at(&row.path)
                .map(|n| n.children.len())
                .unwrap_or(0);
            if child_count > 0 {
                let target = row.path;
                let rows = self.visible_rows();
                if let Some(pos) = rows.iter().position(|r| {
                    r.path.len() == target.len() + 1 && r.path[..target.len()] == target[..]
                }) {
                    self.cursor = pos;
                }
            }
        } else {
            self.expanded.insert(row.id);
        }
    }

    /// `←`: collapse an expanded branch; if already closed, jump to the
    /// parent (spec §8).
    pub fn collapse_or_parent(&mut self) {
        let Some(row) = self.cursor_row() else {
            return;
        };
        if row.is_leaf || !self.expanded.contains(&row.id) {
            // Closed/leaf → jump to parent.
            if row.path.len() > 1 {
                let parent_path = row.path[..row.path.len() - 1].to_vec();
                let rows = self.visible_rows();
                if let Some(pos) = rows.iter().position(|r| r.path == parent_path) {
                    self.cursor = pos;
                }
            }
        } else {
            self.expanded.remove(&row.id);
        }
    }

    /// `Enter` in browse: toggle a branch; inert on a leaf (spec §8). The
    /// leaf default action lands in Phase 3.
    pub fn toggle(&mut self) {
        let Some(row) = self.cursor_row() else {
            return;
        };
        if row.is_leaf {
            return;
        }
        if self.expanded.contains(&row.id) {
            self.expanded.remove(&row.id);
        } else {
            self.expanded.insert(row.id);
        }
    }

    /// Keep the cursor in range after a structural change.
    pub fn ensure_cursor_valid(&mut self) {
        let n = self.visible_rows().len();
        if n == 0 {
            self.cursor = 0;
        } else if self.cursor >= n {
            self.cursor = n - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str, label: &str) -> Node {
        Node {
            id: id.into(),
            kind: Kind::Pane,
            label: label.into(),
            meta: String::new(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: Actions::default(),
        }
    }

    fn branch(id: &str, label: &str, children: Vec<Node>) -> Node {
        Node {
            id: id.into(),
            kind: Kind::Group,
            label: label.into(),
            meta: String::new(),
            crumbs: None,
            children,
            preview: Preview::default(),
            actions: Actions::default(),
        }
    }

    #[test]
    fn visible_rows_respects_expansion() {
        let tree = Tree::new(vec![branch(
            "g",
            "group",
            vec![leaf("a", "alpha"), leaf("b", "beta")],
        )]);
        // Group collapsed by default → 1 row.
        assert_eq!(tree.visible_rows().len(), 1);
        let mut t = tree;
        t.expanded.insert("g".into());
        assert_eq!(t.visible_rows().len(), 3); // group + 2 leaves
    }

    #[test]
    fn cursor_wraps_down() {
        let mut t = Tree::new(vec![leaf("a", "a"), leaf("b", "b"), leaf("c", "c")]);
        t.move_down();
        assert_eq!(t.cursor, 1);
        t.move_down();
        t.move_down();
        assert_eq!(t.cursor, 0); // wraps
    }

    #[test]
    fn cursor_wraps_up() {
        let mut t = Tree::new(vec![leaf("a", "a"), leaf("b", "b")]);
        t.move_up();
        assert_eq!(t.cursor, 1); // wraps from 0 to last
    }

    #[test]
    fn expand_then_step_to_child() {
        let mut t = Tree::new(vec![branch("g", "group", vec![leaf("a", "alpha")])]);
        // collapsed → expand
        t.expand_or_step();
        assert!(t.expanded.contains("g"));
        assert_eq!(t.cursor, 0); // still on group
                                 // already open → step to first child
        t.expand_or_step();
        assert_eq!(t.cursor_row().unwrap().id, "a");
    }

    #[test]
    fn collapse_then_jump_to_parent() {
        let mut t = Tree::new(vec![branch("g", "group", vec![leaf("a", "alpha")])]);
        t.expanded.insert("g".into());
        t.cursor = 1; // on child "a"
        t.collapse_or_parent(); // leaf → jump to parent
        assert_eq!(t.cursor_row().unwrap().id, "g");
        t.collapse_or_parent(); // now on group, expanded → collapse
        assert!(!t.expanded.contains("g"));
    }

    #[test]
    fn toggle_branch() {
        let mut t = Tree::new(vec![branch("g", "group", vec![leaf("a", "alpha")])]);
        t.toggle(); // expand
        assert!(t.expanded.contains("g"));
        t.toggle(); // collapse
        assert!(!t.expanded.contains("g"));
    }

    #[test]
    fn toggle_inert_on_leaf() {
        let mut t = Tree::new(vec![leaf("a", "alpha")]);
        t.toggle();
        assert_eq!(t.cursor, 0); // unchanged
    }

    #[test]
    fn session_default_expansion() {
        // Session with one active workspace + active tab + a pane.
        let session = branch(
            "group:session",
            "session",
            vec![Node {
                id: "ws".into(),
                kind: Kind::Workspace,
                label: "herdr-dev".into(),
                meta: "active".into(),
                crumbs: None,
                children: vec![Node {
                    id: "tab".into(),
                    kind: Kind::Tab,
                    label: "editor".into(),
                    meta: "active".into(),
                    crumbs: None,
                    children: vec![leaf("p1", "nvim")],
                    preview: Preview::default(),
                    actions: Actions::default(),
                }],
                preview: Preview::default(),
                actions: Actions::default(),
            }],
        );
        let t = Tree::new(vec![session]);
        assert!(t.expanded.contains("group:session"));
        assert!(t.expanded.contains("ws"));
        assert!(t.expanded.contains("tab"));
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn sibling_after_expanded_subtree_has_correct_path() {
        // Session with two workspaces; expand the first workspace's
        // tab + pane, then the second workspace's path must be [0,1]
        // (sibling of the first), NOT inherited from the pane.
        let pane = |id: &str| Node {
            id: id.into(),
            kind: Kind::Pane,
            label: id.into(),
            meta: String::new(),
            crumbs: None,
            children: Vec::new(),
            preview: Preview::default(),
            actions: Actions::default(),
        };
        let tab = |id: &str, panes: Vec<Node>| Node {
            id: id.into(),
            kind: Kind::Tab,
            label: id.into(),
            meta: String::new(),
            crumbs: None,
            children: panes,
            preview: Preview::default(),
            actions: Actions::default(),
        };
        let ws = |id: &str, tabs: Vec<Node>| Node {
            id: id.into(),
            kind: Kind::Workspace,
            label: id.into(),
            meta: String::new(),
            crumbs: None,
            children: tabs,
            preview: Preview::default(),
            actions: Actions::default(),
        };
        let w1 = ws(
            "session:ws:w1",
            vec![tab("session:tab:t1", vec![pane("session:pane:p1")])],
        );
        let w2 = ws(
            "session:ws:w2",
            vec![tab("session:tab:t2", vec![pane("session:pane:p2")])],
        );
        let session = Node {
            id: "group:session".into(),
            kind: Kind::Group,
            label: "S".into(),
            meta: String::new(),
            crumbs: None,
            children: vec![w1, w2],
            preview: Preview::default(),
            actions: Actions::default(),
        };
        let mut t = Tree::new(vec![session]);
        // Pre-expand everything so both subtrees are visible.
        t.expanded.insert("group:session".into());
        t.expanded.insert("session:ws:w1".into());
        t.expanded.insert("session:tab:t1".into());
        t.expanded.insert("session:ws:w2".into());
        t.expanded.insert("session:tab:t2".into());
        let rows = t.visible_rows();
        // w2 must be at depth 1, path [0,1] — NOT [0,0,0,0,1].
        let w2 = rows.iter().find(|r| r.id == "session:ws:w2").unwrap();
        assert_eq!(w2.depth, 1);
        assert_eq!(w2.path, vec![0, 1]);
        // And its pane p2 at depth 3, path [0,1,0,0].
        let p2 = rows.iter().find(|r| r.id == "session:pane:p2").unwrap();
        assert_eq!(p2.depth, 3);
        assert_eq!(p2.path, vec![0, 1, 0, 0]);
    }
}
