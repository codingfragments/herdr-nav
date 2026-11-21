# Environment variable reference

The popup is one pane with no per-source selector (spec §1/§2). The
plugin-specific vars (`NAV_*`) are optional overrides; the `HERDR_*`
vars are set by Herdr itself for plugin panes.

| Variable | Set by | Purpose |
| --- | --- | --- |
| `NAV_HOST` | launcher (optional) | Force the host: `herdr` or `rex`. Without it, a set `HERDR_SOCKET_PATH` means herdr. Set by the Rex launcher (rex-migration.md) so a herdr running inside a Rex terminal is never mistaken for the other host. |
| `NAV_CONFIG` | user (optional) | Full path to nav's `config.toml`, tried before `~/.config/cf-nav/config.toml` and the legacy plugin dir (rex-migration R2). |
| `HERDR_SOCKET_PATH` | Herdr | Unix socket path for the Herdr API (`pane.read`, the session graph, the plugin registry). Required under herdr. |
| `HERDR_PLUGIN_CONTEXT_JSON` | Herdr | Launch context: `focused_pane_id`, `focused_pane_cwd`, `tab_id`, `workspace_id`, `selected_text`, `clicked_url`, `invocation_source`. The switcher reads `focused_pane_id` / `workspace_id` to order Session (active workspace first) and to resolve "current pane" for `^p` pin-cwd. |
| `HERDR_PLUGIN_CONFIG_DIR` | Herdr | Legacy directory where the plugin's `config.toml` used to live; still the last fallback in the R2 lookup order. |
| `REX_SESSION` | Rex | The session the popup was opened in (set by Rex in every terminal); marks the active session in the tree. |
| `NAV_SOURCE_BLOCK` | Rex launcher | The pane the user was in when the popup opened (the Rex popup's own `REX_BLOCK` is the popup, not that pane). Optional: without it the source degrades to the session's focused block. |
| `REX_CLI` | Rex launcher | Path of the `rex` CLI (the Rex server runs with launchd's minimal PATH). Defaults to `rex` on `PATH`. |
| `NAV_INTENT_FILE` | Rex launcher | Where the switcher writes its jump record when the host delegates the jump (Rex): `{"kind":"focus","block_id":…}` or `{"kind":"open_session","session_id":…}`. The launcher reads it after the popup closes and carries the jump out. Fallback: `~/.config/cf-nav/intent.json`. |

`HERDR_ACTIVE_PANE_ID` is a fallback for manual dev-testing when
`HERDR_PLUGIN_CONTEXT_JSON` is not set (e.g. running the binary outside a
real Herdr plugin-pane invocation).
