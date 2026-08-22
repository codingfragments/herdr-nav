//! Query-filter parser (spec §15 #2, Phase 11).
//!
//! A small parser that runs **before** the existing nucleo scorer: it
//! splits the query into filter tokens and a fuzzy needle, filters the
//! haystack, then hands the needle to the same `FuzzyEngine` from
//! Phase 4. No second matcher, no scoring-model change.
//!
//! ## Syntax
//!
//! - **Group scope prefix** (leading token only): `agents nvim` → only
//!   Agents leaves, then fuzzy `nvim`. Groups: `session`, `agents`,
//!   `pinned`, `zoxide`, `plugins`.
//! - **Kind filter** `kind:X` or `@X` (position-independent): restricts
//!   to leaves of that `Kind`. Kinds: `pane`, `agent`, `dir`, `zox`,
//!   `plugin`, `tab`, `workspace`. `@` is sugar for `kind:`.
//!   - `dir` is a **union alias**: matches both `Kind::Dir` (pinned)
//!     and `Kind::Zox` (zoxide).
//! - **Negation** `!X`: excludes a kind or group. `!plugin` → no
//!   plugins; `!zox` → no zoxide; `!agents` → no Agents-group leaves.
//!
//! ## Composition
//!
//! ```text
//! result = group_scope ∩ union(positive_kinds) − union(negations) |› nucleo(needle)
//! ```
//!
//! - Only one positive group scope (leading token). A second is fuzzy text.
//! - Multiple positive kinds are OR (a node has one Kind).
//! - Group scope intersects with the kind union.
//! - Negations subtract afterward.
//! - Contradictions → no-match, not errors.
//! - Unrecognised `@`/`kind:` tokens → treated as fuzzy text.
//! - Empty needle after filters → show all matching leaves.

use crate::nav::{Group, Kind};

// ── Parsed query ─────────────────────────────────────────────────────────────

/// The result of parsing a raw query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    /// The group scope (if any). `None` = all groups.
    pub group_scope: Option<Group>,
    /// Positive kind filters (OR semantics).
    pub positive_kinds: Vec<Kind>,
    /// Negative filters (kinds or groups to exclude).
    pub negations: Vec<Negation>,
    /// The remaining fuzzy needle text (filters removed).
    pub needle: String,
}

/// A negation target: a kind or a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Negation {
    Kind(Kind),
    /// `dir` negation = exclude both Dir and Zox (union alias).
    DirUnion,
    Group(Group),
}

// ── Token recognition ────────────────────────────────────────────────────────

/// Known group names (for scope + negation).
const GROUP_NAMES: &[&str] = &["session", "agents", "pinned", "zoxide", "plugins"];

/// Resolve a kind name to a `Kind`. Returns `None` for unknown.
/// `dir` is NOT resolved here (it's a union alias — handled separately).
fn resolve_kind(name: &str) -> Option<Kind> {
    match name {
        "pane" => Some(Kind::Pane),
        "agent" => Some(Kind::Agent),
        "zox" => Some(Kind::Zox),
        "plugin" => Some(Kind::Plugin),
        "tab" => Some(Kind::Tab),
        "workspace" => Some(Kind::Workspace),
        _ => None,
    }
}

/// Resolve a group name to a `Group`. Returns `None` for unknown.
fn resolve_group(name: &str) -> Option<Group> {
    match name {
        "session" => Some(Group::Session),
        "agents" => Some(Group::Agents),
        "pinned" => Some(Group::Pinned),
        "zoxide" => Some(Group::Zoxide),
        "plugins" => Some(Group::Plugins),
        _ => None,
    }
}

/// Resolve a negation token (`!X`). Tries kind first, then group.
/// `dir` → `DirUnion` (exclude both Dir + Zox).
fn resolve_negation(name: &str) -> Option<Negation> {
    if name == "dir" {
        return Some(Negation::DirUnion);
    }
    if let Some(k) = resolve_kind(name) {
        return Some(Negation::Kind(k));
    }
    if let Some(g) = resolve_group(name) {
        return Some(Negation::Group(g));
    }
    None
}

// ── Parser ───────────────────────────────────────────────────────────────────

