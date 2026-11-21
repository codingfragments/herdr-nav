# Config reference

`herdr-nav` reads its config from the first existing of (rex-migration
R2):

1. `$NAV_CONFIG` (a full file path — for launchers and tests),
2. `~/.config/cf-nav/config.toml`,
3. `$HERDR_PLUGIN_CONFIG_DIR/config.toml` (the legacy plugin dir).

A missing or malformed config never crashes the plugin — parse errors are
reported on stderr and built-in defaults are used. Copy
[`config.example.toml`](../config.example.toml) to `~/.config/cf-nav/` as
`config.toml` to customize (the legacy location still works unchanged).

```sh
herdr plugin config-dir herdr-nav   # prints the legacy (plugin) directory
mkdir -p ~/.config/cf-nav              # the new preferred location
```

The palette follows the `[theme]` setting — resolved from
`[host.<host>].theme` (below), then `[theme]`/`[theme.custom]` in nav's
own config, then Herdr's `~/.config/herdr/config.toml`, then the default
(catppuccin). Config covers group order, zoxide limit, preview, browse
expansion, search scoring, and provider bias (spec §13).

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

## `[host]` — per-host overrides

Optional tables scoped to the running host (`herdr` or `rex`), starting
with `groups` and `theme` only:

```toml
[host.rex]
groups = ["session", "agents", "pinned", "zoxide"]  # no plugins group on Rex
theme  = "tokyo-night"

[host.herdr]
groups = ["session", "agents", "plugins", "pinned", "zoxide"]
```

`[host.<host>].groups` overrides the top-level `groups` for that host;
`[host.<host>].theme` (a theme name) overrides the whole `[theme]`
chain. Unknown hosts (e.g. the no-multiplexer dev fallback) use the
top-level values.

## Templates — host-scoped dirs

- **herdr**: reads `~/.config/cf-nav/templates/`, else the legacy
  `~/.config/herdr/templates/`; writes to the first that exists, else
  nav's.
- **Rex**: its own set, `~/.config/cf-nav/rex/templates/` — read and
  write, no herdr fallback (a layout tuned for one multiplexer is
  rarely what you want in the other).

The schema is identical for both hosts (tabs, layout, commands — no
host-specific fields); see [`doc/templates.md`](templates.md).

## Pinned dirs — `targets.toml`

Pinned directories live in the first existing of
`~/.config/cf-nav/targets.toml` and the legacy
`~/.config/herdr/targets.toml` (shared by both hosts — it is pure user
data), separate from the switcher config (spec §13). Writes go to
`~/.config/cf-nav/targets.toml` unless only the legacy file exists, in
which case they stay there:

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
