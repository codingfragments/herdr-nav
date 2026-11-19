# The host layer: muxhost + nav's `host_ext`

herdr-nav runs under two multiplexers, [Herdr](https://herdr.dev) and
[Rex](https://rex.dev), from one binary. Everything multiplexer-specific
goes through two layers:

| Layer | Where | Purpose |
|---|---|---|
| **muxhost** | [`codingfragments/muxhost`](https://github.com/codingfragments/muxhost) (pinned by tag in `Cargo.toml`, currently **v0.3.5**) | A crate shared by herdr-nav, herdr-flash and herdr-zextract: one trait surface over herdr and Rex for things more than one tool needs (reading panes, listing them, the session tree, jumping, closing, renaming). |
| **`host_ext`** | `src/host_ext/` in this repo | nav-only glue: template building, capture, plugins, agent keys. Kept out of muxhost on purpose (rex-migration.md "Decisions"), so nav can change it without cutting a muxhost release. |

Rule of thumb: **if a second tool would need it, it goes in muxhost;
if only nav needs it, it goes in `host_ext`.**

The rest of this document goes through each feature: what it is for,
then how the herdr and Rex implementations work.

---

## 1. Design rules (muxhost)

- **Dependencies:** `std`, `serde_json`, and `arboard` (for the
  clipboard) only. Nothing from a consuming tool, and no tool types in
  any signature.
- **Errors** are plain `String`s and meant to be shown to the user. The
  crate does not log.
- **Additions don't break consumers.** New trait methods get default
  implementations and public enums are `#[non_exhaustive]`, so a tool
  pinned to an older tag keeps compiling against a newer muxhost.
- **Cargo features** keep the small tools small:

  | Feature | Adds | Used by |
  |---|---|---|
  | *(core)* | `Host`, `Caps`, `detect`, `clipboard`, `fake` | flash, zextract, nav |
  | `topology` | `HostTopology` (sessions/windows/panes/agents, `TopologySnapshot`) | nav |
  | `navigate` (implies `topology`) | `HostNavigate` (jump, close, rename) | nav |

- **The concept mapping** (the names nav's tree and docs use):

  | Neutral (muxhost) | herdr | Rex |
  |---|---|---|
  | session | workspace | session |
  | window | tab | window |
  | pane | pane | block |

---

## 2. Core: the `Host` trait

What every tool needs: where it was launched from, read a pane, list
the panes on the tab, type into a pane.

| Method | Purpose |
|---|---|
| `name()` | `"herdr"`, `"rex"` (nav's `NoHost` returns `"none"`). Used for host-scoped behaviour: template dirs, config `[host.<name>]` tables, `theme::load(host)`. |
| `source()` | The pane the tool was opened over (`SourceRef { pane_id, tab_id }`). The popup is a pane of its own, so this has to come from the launcher, not from "the focused pane". |
| `read_pane(pane, source, format, lines)` | A pane's text. `ReadSource::Viewport` = what is on screen; `Scrollback` = the soft-wrapped history, lines joined. `ReadFormat::Plain` or `Ansi` (with escape sequences, for coloured previews). `lines` keeps the newest N. |
| `list_tab_panes()` | The panes on the source tab, the source pane first, without the tool's own popup. (zextract's "grab the whole tab".) |
| `insert_text(pane, text)` | Type text into a pane as if the user typed it. |
| `scroll_info(pane)` *(default `Ok(None)`)* | The viewport's scroll offset (flash's "follow the scroll position"). `None` means the host can't tell. |
| `caps()` *(default: all false)* | The optional capabilities below. |

### `Caps`: what a host can do

| Flag | herdr | Rex | Consumer behaviour |
|---|---|---|---|
| `scroll_follow` | ✅ | ❌ (scroll position is kept by the app, not the server) | flash anchors on the viewport, or degrades gracefully |
| `legacy_keyboard` | ✅ (popup PTY runs in legacy mode) | ❌ | tools debounce duplicate key events (nav's `KEY_DEBOUNCE`) |
| `plugins` | ✅ | ❌ (no plugin system) | nav hides the Plugins group; the `@plugins` filter shows "not available on Rex" |
| `agent_release` | ✅ | ❌ | nav's `^x` (detach agent) does nothing on Rex |

### herdr implementation (`HerdrHost`)

Talks to herdr's JSON socket at `$HERDR_SOCKET_PATH`, with a **new
connection for every request** (herdr closes the socket after each
reply; reusing it gives `BrokenPipe`). The client is private to
muxhost (`socket_client.rs`); everything goes through it.

| Method | Socket call |
|---|---|
| construction | `from_env()`: launch context from `HERDR_PLUGIN_CONTEXT_JSON` (`focused_pane_id`, `tab_id`), falling back to `HERDR_ACTIVE_PANE_ID` for dev runs. `with_socket(path)`: a socket without a launch context (nav's dev path; the source is left empty). |
| `read_pane` | `pane.read {pane_id, source: "visible" \| "recent_unwrapped", lines?, format: "ansi", strip_ansi: false}` → `read.text` |
| `list_tab_panes` | `pane.list`, filtered to `tab_id`, source pane first. Title order: label → `terminal_title_stripped` → `pane <id>`. |
| `insert_text` | `pane.send_text {pane_id, text}` |
| `scroll_info` | `pane.get` → `pane.scroll {offset_from_bottom, max_offset_from_bottom, viewport_rows}` (a missing `scroll` means `None`) |
| `raw_request(method, params)` | **Escape hatch** for tool-local glue (nav's `host_ext/herdr.rs`): any socket method, through the same client. |

### Rex implementation (`RexHost`)

Rex has no socket API for tools. Everything goes through the **`rex`
CLI**, behind a `RexRunner` trait so tests can check exact argument
lists without a server (`CliRunner` is the real one).

**Construction:** `from_env(prefix)` reads the variables the Rex
launcher sets. Inside the popup `REX_BLOCK` is the popup's own block,
so the source pane has to be passed in explicitly:

| Env var | Meaning |
|---|---|
| `REX_SESSION` | the session the popup opened in |
| `<PREFIX>_SOURCE_BLOCK` | the pane the user was in (required by `from_env`; nav falls back without it, see §6) |
| `<PREFIX>_SOURCE_WINDOW` | its window |
| `REX_CLI` | the CLI's absolute path. The Rex server runs with launchd's minimal `PATH`, so plain `rex` is not found. |

| Method | CLI call |
|---|---|
| `read_pane` Scrollback | `rex capture -s S -b B --unwrap --trim --format text\|vt`, then keep the last N lines |
| `read_pane` Viewport | `rex capture -s S -b B --format …` (without `--unwrap`: screen rows are wrapped rows), then the bottom `rows` lines, with `rows` from `rex block call … size` |
| `list_tab_panes` | `rex session inspect S --json` → the source window's **tiled** layers (floating layers, including the popup, are left out), terminal blocks only, source first. Title: a label that isn't `shell` → terminal title (`block call … title`) → label → `pane <id>`. |
| `insert_text` | `rex send -s S -b B`, with the **text on stdin** so leading `-` and newlines arrive unchanged |
| `scroll_info`, `caps` | the defaults: `None` and all-false |
| `session()`, `run(args, stdin)` | **Escape hatch** for tool-local glue (nav's `host_ext/rex.rs`), the Rex counterpart of `raw_request`. `run` also **drops the topology snapshot cache** (§3), because glue calls like `rex new` or `rex split` change the topology. |

**Block session scoping (v0.3.4).** A `-s SESSION -b BLOCK` call fails
with "block target not found" if the block is in a different session.
Block calls (`capture`, `send`, and nav's `pwd`/`split`/`send-key`)
therefore look up the **block's own session** in the topology snapshot
(`block_session(block)`) and fall back to the host's session. Without
this, previews of panes in other sessions failed.

**ANSI under Rex.** `--format vt` uses `CSI n C` (cursor forward) as a
run of n spaces. `ansi-to-tui` drops it, which shifts columns. Tools
expand it before parsing: nav's `preview::vt_prepare`, copied from
flash. It turns `CSI n C` into spaces, keeps SGR, and drops other
escapes. It is tested against `tests/fixtures/rex/capture-vt.txt`.

---

## 3. `topology`: the session tree

**Intent:** a tree-style switcher needs every session, window and pane,
plus the agents, in one go, with the active and focused markers.

| Item | Purpose |
|---|---|
| `SessionInfo {session_id, label, active}` | a session; `active` = the one the user is in |
| `WindowInfo {window_id, session_id, label, active}` | a window and its session |
| `PaneEntry {pane_id, session_id, window_id, title, is_focused, cwd}` | every pane in every session, floating panes left out |
| `AgentEntry {pane_id, terminal_id, name, title, status}` | panes running an agent; `AgentStatus` is `Waiting`, `Working`, `Idle` or `Unknown` |
| `list_sessions` / `list_windows` / `list_all_panes` / `list_agents` | the four lists |
| `topology_snapshot()` | **all four at once** (`TopologySnapshot`). The default implementation calls the four lists; hosts where a round trip is expensive override it. nav builds its tree from this. |

### herdr: four cheap socket calls

| List | Socket call | Notes |
|---|---|---|
| sessions | `workspace.list` | `active` = `focused` |
| windows | `tab.list` | `active` = `focused` |
| panes | `pane.list` | herdr never lists plugin popups here, so nothing needs filtering |
| agents | `agent.list` | `agent_status` → `AgentStatus`; title = `terminal_title_stripped`, falling back to the agent name |

`topology_snapshot` is the default (the four calls). Socket calls take
milliseconds.

### Rex: one `rex do` script, cached

Building the same data from separate CLI calls (`ls`, then `session
inspect` per session, then several `block call`s per block) costs a
process spawn and a server round trip each. For a popup that should
build in under 150 ms that is too slow (spike S4). Instead, **one Lua
script runs on the server** (`rex do -`, the script piped on stdin)
and returns everything as JSON. It takes about 50 ms for several
sessions.

The script (`TOPOLOGY_SCRIPT` in `rex.rs`; the same version is kept as
the spike fixture `tests/fixtures/rex/nav-s4-topology.lua`):

1. `rex.call("session.list")`
2. per session: `rex.call("session.attach")`, which **returns the view
   inline**, then `session.detach`. (`rex.session.view` without an
   attachment returns nil in script context.)
3. per window, **tiled layers only**: the blocks
4. per block: `rex.call("block.method", {method = "title" | "pwd" |
   "program_status"})`. Block methods use their **short** names; the
   creator-prefixed names return nothing.

Parsing (`parse_topology`):

| Field | Source |
|---|---|
| session `active` | `session_id == REX_SESSION` |
| pane `is_focused` | `block_id == <PREFIX>_SOURCE_BLOCK` (the pane the user was in, which is more useful to a switcher than each window's own focused block) |
| pane `title` | **a user label that isn't `shell`** → terminal title → label → `pane <id>`. Rex labels every new block `shell`, so only a renamed block's label wins. (Before v0.3.5 the terminal title won, so renamed panes like `A12` showed as `~`.) |
| pane `cwd` | `pwd` returns a `file://host/path` URL; the path part is kept |
| agents | blocks whose `program_status.records` is non-empty. The record's `status`: `blocked` → `Waiting`, `working` → `Working`, `idle` → `Idle`. gopher-lua encodes an empty table as `{}`, so both `[]` and `{}` count as "no agent". |
| window `active` | always `false`. **Known gap:** the script returns each session's `active_window_id`, but the parser doesn't use it yet. nav's "active tab first" ordering therefore does nothing under Rex. |

**The cache.** The four list methods read from one snapshot, kept for
**250 ms** (`SNAPSHOT_TTL`) so that changes made elsewhere still show
up. It is **dropped right away** after any change made through this
host: closes, renames, and every `run` escape-hatch call. Without
that, a session nav had just created with `rex new` was not in the
cache, and the jump to it failed with "no session holds block".

---

## 4. `navigate`: jump, close, rename

**Intent:** take the user somewhere and change the layout. `jump`
returns an outcome because the two hosts behave very differently.

| Item | Purpose |
|---|---|
| `Jump::FocusPane {pane_id}` | land on a pane, in any session |
| `Jump::SelectSession {session_id, focus_pane?}` | switch session, optionally to a given pane |
| `JumpOutcome::Executed` | the host did the whole jump |
| `JumpOutcome::Delegated` | the host did the **server side**; the **app's view** (which session it shows, the tab strip) is the tool's job |
| `close_pane` / `close_window` / `close_session` | nav's `^d` kill |
| `rename_pane` / `rename_window` | template building (tab and pane labels) |

### herdr: one call, `Executed`

| Method | Socket call |
|---|---|
| `jump` | `pane.focus {pane_id}` switches workspace, tab and pane in one call. For `SelectSession` without a pane: the session's first pane (from `list_all_panes`). |
| closes | `pane.close`, `tab.close`, `workspace.close` |
| renames | `pane.rename`, `tab.rename` |

### Rex: split into server side and app side, `Delegated`

A Rex **app** (the macOS client) decides what is on screen, on the
client side. The server can move focus inside a session, but cannot
make the app show another session. That needs a client action
(`session.select`), and a CLI process can only trigger one through
**Remote Control**: `rex -C <client> do …`, which has to be enabled
once in the app's settings.

`RexHost::jump` does the **server side** and then returns `Delegated`:

1. resolve the target block's session from the snapshot
2. `rex focus -s SESSION BLOCK`. This attaches to the target session
   as a child CLI call. (`session.focus_block` from a connection
   attached elsewhere silently does nothing; R0 finding.)

The **app side** belongs to the tool. nav's implementation, including
the ordering problems found live, is in §6.

| Method | CLI call (session from the snapshot) |
|---|---|
| `close_pane` | `rex block close -s S B` |
| `close_window` | `rex window close -s S W` |
| `close_session` | `rex session kill S` |
| `rename_pane` | `rex api call -s S session.set_block_label {"block_id", "label"}` |
| `rename_window` | `rex window rename -s S W LABEL` |

Each one drops the snapshot cache afterwards.

---

## 5. Clipboard and tests (`clipboard`, `fake`)

- **`clipboard::copy(text)`** tries the system clipboard (`arboard`)
  first. Where there isn't one (a headless or remote Linux session
  behind Rex or ssh), it writes an **OSC 52** sequence to `/dev/tty`,
  which the terminal passes on to the local clipboard. It fails only
  if both do. Used by flash and zextract.
- **`fake::FakeHost`** is an in-memory host for consumers' unit tests:
  canned pane texts, a tab listing, and recorded reads, inserts,
  jumps, closes and renames. With the features on, it implements
  `HostTopology` (`with_topology(sessions, windows, panes, agents)`)
  and `HostNavigate`. nav's tree tests run against it, with nav-local
  glue implementations in `source.rs`'s tests.

---

## 6. nav's `host_ext` (`src/host_ext/`)

### The traits

| Trait | Contents | Why it's here and not in muxhost |
|---|---|---|
| `NavHost` | `Host + HostTopology + HostNavigate`, implemented for anything that has all three | so `muxhost::fake::FakeHost` works as a nav host in tests |
| `TemplateHost` | `create_session`, `create_window`, `split_pane`, `export_layout`, `process_info` | template building and capture are nav-only and still changing (R5) |
| `HostExt` | `list_plugins`, `invoke_plugin_action`, `pane_cwd`, `send_keys`, `agent_send_keys`, `release_agent`, `notify` | herdr-only, or nav-only, surface |
| `NavFull` | `NavHost + TemplateHost + HostExt` | what the binary passes around (`&dyn NavFull`) |

### `detect()` and `NoHost`

nav uses its own `detect` rather than `muxhost::detect`, because it
needs a `NavFull` and a fallback instead of an error:

1. `NAV_HOST=herdr|rex` wins (launchers set it: `bin/nav-launch` pins
   herdr, `rex/nav.lua` pins rex)
2. else `REX_SESSION` set → Rex
3. else `HERDR_SOCKET_PATH` set → herdr
4. else **`NoHost`**: every call fails with one message, so the popup
   still opens and the groups show "unavailable" (the dev path).

On herdr, a socket without a launch context falls back to
`HerdrHost::with_socket`. On Rex, a missing `NAV_SOURCE_BLOCK` (running
nav by hand in a Rex terminal) falls back to `RexHost::new` with an
empty source.

> **Inconsistency to know about:** `muxhost::detect` checks
> `HERDR_SOCKET_PATH` **before** `REX_SESSION`; nav's checks them the
> other way round. It only matters without `NAV_HOST`, and both
> launchers set it.

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

## 7. Versions

| muxhost | Change |
|---|---|
| v0.1.0 | `Host` (herdr + Rex), clipboard, fake (from zextract) |
| v0.2.0 | `topology` + `navigate` for herdr |
| v0.2.1 | `HerdrHost::raw_request`, `with_socket` |
| v0.3.0 | Rex topology (one `rex do` snapshot, cached), Rex navigate (`Delegated`), `topology_snapshot` |
| v0.3.1 | `RexHost::run` + `session()` (Rex escape hatch) |
| v0.3.2 | `RexHost::jump` runs the server-side `rex focus` itself |
| v0.3.3 | `run` drops the snapshot cache; `navigate` implies `topology` |
| v0.3.4 | block calls use the block's own session |
| v0.3.5 | user labels win over terminal titles; the `Delegated` contract documented |

## See also

- [`doc/rex.md`](rex.md): running under Rex (install, bindings, the jump)
- [`doc/env-vars.md`](env-vars.md): every env var the launchers pass in
- `rex-migration.md`: the phase plan, spikes and decisions
- muxhost's own docs (`cargo doc -p muxhost --features topology,navigate`)
