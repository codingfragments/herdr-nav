# Spike: workspace → template reconstruction feasibility

> **Status:** spike, 2026-08-26. Resolves the critical dependency for the
> "capture current workspace as a template" enhancement. Findings are
> ground truth from a live `herdr 0.8.2` daemon.

## Question

Can the plugin reconstruct a workspace's split layout (workspace / tab /
pane / split tree) from the daemon, with enough fidelity to emit a
`Template` (doc/templates.md)?

## Method

Probe the live daemon (`HERDR_SOCKET_PATH`), dump raw JSON for the
candidate methods, and compare against the existing `Template` /
`Layout` / `PaneNode` schema in `src/source.rs`.

## Findings

### `pane.list` — flat, NO geometry

Returns one object per pane with: `pane_id`, `terminal_id`,
`workspace_id`, `tab_id`, `focused`, `cwd`, `foreground_cwd`, `agent`,
`agent_status`, `terminal_title`, `terminal_title_stripped`,
`display_agent`, `scroll {offset_from_bottom, max_offset_from_bottom,
viewport_rows}`, `revision`, `agent_session`.

**No `rect`, no `width`/`height`, no split structure, no parent id,
no order.** This is what `SessionProvider` already consumes. Alone it
cannot reconstruct a layout — only a flat pane list per tab.

### `pane.layout` — has geometry, but IGNORES its `tab_id` param

Returns `{workspace_id, tab_id, zoomed, area {x,y,width,height},
focused_pane_id, panes:[{pane_id, focused, rect}], splits:[{id,
direction, ratio, rect}]}`.

- Per-pane `rect {x,y,width,height}` is present.
- `splits[]` carries `direction` (`"right"` = side-by-side), `ratio`
  (0.0–1.0), and `rect`.
- **Critical bug for our purposes:** passing `tab_id` does NOT switch
  the tab — it always returns the *active* tab's layout (verified:
  `tab_id:"wP:t2"` and `tab_id:"wN:t1"` both returned `wD:t1`). So
  `pane.layout` is current-tab-only and not param-driven. Not usable
  for enumerating arbitrary tabs.

### `layout.export` — the golden path ✅

Returns a **portable, recursive binary split tree**, param-driven by
`tab_id`:

```json
{
  "type": "layout_export",
  "layout": {
    "workspace_id": "wP",
    "tab_id": "wP:t2",
    "zoomed": false,
    "focused_pane_id": "wP:p3",
    "root": {
      "type": "split",
      "direction": "right",
      "ratio": 0.5,
      "first":  { "type": "pane", "pane_id": "wP:p3", "cwd": "...", "label": "..." },
      "second": {
        "type": "split",
        "direction": "down",
        "ratio": 0.5,
        "first":  { "type": "pane", "pane_id": "wP:p4", "label": "testhel", "cwd": "..." },
        "second": { "type": "pane", "pane_id": "wP:p5", "cwd": "..." }
      }
    }
  }
}
```

- `root` is either `{type:"pane", pane_id, cwd, label?}` (leaf) or
  `{type:"split", direction:"right"|"down", ratio, first, second}`
  (binary branch — `first`/`second`, not a list).
- `direction`: `"right"` = side-by-side (left | right); `"down"` =
  stacked (top / bottom).
- `ratio`: 0.0–1.0 (float). Template schema uses 0–100 int, 0 = even.
- Per-pane `cwd` is included. `label` included when set.
- **Param-driven:** `tab_id:"wP:t2"` correctly returned wP:t2 (unlike
  `pane.layout`). Omitting `tab_id` returns the active tab.
- `workspace_id` param does NOT enumerate all tabs of a workspace —
  it still returns a single (active) tab. **We must iterate `tab.list`
  ourselves and call `layout.export` once per tab.**

### `layout.apply` — exists, round-trippable

Present in the method enum (not called in this spike). Per the socket
doc, `layout.apply` "creates a fresh tab from a declarative tree" —
i.e. it accepts the same tree shape `layout.export` produces. This
means the export tree is directly re-applicable, which is strong
evidence the tree is the canonical layout representation.

## Mapping to the existing `Template` schema

