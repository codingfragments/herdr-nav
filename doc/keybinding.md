# Keybinding reference

`herdr-nav` never binds its own keys — Herdr's own
`~/.config/herdr/config.toml` owns all keybindings, via
`[[keys.command]]` entries with `type = "plugin_action"`. This doc covers
the shipped actions, how to bind them, and the in-popup keymap. The
normative keymap is spec §8 / [PLANNING.md](../PLANNING.md) §10.

## Shipped actions

`herdr-plugin.toml` declares two `[[actions]]` — `nav-open` (the
switcher) and `nav-capture` (capture the current workspace as a
template). Each is a thin launcher that opens its interactive popup
(`herdr plugin pane open`).

| Action id | Description |
| --- | --- |
| `nav-open` | Open the switcher popup |
| `nav-capture` | Capture the current workspace as a template (see [`spec/capture-template-spec.md`](../spec/capture-template-spec.md)) |

## Binding the open key

Bind a key to `nav-open` in your `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "Ctrl k"
action = "nav-open"
type = "plugin_action"
```

## Binding the capture key

`nav-capture` is a separate binding. A prefix chord keeps the direct
keys clean; `prefix+ctrl+t` is the recommended default (the `t` family
= herdr-nav; `prefix+t` opens the switcher, `prefix+ctrl+t` captures a
template):

```toml
[[keys.command]]
key = "prefix+ctrl+t"
action = "nav-capture"
type = "plugin_action"
description = "capture workspace as template"
```

## In-popup keymap

Mode is derived from the query: empty → Browse, non-empty → Search.

| Key | Browse | Search |
| --- | --- | --- |
| `↑` | previous visible row (wraps) | previous match (wraps) |
| `↓` / `^n` | next visible row (wraps) | next match (wraps) |
| `↑` / `^p` | previous visible row (wraps) | previous match (wraps) |
| `→` / `Space` / `Tab` | expand; if open, step to first child | `→` inert; `Space` types a space; `Tab` extends zoxide (see below) |
| `←` | collapse; if closed, jump to parent | inert |
| `Enter` | branch → expand/step; leaf → default action, close | default action, close |
| `a–z 0–9 …` | enter search with that char | append, re-rank, cursor → 0 |
| `Backspace` | — | delete last char; empty → browse |
| `Esc` | close popup | clear query → browse |
| `^p` | pin selected dir (or selected pane's cwd) into Pinned dirs; stay open | |
| `^u` | unpin selected pinned dir; stay open | |
| `^d` | kill selected pane / tab / workspace; confirm inline; stay open | |
| `^t` | on a dir/zox entry: pick a workspace template, then name the workspace (Enter uses the auto-resolved default template with no picker) | |
| `^f` | enter directory navigation mode (DirNav) — see [navigation.md](navigation.md#directory-navigation-mode-dirnav--v02) | |
| `^r` `^c` `^x` | context alternates, named per item in the preview footer | |
| `?` | open the in-popup help dialog | |

### `Tab` — extend zoxide (Search mode)

In Search mode, when the result list contains **no directory entries**
(pinned dirs or zoxide), a `⇥ extend zoxide` hint appears in the
footer. Pressing `Tab` re-runs `zoxide query --list --score` against a
much larger limit (`zoxide_extend_limit`, default 1000) so deeper
frecency dirs surface, then re-ranks. The extension is **sticky** for
the rest of the invocation — one subprocess, not one per keystroke —
so later query edits keep the extended set. The hint hides once
extended. If zoxide isn't installed, the row flashes an error and the
popup stays open.

Footer hints are mode-aware: `⏎ open / expand` and `esc close` in Browse
become `⏎ run default action` and `esc clear` in Search. Press `?` for
the full in-popup dialog.

## Query filters

In search mode, the query supports filter tokens: group scope
(`agents nvim`), kind filters (`@pane`, `kind:dir`), and negation
(`!plugin`). See [`doc/query-filters.md`](query-filters.md) for the
full syntax, composition rules, and worked examples.

## Directory navigation mode (DirNav)

`^f` opens a filesystem directory walker at the focused pane's cwd.

| Key | Action |
| --- | --- |
| `↑` / `↓` | move cursor (wraps) |
| `←` | ascend to parent (lands on the entry you came from); clears the search |
| `→` | descend into the cursor directory (inert on non-dirs); clears the search |
| `a–z 0–9 …` | fuzzy-search this level's entry names; cursor → first match |
| `Backspace` | delete last search char; empty → clear search |
| `Enter` | open a new workspace at the selected dir (name prompt) |
| `^t` | pick a workspace template, then name the workspace |
| `^p` | pin the selected dir (or cwd) into Pinned dirs |
| `.` | toggle hidden entries (dotfiles) |
| `Esc` | clear search → exit DirNav (two-stage) |

The commit verb `Enter`/`^t`/`^p` and the `.` hidden-toggle are now
shipped. See
[navigation.md](navigation.md#directory-navigation-mode-dirnav--v02).
