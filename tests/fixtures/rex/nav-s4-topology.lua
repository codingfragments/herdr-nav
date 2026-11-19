-- S4 spike: nav's full topology in ONE rex do script.
-- Returns: every session, its windows (label, active, focused block),
-- and per block: label, terminal title, pwd, program_status — the
-- shape nav's Session and Agents groups need.

local function block_call(session_id, block_id, method)
  local ok, r = pcall(rex.call, "block.method", {
    session_id = session_id, block_id = block_id, method = method,
  })
  if ok then return r end
  return { __error = tostring(r) }
end

local out = {}
out.source_session = rex.session_id

local ok, ls = pcall(rex.call, "session.list", {})
if not ok then
  out.error = tostring(ls)
  return out
end

out.sessions = {}
for i, s in ipairs(ls.sessions or {}) do
  local sid = s.session_id
  -- `session.attach` returns the current view inline (and a plain
  -- `session.view` needs the attachment anyway); detach right after.
  local ok_v, attach = pcall(rex.call, "session.attach", { session_id = sid })
  if ok_v then
    pcall(rex.call, "session.detach", { session_id = sid })
  end
  local view = ok_v and attach and attach.view or nil
  if not ok_v or view == nil then
    out.sessions[#out.sessions + 1] = {
      session_id = sid, label = s.label, error = tostring(attach),
    }
  else
    local entry = {
      session_id = sid,
      label = view.label or s.label,
      active = (sid == out.source_session),
      active_window_id = view.active_window_id,
      windows = {},
    }
    for j, w in ipairs(view.windows or {}) do
      local window = {
        window_id = w.window_id,
        label = w.label,
        active = (w.window_id == view.active_window_id),
        focused_block_id = w.focused_block_id,
        blocks = {},
      }
      for k, layer in ipairs(w.layers or {}) do
        if layer.kind == "tiled" then
          for m, b in ipairs(layer.blocks or {}) do
            local block = {
              block_id = b.block_id,
              label = b.label,
              title = block_call(sid, b.block_id, "title"),
              pwd = block_call(sid, b.block_id, "pwd"),
              program_status = block_call(sid, b.block_id, "program_status"),
            }
            window.blocks[#window.blocks + 1] = block
          end
        end
      end
      entry.windows[#entry.windows + 1] = window
    end
    out.sessions[#out.sessions + 1] = entry
  end
end

return out
