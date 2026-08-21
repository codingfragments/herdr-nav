//! Search mode: haystack build, fuzzy matching, ranking, and highlight
//! (spec §3.2/§6).
//!
//! Mode is derived: `mode = if query.is_empty() { Browse } else { Search }`.
//! The haystack is built **once per invocation** (not per keystroke):
//! a depth-first walk of all five subtrees, leaves only, in group order.
//! Each leaf's match text is `crumbs + " › " + label` — crumbs are
//! searchable, which lets `editor nvim` and `notes zsh` work as queries.
//!
//! **Ranking contract (decided 2026-08-21):** use `nucleo-matcher`'s
//! scoring verbatim (reuse the `FuzzyEngine` shape from
//! `herdr-zextract/src/picker/fuzzy.rs`), pin the version in
//! `Cargo.lock`. The spec's §6.2 formula stays advisory/optional.
//! Provider bias (§6.3) is added via `filter_with_bonus`.

use crate::nav::{Kind, Node, Tree};

/// Fuzzy matching wrapper over `nucleo-matcher` (ported from
/// `herdr-zextract/src/picker/fuzzy.rs`). Smart-case: query containing
/// any uppercase char → case-sensitive. Empty query returns all items
/// in input order with empty indices.
pub struct FuzzyEngine {
    matcher: nucleo_matcher::Matcher,
}

#[derive(Debug, Clone)]
pub struct ScoredMatch {
    /// Index into the haystack `Vec<Leaf>`.
    pub index: usize,
    /// Fuzzy score (higher = better) plus provider bias.
    pub score: i32,
    /// Character positions in the match text that matched.
    pub indices: Vec<u32>,
}

impl Default for FuzzyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyEngine {
    pub fn new() -> Self {
        Self {
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
        }
    }

    /// Filter/score `items` against `query`, adding a per-item bonus.
    pub fn filter_with_bonus<F: Fn(usize) -> i32>(
        &mut self,
        query: &str,
        items: &[String],
        bonus_fn: F,
    ) -> Vec<ScoredMatch> {
        if query.is_empty() {
            return (0..items.len())
                .map(|i| ScoredMatch {
                    index: i,
                    score: 0,
                    indices: Vec::new(),
                })
                .collect();
        }

        self.matcher.config.ignore_case = !query.chars().any(|c| c.is_ascii_uppercase());

        let needle_chars: Vec<char>;
        let needle = if query.is_ascii() {
            nucleo_matcher::Utf32Str::Ascii(query.as_bytes())
        } else {
            needle_chars = query.chars().collect();
            nucleo_matcher::Utf32Str::Unicode(&needle_chars)
        };

        let mut results: Vec<ScoredMatch> = Vec::with_capacity(items.len());
        let mut haystack_chars: Vec<char> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for (i, item) in items.iter().enumerate() {
            let haystack = if item.is_ascii() {
                nucleo_matcher::Utf32Str::Ascii(item.as_bytes())
            } else {
                haystack_chars.clear();
                haystack_chars.extend(item.chars());
                nucleo_matcher::Utf32Str::Unicode(&haystack_chars)
            };
            indices.clear();
            if let Some(score) = self.matcher.fuzzy_indices(haystack, needle, &mut indices) {
                results.push(ScoredMatch {
                    index: i,
                    score: score as i32 + bonus_fn(i),
                    indices: indices.clone(),
                });
            }
        }

        results.sort_unstable_by_key(|b| std::cmp::Reverse(b.score));
        results
    }
}

// ── Haystack (spec §6.1) ───────────────────────────────────────────────────

/// One leaf in the search haystack. Carries a child-index path from
/// root so the preview can look up the live node, plus the precomputed
/// crumb prefix and match text.
#[allow(dead_code)] // label/meta/crumbs used in tests + Phase 9 meta column
#[derive(Debug, Clone)]
pub struct Leaf {
    /// Child indices from root to this leaf (for node_at lookup).
    pub path: Vec<usize>,
    pub kind: Kind,
    pub id: String,
    pub label: String,
    pub meta: String,
    /// "herdr-dev › editor" — the crumb prefix (no trailing " › ").
    pub crumbs: String,
    /// Length of `crumbs + " › "` in **characters** — rendering dims
    /// the first `crumb_prefix_len` chars of a match-text row.
    pub crumb_prefix_len: usize,
    /// `crumbs + " › " + label` — the searchable text.
    pub match_text: String,
}

