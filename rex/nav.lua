-- herdr-nav for Rex — the launcher.
--
-- Opens the herdr-nav switcher in a floating layer over the pane you
-- are in: browse/search sessions, windows, panes, agents, pinned dirs
-- and zoxide entries, with a live preview. The binary does the work
-- (host layer: muxhost); this file finds the source pane, checks the
-- binary is usable, starts it with the right context, waits it out,
-- and carries the jump out: nav writes a jump intent (it cannot queue
-- the app's actions from a CLI process), and this launcher — an
-- app-invoked action, still in the app's context after the inline
-- wait — performs it (session.select for the app, rex focus for the
-- server side). Verified live by the R0 spike (rex-migration.md).
--
-- Install: copy this file to ~/.config/rex/plugins/nav.lua and add
--   require("plugins.nav")
-- to init.lua. Bindings (herdr parity, prefix ctrl+s):
--   ctrl+s>s  the switcher
--   ctrl+s>a  agents only (opens seeded with the @agents group tag)
-- Capture gets a binding in Phase R5.
-- Check conf/keys.lua for conflicts before changing them (ctrl+s>w is
-- session.switch, ctrl+k belongs to vim-herdr-navigation).
--
-- Self-contained: uses lib.env (login PATH, rex CLI path) when present
-- and falls back to inline equivalents. (https://github.com/codingfragments/herdr-nav)

-- ── Environment helpers (lib/env.lua when present, else inline) ──
local login_path = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
local rex_cli = "rex"
do
  local ok, env = pcall(require, "lib.env")
  if ok and env and env.path and env.path ~= "" then login_path = env.path end
  if ok and env and env.rex_cli and env.rex_cli ~= "" then
    rex_cli = env.rex_cli
  else
    local bundle = (os.getenv("HOME") or "") ..
      "/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex"
    local f = io.open(bundle, "r")
    if f then f:close() rex_cli = bundle end
  end
end

-- The host-api version this launcher needs (the binary's
-- `--host-api`); bump when the launcher starts relying on something
-- new.
local NEED_HOST_API = 1

-- Where the switcher writes its jump record (passed as
-- NAV_INTENT_FILE; the launcher deletes it before opening the popup,
-- so a stale record from a crashed run can never jump).
local INTENT = "/tmp/herdr-nav-intent.json"

-- POSIX sh, so it works on macOS and Linux hosts alike (the layer may
-- run on a remote host). Checks the binary, then execs it with the
-- seed appended. Env comes from `env` before sh, so the check and the
-- run see the same context.
local PREFLIGHT = [[
if ! command -v herdr-nav >/dev/null 2>&1; then
  printf '\n  nav: herdr-nav not found on this host.\n\n'
  printf '  Install it:  mise use -g github:codingfragments/herdr-nav\n\n'
  printf '  Press Enter to close. '
  read -r _
  exit 0
fi
api=$(herdr-nav --host-api 2>/dev/null || echo 0)
if [ "$api" -lt %d ]; then
  printf '\n  nav: herdr-nav is too old (host-api %%s, need %d).\n\n' "$api"
  printf '  Update it:  mise up herdr-nav\n\n'
  printf '  Press Enter to close. '
  read -r _
  exit 0
fi
exec herdr-nav --seed "$1"
]]

local function popup_command(sid, block, window, client_id, seed)
  return {
    "env",
    "PATH=" .. login_path,
    "NAV_HOST=rex",
    "REX_CLI=" .. rex_cli,
    "REX_SESSION=" .. sid,
    "NAV_SOURCE_BLOCK=" .. block,
    "NAV_SOURCE_WINDOW=" .. window,
    "NAV_CLIENT_ID=" .. client_id,
    "NAV_INTENT_FILE=" .. INTENT,
    "/bin/sh", "-c", (PREFLIGHT):format(NEED_HOST_API, NEED_HOST_API),
    "sh", seed,
  }
end

-- Read one string field out of the compact JSON intent (serde_json
-- writes {"kind":"focus",...}; plain Lua patterns, no regex needed).
local function json_field(text, key)
  return text:match('"' .. key .. '"%s*:%s*"([^"]*)"')
end

-- Carry the jump out. Both kinds select the session in the APP (a CLI
-- process cannot — that is why the jump is delegated here); a focus
-- record additionally focuses the target block server-side through a
-- child `rex focus` call (session.focus_block from a foreign-attached
-- connection silently does nothing — R0 finding).
local function jump(intent, ctx)
  local kind = json_field(intent, "kind")
  if kind == "focus" or kind == "open_session" then
    local session = json_field(intent, "session_id")
    if not session or session == "" then
      return "nav: intent without a session_id — not jumping"
    end
    pcall(rex.client.queue, "session.select", { session_id = session })
    if kind == "focus" then
      local block = json_field(intent, "block_id")
      if block and block ~= "" then
        pcall(rex.exec, { rex_cli, "focus", "-s", session, block })
      end
    end
    return "jumped"
  end
  return nil -- unknown or no record
end

rex.action{
  name  = "nav",
  title = "nav: switcher",
  description = "Open the herdr-nav switcher over the focused pane: fuzzy-find and jump to sessions, windows, panes, agents, pinned dirs and zoxide entries, with a live preview. Optional seed pre-fills the query (e.g. \"@agents\").",
  args  = { seed = "string?" },
  run   = function(ctx, args)
    -- The source pane: what the user is looking at, read BEFORE the
    -- layer takes focus. Inside the popup, REX_BLOCK is the popup
    -- itself, so this must be passed explicitly.
    local v = rex.session.view{ session_id = ctx.session_id }
    local source_block, window
    for _, w in ipairs(v.windows or {}) do
      if w.window_id == v.active_window_id then
        source_block = w.focused_block_id
        window = w.window_id
      end
    end
    if not source_block or not window then
      error("no focused pane to switch from", 0)
    end

    os.remove(INTENT) -- a stale record can never jump

    local seed = args.seed or ""
    local layer, err = rex.session.new_layer{
      session_id = ctx.session_id,
      bounds     = { 0.1, 0.1, 0.8, 0.8 },  -- herdr's 80% x 80% popup
      layout     = rex.layout.block{
        flavor  = "com.superlogical.terminal.shell",
        options = {
          command = popup_command(
            ctx.session_id, source_block, window, ctx.client_id, seed
          ),
          shell = "none",
        },
      },
      focus = true,
    }
    if err then error(err, 0) end
    local popup = layer.block_ids[1]

    -- Wait out the popup: config Lua has no rex.wait (script context
    -- only), so the synchronous CLI wait runs as a child — the action
    -- stays in the app-invoked context the whole time (R0 finding),
    -- which is what lets the queue below reach the app.
    local wres = rex.exec{ rex_cli, "wait", "-s", ctx.session_id, popup }

    -- The jump (or focus-return when the popup closed without one).
    local f = io.open(INTENT, "r")
    local intent = f and f:read("*a")
    if f then f:close() end
    os.remove(INTENT)
    local jumped = intent and jump(intent, ctx)
    if not jumped then
      -- No jump (Esc): put the user back on the pane they came from.
      pcall(rex.session.focus_block, {
        session_id = ctx.session_id, block_id = source_block,
      })
      jumped = "stayed"
    end

    return {
      popup_wait_code = wres and wres.code,
      outcome = jumped,
    }
  end,
}

-- ── Bindings (herdr parity) ──────────────────────────────────────
-- ctrl+s>s: the switcher. ctrl+k is vim-herdr-navigation's (Rex port)
-- and must stay free (D3); ctrl+s>w is session.switch.
rex.bind("ctrl+s>s", "nav")
rex.bind("ctrl+s>a", "nav", { seed = "@agents" })