| `layout.export` field | `Template` schema field | Conversion |
| --- | --- | --- |
| `root` (split) `direction:"right"` | `Layout.direction:"v"` | side-by-side |
| `root` (split) `direction:"down"` | `Layout.direction:"h"` | stacked |
| split `ratio` (0.0–1.0) | `Layout.ratio` (0–100, 0=even) | `round(ratio*100)` |
| split `first` / `second` | `Layout.panes[0]` / `panes[1]` | binary → 2-list |
| pane `cwd` | `PaneNode::Pane.cwd` | direct |
| pane `label` | `PaneNode::Pane.name` | direct |
| pane `pane_id` | — (not in template) | drop, or keep as comment |
| per-tab `tab_id`/`workspace_id` | `TemplateTab.name` | from `tab.list` label |

## Gaps / open questions for the spec

1. **`command` is NOT captured.** `layout.export` gives `cwd` and
   `label` per pane but not the running command. `pane.list` also has
   no command field (only `agent`, `terminal_title`). So an auto-
   generated template cannot restore the exact startup commands — it
   can only reproduce the *structure* and *cwds*. Capturing commands
   needs another source (e.g. `pane.process_info`, or asking the user).
   This is a fidelity ceiling to decide on in the spec.
2. **`pane.layout` param bug** means we must use `layout.export`
   (param-driven) and never `pane.layout` for arbitrary tabs.
3. **Workspace-level cwd** — `layout.export` is per-tab; a workspace's
   tabs may each have different cwds. The template's tab-level `cwd`
   handles this; workspace-level cwd is the first tab's cwd or needs a
   decision.
4. **`ratio` representation** — export is float 0–1, template is int
   0–100 with 0=even. Conversion is trivial but must be specified
   (round? floor? how to represent "even" — 0 or 50?).

## Second spike: `pane.process_info` (command recovery)

Decision from grilling: probe `pane.process_info` per pane to recover
the running command, with reasonable defaults. Spiked live.

### What it returns

`{pane_id, shell_pid, foreground_process_group_id,
foreground_processes:[{pid, name, argv0, argv[], cmdline, cwd}]}`.

### Findings

- It returns the **entire foreground process group**, not just the
  launched command. For the `pi` agent pane (wD:p1) it lists `bun`,
  `trajectory`, `node`, `volta-shim` — the agent's MCP-server children
  are mixed in with the actual `pi` launch.
- **No `ppid`** is returned, so we cannot reliably identify "the
  process whose parent is the shell" (i.e. the user's launched
  command) within the group.
- **Plain-shell detection IS reliable:** when the only foreground
  process is the shell itself (`fish`/`bash`/`zsh`/`sh`), `command`
  should be blank (plain shell). High confidence.
- **Non-shell panes are ambiguous:** e.g. wM:p1 is an interactive
  `ssh … -- cd … ; exec $SHELL` — a verbatim capture would re-run a
  fragile remote shell on template apply. Best-effort capture here is
  risky without human review.

### Reasonable-default policy for the spec

1. **Plain shell** (only foreground process is a known shell) →
   `command: null`. High confidence, no annotation.
2. **Non-shell** → take a best-effort `command:` from the foreground
   process group, preferring the non-shell process whose `cwd` matches
   the pane cwd; annotate the YAML with a `# best-effort: captured
   from pane <id> process <name>; verify` comment so the editor step
   flags it for review.
3. **Always** the optional editor step is where the user confirms/fixes
   commands. The generated template is honest about which commands are
   guessed vs. confirmed-blank.

Open: the exact pick heuristic among multiple non-shell foreground
processes (smallest pid? argv0 match to terminal_title?) is a spec
detail to nail down, but the ceiling is acknowledged: without `ppid`,
no heuristic is guaranteed correct.

## Conclusion

**World B confirmed:** a split-tree method (`layout.export`) exists and
returns a portable recursive tree with per-pane `cwd`/`label`. Full
automatic structure reconstruction is feasible. The startup `command`
is recoverable only as a best-effort from `pane.process_info` (reliable
for plain shells, ambiguous for non-shell panes); the optional editor
step is the verification path. The spec should mandate `layout.export`
iterated per tab (via `tab.list`) as the capture path, `pane.process_info`
for best-effort command defaults, and the editor step as the fidelity
safety net.