/// Build the haystack once per invocation: a depth-first walk of all
/// five subtrees, leaves only, in group order (spec §6.1). Crumbs are
/// precomputed at tree-build time so search-mode rows need no upward
/// traversal during keystroke handling.
pub fn build_haystack(tree: &Tree) -> Vec<Leaf> {
    let mut out = Vec::new();
    for (i, node) in tree.root.iter().enumerate() {
        walk(node, &[i], "", &mut out);
    }
    out
}

fn walk(node: &Node, path: &[usize], parent_crumbs: &str, out: &mut Vec<Leaf>) {
    if node.is_leaf() {
        let crumbs = parent_crumbs.to_string();
        let crumb_prefix_len = if crumbs.is_empty() {
            0
        } else {
            crumbs.chars().count() + 3 // " › "
        };
        let match_text = if crumbs.is_empty() {
            node.label.clone()
        } else {
            format!("{crumbs} › {}", node.label)
        };
        out.push(Leaf {
            path: path.to_vec(),
            kind: node.kind,
            id: node.id.clone(),
            label: node.label.clone(),
            meta: node.meta.clone(),
            crumbs,
            crumb_prefix_len,
            match_text,
        });
        return;
    }
    // Branch: build crumb for children. The group row's own label is
    // the first crumb segment; interior nodes append their label too.
    let crumb = if parent_crumbs.is_empty() {
        node.label.clone()
    } else {
        format!("{parent_crumbs} › {}", node.label)
    };
    for (i, child) in node.children.iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(i);
        walk(child, &child_path, &crumb, out);
    }
}

// ── Provider bias (spec §6.3) ───────────────────────────────────────────────

/// Flat additive bias so live things nudge ahead of stored things
/// (spec §6.3): agents needing input +6, live panes +4, pinned dirs
/// +3, other agents +2, zoxide +0, plugins −2. In Phase 4 only Session
/// (panes) is live; other kinds get their bias when their providers
/// land. `unavailable` groups contribute no leaves to the haystack.
pub fn provider_bias(leaf: &Leaf) -> i32 {
    match leaf.kind {
        Kind::Pane => 4,
        Kind::Agent => 2, // +6 when "needing input" is detectable (Phase 5)
        Kind::Dir => 3,
        Kind::Zox => 0,
        Kind::Plugin => -2,
        _ => 0,
    }
}

// ── Search view state ───────────────────────────────────────────────────────

/// The search view's mutable state. `None` in the event loop = browse
/// mode; `Some` = search mode (query non-empty).
#[derive(Debug, Clone)]
pub struct SearchView {
    pub query: String,
    /// Ranked matches, indices into the haystack.
    pub matches: Vec<ScoredMatch>,
    /// Cursor into `matches`.
    pub cursor: usize,
}

/// Run the fuzzy search against the haystack, returning ranked matches.
pub fn search(haystack: &[Leaf], query: &str) -> Vec<ScoredMatch> {
    let mut engine = FuzzyEngine::new();
    let items: Vec<String> = haystack.iter().map(|l| l.match_text.clone()).collect();
    engine.filter_with_bonus(query, &items, |i| provider_bias(&haystack[i]))
}

/// Build a fresh `SearchView` from a query.
pub fn view(haystack: &[Leaf], query: String) -> SearchView {
    let matches = search(haystack, &query);
    SearchView {
        query,
        matches,
        cursor: 0,
    }
}

impl SearchView {
    /// Re-run the search after a query mutation; reset cursor to 0
    /// (spec §3: "the cursor index resets to 0 on every query mutation").
    pub fn requery(&mut self, haystack: &[Leaf]) {
        self.matches = search(haystack, &self.query);
        self.cursor = 0;
    }

