# Navigation reference

`herdr-nav` is a popup target switcher for the Herdr terminal
multiplexer: one keystroke opens it, you aim, Enter moves you, it closes.
This doc covers the two modes, the five target groups, and the node
model — see [`doc/keybinding.md`](keybinding.md) for the keymap and
[`doc/config-reference.md`](config-reference.md) for config keys. The
normative design is [PLANNING.md](../PLANNING.md); this doc is the
user-facing summary.

---

## Two modes (derived, never toggled)

Mode is `if query.is_empty() { Browse } else { Search }`. There is no
mode key.

- **Browse** — a single-selection tree of the five target groups. Groups
  expand; `Enter` on a branch expands/steps into it, on a leaf runs its
  default action. Typing any printable character flips to Search.
  Expand/collapse is `→`/`←`/`Space`/`Tab` only — `Enter` is the
  main action verb on every row (spec §8 amended).
- **Search** — the tree is replaced by the fully rolled-out set of leaves
  only, each row showing its path as a dimmed breadcrumb prefix. The
  query fuzzy-filters and re-ranks this list. `Backspace` to empty
  returns to Browse with the tree's expansion state exactly as it was.

`Esc` is two-stage: in Search it clears the query (back to Browse); in
Browse it closes the popup.

## Directory navigation mode (DirNav) — v0.2

`^f` opens a **third, toggled** mode: a filesystem directory walker
starting at the focused pane's cwd. This is a v0.2 scope expansion —
spec §1 lists "not a file browser" as a non-goal; DirNav is accepted as
a deliberate departure (see [PLANNING.md](../PLANNING.md) §17 "v0.2
phases").

- **Listing:** directories + symlinks that resolve to a directory only
  (files and links-to-files are not shown). Hidden entries (dotfiles)
  are hidden by default.
- **Navigation:** `↑↓` move the cursor (wraps); `←` ascends to the
  parent directory (landing on the entry you came from); `→` descends
  into the cursor directory. Each left/right resets the in-level search.
- **In-level search:** typing fuzzy-filters the current level's entry
  names and lands on the first match; `↑↓` then jump between matches
  (find). *(Lands in Phase 18.)*
- **Commit:** `Enter` opens a new workspace at the selected dir; `^t`
  picks a template first; `^p` pins the cwd. *(Lands in Phase 19.)*
- **Esc:** two-stage — active in-level search → clear; no search → exit
  DirNav and restore the prior switcher state (Browse expansion or
  Search query intact). *(Phase 17: single-stage — exits DirNav.)*

The switcher's tree and search state are preserved off-screen while
DirNav is active, so Esc always returns you to exactly where you were.

## The five target groups

Fixed order (§4): the two live/volatile groups first, the three stable
groups after.

| # | Group | Shape | Leaf kind | Sort |
| --- | --- | --- | --- | --- |
| 1 | Session | 3-level tree: workspace / tab / pane | pane | session order, active workspace first |
| 2 | Agents | flat list | agent | waiting → working → idle, then recency |
| 3 | Pinned dirs | flat list | dir | config file order (slot ⌘1–⌘9) |
| 4 | zoxide | flat list | zox | frecency score, descending |
| 5 | Plugins | flat list | plugin | name, errors surfaced in group meta |

Session is expanded by default to its active workspace and that
workspace's active tab. A restored expansion set (optional) expires after
10 minutes.

## Preview

One shape for every kind, in four stacked regions: kind glyph + title,
provenance subtitle, 1–3 status chips, a labelled monospace body, and a
footer naming the default action + alternate. Read-only, never scrolls —
clipped at the pane's height. Resolution is debounced 60ms; a slow
provider keeps the previous preview visible and dimmed. On narrow
terminals the pane is hidden and bound to a toggle key.

## Actions per kind

`Enter` on a leaf runs its default action and closes the popup (with a
one-line toast in the host terminal). Side actions (`^p` pin, `^d` kill,
`^r ^c ^x` alternates) keep the popup open and toast in place.

| Kind | `Enter` default | Alternate |
| --- | --- | --- |
| pane / agent | jump to the pane (switch workspace + tab + focus) | `^d` kill · `^r` restart / `^c` interrupt / `^x` detach |
| workspace / tab | switch to it, keeping its active pane | `^p` pin · `^d` kill |
| dir / zox | **always** open a new workspace at that path, built from the auto-resolved default template (match-glob → `default: true` → hardcoded 1-tab/1-pane) | `^t` pick template then name · `^p` pin |
| plugin | open the plugin's action picker (secondary selector) | — |

The default action is always named in the preview footer before you
commit — Enter is never a guess.
