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

## How the jump works

`rex/nav.lua`'s `nav` action (R0 spike, verified live):

1. Reads the focused block + window (`rex.session.view`) before the
   layer takes focus — inside the popup, `REX_BLOCK` is the popup
   itself, so the source is passed explicitly as `NAV_SOURCE_BLOCK`.
2. Opens a floating layer at 80% × 80% running the binary with
   `NAV_HOST=rex`, `REX_CLI`, `REX_SESSION`, `NAV_INTENT_FILE`, and the
   login `PATH` (the Rex server runs with launchd's minimal PATH).
3. Waits the popup out with a synchronous CLI `rex wait` (config Lua
   has no `rex.wait`), staying in the app-invoked context — which is
   what lets it queue the app's actions.
4. Reads the jump record the switcher wrote:
   `{"kind":"focus","session_id":…,"block_id":…}` or
   `{"kind":"open_session","session_id":…}`, then
   - `session.select` for the **app** (`rex.client.queue`),
   - `rex focus -s SESSION BLOCK` for the server side
     (`session.focus_block` from a foreign-attached connection silently
     does nothing — R0 finding).
5. No record (Esc): re-focuses the source pane.

The record is deleted before each popup opens, so a stale file from a
crashed run can never jump.

## Differences under Rex

- **Plugins group** — hidden (`caps.plugins` is false); the `@plugins`
  query filter flashes "not available on Rex".
- **`^x` detach agent** — hidden (no agent-release action).
- **Templates/capture** — opening a pinned/zoxide dir builds the
  hardcoded one-window-one-block default (`rex new`); full templates
  and `herdr-nav capture` land in Phase R5.
- Config and data files live in `~/.config/nav/` with the herdr
  locations as fallbacks — see [`doc/config-reference.md`](config-reference.md).