impl ParsedQuery {
    /// Parse a raw query string into filters + needle.
    ///
    /// Rules:
    /// - The first whitespace-delimited token may be a group scope
    ///   (must match a known group name). If it does, it's consumed as
    ///   the scope; otherwise it's part of the needle.
    /// - `kind:X` and `@X` tokens (anywhere) are positive kind filters.
    ///   Unrecognised ones stay in the needle.
    /// - `!X` tokens (anywhere) are negations. Unrecognised ones stay
    ///   in the needle.
    /// - Everything else is the fuzzy needle (joined with spaces).
    pub fn parse(raw: &str) -> Self {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.is_empty() {
            return ParsedQuery {
                group_scope: None,
                positive_kinds: Vec::new(),
                negations: Vec::new(),
                needle: String::new(),
            };
        }

        let mut group_scope: Option<Group> = None;
        let mut positive_kinds: Vec<Kind> = Vec::new();
        let mut negations: Vec<Negation> = Vec::new();
        let mut needle_parts: Vec<String> = Vec::new();

        // First token: check for group scope.
        let start_idx = if GROUP_NAMES.contains(&tokens[0]) {
            group_scope = resolve_group(tokens[0]);
            1
        } else {
            0
        };

        for tok in &tokens[start_idx..] {
            // Try `kind:X` or `@X` (positive kind filter).
            if let Some(kind_name) = tok.strip_prefix("kind:") {
                if kind_name == "dir" {
                    // Union alias: add both Dir and Zox.
                    add_kind(&mut positive_kinds, Kind::Dir);
                    add_kind(&mut positive_kinds, Kind::Zox);
                } else if let Some(k) = resolve_kind(kind_name) {
                    add_kind(&mut positive_kinds, k);
                } else {
                    // Unrecognised → fuzzy text.
                    needle_parts.push((*tok).to_string());
                }
                continue;
            }
            if let Some(kind_name) = tok.strip_prefix('@') {
                if kind_name == "dir" {
                    add_kind(&mut positive_kinds, Kind::Dir);
                    add_kind(&mut positive_kinds, Kind::Zox);
                } else if let Some(k) = resolve_kind(kind_name) {
                    add_kind(&mut positive_kinds, k);
                } else {
                    needle_parts.push((*tok).to_string());
                }
                continue;
            }
            // Try `!X` (negation).
            if let Some(neg_name) = tok.strip_prefix('!') {
                if let Some(n) = resolve_negation(neg_name) {
                    add_negation(&mut negations, n);
                } else {
                    needle_parts.push((*tok).to_string());
                }
                continue;
            }
            // Ordinary text → needle.
            needle_parts.push((*tok).to_string());
        }

        ParsedQuery {
            group_scope,
            positive_kinds,
            negations,
            needle: needle_parts.join(" "),
        }
    }

    /// Whether any filters are active (for the status strip).
    pub fn has_filters(&self) -> bool {
        self.group_scope.is_some() || !self.positive_kinds.is_empty() || !self.negations.is_empty()
    }

