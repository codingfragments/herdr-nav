# Environment variable reference

`herdr-nav` reads no plugin-specific env var at launch — the popup is one
pane with no per-source selector (spec §1/§2). All env vars below are set
by Herdr itself for plugin panes.

| Variable | Set by | Purpose |
| --- | --- | --- |
| `HERDR_SOCKET_PATH` | Herdr | Unix socket path for the Herdr API (`pane.read`, the session graph, the plugin registry). Required. |
| `HERDR_PLUGIN_CONTEXT_JSON` | Herdr | Launch context: `focused_pane_id`, `focused_pane_cwd`, `tab_id`, `workspace_id`, `selected_text`, `clicked_url`, `invocation_source`. The switcher reads `focused_pane_id` / `workspace_id` to order Session (active workspace first) and to resolve "current pane" for `^p` pin-cwd. |
| `HERDR_PLUGIN_CONFIG_DIR` | Herdr | Directory where the plugin's `config.toml` lives. The plugin reads `$HERDR_PLUGIN_CONFIG_DIR/config.toml` at launch. |

`HERDR_ACTIVE_PANE_ID` is a fallback for manual dev-testing when
`HERDR_PLUGIN_CONTEXT_JSON` is not set (e.g. running the binary outside a
real Herdr plugin-pane invocation).
