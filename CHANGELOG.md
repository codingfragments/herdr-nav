# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Phase 7 — Plugins provider + plugin action picker

- `PluginsProvider` via `plugin.list` socket method (confirmed
    live): flat list of plugin leaves. Meta = version;
    disabled → red "disabled"; no actions → "no actions"
    (not selectable).
- Plugin preview: one-line description, declared actions list,
    chips: enabled/version. Footer: "open actions".
- Secondary selector (spec §8.3): `Enter` on a plugin opens
    a centered action-picker dialog listing its declared actions
    (declaration order, default preselected). `↑↓` move,
    `Enter` runs the highlighted action via `plugin.action.invoke` +
    closes; `Esc` returns to the switcher with the plugin still
    selected. A plugin with no actions is inert.
- Plugins group drops its "unavailable" stub and enters the
    search haystack.
- 3 new unit tests (flat plugin list, disabled red meta,
    no-actions not selectable). 51 tests total.

### Phase 6b — Templates (open-with-template picker, §8.4)

- **Format change:** templates are now one YAML file per template in
    `~/.config/herdr/templates/` (was a single `templates.toml`).
    Recursive multi-level split layout, `cwd` at tab and pane level.
    Added `serde-yaml` dependency.
- `templates/*.yaml` parsing (one file per template; filename stem =
    default name). Recursive `Layout`/`PaneNode` (leaf =
    `command:`, branch = `layout:`).
- `build_workspace_from_template` rewritten for the recursive
    layout (depth-first split + send).
- `^t` on a dir/zox opens a centered template-picker dialog listing
    configured templates, one preselected: the template whose
    `match` glob fits the path, else the configured `default`.
- `Enter` builds a new workspace at that path from the highlighted
    template, then focuses its first pane. `Esc` returns
    to the switcher.
- With no `templates/` dir: `^t` is unbound (inert).
- Empty `command` = plain login shell (no nested shell).
- `cwd` passed to `pane.split`/`tab.create` natively (no `cd`).
- 5 new unit tests (YAML parse, nested layout, per-pane cwd, glob
    preselect). 45 tests total.
- Docs: `doc/query-filters.md` → Templates section with the full schema + examples.
- Spec §8.4 amended (YAML, recursive layout, cwd).

### Phase 6a — Pinned + zoxide providers + "open new workspace"

- `PinnedProvider` reads `~/.config/herdr/targets.toml` (slot order,
    mtime refresh). `ZoxideProvider` runs `zoxide query --list --score`,
    top 50, existing paths only. Both flat lists; meta = slot / frecency.
- Dir/zox preview: first ~8 entries (dirs first), git branch + dirty
    chip, entry-count chip. Footer: "open workspace".
- `Enter` on a dir/zox **always** opens a new workspace at the path —
    a worktree-space if inside a git repo (`worktree.create`, fall back
    to `workspace.create`), a plain workspace otherwise. Never
    reuses the current workspace.
- Both groups drop their "unavailable" stubs and enter the
    search haystack.
- 4 new unit tests (pinned empty, zoxide parse, expand path, prefix strip).
    Fixed zoxide score parse (leading spaces). 39 tests total.

### Phase 5 — Agents provider + agent jump

- `AgentsProvider` via `agent.list` socket method (confirmed
    live): flat list of agent leaves with status (waiting →
    working → idle, then recency). Meta = status. Label prefers
    `terminal_title_stripped`.
- Agent preview: transcript tail via `pane.read` (the agent runs
    in a pane), chips = status (blocked/waiting red, working green).
    Footer: "jump to agent pane".
- `Enter` on an agent jumps to its pane (same `pane.focus` invoke
    path as Phase 3). Agents group drops its "unavailable" stub.
- Agents enter the search haystack for free (built from providers).
- 2 new unit tests (flat agent list + sort, agent leaf id). 35 total.

### Phase 4 — Search mode (fuzzy + ranking + highlight)

- Derived mode: `mode = if query.is_empty() { Browse } else { Search }`.
    Typing any printable char flips to Search; `Backspace` to empty
    restores Browse (expansion state untouched).
- `search.rs`: haystack built once per invocation (DFS, leaves
    only, group order); match text = `crumbs + " › " + label`.
    Reuses the `FuzzyEngine` shape from herdr-zextract (nucleo-matcher,
    smart-case, `fuzzy_indices`, `filter_with_bonus`). Provider
    bias (§6.3) via `filter_with_bonus`.
- Two-stage `Esc`: Search → clear query (Browse); Browse → close.
    Cursor resets to 0 on every query mutation.
