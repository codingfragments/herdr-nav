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

The launcher (`rex/nav.lua`) is **non-blocking** — it opens the popup
and returns, so the app's key delivery keeps working while the popup is
open. The switcher carries the jump out itself on Enter:

1. **Server side**: `rex focus -s SESSION BLOCK` (muxhost's
   `RexHost::jump`; `session.focus_block` from a foreign-attached
   connection silently does nothing — R0 finding).
2. **App side**: `rex -C <NAV_CLIENT_ID> do session.select session_id=…`
   — asks the app to follow. **This needs the app's Remote Control
   setting enabled (once, in Rex's settings).** Without it the popup
   stays open with a hint telling you to enable it.

Esc (no jump): the switcher runs `rex focus` back onto
`NAV_SOURCE_BLOCK`, the pane you opened it from.

> History: the original R0 design had the launcher wait out the popup
> and queue the app actions itself — no Remote Control needed — but the
> blocking wait froze the app's key delivery for the popup's lifetime
> (the popup rendered but accepted no key presses; observed live).
> That's why the plan's R1 fallback is the active mode. The jump record
> is still written to `$NAV_INTENT_FILE` (diagnostics/fallback), and
> deleted before each open so a stale file never lingers.

## Differences under Rex

- **Plugins group** — hidden (`caps.plugins` is false); the `@plugins`
  query filter flashes "not available on Rex".
- **`^x` detach agent** — hidden (no agent-release action).
- **Templates/capture** — opening a pinned/zoxide dir builds the
  hardcoded one-window-one-block default (`rex new`); full templates
  and `herdr-nav capture` land in Phase R5.
- Config and data files live in `~/.config/nav/` with the herdr
  locations as fallbacks — see [`doc/config-reference.md`](config-reference.md).
