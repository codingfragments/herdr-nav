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
//! **Status: scaffold only.** The real providers land in the phase
//! sequence in PLANNING.md §17; this module defines the vocabulary every
//! phase builds on.

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
    pub body: Vec<String>,
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
