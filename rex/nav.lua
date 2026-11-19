-- herdr-nav for Rex — the launcher.
--
-- Opens the herdr-nav switcher in a floating layer over the pane you
-- are in: browse/search sessions, windows, panes, agents, pinned dirs
-- and zoxide entries, with a live preview. The binary does the work
-- (host layer: muxhost) — including the jump: the server-side focus
-- through a `rex focus` child, and the app's `session.select` through
-- Remote Control (`rex -C <client> do`, enabled once in the app's
-- settings). This launcher only finds the source pane, checks the
-- binary is usable, and starts it with the right context.
--
-- History: the R0 design had THIS action wait out the popup and queue
-- the app actions itself (nicer: no Remote Control needed) — but the
-- blocking wait freezes the app's key delivery for the popup's whole
-- lifetime: the popup rendered but accepted no key presses (observed
-- live 2026-11-20). The launcher is therefore non-blocking like
-- zextract's, and the switcher carries the jump out (the plan's
-- documented R1 fallback, rex-migration.md).
--
-- Install: copy this file to ~/.config/rex/plugins/nav.lua and add
--   require("plugins.nav")
-- to init.lua. Bindings (herdr parity, prefix ctrl+s):
--   ctrl+s>s, cmd+k          the switcher
--   ctrl+s>a, cmd+shift+k    agents only (opens seeded with the @agents group tag)
--   ctrl+s>ctrl+t            capture the session as a template
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

-- Where the switcher writes its jump record (diagnostics + the
-- fallback record; the switcher carries the jump out itself in R1
-- mode). Deleted before each open, so a stale record never lingers.
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
exec herdr-nav "$@"
]]

-- `args` is what runs after the preflight: {"--seed", seed} for the
-- switcher, {"capture"} for the capture wizard.
local function popup_command(sid, block, window, client_id, args)
  local cmd = {
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
    "sh",
  }
  for _, a in ipairs(args) do cmd[#cmd + 1] = a end
  return cmd
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

    os.remove(INTENT) -- a stale record can never linger

    local layer, err = rex.session.new_layer{
      session_id = ctx.session_id,
      bounds     = { 0.1, 0.1, 0.8, 0.8 },  -- herdr's 80% x 80% popup
      layout     = rex.layout.block{
        flavor  = "com.superlogical.terminal.shell",
        options = {
          command = popup_command(
            ctx.session_id, source_block, window, ctx.client_id,
            { "--seed", args.seed or "" }
          ),
          shell = "none",
        },
      },
      focus = true,
    }
    if err then error(err, 0) end
    -- Non-blocking by design (see the header): the switcher performs
    -- the jump itself — this action must be back in the app's key
    -- path before the first keystroke reaches the popup.
    return { layer_id = layer.layer_id, block_ids = layer.block_ids }
  end,
}

-- Capture (herdr parity: prefix+ctrl+t / nav-capture): analyze the
-- current session and write a template YAML to ~/.config/nav/rex/
-- templates/. Non-blocking like `nav`, for the same reason.
rex.action{
  name  = "nav_capture",
  title = "nav: capture session as template",
  description = "Open the capture wizard over the focused session: reads its windows, layouts, cwds and best-effort commands, and writes a template YAML to ~/.config/nav/rex/templates/.",
  args  = {},
  run   = function(ctx)
    local v = rex.session.view{ session_id = ctx.session_id }
    local source_block, window
    for _, w in ipairs(v.windows or {}) do
      if w.window_id == v.active_window_id then
        source_block = w.focused_block_id
        window = w.window_id
      end
    end
    if not source_block or not window then
      error("no focused pane to capture from", 0)
    end
    local layer, err = rex.session.new_layer{
      session_id = ctx.session_id,
      bounds     = { 0.1, 0.1, 0.8, 0.8 },
      layout     = rex.layout.block{
        flavor  = "com.superlogical.terminal.shell",
        options = {
          command = popup_command(
            ctx.session_id, source_block, window, ctx.client_id, { "capture" }
          ),
          shell = "none",
        },
      },
      focus = true,
    }
    if err then error(err, 0) end
    return { layer_id = layer.layer_id, block_ids = layer.block_ids }
  end,
}

-- ── Bindings (herdr parity) ──────────────────────────────────────
-- ctrl+s>s: the switcher. ctrl+k is vim-herdr-navigation's (Rex port)
-- and must stay free (D3); ctrl+s>w is session.switch.
rex.bind("ctrl+s>s", "nav")
rex.bind("ctrl+s>a", "nav", { seed = "@agents" })
-- herdr parity: super+k / super+shift+k (herdr's config.toml binds
-- nav-open and nav-agents to them).
rex.bind("cmd+k", "nav")
rex.bind("cmd+shift+k", "nav", { seed = "@agents" })
-- Capture (herdr: prefix+ctrl+t).
rex.bind("ctrl+s>ctrl+t", "nav_capture")
