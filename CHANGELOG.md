# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
