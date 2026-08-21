# Planning: herdr-nav

A popup target switcher for the Herdr terminal multiplexer, per the
**Herdr Switcher Spec** (rev 1, 2026-08-21): one keystroke opens it, you
aim, Enter moves you, it closes. Two derived modes (Browse = tree,
Search = flat leaves), five target groups, a single-shape live preview,
Catppuccin Macchiato.

This is a new plugin (not a port). The spec is the normative design;
where the scaffold and this doc differ, the spec wins. Reference
prototype: `Herdr Switcher.dc.html` (behaviour normative where it and the
spec agree; the spec wins where they differ).

## 1. Purpose and scope

The switcher answers one question — *where do I want to be?* — for every
kind of place Herdr knows about: a live pane, an agent waiting on you, a
directory you visit often, a plugin you want to poke. It is a modal popup,
not a persistent panel.

Two hard constraints drive the design (spec §1):

1. A single keystroke changes interaction model: browsing structure and
   searching text are different tasks, so typing any printable character
   flips the left pane from a tree into a ranked flat list.
2. The preview pane never changes shape — it is always "what is the thing
   under the cursor", in both modes, for every node kind. That invariant
   is what makes the mode switch feel free.

**Non-goals:** not a session manager UI (no rename/resize/layout editing
beyond the destructive shortcuts in §8); not a file browser (dirs are
jump destinations, never path-by-path navigation); not a command palette.

## 2. Window, geometry, chrome (spec §2)

Floating, bordered, rounded rectangle centred over the dimmed host
terminal. Four horizontal bands:

| Band | Height | Content |
| --- | --- | --- |
| Title bar | 1 row | `herdr switch` left; mode badge + counts right |
| Search bar | 1 row | `❯` prompt, query, block caret, placeholder |
| Body | flex | list 44% · vertical rule · preview 56% |
| Footer | 1 row | mode-aware global keymap hints |

Target size 80% of the host terminal, clamped to 100×34 cells. Below 60
cols: drop the preview pane (toggle key). Below 20 rows: footer collapses
to a single `?` affordance. The list pane owns a one-row status strip at
its bottom: scope on the left (`tree · target groups` / `flat leaves ·
fuzzy`), cursor position on the right (`6/12`).

## 3. The two modes (spec §3)

Mode is **derived, never toggled**: `mode = if query.is_empty() { Browse } else { Search }`.

- Typing the first character enters Search; deleting the last character
  returns to Browse with the tree's expansion state exactly as it was.
- The cursor index resets to 0 on every query mutation (insert or delete).
- Expansion state (`expanded: Set<NodeId>`) is browse-only — untouched by
  searching.
- `Esc` is two-stage: Search → clears query (back to Browse); Browse →
  closes the popup.

### Browse (§3.1)
Single-selection tree. Rows indented 2 cells per depth, prefixed by a
twisty (▾ open, ▸ closed, blank for leaves) and a kind glyph. Both
branches and leaves are selectable; the cursor moves through the visible
flattening, never into a collapsed subtree.

### Search (§3.2)
The tree is replaced by the fully rolled-out set of **leaves only** —
every pane, agent, directory and plugin in one list, no branches, no
indentation. Each row shows its own path as a dimmed breadcrumb prefix so
a leaf is unambiguous without its tree: `herdr-dev › editor › nvim   src/switcher.rs`. The query fuzzy-filters and re-ranks (§6). The preview
behaves identically to Browse.

## 4. Target groups and the node model (spec §4)

Five groups at root, in fixed order — live/volatile first, stable after:

| # | Group | Shape | Leaf kind | Sort |
| --- | --- | --- | --- | --- |
| 1 | Session | 3-level tree: workspace / tab / pane | pane | session order, active workspace first |
| 2 | Agents | flat list | agent | waiting → working → idle, then recency |
| 3 | Pinned dirs | flat list | dir | config file order (slot ⌘1–⌘9) |
| 4 | zoxide | flat list | zox | frecency score, descending |
| 5 | Plugins | flat list | plugin | name, errors surfaced in group meta |

Groups collapsed by default except Session, which opens to its active
workspace and that workspace's active tab. Restoring a prior expansion set
across invocations is optional; if implemented, expire after 10 minutes.

### Node structure (§4.1)

```rust
enum Kind { Group, Workspace, Tab, Pane, Dir, Zox, Plugin, Agent }

struct Node {
    id: NodeId,       // stable, provider-scoped: "session:%2", "zox:/home/dd/dotfiles"
    kind: Kind,
    label: String,    // what the row shows: command, path, plugin name
    meta: String,     // right-aligned column: "%2", "902", "waiting", "v0.3.0"
    crumbs: Option<String>, // "herdr-dev › editor" — LEAVES ONLY, for search mode
    children: Vec<Node>,     // empty ⇒ leaf ⇒ appears in search mode
    preview: Preview,       // §7 — resolved lazily, cached per id
    actions: Actions,       // §8.2 — default + alternate
}
```

Two rules make the UI fall out for free: **leaf-ness is structural**
(`children.is_empty()`), so Search is one recursive walk with no
per-group special-casing; **crumbs are precomputed at tree-build time**,
so Search rows need no upward traversal during keystroke handling.

## 5. Providers (spec §5)

One provider per group, each producing a subtree and resolving previews
for its own nodes. Cheap to enumerate, lazy to preview.

