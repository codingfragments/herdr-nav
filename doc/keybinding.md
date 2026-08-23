# Keybinding reference

`herdr-nav` never binds its own keys — Herdr's own
`~/.config/herdr/config.toml` owns all keybindings, via
`[[keys.command]]` entries with `type = "plugin_action"`. This doc covers
the one shipped action, how to bind it, and the in-popup keymap. The
normative keymap is spec §8 / [PLANNING.md](../PLANNING.md) §10.

## Shipped action

`herdr-plugin.toml` declares one `[[actions]]` — `nav-open` — a thin
launcher that opens the real interactive popup (`herdr plugin pane open`).
There is one popup; in-popup navigation handles the five target groups,
so there are no per-group launcher actions.

| Action id | Description |
| --- | --- |
| `nav-open` | Open the switcher popup |

## Binding the open key

Bind a key to `nav-open` in your `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "Ctrl k"
action = "nav-open"
type = "plugin_action"
```

## In-popup keymap

Mode is derived from the query: empty → Browse, non-empty → Search.

| Key | Browse | Search |
| --- | --- | --- |
| `↑` | previous visible row (wraps) | previous match (wraps) |
| `↓` / `^n` | next visible row (wraps) | next match (wraps) |
| `↑` / `^p` | previous visible row (wraps) | previous match (wraps) |
| `→` / `Space` / `Tab` | expand; if open, step to first child | `→`/`Tab` inert; `Space` types a space |
| `←` | collapse; if closed, jump to parent | inert |
| `Enter` | branch → expand/step; leaf → default action, close | default action, close |
| `a–z 0–9 …` | enter search with that char | append, re-rank, cursor → 0 |
| `Backspace` | — | delete last char; empty → browse |
| `Esc` | close popup | clear query → browse |
| `^p` | pin selected dir (or selected pane's cwd) into Pinned dirs; stay open | |
| `^u` | unpin selected pinned dir; stay open | |
| `^d` | kill selected pane / tab / workspace; confirm inline; stay open | |
| `^t` | on a dir/zox entry: pick a workspace template, then name the workspace (Enter uses the auto-resolved default template with no picker) | |
| `^r` `^c` `^x` | context alternates, named per item in the preview footer | |
| `?` | open the in-popup help dialog | |

Footer hints are mode-aware: `⏎ open / expand` and `esc close` in Browse
become `⏎ run default action` and `esc clear` in Search. Press `?` for
the full in-popup dialog.

## Query filters

In search mode, the query supports filter tokens: group scope
(`agents nvim`), kind filters (`@pane`, `kind:dir`), and negation
(`!plugin`). See [`doc/query-filters.md`](query-filters.md) for the
full syntax, composition rules, and worked examples.