- Search list: flat leaves with dimmed breadcrumb prefix, matched
    chars peach+bold (coalesced into runs), label subtext0/text.
    Status strip: `flat leaves · fuzzy` + `matches/total`.
- `Enter` in Search invokes the action on the cursor leaf; preview
    follows the search cursor.
- 6 new unit tests (haystack walk, empty query, narrow, bias,
    cursor wrap, requery reset). 31 tests total.

### Phase 3 — Pane jump (the first real switch)

- `Enter` is now the main action verb: on a pane leaf,
    `SessionProvider::invoke` calls `pane.focus` (one socket call
    switches workspace + tab + focuses the pane), the popup closes,
    and a one-line toast shows in the host terminal via
    `notification.show` ("jumped to pane <id>").
- On a workspace/tab branch, `Enter` steps into it (focus the first
    pane under it on a second Enter). `Enter` no longer toggles
    branches — expand/collapse is `→`/`←`/`Space`/`Tab` only (spec
    §8 amended: `Enter` is reserved for the action on every row).
- Dead-target / provider error: the row flashes red, the popup
    stays open, the query is kept (spec §11).
- 2 new unit tests (first_pane_under, node_for). 25 tests total.

### Phase 2 — Preview pane (session kinds)

- `Preview` four-region render (icon+title, subtitle, chips,
    body_label+body, action+alt) per spec §7. Resolution debounced
    60ms; stale-and-dim while resolving (spec §7.4).
- Previews for the Session kinds: group (roster + role line),
    workspace/tab (child inventory, active marked), pane (last-N
    scrollback via `pane.read`, ANSI colour preserved via
    `ansi-to-tui`). Footer names the (still-inert) default action.
- Preview updates on every cursor move; the cursor-change instant
    drives the debounce in the event loop.
- 5 new unit tests (pane id strip, group roster, unavailable chip,
    workspace active mark, ANSI plain fallback). 19 tests total.

### Phase 1 — Popup shell + Session tree browse

- Four bands (title/search/body/footer) at 80%×80%, clamped to 100×34;
    list status strip (scope + position). Body split list 44% / preview
    56%; preview dropped below 60 cols (spec §2).
- `Node`/`Kind`/`Group`/`Provider` model (spec §4/§5); `Tree` state
    with visible-row flattening, cursor (wraps), expand/collapse/step,
    toggle, Session pre-expanded to its active workspace + tab.
- `SessionProvider` via herdr daemon IPC: reconstructs the
    workspace → tab → pane tree from the flat `pane.list` response
    (grouping on `workspace_id`/`tab_id`), active workspace/tab first
    (from `HERDR_PLUGIN_CONTEXT_JSON`). Other four groups render as
    red "unavailable" stubs (spec §11).
- Browse only: tree render with indent + twisty + kind glyph, `↑↓`/`^n`/`^p`
    cursor, `→`/`Space`/`Tab` expand-or-step, `←` collapse-or-parent,
    `Enter` toggles branches (inert on leaves), `Esc` closes. Query bar
    empty; printable chars inert (search mode is Phase 4). Preview =
    placeholder.
- 14 unit tests (tree flattening, cursor wrap, expand/step, collapse/
    parent, toggle, session default expansion, session-tree
    reconstruction, active ordering, degrade-on-missing-fields,
    pane-label fallback, unavailable stub, five-groups-in-spec-order).

### Scaffold

- Initial repo structure: `Cargo.toml`, `herdr-plugin.toml`, `justfile`,
  `CLAUDE.md`, `README.md`, `PLANNING.md`, `config.example.toml`,
  `LICENSE`, `.gitignore`.
- Source modules (stubs): `main.rs`, `socket_client.rs`, `config.rs`,
  `nav.rs`, `source.rs`, `preview.rs`, `render.rs`.
- Docs (stubs): `doc/config-reference.md`, `doc/env-vars.md`,
  `doc/keybinding.md`, `doc/navigation.md`, `doc/use-cases.md`.
- CI: `ci.yml` (fmt/clippy/test on macOS + Linux) and `release.yml`
  (tag-triggered, 3 target triples, SHA-256, rolling `latest` release).
- Manifest with one `[[actions]]` (`nav-open`), `[[build]]` step, one
  popup pane (`switcher`) at 80%x80%, clamped to 100x34 cells.
- Placeholder event loop: renders a scaffold banner, exits on `Esc`.
  No switcher functionality yet — lands in the 15-phase sequence in
  PLANNING.md §17 (popup shell + Session tree browse is Phase 1).
- Normative spec, interactive prototype, and reference figures stored in
  `spec/` so implementation does not depend on files in `~/Downloads`.
