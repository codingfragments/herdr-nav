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

use crate::nav::{Group, Kind, Node, Tree};
use crate::query;

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
    /// Which root group this leaf belongs to (for `!group` filters).
    pub group: Group,
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
        // Resolve the root group from its id ("group:session" → Session).
        let group = Group::from_node_id(&node.id);
        walk(node, &[i], "", group, &mut out);
    }
    out
}

fn walk(node: &Node, path: &[usize], parent_crumbs: &str, group: Group, out: &mut Vec<Leaf>) {
    if node.is_leaf() {
        // Exclude populate-hint children (spec §11) from the haystack —
        // they're not real targets.
        if node.id.ends_with(":hint") {
            return;
        }
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
            group,
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
        walk(child, &child_path, &crumb, group, out);
    }
}

// ── Provider bias (spec §6.3) ───────────────────────────────────────────────

/// Flat additive bias so live things nudge ahead of stored things
/// (spec §6.3): agents needing input +6, live panes +4, pinned dirs
/// +3, other agents +2, zoxide +0, plugins −2. In Phase 4 only Session
/// (panes) is live; other kinds get their bias when their providers
/// land. `unavailable` groups contribute no leaves to the haystack.
///
/// Phase 10: the bias values are now configurable via `switcher.toml`
/// `[bias]`; `cfg` overrides the spec defaults.
pub fn provider_bias(leaf: &Leaf, cfg: &crate::config::BiasCfg) -> i32 {
    match leaf.kind {
        Kind::Pane => cfg.pane as i32,
        Kind::Agent => cfg.agent as i32, // +agent_waiting when detectable (Phase 5)
        Kind::Dir => cfg.pinned as i32,
        Kind::Zox => cfg.zoxide as i32,
        Kind::Plugin => cfg.plugin as i32,
        _ => 0,
    }
}

// ── Search view state ───────────────────────────────────────────────────────

/// The search view's mutable state. `None` in the event loop = browse
/// mode; `Some` = search mode (query non-empty).
#[derive(Debug, Clone)]
pub struct SearchView {
    pub query: String,
    /// The parsed query filters (Phase 11).
    pub parsed: query::ParsedQuery,
    /// Ranked matches, indices into the haystack.
    pub matches: Vec<ScoredMatch>,
    /// Cursor into `matches`.
    pub cursor: usize,
}

/// Phase 16 "extend zoxide" condition: true when the search result
/// list contains no directory entries (`Dir` or `Zox`). This is the
/// gate for showing the `Tab extend` hint and for the `Tab` keybind
/// itself — extending zoxide can only ever add directory leaves, so
/// it's only useful when none are present.
pub fn has_no_dir_matches(haystack: &[Leaf], matches: &[ScoredMatch]) -> bool {
    matches.iter().all(|m| {
        haystack
            .get(m.index)
            .is_some_and(|l| l.kind != Kind::Dir && l.kind != Kind::Zox)
    })
}

/// Run the fuzzy search against the haystack, returning ranked matches.
/// `bias_cfg` overrides the spec §6.3 defaults (Phase 10).
///
/// Phase 11: the query is first parsed for filters (group scope,
/// `kind:`/`@`, `!` negation). The haystack is filtered to the
/// matching indices, then nucleo runs only on those. No second
/// matcher, no scoring-model change.
pub fn search(
    haystack: &[Leaf],
    raw_query: &str,
    bias_cfg: &crate::config::BiasCfg,
) -> Vec<ScoredMatch> {
    let parsed = query::ParsedQuery::parse(raw_query);
    let filtered = parsed.filter_haystack(haystack);

    let mut engine = FuzzyEngine::new();
    let items: Vec<String> = filtered
        .iter()
        .map(|&i| haystack[i].match_text.clone())
        .collect();
    let bonus_fn = |j: usize| provider_bias(&haystack[filtered[j]], bias_cfg);
    let scored = engine.filter_with_bonus(&parsed.needle, &items, bonus_fn);

    // Remap from filtered-index back to haystack-index.
    scored
        .into_iter()
        .map(|m| ScoredMatch {
            index: filtered[m.index],
            score: m.score,
            indices: m.indices,
        })
        .collect()
}

/// Build a fresh `SearchView` from a query.
pub fn view(haystack: &[Leaf], query: String, bias_cfg: &crate::config::BiasCfg) -> SearchView {
    let parsed = query::ParsedQuery::parse(&query);
    let matches = search(haystack, &query, bias_cfg);
    SearchView {
        query,
        parsed,
        matches,
        cursor: 0,
    }
}

