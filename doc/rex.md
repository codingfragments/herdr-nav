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

   A binary installed with mise is only found if you also add a
   `lib/env.lua`; see [Environment](#environment-the-rex-cli-and-your-login-path)
   below.

3. Bindings (the same keys as under herdr):

   | Key | Action | herdr equivalent |
   |---|---|---|
   | `ctrl+s>s`, `cmd+k` | the switcher | `prefix+s`, `super+k` |
   | `ctrl+s>a`, `cmd+shift+k` | agents only (opens seeded with `@agents`) | `super+shift+k` |
   | `ctrl+s>ctrl+t` | capture the session as a template | `prefix+ctrl+t` |

   `ctrl+k` is [vim-herdr-navigation](https://github.com/codingfragments/vim-herdr-navigation)'s
   and stays free; `ctrl+s>w` is `session.switch`.

## Environment: the `rex` CLI and your login PATH

The launcher needs two things from your Rex config, because the Rex
server is started by the macOS app with a minimal `PATH`
(`/usr/bin:/bin:/usr/sbin:/sbin`, no Homebrew, no mise shims):

| It needs | Without it |
|---|---|
| the **`rex` CLI's absolute path** | the popup opens, but `herdr-nav` cannot call back into Rex |
| a **login `PATH`** that contains `herdr-nav` | the popup says `nav: herdr-nav not found on this host` |

On a standard macOS install the CLI is found with no setup (the launcher
falls back to the app bundle under your own `$HOME`). Its fallback
`PATH`, though, has **no mise shims and no `~/.local/bin`**, so a binary
installed with `mise use -g` is **not found until you add a
`~/.config/rex/lib/env.lua`**. One file serves every Rex launcher:

```sh
mkdir -p ~/.config/rex/lib
curl -fsSL https://raw.githubusercontent.com/codingfragments/muxhost/main/examples/rex/lib/env.lua \
  -o ~/.config/rex/lib/env.lua
```

That sample finds the CLI and asks your login shell (fish, zsh, bash, …)
for its `PATH`, with nothing tied to an account or a home directory. To
pin values by hand instead, to check what it resolves to, or to
troubleshoot, see the muxhost guide:
[The Rex environment for launchers](https://github.com/codingfragments/muxhost/blob/main/doc/rex-environment.md).
Reload your config afterwards (`ctrl+s>shift+r`).

## How the jump works (Remote Control)

> nav's host glue is in [`doc/host-layer.md`](host-layer.md); the shared
> layer (every muxhost feature, herdr vs Rex) is in the
> [muxhost reference](https://github.com/codingfragments/muxhost/tree/main/doc).

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
- **Templates/capture** — both work. Templates are Rex's own set
  (`~/.config/cf-nav/rex/templates/`, no herdr fallback). `capture`
  (the wizard, `ctrl+s>ctrl+t`, or `herdr-nav capture` with flags)
  reads the session's windows, layouts, cwds and **best-effort
  commands — name-only**: Rex reports the foreground process name but
  not its arguments, so a running `nvim src/ --clean` is captured as
  `nvim` with a `best-effort: verify` annotation.
- Config and data files live in `~/.config/cf-nav/` with the herdr
  locations as fallbacks — see [`doc/config-reference.md`](config-reference.md).
