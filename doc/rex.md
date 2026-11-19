# Running herdr-nav under Rex

The same `herdr-nav` binary runs under [Rex](https://rex.dev) (from the
R3 migration on): the switcher's tree, previews and groups all work,
with two host differences — the Plugins group is hidden (Rex has no
plugin system), and jumps are **delegated**: a CLI process cannot queue
the app's view actions (`session.select` / `client.tab.*`), so the
switcher writes a jump record and the launcher carries it out.

## Install

1. The binary, with [mise](https://mise.jdx.dev):

   ```sh
   mise use -g github:codingfragments/herdr-nav
   ```

   (The launcher checks for it and shows an install hint otherwise; it
   also checks `--host-api` so a too-old binary fails loudly.)

2. The launcher — copy [`rex/nav.lua`](../rex/nav.lua) to
   `~/.config/rex/plugins/nav.lua` and add to `init.lua`:

   ```lua
   require("plugins.nav")
   ```

3. Bindings (prefix `ctrl+s`):

   | Key | Action |
   |---|---|
   | `ctrl+s>s` | the switcher |
   | `ctrl+s>a` | agents only (opens seeded with `@agents`) |

   `ctrl+k` is [vim-herdr-navigation](https://github.com/codingfragments/vim-herdr-navigation)'s
   and stays free; `ctrl+s>w` is `session.switch`. Capture gets a
   binding in Phase R5.

## How the jump works (Remote Control)

**Who owns what** — the jump is split by responsibility:

| Half | Owner | What it does |
|---|---|---|
| Server side | **muxhost** (`RexHost::jump`) | resolves the target block's session from the topology snapshot, runs `rex focus -s SESSION BLOCK`, returns `JumpOutcome::Delegated` (= "server state done; the app's view is the tool's to move") |
| App side | **herdr-nav** (`host_ext::jump` → `carry_out_rex_select`) | writes the jump record, runs the sync select (error detection), and fires the deferred switch |
| Popup | **rex/nav.lua** | opens the floating layer with the context env vars and returns immediately — a blocking action freezes the app's key delivery |

**The full sequence** — you are in pane B1 (session B), you select pane
A12 (session A, window W) and press Enter:

1. **Server side, immediately** (in the popup process): `rex focus -s A
   A12` — session A's active window becomes W and its focused block
   becomes A12. The app still shows B at this point.
2. **Jump record** (`$NAV_INTENT_FILE`):
   `{"kind":"focus","block_id":"A12","session_id":"A","window_id":"W"}`
   — the window matters: the app's tab strip follows client actions,
   not server focus.
3. **Sync select** — `rex -C <NAV_CLIENT_ID> do session.select
   session_id=A window_id=W`. This one exists for the error path: a
   refusal (Remote Control off) keeps the popup open with a hint.
   It does **not** move the app — a select made while the popup's
   layer is still open is undone when the app's focus logic snaps
   back on layer close.
4. **nav exits** → the popup closes → the app briefly refocuses B1.
5. **~0.5 s later**, a detached child (nohup'd, it outlives nav, and
   escapes the popup's process group so the layer's teardown cannot
   kill it) runs the real switch:
   - `rex -C <app> do session.select session_id=A window_id=W` — the
     app switches to session A and window W, in clean state;
   - **then** `rex focus -s A A12` — the block focus is re-asserted
     *after* the select, because the app's own select may focus the
     window's previously-remembered block. The focus must be the last
     action, or you land on whatever the app remembered instead of
     the pane you picked.
6. **End state**: the app shows session A, window W, focus on A12,
   keyboard in A12.

Esc (no jump): the switcher runs `rex focus` back onto
`NAV_SOURCE_BLOCK` — the pane you opened it from — on its way out.

Every carry-out appends to `/tmp/herdr-nav-jump.log` (client, session,
window, exit code) — the popup is gone a moment later, so that log is
where a failed jump leaves its evidence.

> History: the original R0 design had the launcher wait out the popup
> and queue the app actions itself — no Remote Control needed — but the
> blocking wait froze the app's key delivery for the popup's lifetime
> (the popup rendered but accepted no key presses; observed live).
> That is why the plan's R1 fallback is the active mode. Along the way
> two more ordering traps were found and are baked into the sequence
> above: the select being undone by the layer-close focus return
> (fixed by the deferred child), and the app landing on a remembered
> block instead of the chosen one (fixed by re-asserting focus after
> the select).

## Differences under Rex

- **Plugins group** — hidden (`caps.plugins` is false); the `@plugins`
  query filter flashes "not available on Rex".
- **`^x` detach agent** — hidden (no agent-release action).
- **Templates/capture** — opening a pinned/zoxide dir builds the
  hardcoded one-window-one-block default (`rex new`); full templates
  and `herdr-nav capture` land in Phase R5.
- Config and data files live in `~/.config/nav/` with the herdr
  locations as fallbacks — see [`doc/config-reference.md`](config-reference.md).