impl SearchView {
    /// Re-run the search after a query mutation; reset cursor to 0
    /// (spec §3: "the cursor index resets to 0 on every query mutation").
    pub fn requery(&mut self, haystack: &[Leaf], bias_cfg: &crate::config::BiasCfg) {
        self.parsed = query::ParsedQuery::parse(&self.query);
        self.matches = search(haystack, &self.query, bias_cfg);
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
            group: Group::Session,
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
        let m = search(&h, "", &crate::config::BiasCfg::default());
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
        let m = search(&h, "nv", &crate::config::BiasCfg::default());
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
        let m = search(&h, "nvim", &crate::config::BiasCfg::default());
        assert_eq!(h[m[0].index].kind, Kind::Pane);
        assert_eq!(h[m[1].index].kind, Kind::Plugin);
    }

    #[test]
    fn cursor_wraps() {
        let h = vec![leaf("a", "a", Kind::Pane), leaf("b", "b", Kind::Pane)];
        let mut v = view(&h, "a".into(), &crate::config::BiasCfg::default());
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
        let mut v = view(&h, "al".into(), &crate::config::BiasCfg::default());
        v.cursor = 5; // out of range
        v.requery(&h, &crate::config::BiasCfg::default());
        assert_eq!(v.cursor, 0);
    }

    #[test]
    fn has_no_dir_matches_true_when_only_panes() {
        let h = vec![leaf("a", "nvim", Kind::Pane)];
        let m = search(&h, "nvim", &crate::config::BiasCfg::default());
        assert!(has_no_dir_matches(&h, &m));
    }

    #[test]
    fn has_no_dir_matches_false_when_a_dir_matches() {
        let h = vec![
            leaf("a", "nvim", Kind::Pane),
            Leaf {
                path: vec![0],
                kind: Kind::Zox,
                group: Group::Zoxide,
                id: "zox:/x".into(),
                label: "nvim".into(),
                meta: String::new(),
                crumbs: String::new(),
                crumb_prefix_len: 0,
                match_text: "nvim".into(),
            },
        ];
        let m = search(&h, "nvim", &crate::config::BiasCfg::default());
        assert!(!has_no_dir_matches(&h, &m));
    }

    #[test]
    fn has_no_dir_matches_true_on_empty_results() {
        let h = vec![leaf("a", "nvim", Kind::Pane)];
        let m = search(&h, "zzz", &crate::config::BiasCfg::default());
        assert!(m.is_empty());
        assert!(has_no_dir_matches(&h, &m));
    }

    /// Performance budget (spec §12): keystroke → re-ranked < 8ms
    /// for 1,000 leaves. This is a smoke test, not a hard CI gate —
    /// it measures the actual time and asserts it stays under 50ms
    /// (a generous margin over the 8ms target to avoid CI flakiness on
    /// slow runners; the real budget is 8ms).
    #[test]
    fn perf_1000_leaves_under_budget() {
        let h: Vec<Leaf> = (0..1000)
            .map(|i| Leaf {
                path: vec![0],
                kind: if i % 2 == 0 { Kind::Pane } else { Kind::Agent },
                group: Group::Session,
                id: format!("id{i}"),
                label: format!("item-{i}-nvim-cargo"),
                meta: String::new(),
                crumbs: String::new(),
                crumb_prefix_len: 0,
                match_text: format!("item-{i}-nvim-cargo"),
            })
            .collect();

        let start = std::time::Instant::now();
        let m = search(&h, "nvim", &crate::config::BiasCfg::default());
        let elapsed = start.elapsed();

        assert!(!m.is_empty(), "should have matches");
        assert!(
            elapsed.as_millis() < 50,
            "search of 1000 leaves took {:?} (budget: <8ms target, <50ms gate)",
            elapsed
        );
    }

    /// Query-filter parse must be negligible (spec §12).
    #[test]
    fn perf_filter_parse_negligible() {
        let queries = [
            "@pane nvim",
            "session @pane @dir !plugin !zox nvim",
            "!plugin !zox !agents !session @workspace @tab",
        ];
        for q in &queries {
            let start = std::time::Instant::now();
            for _ in 0..1000 {
                let _ = crate::query::ParsedQuery::parse(q);
            }
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_millis() < 100,
                "1000 parses of '{q}' took {:?}",
                elapsed
            );
        }
    }
}