```rust
trait Provider {
    fn id(&self) -> &'static str;                 // "session"|"agents"|"pinned"|"zoxide"|"plugins"
    fn enumerate(&self) -> Result<Node>;          // < 30ms, cached data OK
    fn preview(&self, id: &NodeId) -> Preview;    // may block ≤ 80ms; stale+spinner past that
    fn invoke(&self, id: &NodeId, act: Act) -> Result<Outcome>;
}
```

| Provider | Source | Refresh |
| --- | --- | --- |
| session | herdr daemon IPC: workspace/tab/pane graph, pane pids, cwd, last command, scrollback tail | on open + on daemon event |
| agents | agent-detect plugin (agent.start/agent.stop hooks), else process-tree heuristic | on open + on hook fire |
| pinned | `~/.config/herdr/targets.toml` | on file mtime change |
| zoxide | `zoxide query --list --score`, top 50, existing paths only | on open (cache 30s) |
| plugins | plugin registry: name, version, enabled, load error, declared actions | on open |

A provider that fails must not break the popup: its group row stays, its
meta becomes red "unavailable", and its preview shows the error text.
Agents and Session must reflect reality at the moment of opening — a jump
to a dead pane is the one failure users will not forgive.

## 6. Search: flattening, matching, ranking (spec §6)

### Haystack (§6.1)
Built **once per invocation** (not per keystroke): a `Vec<Leaf>` from a
depth-first walk of all five subtrees, in group order. Each leaf's match
text is `crumbs + " › " + label` — crumbs are searchable, which lets
`editor nvim` and `notes zsh` work as queries. Store the crumb prefix
length alongside so rendering knows which characters to dim.

### Matching (§6.2) — ranking contract decision

**Decision (2026-08-21):** use `nucleo-matcher`'s scoring verbatim,
reusing the `FuzzyEngine` shape from the sister `herdr-zextract` plugin
(see `herdr-zextract/src/picker/fuzzy.rs`): smart-case, `fuzzy_indices()`
for score + matched positions, `filter_with_bonus()` adding the §6.3
provider bias on top of nucleo's score. **Pin the `nucleo-matcher`
version in `Cargo.lock`** so a library upgrade can't silently reshuffle
rankings under users — the determinism the spec §6.2 demands is then
preserved by version-pinning rather than by a custom formula.

The spec's exact §6.2 formula (consecutive/gap/prefix/word-boundary
weights) is **demoted to advisory / optional**. It is kept in the doc as a
fallback we can switch to after first user tests if nucleo's tuning proves
unsuitable for this haystack (crumbs + labels across five groups). The
matcher interface (`filter_with_bonus` returning `ScoredMatch { index,
score, indices }`) is the contract the rest of the code depends on, so
swapping the scorer later is a localized change behind that interface.

The spec's *structural* search rules stay normative (these are not
library-dependent): haystack built once per invocation, DFS leaves only,
group order; cursor resets to 0 on every query mutation; `Backspace` to
empty restores the exact prior tree + expansion; two-stage `Esc`;
highlighting runs (peach+bold matched, surface2 crumb, subtext0/text
label). Matching is allocation-free per keystroke (reuse haystack +
scratch buffer) per the §12 budget.

```
score = Σ (8.0 if index == prev_index + 1 else 0.0)   // consecutive-run bonus
      − Σ (0.4 × gap_before_this_char)               // scattered matches are worse
      − 0.6 × first_match_index                       // prefix matches win
      + 4.0 if match starts at a word boundary        // after / › · space _ -
      + provider_bias                                 // §6.3
```

Sort descending by score; ties break by the haystack's original order
(= group order = live things before stored things).

### Provider bias (§6.3)
Flat additive constant, nudges not dominates: agents needing input +6,
live panes +4, pinned dirs +3, other agents +2, zoxide +0, plugins −2.

### Highlighting (§6.4)
Render as runs, not per character: coalesce adjacent chars that share a
colour. Matched chars are peach `#f5a97f` + bold (in both crumb prefix and
label). Unmatched crumb chars: surface2 `#5b6078`. Unmatched label chars:
subtext0 `#a5adcb`, or text `#cad3f5` on the selected row.

## 7. Preview pane (spec §7)

One shape for every kind, four stacked regions — the only place content
may be dense.

```rust
struct Preview {
    icon: Glyph, title: String,        // kind glyph + primary identity
    subtitle: String,                  // provenance: full path, or crumbs + pane id + detection source
    chips: Vec<Chip>,                  // 1–3 status pills, coloured by semantics
    body_label: &'static str,          // "PANE PREVIEW"|"DIRECTORY"|"AGENT TRANSCRIPT"|"PLUGIN"|"SUMMARY"
    body: Vec<Line>,                   // monospace, pre-wrapped, clipped — never scrollable
    action: String, alt: String,       // footer: default action name, alternate hint
}
```

| Kind | Body | Chips |
| --- | --- | --- |
| pane | last N lines of live scrollback, ANSI colour preserved; footer line = cwd + cursor position | focused/running, pid, cpu |
| workspace / tab | child inventory + ASCII split diagram for a tab's layout | active/detached, layout name |
| dir / zox | first ~8 entries (dirs first), then last-visit recency and hit count | git branch + dirty, entry count |
| agent | tail of agent transcript; if blocked, the pending question + options verbatim | status + duration, token count |
| plugin | one-line description, declared hooks, matched objects; load error with file+line if failing | enabled/error, version |
| group | aggregate roster of children + one line explaining the group's role | counts, error counts |