    /// Move the cursor down one match, wrapping (spec §8).
    pub fn move_down(&mut self) {
        if !self.matches.is_empty() {
            self.cursor = (self.cursor + 1) % self.matches.len();
        }
    }

    /// Move the cursor up one match, wrapping (spec §8).
    pub fn move_up(&mut self) {
        let n = self.matches.len();
        if n > 0 {
            self.cursor = (self.cursor + n - 1) % n;
        }
    }

    /// The leaf under the cursor, if any (for preview lookup).
    pub fn cursor_leaf<'a>(&self, haystack: &'a [Leaf]) -> Option<&'a Leaf> {
        self.matches
            .get(self.cursor)
            .and_then(|m| haystack.get(m.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::{Actions, Preview};

    fn leaf(id: &str, label: &str, kind: Kind) -> Leaf {
        Leaf {
            path: vec![0],
            kind,
            id: id.into(),
            label: label.into(),
            meta: String::new(),
            crumbs: String::new(),
            crumb_prefix_len: 0,
            match_text: label.into(),
        }
    }

    fn node(id: &str, kind: Kind, label: &str, children: Vec<Node>) -> Node {
        Node {
            id: id.into(),
            kind,
            label: label.into(),
            meta: String::new(),
            crumbs: None,
            children,
            preview: Preview::default(),
            actions: Actions::default(),
        }
    }

    #[test]
    fn haystack_walks_leaves_only() {
        // Session → ws → tab → pane1, pane2. Only pane1/pane2 are leaves.
        let tree = Tree::new(vec![node(
            "group:session",
            Kind::Group,
            "Session",
            vec![node(
                "session:ws:w1",
                Kind::Workspace,
                "w1",
                vec![node(
                    "session:tab:t1",
                    Kind::Tab,
                    "t1",
                    vec![
                        node("session:pane:p1", Kind::Pane, "nvim", vec![]),
                        node("session:pane:p2", Kind::Pane, "cargo", vec![]),
                    ],
                )],
            )],
        )]);
        let h = build_haystack(&tree);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].label, "nvim");
        assert_eq!(h[1].label, "cargo");
        // Crumbs include workspace + tab.
        assert!(h[0].crumbs.contains("w1"));
        assert!(h[0].crumbs.contains("t1"));
        assert!(h[0].match_text.contains("›"));
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let h = vec![
            leaf("a", "alpha", Kind::Pane),
            leaf("b", "beta", Kind::Pane),
        ];
        let m = search(&h, "");
        assert_eq!(m.len(), 2);
        assert!(m.iter().all(|r| r.indices.is_empty()));
    }

    #[test]
    fn narrows_to_matches() {
        let h = vec![
            leaf("a", "nvim", Kind::Pane),
            leaf("b", "cargo", Kind::Pane),
            leaf("c", "zsh", Kind::Pane),
        ];
        let m = search(&h, "nv");
        assert_eq!(m.len(), 1);
        assert_eq!(h[m[0].index].label, "nvim");
    }

    #[test]
    fn provider_bias_nudges_panes_above_plugins() {
        // Same label, different kinds → pane (+4) ranks above plugin (−2).
        let h = vec![
            leaf("p", "nvim", Kind::Pane),
            leaf("pl", "nvim", Kind::Plugin),
        ];
        let m = search(&h, "nvim");
        assert_eq!(h[m[0].index].kind, Kind::Pane);
        assert_eq!(h[m[1].index].kind, Kind::Plugin);
    }

    #[test]
    fn cursor_wraps() {
        let h = vec![leaf("a", "a", Kind::Pane), leaf("b", "b", Kind::Pane)];
        let mut v = view(&h, "a".into());
        // "a" matches both (subsequence), cursor on 0.
        assert!(!v.matches.is_empty());
        v.move_down();
        v.move_down();
        // wraps
        assert!(v.cursor < v.matches.len());
    }

    #[test]
    fn requery_resets_cursor() {
        let h = vec![
            leaf("a", "alpha", Kind::Pane),
            leaf("b", "beta", Kind::Pane),
        ];
        let mut v = view(&h, "al".into());
        v.cursor = 5; // out of range
        v.requery(&h);
        assert_eq!(v.cursor, 0);
    }
}
