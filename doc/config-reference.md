# Config reference

`herdr-nav` reads its config from `$HERDR_PLUGIN_CONFIG_DIR/config.toml`.
A missing or malformed config never crashes the plugin — parse errors are
reported on stderr and built-in defaults are used. Copy
[`config.example.toml`](../config.example.toml) to the config dir as
`config.toml` to customize.

```sh
herdr plugin config-dir herdr-nav   # prints the target directory
```

The palette auto-follows Herdr's `[theme]` setting (spec §9 amended) —
it is **not** configurable here. See [`doc/env-vars.md`](env-vars.md) for
how the theme is resolved. Config covers group order, zoxide limit,
preview, browse expansion, search scoring, and provider bias (spec §13).

## Top-level keys

| Key | Default | Description |
| --- | --- | --- |
| `log_level` | `"info"` | stderr diagnostic verbosity. |
| `groups` | `["session","agents","pinned","zoxide","plugins"]` | Root groups in display order. Any subset; order is the display order. |
| `open_key` | `"ctrl-k"` | Informational — the real binding lives in your `config.toml` as a `plugin_action`. |
| `zoxide_limit` | `50` | How many top zoxide entries to list (existing paths only). |
| `zoxide_extend_limit` | `1000` | Cap for the `Tab` extend-zoxide keybind in Search mode (deeper frecency dirs when a query has no directory hits). |

## `[preview]`

| Key | Default | Description |
| --- | --- | --- |
| `enabled` | `true` | Show the preview pane. |
| `width_pct` | `56` | Preview pane width as a percentage of the body. |
| `min_cols` | `60` | Below this width the preview is hidden and bound to a toggle key. |

## `[expand]`

| Key | Default | Description |
| --- | --- | --- |
| `session_default` | `"active"` | Open Session to its active workspace + active tab. |
| `restore_ttl_secs` | `600` | Expire a restored expansion set after this many seconds. |

## `[scoring]`

Search scoring weights (spec §6.2).

| Key | Default | Description |
| --- | --- | --- |
| `consecutive` | `8.0` | Bonus per consecutive matched char. |
| `gap` | `0.4` | Penalty per gap char before a match. |
| `prefix` | `0.6` | Penalty × first match index (prefix matches win). |
| `word_boundary` | `4.0` | Bonus if the match starts at a word boundary (`/ › · space _ -`). |

## `[bias]`

Provider bias — a flat additive nudge, not a dominate (spec §6.3).

| Key | Default | Description |
| --- | --- | --- |
| `agent_waiting` | `6` | Agent blocked on input. |
| `pane` | `4` | Live pane. |
| `pinned` | `3` | Pinned dir. |
| `agent` | `2` | Other agents. |
| `zoxide` | `0` | zoxide entry. |
| `plugin` | `-2` | Plugin (rarely reached mid-flow). |

## Pinned dirs — `targets.toml`

Pinned directories live in `~/.config/herdr/targets.toml`, separate from
the switcher config (spec §13):

```toml
[[pin]]
path = "~/code/herdr"
slot = 1

[[pin]]
path = "~/work/infra"
slot = 2
```

`^p` in the popup adds a pin (the selected dir, or the selected
pane's cwd) and writes this file. `^u` removes a pin and renumbers
remaining slots.