**Behaviour (§7.4):** read-only, never scrolls — clipped at the pane's
height. Resolution debounced 60ms after the cursor settles; while a slow
provider resolves, keep the previous preview visible and dim it rather
than flashing empty. On narrow terminals the pane is hidden and bound to
a toggle key; the list widens to full width.

## 8. Keymap and actions (spec §8)

| Key | Browse | Search |
| --- | --- | --- |
| `↓`/`^n` | next visible row (wraps) | next match (wraps) |
| `↑`/`^p` | previous visible row (wraps) | previous match (wraps) |
| `→`/`Space`/`Tab` | expand; if open, step to first child | `→`/`Tab` inert; `Space` types a space |
| `←` | collapse; if closed, jump to parent | inert |
| `Enter` | branch → toggle; leaf → default action, close | default action, close |
| `a–z 0–9 …` | enter search with that char | append, re-rank, cursor → 0 |
| `Backspace` | — | delete last char; empty → browse |
| `Esc` | close popup | clear query → browse |
| `^p` | pin selected dir (or selected pane's cwd); toast, stay open | |
| `^d` | kill selected pane/tab/workspace; confirm inline; stay open | |
| `^t` | on dir/zox: open the workspace-template picker (optional) | |
| `^r ^c ^x` | context alternates, named per item in the preview footer | |

Footer hints are mode-aware. The default action is always named in the
preview footer before you commit — Enter is never a guess. Executing it
closes the popup and toasts in the host terminal; non-destructive side
actions (pin) keep the popup open and toast in place.

### Default + alternate per kind (§8.2)

| Kind | `Enter` default | Alternate |
| --- | --- | --- |
| pane | jump to pane (switch workspace + tab + focus) | `^d` kill · `^r` restart command |
| agent | jump to the pane the agent runs in (same as a pane jump) | `^c` interrupt · `^x` detach |
| workspace / tab | switch to it, keeping its active pane | `^p` pin · `^d` kill |
| dir / zox | **always** open a new workspace at that path — a worktree-space inside a git repo, a plain workspace otherwise; never reuse the current one | `^t` open with template · `^p` pin |
| plugin | open the plugin's action picker (§8.3); "view error" if failed to load | — |

### Secondary selector — plugins (§8.3)
Enter on a plugin replaces the list pane with a small selector listing
every action that plugin declares, in declaration order, default
preselected. Same row grammar as the main list: `↑↓` move, `Enter` runs
the highlighted action and closes, `Esc` returns to the switcher with the
plugin still selected. The preview switches to the highlighted action's
description and, where provided, a dry-run summary.

### Optional: open-with-template (§8.4)
`^t` on a dir/zox entry opens the same selector shape listing configured
workspace templates (`~/.config/herdr/templates.toml`), one preselected
(the template whose `match` pattern fits the path, else the configured
default). Enter opens a new workspace/worktree-space at that path built
from the highlighted template; Esc returns. **Optional:** with no
`templates.toml`, `^t` is unbound and the footer omits the hint.

## 9. Theme — Catppuccin Macchiato (spec §9)

Palette is **fixed**; the mapping is the contract. Never introduce a
colour outside the palette, and never use colour as the only carrier of
meaning — every coloured state also has a word in the meta column or a
chip label.

| Role | Hex | Use |
| --- | --- | --- |
| base | `#24273a` | popup body |
| mantle | `#1e2030` | bars, preview bg |
| crust | `#181926` | dimmed host term |
| surface0 | `#363a4f` | selection, rules |
| text | `#cad3f5` | selected label |
| subtext0 | `#a5adcb` | unselected label |
| surface2 | `#5b6078` | crumbs, meta |
| mauve | `#c6a0f6` | prompt, caret, group |
| peach | `#f5a97f` | match hits, SEARCH badge |
| sapphire | `#7dc4e4` | BROWSE badge |
| green | `#a6da95` | agents, ok, action |
| red | `#ed8796` | errors, blocked |

Kind glyph + colour: workspace ◫ `#b7bdf8`, tab ▤ `#91d7e3`, pane ▪ `#8aadf4`, dir ▤ `#eed49f`, zoxide ▤ `#8bd5ca`, plugin ⬢ `#f5bde6`, agent ◆ `#a6da95`, group ❯ `#c6a0f6`. Dir and zoxide share a glyph, differ in colour.

Selected row: surface0 background, a 2px left bar in the row's kind
colour, label promoted to text. Nothing else marks selection — no bold,
no inverse, no arrow — so the eye tracks one moving band. Fall back to
reverse video only on terminals without truecolour.

## 10. Row rendering (spec §10)

One row = one terminal line: `[indent][twisty 1][glyph 1][gap][label — flexible, truncated][gap 2][meta — right-aligned]`. The label truncates
with an ellipsis and never wraps; the meta column never truncates and
keeps ≥ 2 cells of clearance from the label. When a truncated label would
hide a search hit, truncate from the **left of the crumb prefix** so the
hit stays visible.

## 11. Edge cases and empty states (spec §11)

- **No matches:** one dim centred line — `no targets match "qzx"` —
  counts read `0/20`, preview keeps the last resolved item dimmed to 50%,
  Enter inert.
- **Empty group:** stays visible with `empty` in meta; expanding shows one
  dim child row explaining how to populate it (e.g. "no pins — press `^p`
  on a directory").
- **Provider unavailable:** group meta red `unavailable`; its leaves are
  excluded from search rather than shown stale.
- **Target dies while open:** on Enter, if the provider reports the target
  gone, do not close — flash the row red, refresh that provider, keep the
  query.
- **Single match:** never auto-execute. Enter stays explicit.
- **Duplicate labels** (same command in two panes): disambiguated by crumbs
  in Search, by position in Browse; never dedupe.
- **Very long queries that match nothing:** keep accepting input; do not
  clear or beep.

## 12. Performance budgets (spec §12)

- Open → first paint: **< 60ms**. Paint chrome + tree from cached provider
  data; refresh in place as providers answer.
- Keystroke → re-ranked list: **< 8ms for 1,000 leaves**. Matching is
  allocation-free per keystroke; reuse the haystack and a scratch score
  buffer.
- Preview resolution: debounce 60ms, budget 80ms, then stale-and-dim.
  Never block the input loop on a provider.
- Redraw only the two rows whose selection changed, plus the preview region.

## 13. Configuration (spec §13)

`~/.config/herdr/switcher.toml` (the plugin reads it from
`$HERDR_PLUGIN_CONFIG_DIR`):

```toml
groups = ["session", "agents", "pinned", "zoxide", "plugins"]   # order = display order
open_key = "ctrl-k"
zoxide_limit = 50
preview = { enabled = true, width_pct = 56, min_cols = 60 }
expand = { session_default = "active", restore_ttl_secs = 600 }
scoring = { consecutive = 8.0, gap = 0.4, prefix = 0.6, word_boundary = 4.0 }
bias = { agent_waiting = 6, pane = 4, pinned = 3, agent = 2, zoxide = 0, plugin = -2 }
```

`~/.config/herdr/targets.toml` — pinned dirs:

```toml
[[pin]]
path = "~/code/herdr"
slot = 1
```

## 14. Acceptance criteria (spec §14)

- Open shows five groups in §4 order, Session pre-expanded to its active
  tab, cursor on row 0, preview showing the Session summary.
- Typing one printable char flips the badge to SEARCH, replaces the tree
  with leaves only, dims crumbs, highlights hits in peach, resets cursor
  to 0.
- Backspacing to empty restores the exact prior tree (incl. expansion);
  cursor returns to row 0.
- `→`, `Space`, `Tab` all expand the selected group in Browse; `Space`
  types a space in Search.
- `←` collapses an open node and jumps to the parent of a closed one.
- The preview updates for every cursor move in both modes; its footer
  always names the action Enter will perform.
- Enter on a leaf performs that action, closes, toasts; Enter on a branch
  in Browse only toggles it.
- Enter on a pane/agent lands the user in that pane. Enter on a dir/zox
  always creates a new workspace (worktree-space inside a git repo) and
  never reuses the current one.
- Enter on a plugin opens its action picker with the default preselected;
  Esc from the picker returns to the switcher with that plugin selected.
- Esc clears the query in Search and closes the popup in Browse.
- A killed pane, a failed plugin, and an unavailable provider are all
  representable without any modal dialog.
- All colours are Catppuccin Macchiato entries; every coloured status is
  also stated in words.

## 15. Open questions (spec §15)

- Merge Pinned dirs and zoxide into one "Directories" group with a
  provenance tag in meta? Fewer roots, but loses the pinned/frecency
  distinction at a glance.
- Should a group name typed as a prefix act as a scope filter (e.g.
  `agents ` restricting the haystack) rather than ordinary match text?
- `^k` is drawn in the search bar as the open keybind but has no in-popup
  function — document, repurpose, or drop the affordance.
- Should agents needing input raise a passive notification in the host
  status line while the popup is closed?

## 16. Architecture / repo layout

```
herdr-nav/
├── Cargo.toml
├── herdr-plugin.toml        # manifest: build, one pane, one action
├── justfile                 # build / check / link / open
├── CLAUDE.md                # gitflow + conventions
├── README.md
├── CHANGELOG.md
├── PLANNING.md              # this doc
├── config.example.toml      # switcher.toml schema
├── LICENSE
├── .gitignore
├── .github/workflows/{ci,release}.yml
├── src/
│   ├── main.rs              # entry, event loop, terminal setup, geometry
│   ├── socket_client.rs     # one-shot Unix-socket client (fresh conn per call)
│   ├── config.rs            # switcher.toml loading (§13)
│   ├── nav.rs               # Node/Kind/Group/Provider/Preview/Act/Outcome
│   ├── source.rs           # providers: enumerate/preview/invoke per group
│   ├── search.rs            # haystack build + subsequence match + score (§6)  [Phase 4]
│   ├── preview.rs           # per-kind preview rendering (§7)                 [Phase 2+]
│   └── render.rs            # bands, list rows, preview, footer, help (§2/§10)
├── doc/{config-reference,env-vars,keybinding,navigation,query-filters,use-cases}.md
├── spec/                        # normative spec + prototype (frozen, see spec/README.md)
└── tests/                   # fixtures + integration, per phase
```

`main.rs` owns the event loop, terminal setup/teardown, geometry, and
launch context. `source.rs` holds the providers behind a `match Group`
dispatch. `search.rs` owns the haystack + matcher + scorer. `preview.rs`
renders per-kind previews. `render.rs` draws the bands, list rows,
preview region, footer, and help overlay. `config.rs` loads
`switcher.toml`. `socket_client.rs` is the shared one-shot socket client
(fresh connection per request — Herdr's socket closes after one request;
reusing a connection yields `BrokenPipe`).

## 17. Implementation phases

**Recommended: 15 phases** (6 is split into 6a/6b; an advanced query
language phase is inserted before hardening — see below). Each phase is
a vertical, end-to-end testable slice that delivers one independent
user-visible improvement and focuses on a single aspect. One `phase/`
branch/PR per phase, in order. Each phase ends with: what to test, how to
trigger it, what works vs what's still a stub. Remove the crate-level
`#![allow(dead_code)]` in `main.rs` as each phase wires its modules into
the event loop.

The last three phases are hardening, then docs + public-facing, then
release/CI-CD — in that order, deliberately last.

### Phase 1 — Popup shell + Session tree browse
**Aspect:** popup geometry + tree navigation with one real provider.
The thinnest vertical slice: proves the whole spine end-to-end.
- Four bands (title/search/body/footer) at 80%×80%, clamped 100×34; list
  status strip (scope + position).
- `Node`/`Kind`/`Group`/`Provider` model; `SessionProvider` via herdr
  daemon IPC (workspace/tab/pane graph). Other four groups render as
  red "unavailable" stubs.
- Browse only: tree render with indent + twisty + glyph, `↑↓`/`^n`/`^p`
  cursor (wraps), Session pre-expanded to active tab, cursor on row 0.
  `Enter` inert on leaves, toggles branches. `Esc` closes. Query bar
  empty. Preview = placeholder.
- **Exit criteria:** `just link` + bind key → open → see Session tree,
  move cursor, expand/collapse, Esc closes. Other groups visibly pending.

### Phase 2 — Preview pane (session kinds)
**Aspect:** the single-shape preview (§7), for the kinds Phase 1 has.
- `Preview` four-region render (icon+title, subtitle, chips, body_label+
  body, action+alt). Debounced 60ms; stale-and-dim while resolving.
- Previews for: group (roster + role line), workspace/tab (child
  inventory + ASCII layout diagram), pane (last-N scrollback via
  `pane.read`, ANSI preserved, footer = cwd + cursor pos).
- Preview updates on every cursor move; footer names the (still-inert)
  default action.
- **Exit criteria:** cursor over Session nodes shows the right preview;
  pane preview shows real colored scrollback; slow resolve dims instead
  of flashing.

### Phase 3 — Pane jump (the first real switch)
**Aspect:** the switch action — `Provider::invoke` + `Outcome` + close +
host toast. The product's core verb.
- `Act::Default` on a pane → `SessionProvider::invoke` switches workspace
  + tab + focuses the pane; `Outcome::Close { toast }` closes the popup
  and toasts in the host terminal.
- "Target dies while open": if the provider reports the target gone on
  Enter, do not close — flash the row red, refresh, keep the query.
- Workspace/tab `Enter` (switch to it, keep active pane) also wired here
  since it's the same invoke path.
- **Exit criteria:** Enter on a pane lands you in it and toasts; Enter on
  a workspace/tab switches to it. Dead-target case flashes + stays open.

### Phase 4 — Search mode (fuzzy + ranking + highlight)
**Aspect:** the derived Search mode (§3.2/§6) — the second core capability.
- `search.rs`: haystack built once per invocation (DFS, leaves only, group
  order); match text = `crumbs + " › " + label`.
- Reuse the `FuzzyEngine` shape from `herdr-zextract/src/picker/fuzzy.rs`
  (`nucleo-matcher`, smart-case, `fuzzy_indices`, `filter_with_bonus`).
  **Pin `nucleo-matcher` in `Cargo.lock`.** Add the §6.3 provider bias via
  `filter_with_bonus`. The spec's §6.2 formula stays advisory (see §6
  decision above) — nucleo's score is the v0.1 contract; switch later if
  user tests want it.
- Mode flip on first printable char; cursor resets to 0 on every mutation;
  `Backspace` to empty restores the exact prior tree + expansion; two-stage
  `Esc`.
- Highlight coalesced into runs (peach+bold matched, surface2 crumb,
  subtext0/text label). Badge flips to SEARCH; counts → matches/total.
- Allocation-free per keystroke (reuse haystack + scratch buffer).
- **Exit criteria:** typing narrows + re-ranks live; ranking is sensible
  and deterministic (pinned nucleo); backspace-to-empty restores the tree
  exactly.

### Phase 5 — Agents provider + agent jump
**Aspect:** the Agents group (the second live/volatile group).
- `AgentsProvider`: agent-detect plugin hooks (agent.start/stop), else
  process-tree heuristic. Flat list, sort waiting → working → idle then
  recency. Meta = status.
- Preview: agent transcript tail; if blocked, the pending question + its
  options verbatim. Chips: status+duration, token count.
- Action: `Enter` jumps to the pane the agent runs in (same invoke path
  as a pane jump). Group row drops its "unavailable" stub.
- Agents enter the search haystack for free (built from providers).
- **Exit criteria:** agents list with live status; preview shows transcript
  / blocked question; Enter lands in the agent's pane; `agents …` queries
  work in Search.

### Phase 6a — Pinned + zoxide providers + previews + "open new workspace"
**Aspect:** the two directory groups (§4) and the basic dir/zox action (§8.2).
- `PinnedProvider` (`targets.toml`, mtime refresh, slot order) +
  `ZoxideProvider` (`zoxide query --list --score`, top 50, existing only,
  30s cache). Meta: slot / frecency score.
- Preview (dir/zox): first ~8 entries (dirs first), last-visit recency +
  hit count; chips: git branch + dirty, entry count.
- Action: `Enter` **always** opens a new workspace at the path — a
  worktree-space if the path is inside a git repo, a plain workspace
  otherwise; never reuses the current workspace. No template picker yet.
- Both groups drop their "unavailable" stubs and enter the haystack.
- **Exit criteria:** pins + zoxide entries list with git/dir previews;
  Enter opens a fresh workspace/worktree-space at the path; `dir …` /
  `zox …` queries work in Search.

### Phase 6b — Templates (open-with-template picker, §8.4)
**Aspect:** the `^t` open-with-template feature for dirs and zoxide.
- `templates.toml` parsing (tmuxinator-style: `name`, `match` globs,
  `default`, `tabs[]` with `name`/`panes`/`split`/`ratio`).
- `^t` on a dir/zox entry opens the secondary-selector shape listing
  configured templates, one preselected: the template whose `match`
  pattern fits the path, else the configured `default`.
- `Enter` opens a new workspace (or worktree-space) at that path built
  from the highlighted template; `Esc` returns to the switcher.
- With no `templates.toml`: `^t` is unbound and the preview footer omits
  the hint (spec §8.4). Plain `Enter` never consults templates beyond the
  default one.
- **Exit criteria:** `^t` on a dir/zox opens the template picker with the
  right one preselected; Enter builds the workspace from it; Esc returns;
  with no templates.toml the key is inert and the hint is gone.

### Phase 7 — Plugins provider + plugin action picker
**Aspect:** the Plugins group + the secondary selector (§8.3).
- `PluginsProvider` (registry: name, version, enabled, load error,
  declared actions). Meta: version or error. Failed plugin → red meta,
  preview shows error + file:line, default action "view error".
- Preview (plugin): one-line description, declared hooks, matched
  objects; load error with file+line if failing. Chips: enabled/error,
  version.
- Secondary selector: `Enter` on a plugin replaces the list with the
  plugin's declared actions (declaration order, default preselected);
  `↑↓` move, `Enter` runs the highlighted action + closes, `Esc` returns
  with the plugin still selected. Preview switches to the action's
  description + dry-run summary where provided. A plugin declaring no
  actions is not selectable.
- **Exit criteria:** plugins list with errors surfaced; Enter opens the
  action picker; running an action closes + toasts; Esc returns.

### Phase 8 — Side actions (pin, kill, context alternates)
**Aspect:** the non-primary keymap (§8) — everything except `Enter`.
- `^p` pin: pin the selected dir (or the selected pane's cwd) into Pinned
  dirs; writes `targets.toml`; toast, stay open.
- `^d` kill: kill selected pane/tab/workspace; inline confirm in the
  footer; stay open.
- `^r ^c ^x` context alternates, named per item in the preview footer
  (restart command / interrupt agent / detach agent).
  (`^t` open-with-template lives in Phase 6b — it's a dir/zox feature,
  not a generic side action.)
- **Exit criteria:** pin writes the file + toasts; kill confirms + kills;
  alternates run the right per-kind action; popup stays open for side
  actions.

### Phase 9 — Visual contract: theme + row rendering
**Aspect:** render exactly to spec §9/§10 across all kinds/modes.
- Catppuccin Macchiato palette applied everywhere; kind glyphs + colours;
  selection = surface0 bg + 2px kind-colour left bar + label→text (no
  bold/inverse/arrow); reverse-video fallback only without truecolour.
- Row grammar (§10): indent + twisty + glyph + gap + label (ellipsis
  truncation) + gap + meta (right-aligned, ≥2 cell clearance). Left-truncate
  the crumb prefix when a truncation would hide a search hit.
- Highlight run coalescing (§6.4) finalized. Mode-aware footer hints.
  Narrow-terminal rules: drop preview < 60 cols (toggle key), footer → `?`
  < 20 rows.
- **Exit criteria:** a side-by-side check against the prototype figures
  (Figs 1–7) passes; no colour outside the palette; every coloured state
  also stated in words.

### Phase 10 — Configuration
**Aspect:** make the built-in defaults overridable (§13).
- `switcher.toml` schema wired: `groups` order, `zoxide_limit`,
  `[preview]`, `[expand]`, `[scoring]`, `[bias]`. Earlier phases already
  use the built-in defaults; this phase makes them overridable.
- `targets.toml` read by `PinnedProvider` (already written by `^p` in
  Phase 8 — close the loop).
- Missing/malformed config → stderr report + built-in defaults, no crash.
- **Exit criteria:** editing `switcher.toml` changes group order /
  scoring / preview width live; `targets.toml` edits reflect on reopen.

### Phase 11 — Query filters (group scope + kind + negation)
**Aspect:** an advanced query language layered on the Phase 4 fuzzy scorer —
resolve spec §15 open question #2 and the pane-vs-dir-vs-plugin ambiguity.
**Search-mode only** (typing any filter token implicitly enters Search,
as plain text does today; Browse is structure, filters are a search
concept — mixing them would muddy the §1 "two tasks, two modes" invariant).

A small parser runs **before** the existing nucleo scorer: it splits the
query into filter tokens and a fuzzy needle, filters the haystack, then
hands the needle to the same `FuzzyEngine` from Phase 4. No second matcher,
no scoring-model change — the ranking contract is unchanged.

**Features (scope A):**

1. **Group scope prefix** (spec §15 #2): a leading token ending in a
   space that matches a group name restricts the haystack to that group.
   `agents nvim` → only Agents leaves, then fuzzy `nvim`. Groups:
   `session`, `agents`, `pinned`, `zoxide`, `plugins`.
2. **Kind filter** — `kind:X` anywhere in the query restricts to leaves
   of that `Kind`. Kinds: `pane`, `agent`, `dir`, `zox`, `plugin`,
   `tab`, `workspace`.
   - **`@` shortcut** for the kinds users reach for most: `@pane`,
     `@agent`, `@dir`, `@plugin` are exactly `kind:pane` etc. The
     interior Session kinds (`@tab`, `@workspace`) also get `@` for
     symmetry. `@` is just sugar; `kind:` is the canonical form.
   - **`dir` is a union alias**: `kind:dir` / `@dir` matches **both**
     `Kind::Dir` (pinned) and `Kind::Zox` (zoxide), because users think
     of them as "directories" and rarely care about provenance when
     filtering. `kind:zox` / `@zox` targets zoxide only; `kind:pinned`
     is **not** a kind (pinned is a group, not a kind) — use the
     `pinned` group scope for that.
3. **Negation** `!X` excludes a kind (or, for `!agents`/`!plugins` etc.,
   a group). `!plugin` → everything except plugins; `!zox` → everything
     except zoxide entries. Cheap once kind/group filters exist (same
   filter-combine step).

**Composition semantics (decided 2026-08-21):**

The parser splits the raw query into filter tokens + a fuzzy needle.
Filters are set operations applied in this order:

```text
result =
    initial_group_scope          ← one allowed; restricts starting set
    ∩ union(positive_kind_filters)  ← multiple @/@kind: are OR (a node has one Kind)
    − union(negative_filters)       ← !kind or !group subtracts from the result
    |› nucleo(fuzzy_needle)          ← remaining plain text
```

Rules:
- Only **one** positive group scope is allowed (`session nvim`). A second
  group-scope token is treated as fuzzy text, not a filter.
- Multiple positive kind filters are **OR**: `@pane @dir` = panes OR
  directory-like entries (AND would always be empty since a node has
  exactly one Kind).
- Group scope **intersects** with the kind union: `session @pane nvim` =
  pane leaves within Session, fuzzy-matched with `nvim`.
- Negations **subtract afterward**: `@dir !zox` = directory-like entries
  minus zoxide = pinned dirs only.
- Contradictory filters (e.g. `agents @pane`) produce the normal no-match
  state (dim centred line, `0/N`, Enter inert) rather than a syntax error.
- Repeated / redundant filters are deduplicated silently.
- Unrecognised `@` or `kind:` tokens (e.g. `@pnae`) are treated as
  ordinary fuzzy text, not errors — the user can keep typing.
- If there are no positive kind filters, the kind intersection step is
  skipped (all kinds pass).
- If there is no group scope, the initial set is all leaves.
- An empty needle after filters (e.g. just `@pane`): show all matching
  leaves, ranked by haystack order (the empty-query passthrough from
  `FuzzyEngine` already does this).

Examples:

| Query | Meaning |
| --- | --- |
| `nvim` | plain fuzzy across all leaves |
| `agents nvim` | group scope Agents, fuzzy `nvim` |
| `@pane` | all pane leaves, no fuzzy filter |
| `@pane nvim` | pane leaves fuzzy-matched with `nvim` |
| `@pane @dir` | panes OR directory-like entries (pinned+zoxide) |
| `session @pane nvim` | panes within Session only, fuzzy `nvim` |
| `@dir !zox` | directory-like minus zoxide = pinned dirs only |
| `!plugin !zox nvim` | everything except plugins and zoxide, fuzzy `nvim` |
| `agents @pane` | contradiction → no matches (Agents has no Pane leaves) |
| `@pnae nvim` | `@pnae` unrecognised → treated as fuzzy text; needle = `@pnae nvim` |

**Deferred to v0.2 (pending user tests):** quoted exact (`"src/switcher"`),
space-separated AND-tokens. Both add a second matcher path / scoring-model
change and a UX affordance (how does `"` echo?); ship the cheap high-pain
trio first.

**Cut from v0.1:** field targets (`path:herdr name:nvim`) — drifts toward
the command palette §1 disclaims as a non-goal.

**Design points to resolve in-phase:**
- **Group-vs-kind naming overlap.** `agents` (group) ≈ `agent` (kind);
   `plugins` (group) ≈ `plugin` (kind); `pinned`/`zoxide` (groups) map
   to `dir`/`zox` kinds. **Disambiguation rule:** group scope is a
   **leading-prefix token only** (must appear first); `kind:`/`@`/`!` are
   **position-independent tokens**. So `agents nvim` = group scope
   `agents` + needle `nvim`; `nvim @agent` = needle `nvim` + kind
   `agent`. The singular/plural difference (`agent` vs `agents`) is the
   user-visible cue that one is a kind and the other is a group;
   document it in `?` and in `doc/query-filters.md`.
- **Status strip** shows the active filters: e.g. `agents · @pane`.
- **Filter tokens in the search bar** stay visible as typed (they are not
  consumed into chips). The status strip is the confirmation.

**Deliverables:**
- `doc/query-filters.md` — the user-facing query syntax doc: operators,
  aliases, composition rules, worked examples, edge cases (unrecognised
  tokens, contradictions, empty needle). Referenced from `?` overlay,
  `doc/keybinding.md`, and `doc/navigation.md`.
- Unit tests: recognised tokens, unrecognised tokens, composition,
  contradictions, empty needle, dedup, negation subtract, union kind,
  group∩kind.

**Exit criteria:** `agents nvim` restricts to agents; `@pane` shows panes
only; `@dir` shows pinned+zoxide; `!plugin` excludes plugins; filters
compose per the rules above; contradictions show no-match, not errors;
unrecognised `@` tokens fall through to fuzzy; `doc/query-filters.md`
exists with examples; `?` overlay references it; plain fuzzy with no
filter tokens behaves exactly as Phase 4.

### Phase 12 — Edge cases + performance budgets
**Aspect:** robustness and the §11/§12 contracts.
- Empty states: no matches (dim centred line, `0/20`, preview dims to
  50%, Enter inert); empty group (`empty` meta + populate-hint child);
  provider unavailable (red meta, leaves excluded from search); single
  match never auto-executes; duplicate labels disambiguated by crumbs/
  position; very long no-match queries keep accepting input.
- Performance: open→first-paint < 60ms; keystroke→re-ranked < 8ms for
  1,000 leaves (allocation-free matching, scratch buffer); preview
  debounce 60ms / budget 80ms / stale-and-dim; redraw only changed rows
  + preview region. Query-filter parse must stay within the same
  per-keystroke budget (parse is O(query length), negligible).
- **Exit criteria:** all §11 states render correctly; §12 budgets met on
  a 1,000-leaf fixture; filter queries meet the budget too.

### Phase 13 — Docs refinement + public-facing elements
**Aspect:** user-facing documentation and repo surface.
- Finalize `doc/{navigation,keybinding,config-reference,env-vars,use-cases}.md`
  against the shipped behavior; `README.md` (what it does, install, build,
  platform support); `CHANGELOG.md` for `0.1.0`; `config.example.toml`
  matches the schema.
- `?` in-popup help dialog finalized, including the query-filter syntax
  from Phase 11.
- **Exit criteria:** docs match the binary; README install instructions
  verified; CHANGELOG covers the release.

### Phase 14 — Release / CI-CD
**Aspect:** shipping v0.1.0.
- Confirm `ci.yml` (fmt/clippy/test on macOS + Linux) and `release.yml`
  (tag-triggered, 3 target triples, SHA-256, rolling `latest`) are green.
- Release PR (`release/0.1.0`): `Cargo.toml` version bump + `CHANGELOG.md`.
- Tag `v0.1.0` → `release.yml` publishes binaries for
  `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu`.
- **Exit criteria:** tagged release ships for all three targets; rolling
  `latest` moves to it.

---

### Why 13, and how to adjust

- **Vertical slices:** Phase 1 proves the spine (geometry + node model +
  provider + tree render + event loop) with one real provider; each later
  phase adds exactly one user-visible capability end-to-end. After Phase 3
  you have a working *pane switcher*; after Phase 4 a working *fuzzy
  switcher*; each provider phase (5/6/7) adds a whole group you can
  actually jump to.
- **One aspect each:** browse (1), preview (2), the switch verb (3),
  search (4), then one phase per remaining group (5/6/7), the secondary
  keymap (8), visual conformance (9), config (10), hardening (11), docs
  (12), release (13).
- **Not too large:** the riskiest phase was 6 (Pinned+zoxide + the
  "always new workspace / worktree-space" action + templates); it's now
  split into 6a (providers + previews + the basic open-new-workspace
  action) and 6b (templates). If 6a still grows, split providers (6a)
  from the workspace-creation action (6a′). Phase 8 is the other
  candidate to split if `^d` kill-confirm grows.
- **Fewer phases?** 9 (visual) and 11 (edge+perf) could merge into one
  "hardening" phase if you prefer ~13, at the cost of a larger phase.
  Phases 3 and 8 are small but architecturally load-bearing (the invoke
  contract; the side-keymap) — folding them in would bloat their
  neighbours.
- **More phases?** The advanced query-language phase (grilling in
  progress) is the one addition already anticipated.

## 18. Ideas beyond v0.1

- Merge Pinned + zoxide into one "Directories" group (open question §15).
- Group-name prefix as a scope filter in Search.
- Passive host-status-line notification for agents needing input.
- MRU ordering (most-recently-focused pane first when query empty).
- Cross-workspace navigation if Herdr exposes multiple workspaces.

## License

MIT.
