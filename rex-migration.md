# Rex migration plan: one binary, two hosts, on the shared `muxhost` crate

> **Status: planned, not started.** This is the active plan for this repo.
> Agents: read this file before you start, work the phases in order
> (one `phase/rex-N-…` branch and PR per phase, see CLAUDE.md gitflow),
> and tick the checkboxes and update the "Status" lines as phases land.
>
> **Dependencies:** the sister repos go first.
> - `herdr-zextract` (`rex-migration.md`) builds the `Host` trait and the
>   Rex backend.
> - `herdr-flash` (`rex-migration.md`, its Phase 1) moves them into the
>   shared crate `codingfragments/muxhost`.
>
> This plan starts once `muxhost` v0.1 exists. Phase R0 (the jump spike, S0)
> needs no `muxhost` and may run earlier.

## Goal

Make `herdr-nav`, the switcher, serve **both** Herdr and Rex
(superlogical's terminal multiplexer, `com.superlogical.rex`) from the
**same binary, same config and same release**:

- Herdr keeps working exactly as today.
- Rex gets a thin Lua launcher, plus a small Lua handler that performs
  the jump (see "The jump problem").

```
                          herdr-nav
   browse/search UI · groups · preview · templates · capture
        │                                    │
   muxhost (shared)                    nav-local host glue
   core · topology · navigate          (layout export, create
        │                               session/window/split, process
        │                               info: templates and capture only)
   ┌────┴────────────────┐                   │
 HerdrHost          RexHost  ◄───────────────┘ (implemented for both, in nav)
   │                    │
herdr-plugin.toml   ~/.config/rex/plugins/nav.lua
                    (launcher and jump handler)
```

## Decisions already made

- **Keep Rust.** Rex's config runtime is gopher-lua: Lua 5.1, no regex,
  no `io.popen`, no UI widgets. A switcher with a live preview has to be
  a program in a floating layer anyway.
- **Jump mechanism:** first prototype **R2**, where nav hands the jump
  back to Lua. Fall back to **R1**, Rex Remote Control. See "The jump
  problem" and spike S0.
- **Capture and templates stay in scope, and their host glue stays local
  to nav.** That glue is layout export, creating sessions, windows and
  splits, and process info. It is *not* added to `muxhost`. It is the
  last feature phase and has a go/no-go gate: if it proves too complex on
  Rex, it is dropped for Rex (it keeps working on Herdr) and the reason
  is written here.
- **Plugins group: skipped on Rex.** Rex has no plugin system. The group
  is hidden when the host lacks the capability, and Herdr keeps it.
- **Out of scope:** `vim-herdr-navigation`, which is a separate tool and
  repo. This plan covers herdr-nav only.
- **Distribution:** use the existing GitHub releases (aarch64-apple-darwin,
  x86_64/aarch64-unknown-linux-gnu) through **mise's GitHub backend**,
  pinned in the user's dotfiles:
  ```toml
  [tools]
  "github:codingfragments/herdr-nav" = "0.7.0"
  ```
  Verified: `mise ls-remote github:codingfragments/herdr-nav` resolves
  `… 0.5.1, 0.5.2, 0.6.0`. This repo has no rolling `latest` tag, so
  nothing needs cleaning up. That is the **Rex** install path.
- **herdr install stays self-sufficient (option B, decided 2026-11 for
  all plugins, see `../herdr-zextract/rex-migration.md` "Decisions"):**
  `herdr plugin install codingfragments/herdr-nav` stays a single
  command and needs no mise. Its `[[build]]` runs `bin/fetch-binary`
  (download the release binary matching the checked-out version into
  `target/release/`, sha256-checked, fall back to `cargo build
  --release`), and every pane starts `bin/nav-launch [args…]` (prefer
  the plugin's own binary, then `herdr-nav` on `PATH`, passing the
  pane's arguments through). herdr and Rex each keep their own copy;
  update both. Reuse zextract's scripts, renamed.
- **Concept mapping:** a herdr **workspace** is a Rex **session**, a herdr
  **tab** is a Rex **window**, and a herdr **pane** is a Rex **block**.
  Labels follow the user's terms: "session", "tab", "pane".

## The jump problem (why R0 comes first)

In Rex, focusing a block inside one session is server-side
(`session.focus_block` / `session.focus_window`). What the **app** shows
is client-side: the selected session (`session.select`) and the tab
strip (`client.tab.*`). A CLI process can only drive these with
`rex -C <client> do …` (`client.perform`), and only when the app's
**Remote Control** setting is on, which it is not by default. The user's
Rex config already ran into this: `rex.client.queue` only works when the
app itself invoked the action, and does nothing for CLI clients (see
`~/.config/rex/lib/focus.lua`).

- **R2 (preferred): the jump goes back to Lua.**
  1. The Lua launcher records `ctx.client_id`, `ctx.session_id` and the
     popup's layer and block ids.
  2. nav does not jump itself. It writes a jump record (JSON) to
     `$NAV_INTENT_FILE` and exits:
     `{ "kind": "focus", "session_id": …, "window_id": …, "block_id": … }`
     or `{ "kind": "open_session", … }`.
  3. A Lua handler (`rex.on(<block/layer closed event>)`) reads the file
     with `rex.exec`, then calls `rex.session.focus_block` and queues the
     app actions (`session.select`, `client.tab.*`) for the recorded
     client.
  - Open questions: does config Lua get such an event, and may it queue
    client actions from it? (Spike S0.)
- **R1 (fallback): Remote Control.** The launcher passes
  `NAV_CLIENT_ID=<ctx.client_id>`, and nav runs
  `rex -C $NAV_CLIENT_ID do session.select session_id=…`, plus the focus
  calls, itself. Needs a one-time "enable Remote Control" step per Mac.
  nav detects a refusal and shows a clear hint.
- **R3 (not used): server-side focus only.** The app view could lag
  behind. Rejected.

In both R1 and R2 the binary has to produce the same jump record. Only
what carries it out differs, so `navigate::Jump` is one enum with two
ways of carrying it out (`ExecViaLua` / `ExecViaClient`).

## Host-coupled surface today

| Area | herdr calls (file) | Rex equivalent | Goes to |
|---|---|---|---|
| Launch context | `HERDR_PLUGIN_CONTEXT_JSON` (focused pane, `workspace_id`, `tab_id`), `HERDR_ACTIVE_PANE_ID` (`main.rs`, `source.rs::active_ids_from_env`) | Env vars from the Lua launcher: `NAV_SOURCE_BLOCK`, `NAV_SOURCE_WINDOW`, `REX_SESSION`, `NAV_CLIENT_ID`, `NAV_INTENT_FILE`. Inside the popup `REX_BLOCK` is the popup itself | muxhost core (`detect("NAV")`) |
| Topology | `workspace.list`, `tab.list`, `pane.list`, `pane.get` (`source.rs`, `main.rs`) | `rex ls --json`, then `rex session inspect S --json` for each session (windows, layout and blocks), or one `rex do` Lua script that returns everything as JSON (spike S4) | muxhost `topology` |
| Preview | `pane.read` with ANSI (`preview.rs`) | `rex capture -s S -b B --format=vt --unwrap`. nav already uses `ansi-to-tui`; expand `CSI n C` into spaces, the same fix as flash | muxhost core |
| Agents | `agent.list`, `agent.send_keys`, `pane.send_keys`, `pane.release_agent` (`source.rs`, `main.rs`) | Agents are terminal blocks with a reported status: `block call … program_status` (OSC 7501, working/blocked, "Rex for Agents"). Sending keys: `rex send` / `send-key`. Release: none, so hide the action | muxhost `topology` (+ core insert) |
| Jump | `pane.focus` (×9) | R2 or R1, see above | muxhost `navigate` |
| Close | `pane.close`, `tab.close`, `workspace.close` (`main.rs`) | `rex block close`, `rex window close`, `rex kill` | muxhost `navigate` |
| Rename | `pane.rename`, `tab.rename` | `session.set_block_label`, `rex window rename` | muxhost `navigate` |
| Dir and template open | `workspace.create`, `tab.create`, `pane.split`, `pane.send_text` (`source.rs::build_workspace_from_template`, `build_layout`) | `rex new NAME -c DIR --json`, `rex window new`, `rex split --split=right/below --ratio`, `rex send` (or one `session.new_window` with a layout tree) | **nav-local** |
| Capture | `layout.export`, `pane.process_info` (`capture.rs`) | The layout tree from `session.view` / `session inspect`, and `block call … process` for the foreground command | **nav-local** |
| Plugins | `plugin.list`, `plugin.action.invoke` | none | skipped on Rex (capability off) |
| Theme | reads `~/.config/herdr/config.toml` `[theme]` (`theme.rs`) | no Rex theme API: use `[theme]` from nav's own config, else herdr's file if present, else the default | nav-local |
| zoxide, git | `Command::new("zoxide")`, `git` | unchanged. The launcher's login `PATH` makes them findable | none |

### Config and data files (all currently under `~/.config/herdr/`)

| File | Today | New lookup order |
|---|---|---|
| `config.toml` | `$HERDR_PLUGIN_CONFIG_DIR/config.toml` | `$NAV_CONFIG`, then `~/.config/nav/config.toml`, then the legacy herdr path |
| `targets.toml` (pinned dirs, ⌘1–9 slots) | `~/.config/herdr/targets.toml`, then the plugin dir | `~/.config/nav/targets.toml`, then `~/.config/herdr/targets.toml`, then the plugin dir. **Shared by both hosts.** |
| `templates/*.yaml` | `~/.config/herdr/templates/` | `~/.config/nav/templates/`, then `~/.config/herdr/templates/`. Write to the first one that exists, else to the nav path. The template schema stays as it is: tabs, layout and commands carry no host-specific details. |

## Phases

### Phase R0: jump spike (R2, then R1), in `phase/rex-0-jump-spike`

Status: ☑ done (2026-11-19) — **R2 works; the design gets simpler
than planned.**

**Result: yes** — confirmed live in the app (the user pressed the
bound key): popup over the focused pane → action waits out the popup →
`queue session.select` → the app switches to the target session.

**Design change found by the spike — the action jumps itself:**
- config Lua has no `rex.wait` (script context only, like
  `rex.exec.start`); the synchronous CLI wait
  (`rex.exec{rex_cli, "wait", "-s", S, popup_block}`) blocks in the
  action for the popup's lifetime and works — the action stays in its
  app-invoked context the whole time.
- So the launcher does NOT need the intent-file + separate watcher:
  the action opens the popup, waits, reads nav's jump intent (the
  JSON file nav writes before exiting), and performs the jump itself
  (queue `session.select` / `client.tab.*` — app context, exactly what
  vim-herdr-navigation's crossings proved; the queue no-ops from CLI
  contexts, re-confirmed).
- Cross-session block landing: `session.focus_block` from a
  foreign-attached connection silently returns `nil` — landing on a
  specific pane in the target session goes through a child CLI call
  (`rex focus -s <target> <block>`), which attaches and works.
- Tab-strip crossings (`client.tab.*` from the app context) were
  already proven by vim-herdr-navigation (its shipped crossings,
  user-confirmed).

R1 (Remote Control) stays the documented fallback if the inline wait
ever proves problematic (long-lived actions); not needed.

- [ ] Throwaway Lua plus a throwaway shell script in the user's Rex
  config, nothing in nav yet:
  1. An action opens a floating layer that writes a jump record JSON
     and exits.
  2. Find out which event config Lua gets when the layer or block ends
     (`rex events` shows the stream). Check whether `rex.on(…)` sees it
     and whether the handler has a usable client context.
  3. From the handler, call `rex.session.focus_block` and
     `rex.client.queue("session.select", …)` / `client.tab.*` for the
     recorded client id. Check that the app really switches session and
     tab.
- [ ] If R2 fails: enable Remote Control and check that
  `rex -C <client> do session.select session_id=…` and the tab actions
  work from inside a popup.
- [ ] Write the result into the spike table (S0) and update "Decisions"
  with the chosen mechanism.

### Phase R1: adopt `muxhost` and add nav's trait groups, in `phase/rex-1-muxhost`

Status: ☑ done (2026-11-20) — no behaviour change on Herdr; all
191 tests green; tree-building tests run against muxhost's
`FakeHost` (with nav-local glue impls) and `host_ext::NoHost`.

- ☑ In `muxhost` (v0.2.0): the trait groups behind cargo features
  (`topology`: list sessions/windows/panes with labels, titles, cwd,
  focused/active flags + agents with status; `navigate`: the `Jump`
  enum, close and rename), the capability flags, and the `HerdrHost`
  implementations with today's socket calls.
- ☑ In `muxhost` (v0.2.1, tagged on origin 2026-11-20): 
  `HerdrHost::raw_request` — the documented escape hatch nav's
  host-local glue makes its herdr-only calls through (one socket
  client, muxhost's, keeps the fresh-connection contract) — and
  `HerdrHost::with_socket` (a lenient constructor: a live socket
  without a launch context, preserving nav's dev path). nav's
  `Cargo.toml` pins `git+tag = v0.2.1`.
- ☑ In nav, depend on `muxhost` with `features = ["topology",
  "navigate"]`. Every socket call in `source.rs`, `main.rs`,
  `preview.rs` (and `capture.rs`/`capture_ui.rs`, which the plan's
  file list missed — deleting `socket_client.rs` forced their hand)
  moved onto the host. `socket_client.rs` deleted.
  - nav's host layer lives in `src/host_ext/`: `NavHost` (the three
    muxhost trait groups, so `dyn NavHost` is testable with
    `muxhost::FakeHost`), `TemplateHost` + `HostExt` (the nav-local
    glue), `NavFull` (what the binary threads), `NoHost` (the dev
    fallback: no multiplexer → every call fails → "unavailable"
    stubs, exactly like an empty `HERDR_SOCKET_PATH` before).
    `TemplateHost` impls for `HerdrHost` in `src/host_ext/herdr.rs`
    with today's calls via `raw_request`.
  - Session/Agents tree building is typed (`PaneEntry`/
    `SessionInfo`/`WindowInfo`/`AgentEntry`); the active markers now
    come from the host's focused flags (they used to be re-parsed
    from `HERDR_PLUGIN_CONTEXT_JSON` — same daemon state).
  - Bug found and fixed in passing: `^c`/`^x` on an agent row never
    worked — `agent_pane_id` stripped an `agent:` prefix, but agent
    node ids are `agents:<pane_id>`. It now strips the right prefix
    (no `agent.list` round-trip needed).
- ☑ In nav, add the nav-local `TemplateHost` trait for create session,
  window and split, send text (via muxhost's `Host::insert_text`),
  layout export and process info. Implemented for `HerdrHost` in
  `src/host_ext/herdr.rs` with today's calls.
- ☑ No behaviour change on Herdr. `cargo test` stays green (191), and
  the tree-building tests run against a fake host
  (`build_tree_runs_against_muxhost_fake_host`).

Original checklist:

- [ ] In `muxhost`, a PR there, add the trait groups behind cargo
  features so zextract and flash stay lean:
  - `topology`: list sessions, windows and blocks with labels, titles,
    cwd and the focused or active flags; list agents with their status.
  - `navigate`: the `Jump` enum and its carry-out (R2 / R1), plus close
    and rename.
  - Capabilities: `caps.plugins` (Herdr true, Rex false),
    `caps.agent_release` (Herdr true, Rex false), `caps.jump_mode`.
  - `HerdrHost` implements them with today's socket calls.
- [ ] In nav, depend on `muxhost` with `features = ["topology", "navigate"]`
  and a tag pin. Move every socket call in `source.rs`, `main.rs` and
  `preview.rs` onto the host. Delete `socket_client.rs` from nav.
- [ ] In nav, add the nav-local `TemplateHost` trait for create session,
  window and split, send text, layout export and process info. Implement
  it for `HerdrHost` in `src/host_ext/herdr.rs` with today's calls.
- [ ] No behaviour change on Herdr. `cargo test` stays green, and the
  tree-building tests run against a fake host.

### Phase R2: neutral config, pinned and template paths, in `phase/rex-2-paths`

Status: ☑ done (2026-11-20) — 200 tests green.

- ☑ Lookup orders as in the table above, in the new `src/paths.rs`:
  config.toml `$NAV_CONFIG` → `~/.config/nav/` → the legacy plugin
  dir; targets.toml nav → herdr → plugin dir (shared by both hosts);
  templates nav → herdr dir, write to the first existing dir else
  nav's. Pin/unpin writes stay on a legacy file when it is the only
  one that exists (no data fork — existing setups keep working
  unchanged).
- ☑ Theme: `[host.<host>].theme` (a name), then `[theme]`/
  `[theme.custom]` in nav's config (only when that config declares a
  theme section — a nav config without one still follows herdr's),
  then herdr's config, then the default. `theme::load` takes the host
  name.
- ☑ Optional `[host.rex]` / `[host.herdr]` override tables with
  `groups` and `theme` only; `resolved_groups(host)` and
  `host_theme(host)` resolve them (unknown hosts — e.g. the dev
  `NoHost` — use the top-level values).
- ☑ Docs: `doc/config-reference.md`, `doc/templates.md`,
  `doc/env-vars.md` (`NAV_HOST`, `NAV_CONFIG`), README, commented
  examples in `config.example.toml`.
- Also: the path logic is unit-tested with throwaway HOMEs
  (`src/paths.rs`), and env-mutating tests now share an
  `ENV_LOCK` (cargo test runs them in parallel threads — they were
  racing on `HOME`).

Original checklist:

- [ ] Lookup orders as in the table above. Pinning (`write_pin`,
  `unpin`) and capture writes go to the nav paths unless a legacy file is
  the only one that exists.
- [ ] Theme: `[theme]` in nav's config, then herdr's config, then the
  default.
- [ ] Optional `[host.rex]` / `[host.herdr]` override tables (start with
  `groups` and `theme` only).
- [ ] Docs: `doc/config-reference.md`, `doc/templates.md`, `doc/env-vars.md`,
  README.

### Phase R3: Rex switcher (Session, Agents, Pinned, zoxide, preview), in `phase/rex-3-rex-switcher`

Status: ☑ done (2026-11-20) — live-verified against the running Rex
server (tree, pane titles, live vt preview with colours, plugins
hidden); 201 tests green. **Spike S4 result: one `rex do` script**
(piped on stdin) collects the whole topology — session.list +
per-session session.attach (returns the view inline; session.view
without attachment returns nil) + per-block block.method title/pwd/
program_status — ~50 ms for several sessions, cached in muxhost
(mutation-invalidated, 250 ms TTL). Four list-methods-each-run would
blow the popup budget; `HostTopology::topology_snapshot` (new in
muxhost v0.3.0, default-assembled for herdr/FakeHost) is the one-call
answer.

- ☑ `RexHost` topology in muxhost v0.3.0 (tagged): sessions/windows/
  blocks from the snapshot, floating-layer blocks left out, terminal
  titles replacing the uninformative "shell" labels, cwd from the
  file:// pwd (path only). Active = `REX_SESSION`; focused = the
  source block.
- ☑ Agents: blocks with non-empty `program_status.records` map
  blocked→waiting/working/idle. **Caveat:** no live block reported a
  status during the spike (records `[]`/`{}`), so the record shape is
  parsed defensively (array or object, `status` field) — verify
  against the user's real agents in R4 live testing.
- ☑ Pinned/zoxide unchanged; dir-open under Rex = `rex new -c DIR` +
  a delegated FocusPane jump (the intent file the launcher carries
  out). The provider path now jumps to the created pane explicitly
  (herdr relied on the daemon's auto-switch; Rex needs the intent).
- ☑ Preview: RexHost read_pane (vt capture) + flash's `vt_prepare`
  (CSI-n-C → spaces, SGR kept) in nav's preview, tested against the
  recorded fixture `tests/fixtures/rex/capture-vt.txt`; live-verified
  (captured pane content renders with colours).
- ☑ Plugins group hidden when `!caps.plugins` (Rex + the dev NoHost);
  the `@plugins` query filter flashes "not available on Rex".
- ☑ Performance: S4 above — ~50 ms per snapshot, well under 150 ms.
- ☑ `--version` and `--host-api` (integer, 1) for launcher preflight.
- ☑ Herdr install, option B: `bin/fetch-binary` (asset
  `herdr-nav-<triple>.tar.gz`, sha256-checked, source fallback) +
  `bin/nav-launch` (target/release → PATH, pins `NAV_HOST=herdr`);
  all three panes go through `nav-launch`.
- ☑ muxhost v0.3.1: `RexHost::run` + `session` (the raw-CLI escape
  hatch nav's Rex glue calls `rex new`/`block call pwd`/`send-key`
  through — same contract as `HerdrHost::raw_request`).
- ☑ Jump delegation (R0's design): `host_ext::jump` writes the intent
  record on `JumpOutcome::Delegated` to `$NAV_INTENT_FILE` (fallback
  `~/.config/nav/intent.json`); the final user jumps go through it,
  template building's intermediate focus ops call the raw host jump
  and ignore a delegated outcome.

Original checklist:

- [ ] `RexHost` topology lives in `muxhost` (a PR there), and nav bumps
  the tag:
  - The Session group lists every session, then its windows, then its
    blocks, with floating-layer blocks (including the popup itself) left
    out.
  - Active and focused markers come from `active_window_id` and
    `focused_block_id`.
- [ ] The Agents group lists blocks whose `program_status` is non-empty,
  with the status dot mapped to working/blocked/idle, plus send keys.
  Hide the release action.
- [ ] Pinned and zoxide work unchanged. Opening a directory as a new
  session uses the nav-local `TemplateHost` with the hardcoded
  one-window, one-block default, which is `rex new NAME -c DIR` followed
  by a jump of kind `open_session`.
- [ ] The preview uses vt capture with `ansi-to-tui` and the `CSI n C`
  expansion, tested against recorded Rex fixtures in
  `tests/fixtures/rex/`.
- [ ] The Plugins group is hidden when `!caps.plugins`, and the `@plugins`
  query filter shows a "not available on Rex" hint.
- [ ] Performance: building the tree on a server with many sessions and
  blocks should take under 150 ms. Spike S4 decides between many CLI
  calls and one `rex do` Lua script returning JSON.
- [ ] `--version` and `--host-api` (an integer, starting at 1) so the
  launcher can check compatibility.
- [ ] **Herdr install, option B** (copy from zextract's Phase 3):
  - `bin/fetch-binary` with the asset name `herdr-nav-<triple>.tar.gz`;
    `[[build]] command = ["sh", "bin/fetch-binary"]`.
  - `bin/nav-launch "$@"`: exec `./target/release/herdr-nav` if present,
    else `herdr-nav` on `PATH`, else an install hint. All three panes
    use it: `["sh", "bin/nav-launch"]`, `["sh", "bin/nav-launch",
    "capture"]`, `["sh", "bin/nav-launch", "--seed", "@agents"]`.
  - Launcher actions pass `--env NAV_HOST=herdr`.

### Phase R4: Rex launcher and jump handler (`rex/nav.lua`), in `phase/rex-4-lua-launcher`

Status: ☑ done — **working live, user-confirmed** (2026-11-20;
fixed three times that day, all silent-edit no-ops found by driving
the real popup remotely (`rex do nav` + `rex send`) with stderr
traces; the final verified sequence is documented in `doc/rex.md` — third fix: a select
made while the popup's layer is still open is undone when the app's
focus logic snaps back on layer close (observed live: select exit=0,
app attached, view stayed). The carry-out now fires a SECOND select
from a detached nohup child 0.5 s after nav exits — the layer is
closed, the state is clean, the deferred select wins (survival +
switch verified live). The select also carries window_id: the app's
tab strip follows client actions, not server focus, so cross-window
jumps need it: the R3 jump-swap edit had
silently no-op'd for the Session pane-jump and the Agents invoke —
they still called the raw `host.jump`, whose Delegated outcome was
ignored, so Enter closed the popup without jumping at all. Found by
driving the real launcher popup remotely (`rex do nav` + `rex send`
into its block) with lifecycle traces on stderr. Also muxhost v0.3.4:
block-scoped calls (capture/send/pwd/split/send-key) resolve the
block's OWN session from the topology — a pane in another session
was "block target not found" (broken previews). Diagnostics:
/tmp/herdr-nav-jump.log + the stderr traces.) — **redesigned to the
R1 fallback the same day**: the
R0 inline-wait design froze the app's key delivery for the popup's
lifetime (the popup rendered but accepted no key presses — a blocking
`rex.exec` in an action holds the app's Lua, exactly the
"problematic long-lived actions" case the plan reserved the fallback
for). The launcher is now non-blocking (zextract-shaped); the
switcher carries the jump out itself: `rex focus -s S B` (muxhost
v0.3.2's RexHost::jump server side) + `rex -C $NAV_CLIENT_ID do
session.select` (Remote Control — one-time enable in the app; a
refusal keeps the popup open with a hint), Esc re-focuses
NAV_SOURCE_BLOCK on exit. **Awaiting the live jump test** with Remote
Control enabled.

- ☑ Action `nav{ seed = "string?" }` in `rex/nav.lua` (the shipped
  reference; installed at `~/.config/rex/plugins/nav.lua` +
  `require("plugins.nav")`): reads the source block/window before the
  layer takes focus, opens the 80%×80% floating layer with the login
  PATH + `NAV_HOST=rex` `REX_CLI` `REX_SESSION` `NAV_SOURCE_BLOCK`
  `NAV_SOURCE_WINDOW` `NAV_CLIENT_ID` `NAV_INTENT_FILE`, preflights
  `command herdr-nav` (mise hint) and `--host-api` ≥ 1.
- ☑ The R0-simplified design: no config-load jump handler — the action
  waits the popup out with the inline CLI wait (`rex.exec` — the
  action stays app-invoked, which is what lets `rex.client.queue`
  reach the app), then reads the intent record and performs the jump:
  `session.select` for the app, `rex focus -s S B` for the server
  side. Esc-close re-focuses the source pane. The intent file is
  deleted before each open, so a stale record can never jump.
- ☑ Bindings: `ctrl+s>s` (switcher) + `ctrl+s>a` (agents seed) —
  `ctrl+k` stays vim-herdr-navigation's, `ctrl+s>w` is
  session.switch; no conflicts in the user's conf/keys.lua. The R0
  spike (`conf/zz-nav-r0.lua`) is deleted from the user's config.
- ☑ Docs: `doc/rex.md` + README Hosts section.
- ☑ nav side: focus intents now carry `session_id` too (resolved from
  the cached topology snapshot), so the launcher doesn't need to
  resolve block→session.
- Dev note: `~/.local/bin/herdr-nav` symlinks to this checkout's
  `target/release/herdr-nav` for the live test (the login PATH finds
  it); R6 replaces that with the mise pin once v0.7.0 releases —
  the RELEASE v0.6.0 binary is too old for the launcher's
  `--host-api` preflight by design.

Original checklist:

A reference implementation ships in this repo as `rex/nav.lua`. The user
copies or symlinks it to `~/.config/rex/plugins/nav.lua` and adds
`require("plugins.nav")` to `init.lua`. Its shape follows the user's
`~/.config/rex/plugins/edit_scrollback.lua` and `lib/popup.lua`.

- [ ] Action `nav{ seed = "string?" }`:
  1. Read the source block and window with `rex.session.view` before the
     layer takes focus.
  2. Open a floating layer at 80% × 80% with `shell = "none"` and this
     command:
     `env PATH=<login PATH> NAV_HOST=rex REX_CLI=<abs> REX_SESSION=<sid> NAV_SOURCE_BLOCK=<b> NAV_SOURCE_WINDOW=<w> NAV_CLIENT_ID=<cid> NAV_INTENT_FILE=<tmp> herdr-nav [--seed <seed>]`
  3. Preflight: check `command herdr-nav` (hint
     `mise use -g github:codingfragments/herdr-nav`) and that
     `--host-api` is at least the expected value.
- [ ] Jump handler (R2): register it once at config load. It finds the
  matching record by layer id, carries out the jump, deletes the file,
  and ignores stale or unknown records. With R1 the handler is a no-op
  and nav carries out the jump itself.
- [ ] Bindings: ~~`ctrl+k`~~ **`ctrl+s>s`** opens the switcher —
  `ctrl+k` belongs to vim-herdr-navigation's Rex port (navigate up,
  see `../vim-herdr-navigation/rex-migration.md` D3). An agents-only
  binding uses `{ seed = "@agents" }`. Capture gets a binding in Phase
  R5. Check for conflicts with the user's `conf/keys.lua` (for example
  `ctrl+s>w` is `session.switch`).
- [ ] Docs: `doc/rex.md` and a README "Hosts" section.

### Phase R5: templates and capture on Rex, in `phase/rex-5-templates-capture` (go/no-go)

Status: ▐ in progress (2026-11-20, started early on user direction:
"the templates should be different from the herdr templates"):
- ☑ Template dirs are host-scoped: herdr keeps the R2 chain
  (`~/.config/nav/templates/` → the legacy herdr dir); **Rex gets its
  own set** — `~/.config/nav/rex/templates/` — read and write, no
  herdr fallback. Capture writes follow the host's chain.
- ☑ Fixed the Rex template build (found live, 2026-11-20):
  `rex split --ratio` is an INTEGER PERCENT (nav's 0–1 fraction was a
  flag parse error — "the split doesn't work at all"); the new block
  comes back as top-level `block_ids[0]` (nav parsed a nested
  `block.pane_id`-style object); `rex window new --json` is flat
  (`window_id` + `block_ids`), same fix.
- ☑ muxhost v0.3.3: `RexHost::run` drops the cached topology snapshot
  after every escape-hatch call, so a session created by `rex new` is
  visible to the jump that follows (the stale 250 ms cache made the
  dir-open jump fail with "no session holds block").
- ☐ The rest of R5 (layout export via `session.view`, process info via
  `block call process`, capture fixtures, the go/no-go gate) is still
  open.

Original checklist:

Status: ☐ not started

- [ ] **Gate first:** in this phase's spike, map a 3-window session with
  nested splits:
  - `session inspect` layout to the template schema (direction, ratio,
    cwd, command), using `block call … process` for the foreground
    command and `pwd` for the cwd;
  - and back, through `rex new`, `window new`, `split --ratio` and
    `send`, or through one `session.new_window` with a layout tree.
  - If the round trip is not faithful enough, or the work estimate goes
    past about 2 phases, **stop**. Hide capture on Rex
    (`caps.capture = false`), keep opening templates if that part works,
    and record why here.
- [ ] Opening a template on Rex: `RexTemplateHost::build_workspace`
  creates the session, then each window, then the split tree with
  ratios, then the commands, then the labels. It ends with a jump of
  kind `open_session`.
- [ ] Capture on Rex: `RexTemplateHost::export_layout` reads the layout
  tree of the current session into nav's `Template`, using the existing
  cwd policy, best-effort command and comment annotations. A Lua action
  `nav_capture` with the binding `ctrl+s>ctrl+t` opens it as
  `herdr-nav capture`.
- [ ] Fixtures: recorded `session inspect` JSON in
  `tests/fixtures/rex/layout-*.json`, with round-trip tests.

### Phase R6: distribution and release `v0.7.0`, in `phase/rex-6-distribution` then `release/0.7.0`

Status: ☐ not started

- [ ] Check that `mise use -g github:codingfragments/herdr-nav@<ver>`
  works on macOS arm64 and on one Linux host.
- [ ] Rewrite the README Install section: "Herdr" (`herdr plugin install`,
  no Rust needed for tagged versions) and "Rex" (mise), with
  `cargo install --git` as the build-from-source fallback.
- [ ] Version bump and CHANGELOG entries: "Rex support", "muxhost",
  "neutral config, pinned and template paths", "herdr plugin install
  downloads the release binary". Tag following the CLAUDE.md release process.
- [ ] Update the user's Rex `migration-plan.md` (`~/.config/rex/`, in
  the dotfiles repo, as a separate PR there): move herdr-nav (switcher,
  pinned dirs, nav-agents and nav-capture if Phase R5 went ahead) from
  "impossible" to "done".

## Spike checklist (answer these early and record the results here)

| # | Question | Result |
|---|---|---|
| S0 | Jump: does config Lua get a layer/block-closed event, can it queue `session.select` / `client.tab.*` for a recorded client, and does the app follow? If not, does R1 (Remote Control plus `rex -C`) work from a popup? | ☑ 2026-11-20, **R1 (Remote Control) is the shipped design.** The first answer (the launcher waits out the popup and queues `session.select` itself) worked in the spike but froze the app's key delivery for the popup's lifetime, so the launcher now returns at once and nav does the jump: `rex focus` (muxhost) + `rex -C <client> do session.select session_id window_id`, repeated from a detached child after the layer closes, then `rex focus` again last. Sequence: `doc/rex.md`; contract: muxhost `doc/navigate.md`. |
| S1 | Focus returning after the popup closes, when nav does *not* jump (Esc): back on the source block? | ☑ 2026-11-20: not on its own. nav runs `rex focus -s $REX_SESSION $NAV_SOURCE_BLOCK` on a no-jump exit (R4). Implemented and exercised in the remote-driven popup tests; not separately confirmed by the user. |
| S2 | `program_status` across blocks: what does it report for pi, Claude Code and others? Is "idle" distinguishable from "no agent"? | ☐ |
| S3 | `session inspect --json`: how are floating-layer blocks marked, and where do block labels, titles and cwd live? | ☑ 2026-11-20: floating-layer blocks are in layers with `kind != "tiled"`; labels in `session.view`/`block ls` (new blocks are labelled `shell`); terminal titles, cwd and status via `block call title|pwd|program_status`. In muxhost `doc/topology.md`. |
| S4 | Cost of building the tree: N × `rex` CLI calls compared with one `rex do script.lua` returning JSON, measured with 5 sessions and 40 blocks | ☑ 2026-11-20: one `rex do -` script (session.list + per-session attach + per-block methods) takes ~50 ms for several sessions; N× CLI calls would be dozens of process spawns. Shipped in muxhost v0.3.0 with a 250 ms cache. Not re-measured at 5 sessions / 40 blocks. |
| S5 | Templates: is `session.new_window` with a layout tree (ratios, commands, cwd) enough to build a window in one call? | ☐ |
| S6 | vt preview edge cases (after a clear, full-screen apps): shared with flash S1 | ☐ |
| S7 | Does Herdr's popup PATH include mise installs (bare `herdr-nav` in the manifest)? Shared with zextract S5 | ☑ answered in zextract S5 (yes, via the mise shims dir); with option B only relevant for `nav-launch`'s PATH fallback |

## Rex facts an agent needs (verified 2026-11, rex 0.1.0-preview.19, api rev 14)

- **CLI commands:**
  - `rex ls --json` and `rex session inspect S --json` (`active_window_id`,
    `windows[].window_id`, `windows[].focused_block_id`, layouts)
  - `rex block ls -s S [-w W] --json` / `-A`
  - `rex capture -s S -b B [--unwrap] [--format text|html|vt]`
  - `rex send` / `send-key`
  - `rex new NAME -c DIR --json`, `rex window new -s S NAME`,
    `rex split -s S -b B --split=right|below --ratio N --json`
  - `rex window rename`, `rex block close`, `rex window close`, `rex kill`
  - `rex block call -s S -b B process|pwd|title|program_status|size`
- **Server API** (`rex api list`): `session.focus_block`,
  `session.focus_window`, `session.new_window` (layout tree),
  `session.set_block_label`, `client.perform` (needs Remote Control),
  `client.list`, `session.list_clients`.
- **Client actions** (app, `rex -C <client> actions`): `session.select`,
  `client.tab.next/previous`, `session.switch`. A CLI client cannot
  queue these for the app. Config actions invoked by the app can, with
  `rex.client.queue` (see `~/.config/rex/lib/focus.lua`).
- **Environment:** inside a Rex terminal, `REX_SESSION` and `REX_BLOCK`
  are set. In the popup, `REX_BLOCK` is the popup's own block. The Rex
  server runs with launchd's minimal PATH, and the rex CLI lives at
  `~/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex`.
  The Lua side resolves both and passes them on as `PATH` and `REX_CLI`.
- **Lua config:** `rex.action{name,title,args,run}`,
  `rex.bind(key, action, args?)`, `rex.session.view`,
  `rex.session.new_layer{…}`, `rex.exec{argv}`, `rex.json`,
  `rex.on(event, fn)`, `rex.client.queue(action, args?)`. There are no UI
  widgets, no regex and no `io.popen`.
- **Sister plans:** `../herdr-zextract/rex-migration.md` and
  `../herdr-flash/rex-migration.md`, which owns the creation of
  `muxhost`.
- **Headless test recipe:**
  `rex new selftest && rex do -s selftest selftest.lua && rex session kill selftest`.

## Out of scope

- `vim-herdr-navigation`: a different tool, not part of this plan.
- A Plugins group on Rex, until Rex has plugins. Revisit then.
- Windows and Intel Mac release binaries (the policy is unchanged).
