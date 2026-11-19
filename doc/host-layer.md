# The host layer in herdr-nav

herdr-nav reaches herdr and Rex through two layers:

| Layer | Where | Purpose |
|---|---|---|
| **muxhost** | [`codingfragments/muxhost`](https://github.com/codingfragments/muxhost), pinned by tag in `Cargo.toml` | Shared with herdr-flash and herdr-zextract: reading and listing panes, the session tree, jump, close, rename. **Its reference docs live in that repo**, see below. |
| **`host_ext`** | `src/host_ext/` in this repo | nav-only glue (template building, capture, plugins, agent keys) and the app side of a Rex jump. Documented here. |

Rule of thumb: if a second tool would need it, it goes in muxhost; if
only nav needs it, it goes in `host_ext` and uses muxhost's escape
hatches (`HerdrHost::raw_request`, `RexHost::run`).

## muxhost reference

| Topic | Page |
|---|---|
| layers, features, design rules, escape hatches | [architecture](https://github.com/codingfragments/muxhost/blob/main/doc/architecture.md) |
| herdr's socket vs Rex's CLI, environment, every quirk | [hosts](https://github.com/codingfragments/muxhost/blob/main/doc/hosts.md) |
| `Host`, `Caps`, `detect_kind`, clipboard | [core](https://github.com/codingfragments/muxhost/blob/main/doc/core.md) |
| sessions/windows/panes/agents; Rex's one-script snapshot and its cache | [topology](https://github.com/codingfragments/muxhost/blob/main/doc/topology.md) |
| jump/close/rename; the `Delegated` contract | [navigate](https://github.com/codingfragments/muxhost/blob/main/doc/navigate.md) |
| `FakeHost`, the runner fake, fixtures | [testing](https://github.com/codingfragments/muxhost/blob/main/doc/testing.md) |

## nav's `host_ext` (`src/host_ext/`)

### The traits

| Trait | Contents | Why it's here and not in muxhost |
|---|---|---|
| `NavHost` | `Host + HostTopology + HostNavigate`, implemented for anything that has all three | so `muxhost::fake::FakeHost` works as a nav host in tests |
| `TemplateHost` | `create_session`, `create_window`, `split_pane`, `export_layout`, `process_info` | template building and capture are nav-only and still changing (R5) |
| `HostExt` | `list_plugins`, `invoke_plugin_action`, `pane_cwd`, `send_keys`, `agent_send_keys`, `release_agent`, `notify` | herdr-only, or nav-only, surface |
| `NavFull` | `NavHost + TemplateHost + HostExt` | what the binary passes around (`&dyn NavFull`) |

### `detect()` and `NoHost`

nav builds its own host types (it needs a `NavFull`, and a fallback
instead of an error), but the **order** comes from muxhost: it calls
[`muxhost::detect_kind("NAV")`](https://github.com/codingfragments/muxhost/blob/main/doc/core.md#detection),
which checks `NAV_HOST`, then `HERDR_SOCKET_PATH`, then `REX_SESSION`
(the innermost multiplexer first). Launchers always set `NAV_HOST`:
`bin/nav-launch` pins herdr, `rex/nav.lua` pins rex. An unknown
`NAV_HOST`, or no multiplexer at all, gives **`NoHost`**: every call
fails with one message, so the popup still opens and the groups show
"unavailable" (the dev path).

On herdr, a socket without a launch context falls back to
`HerdrHost::with_socket`. On Rex, a missing `NAV_SOURCE_BLOCK` (running
nav by hand in a Rex terminal) falls back to `RexHost::new` with an
empty source.

### herdr glue (`host_ext/herdr.rs`, through `raw_request`)

| Method | Socket call |
|---|---|
| `create_session` | `workspace.create {cwd, label}` → `workspace.workspace_id`, `tab.tab_id` (or `workspace.active_tab_id`), `root_pane.pane_id` |
| `create_window` | `tab.create {workspace_id, cwd, label?}` → `root_pane.pane_id` |
| `split_pane` | `pane.split {pane_id, direction, ratio (0–1), cwd}` → `pane.pane_id` |
| `export_layout` / `process_info` | `layout.export {tab_id}` / `pane.process_info {pane_id}`, returned raw (capture's parsers own the format) |
| plugins | `plugin.list`, `plugin.action.invoke {plugin, action_id}` |
| `pane_cwd` | `pane.get` → `pane.cwd` |
| keys / agents | `pane.send_keys`, `agent.send_keys {target}`, `pane.release_agent` |
| `notify` | `notification.show` |

Template building has its own ordering rules on herdr, all confirmed
live and commented in `source.rs::build_workspace_from_template`:
`pane.split` splits the **active** pane, not the one you pass, so the
target is focused before every split, and focus is put back at the end.

### Rex glue (`host_ext/rex.rs`, through `RexHost::run`)

| Method | CLI call | Notes |
|---|---|---|
| `create_session` | `rex new LABEL -c DIR --json` | → `session_id`, `initial_windows[0].window_id`, `initial_windows[0].block_ids[0]` |
| `create_window` | `rex window new -s S [LABEL] --json` | the reply is flat: `block_ids[0]` |
| `split_pane` | `rex split -s S -b B --split right\|below --ratio PCT --json` | `--ratio` is an **integer percent** (nav's 0–1 value ×100, clamped to 1–99; a fraction is a flag parse error, which is why splits never worked before). herdr's `down` maps to `below`. The reply is flat: `block_ids[0]`. |
| `export_layout` / `process_info` | error, coming in R5 | capture on Rex is open |
| `pane_cwd` | `rex block call -s <block's session> -b B pwd` | `file://` URL, path part kept |
| `send_keys` / `agent_send_keys` | `rex send-key -s <block's session> -b B KEY` | Rex addresses agents by their block |
| plugins / `release_agent` / `notify` | error | off via `caps` |

### `host_ext::jump`: the app side of a Rex jump

This is the reference implementation of muxhost's
[`Delegated` consumer contract](https://github.com/codingfragments/muxhost/blob/main/doc/navigate.md#consumer-contract-carrying-out-a-delegated-jump).

Every jump the user chooses goes through `host_ext::jump(host, jump)`,
never `host.jump` directly. (Two invoke paths calling `host.jump`
directly were the reason the jump "didn't work" during R4; template
building's intermediate focus calls are the only intended direct
uses.) When the outcome is `Delegated`, nav does the app side.

**Example:** you're in B1 (session B), pick A12 (session A, window W),
press Enter.

| # | Who | What |
|---|---|---|
| 1 | muxhost | `rex focus -s A A12`. Server side done; the app still shows B. |
| 2 | nav | writes the jump record to `$NAV_INTENT_FILE`: `{"kind":"focus","block_id":"A12","session_id":"A","window_id":"W"}` (session and window from the snapshot) |
| 3 | nav | **sync** `rex -C $NAV_CLIENT_ID do session.select session_id=A window_id=W`. It exists to **catch errors**: if it fails (Remote Control off), the popup stays open with a hint. It does **not** switch the app reliably (see 4). |
| 4 | nav exits | the popup's layer closes. The app moves focus back to B1, which **undoes** any select made while the layer was open. |
| 5 | detached child | ~0.5 s later, a `nohup`'d shell started in step 3 (outside the popup's process group, so closing the layer can't kill it) runs `session.select session_id=A window_id=W` **and then** `rex focus -s A A12`. `window_id` is needed because the tab strip follows client actions, not server focus. The focus comes **last** because the app's select lands on the window's previously focused block. |
| 6 | end state | the app shows A, window W, focus and keyboard in A12 |

**Esc** (no jump): `main` runs `rex focus -s $REX_SESSION
$NAV_SOURCE_BLOCK` on the way out, putting you back where you started
(`jump_performed()` tells it whether a jump happened).

**Why the launcher doesn't do this.** The R0 design had `rex/nav.lua`
wait for the popup to close (a synchronous `rex wait`) and queue the
app actions itself, which needs no Remote Control. But a blocking
action holds the app's Lua for as long as the popup is open, and the
popup **got no key presses**. The launcher now opens the layer and
returns at once (like zextract's), and the switcher does the jump
itself. This is the plan's R1 fallback.

**Diagnostics:** every app-side jump appends
`select client=… session=… window=… exit=…` to
`/tmp/herdr-nav-jump.log`. The popup is gone a moment later, so a
failed jump leaves its trace there.

### Host-scoped data

| Data | herdr | Rex |
|---|---|---|
| templates | `~/.config/nav/templates/`, then the legacy `~/.config/herdr/templates/` | **its own set**, `~/.config/nav/rex/templates/`, no fallback to herdr's |
| config groups / theme | `[host.herdr]` | `[host.rex]` |
| pinned dirs (`targets.toml`) | shared by both hosts | shared by both hosts |

---