    /// A compact status-strip label, e.g. `"agents · pane · !zox"`.
    pub fn status_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(g) = self.group_scope {
            parts.push(g.provider_id().to_string());
        }
        for k in &self.positive_kinds {
            parts.push(kind_label(*k).to_string());
        }
        for n in &self.negations {
            match n {
                Negation::Kind(k) => parts.push(format!("!{}", kind_label(*k))),
                Negation::DirUnion => parts.push("!dir".to_string()),
                Negation::Group(g) => parts.push(format!("!{}", g.provider_id())),
            }
        }
        parts.join(" · ")
    }

    /// Filter a haystack: return indices of leaves that pass all
    /// filters (group scope ∩ positive kinds − negations).
    pub fn filter_haystack(&self, haystack: &[crate::search::Leaf]) -> Vec<usize> {
        haystack
            .iter()
            .enumerate()
            .filter(|(_, leaf)| self.passes(leaf))
            .map(|(i, _)| i)
            .collect()
    }

    /// Does a single leaf pass all filters?
    fn passes(&self, leaf: &crate::search::Leaf) -> bool {
        // Group scope.
        if let Some(g) = self.group_scope {
            if leaf.group != g {
                return false;
            }
        }
        // Positive kinds (OR). Empty = all pass.
        if !self.positive_kinds.is_empty() && !self.positive_kinds.contains(&leaf.kind) {
            return false;
        }
        // Negations (subtract).
        for n in &self.negations {
            match n {
                Negation::Kind(k) => {
                    if leaf.kind == *k {
                        return false;
                    }
                }
                Negation::DirUnion => {
                    if leaf.kind == Kind::Dir || leaf.kind == Kind::Zox {
                        return false;
                    }
                }
                Negation::Group(g) => {
                    if leaf.group == *g {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Add a kind, deduplicated.
fn add_kind(kinds: &mut Vec<Kind>, k: Kind) {
    if !kinds.contains(&k) {
        kinds.push(k);
    }
}

/// Add a negation, deduplicated.
fn add_negation(negs: &mut Vec<Negation>, n: Negation) {
    if !negs.contains(&n) {
        negs.push(n);
    }
}

/// Short label for a kind (for the status strip).
fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Pane => "pane",
        Kind::Agent => "agent",
        Kind::Dir => "dir",
        Kind::Zox => "zox",
        Kind::Plugin => "plugin",
        Kind::Tab => "tab",
        Kind::Workspace => "workspace",
        Kind::Group => "group",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pq(raw: &str) -> ParsedQuery {
        ParsedQuery::parse(raw)
    }

    // ── Basic parsing ─────────────────────────────────────────────────────────

    #[test]
    fn plain_text_no_filters() {
        let p = pq("nvim");
        assert_eq!(p.group_scope, None);
        assert!(p.positive_kinds.is_empty());
        assert!(p.negations.is_empty());
        assert_eq!(p.needle, "nvim");
        assert!(!p.has_filters());
    }

    #[test]
    fn group_scope_first_token() {
        let p = pq("agents nvim");
        assert_eq!(p.group_scope, Some(Group::Agents));
        assert_eq!(p.needle, "nvim");
        assert!(p.has_filters());
    }

    #[test]
    fn group_scope_consumes_only_first() {
        let p = pq("session agents");
        // "session" is the scope; "agents" is needle text (second group
        // token is NOT a scope — it's fuzzy text).
        assert_eq!(p.group_scope, Some(Group::Session));
        assert_eq!(p.needle, "agents");
    }

    // ── Kind filters ─────────────────────────────────────────────────────────

    #[test]
    fn at_kind_filter() {
        let p = pq("@pane");
        assert_eq!(p.positive_kinds, vec![Kind::Pane]);
        assert_eq!(p.needle, "");
        assert!(p.has_filters());
    }

    #[test]
    fn kind_colon_filter() {
        let p = pq("kind:agent");
        assert_eq!(p.positive_kinds, vec![Kind::Agent]);
    }

    #[test]
    fn dir_union_alias() {
        let p = pq("@dir");
        assert!(p.positive_kinds.contains(&Kind::Dir));
        assert!(p.positive_kinds.contains(&Kind::Zox));
    }

    #[test]
    fn multiple_positive_kinds_are_or() {
        let p = pq("@pane @dir");
        assert!(p.positive_kinds.contains(&Kind::Pane));
        assert!(p.positive_kinds.contains(&Kind::Dir));
        assert!(p.positive_kinds.contains(&Kind::Zox));
    }

    #[test]
    fn kind_filter_with_needle() {
        let p = pq("@pane nvim");
        assert_eq!(p.positive_kinds, vec![Kind::Pane]);
        assert_eq!(p.needle, "nvim");
    }

    #[test]
    fn unrecognised_at_is_fuzzy() {
        let p = pq("@pnae nvim");
        assert!(p.positive_kinds.is_empty());
        assert_eq!(p.needle, "@pnae nvim");
    }

    #[test]
    fn unrecognised_kind_colon_is_fuzzy() {
        let p = pq("kind:xyz nvim");
        assert!(p.positive_kinds.is_empty());
        assert_eq!(p.needle, "kind:xyz nvim");
    }

    // ── Negations ────────────────────────────────────────────────────────────

    #[test]
    fn negation_kind() {
        let p = pq("!plugin");
        assert_eq!(p.negations, vec![Negation::Kind(Kind::Plugin)]);
    }

    #[test]
    fn negation_group() {
        let p = pq("!agents");
        assert_eq!(p.negations, vec![Negation::Group(Group::Agents)]);
    }

    #[test]
    fn negation_dir_union() {
        let p = pq("!dir");
        assert_eq!(p.negations, vec![Negation::DirUnion]);
    }

    #[test]
    fn negation_zox_kind() {
        let p = pq("!zox");
        assert_eq!(p.negations, vec![Negation::Kind(Kind::Zox)]);
    }

    #[test]
    fn negation_zoxide_group() {
        let p = pq("!zoxide");
        assert_eq!(p.negations, vec![Negation::Group(Group::Zoxide)]);
    }

    #[test]
    fn multiple_negations() {
        let p = pq("!plugin !zox nvim");
        assert_eq!(p.negations.len(), 2);
        assert_eq!(p.needle, "nvim");
    }

    #[test]
    fn unrecognised_negation_is_fuzzy() {
        let p = pq("!xyz nvim");
        assert!(p.negations.is_empty());
        assert_eq!(p.needle, "!xyz nvim");
    }

    // ── Composition ──────────────────────────────────────────────────────────

    #[test]
    fn group_scope_and_kind() {
        let p = pq("session @pane nvim");
        assert_eq!(p.group_scope, Some(Group::Session));
        assert_eq!(p.positive_kinds, vec![Kind::Pane]);
        assert_eq!(p.needle, "nvim");
    }

    #[test]
    fn dir_not_zox() {
        let p = pq("@dir !zox");
        // @dir adds Dir+Zox; !zox subtracts Zox → effectively Dir only.
        assert!(p.positive_kinds.contains(&Kind::Dir));
        assert!(p.positive_kinds.contains(&Kind::Zox));
        assert_eq!(p.negations, vec![Negation::Kind(Kind::Zox)]);
    }

    // ── Dedup ────────────────────────────────────────────────────────────────

    #[test]
    fn dedup_positive_kinds() {
        let p = pq("@pane @pane nvim");
        assert_eq!(p.positive_kinds.len(), 1);
    }

    #[test]
    fn dedup_negations() {
        let p = pq("!plugin !plugin nvim");
        assert_eq!(p.negations.len(), 1);
    }

    // ── Empty / edge ──────────────────────────────────────────────────────────

    #[test]
    fn empty_query() {
        let p = pq("");
        assert!(!p.has_filters());
        assert_eq!(p.needle, "");
    }

    #[test]
    fn only_filter_no_needle() {
        let p = pq("@pane");
        assert_eq!(p.needle, "");
        assert!(p.has_filters());
    }

    // ── Status label ──────────────────────────────────────────────────────────

    #[test]
    fn status_label_format() {
        let p = pq("session @pane !zox nvim");
        assert_eq!(p.status_label(), "session · pane · !zox");
    }

    #[test]
    fn status_label_empty() {
        let p = pq("nvim");
        assert_eq!(p.status_label(), "");
    }

    // ── Filter haystack ──────────────────────────────────────────────────────

    use crate::search::Leaf;

    fn leaf(id: &str, kind: Kind, group: Group) -> Leaf {
        Leaf {
            path: vec![0],
            kind,
            group,
            id: id.into(),
            label: id.into(),
            meta: String::new(),
            crumbs: String::new(),
            crumb_prefix_len: 0,
            match_text: id.into(),
        }
    }

    #[test]
    fn filter_group_scope() {
        let h = vec![
            leaf("p1", Kind::Pane, Group::Session),
            leaf("a1", Kind::Agent, Group::Agents),
        ];
        let p = pq("session");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0]); // only the Session pane
    }

    #[test]
    fn filter_positive_kind() {
        let h = vec![
            leaf("p1", Kind::Pane, Group::Session),
            leaf("a1", Kind::Agent, Group::Agents),
            leaf("d1", Kind::Dir, Group::Pinned),
        ];
        let p = pq("@pane");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0]);
    }

    #[test]
    fn filter_dir_union() {
        let h = vec![
            leaf("d1", Kind::Dir, Group::Pinned),
            leaf("z1", Kind::Zox, Group::Zoxide),
            leaf("p1", Kind::Pane, Group::Session),
        ];
        let p = pq("@dir");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0, 1]); // Dir + Zox, not Pane
    }

    #[test]
    fn filter_negation() {
        let h = vec![
            leaf("p1", Kind::Pane, Group::Session),
            leaf("pl1", Kind::Plugin, Group::Plugins),
        ];
        let p = pq("!plugin");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0]); // pane only, plugin excluded
    }

    #[test]
    fn filter_contradiction_no_matches() {
        let h = vec![
            leaf("p1", Kind::Pane, Group::Session),
            leaf("a1", Kind::Agent, Group::Agents),
        ];
        // Agents group has no Pane leaves → contradiction → no matches.
        let p = pq("agents @pane");
        let r = p.filter_haystack(&h);
        assert!(r.is_empty());
    }

    #[test]
    fn filter_dir_not_zox() {
        let h = vec![
            leaf("d1", Kind::Dir, Group::Pinned),
            leaf("z1", Kind::Zox, Group::Zoxide),
        ];
        let p = pq("@dir !zox");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0]); // Dir only, Zox excluded
    }

    #[test]
    fn filter_no_filters_passes_all() {
        let h = vec![
            leaf("p1", Kind::Pane, Group::Session),
            leaf("a1", Kind::Agent, Group::Agents),
        ];
        let p = pq("nvim");
        let r = p.filter_haystack(&h);
        assert_eq!(r, vec![0, 1]); // all pass
    }
}
