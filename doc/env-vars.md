# Environment variable reference

The popup is one pane with no per-source selector (spec §1/§2). The
plugin-specific vars (`NAV_*`) are optional overrides; the `HERDR_*`
vars are set by Herdr itself for plugin panes.

| Variable | Set by | Purpose |
| --- | --- | --- |
| `NAV_HOST` | launcher (optional) | Force the host: `herdr` or `rex`. Without it, a set `HERDR_SOCKET_PATH` means herdr. Set by the Rex launcher (rex-migration.md) so a herdr running inside a Rex terminal is never mistaken for the other host. |
| `NAV_CONFIG` | user (optional) | Full path to nav's `config.toml`, tried before `~/.config/nav/config.toml` and the legacy plugin dir (rex-migration R2). |
| `HERDR_SOCKET_PATH` | Herdr | Unix socket path for the Herdr API (`pane.read`, the session graph, the plugin registry). Required under herdr. |
| `HERDR_PLUGIN_CONTEXT_JSON` | Herdr | Launch context: `focused_pane_id`, `focused_pane_cwd`, `tab_id`, `workspace_id`, `selected_text`, `clicked_url`, `invocation_source`. The switcher reads `focused_pane_id` / `workspace_id` to order Session (active workspace first) and to resolve "current pane" for `^p` pin-cwd. |
| `HERDR_PLUGIN_CONFIG_DIR` | Herdr | Directory where the plugin's `config.toml` lives. The plugin reads `$HERDR_PLUGIN_CONFIG_DIR/config.toml` at launch. |

`HERDR_ACTIVE_PANE_ID` is a fallback for manual dev-testing when
`HERDR_PLUGIN_CONTEXT_JSON` is not set (e.g. running the binary outside a
real Herdr plugin-pane invocation).
