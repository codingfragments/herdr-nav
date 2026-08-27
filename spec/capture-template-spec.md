# Spec: Capture workspace as template

> **Status:** draft spec, 2026-08-26. A new, separate action that analyzes
> the current workspace (workspace / tab / pane / layout) and generates a
> workspace template YAML, with an optional in-flow `$EDITOR` fine-tune.
> Derived from a grilling session; feasibility backed by the live spikes
> in [`spec/spike-layout-export.md`](spike-layout-export.md).
>
> This is a **new enhancement** on a **separate binding**, distinct from
> the existing `^t` open-with-template flow (doc/templates.md). It
> captures; `^t` applies.

## 1. Purpose

One keystroke opens a wizard that reads the *current* workspace from the
herdr daemon, asks a handful of questions, and writes a template file to
`~/.config/herdr/templates/<name>.yaml` — then optionally hands off to
`$EDITOR` for fine-tuning. The template is immediately usable by the
existing `^t` open-with-template path.

**In scope:** capture the active workspace's structure (tabs → recursive
splits → panes), per-pane `cwd` and `label`, and a best-effort startup
`command`; ask the user for template metadata and two global policies;
write + optionally edit.

**Non-goals:**
- Does **not** apply templates (that's `^t` / `layout.apply`, unchanged).
- Does **not** edit existing templates (capture-only; editing is `$EDITOR`).
- Does **not** capture scrollback, agent transcripts, or pane pixel sizes.
- Does **not** reconstruct layout from pane rects by inference — the
  daemon's `layout.export` already gives the canonical tree (spike §3).
- Does **not** special-case agent panes (they are captured like any pane).

## 2. Feasibility (spike outcomes)

Ground truth from a live `herdr 0.8.2` daemon — see
[`spike-layout-export.md`](spike-layout-export.md).

| Need | Method | Result |
| --- | --- | --- |
| Split tree per tab | `layout.export {tab_id}` | ✅ portable recursive binary tree: `root` is `split{direction,ratio,first,second}` or `pane{pane_id,cwd,label?}`. Param-driven by `tab_id`. |
| Pane list / labels / agent | `pane.list` | ✅ flat list, `agent`/`agent_status`/`terminal_title` per pane. No geometry. |
| Running command | `pane.process_info {pane_id}` | ⚠️ returns the **whole foreground process group**, no `ppid`. Reliable for plain shells, ambiguous for non-shell panes. |
| Workspace base cwd | `workspace.get` / `workspace.list` | ❌ no `cwd` field. Only `worktree.checkout_path` for worktree-backed ws. **Derived** (§6). |
| Tab labels | `tab.list` | ✅ `tab_id` → `label`. |

**Critical gotchas baked into the spec:**
- `pane.layout` **ignores its `tab_id` param** (always returns the active
  tab). The capture path **must use `layout.export`**, iterated per tab
  via `tab.list`. Never `pane.layout`.
- `layout.export` with `workspace_id` does **not** enumerate all tabs —
  it returns a single (active) tab. Iterate tabs ourselves.

## 3. Entry point and binding

A **second `[[actions]]`** in `herdr-plugin.toml`, symmetric to
`nav-open`, with its own keybinding. The capture flow is a dedicated
ratatui popup (a step-by-step wizard), reusing the existing
render/theme stack.

```toml
# herdr-plugin.toml (additions)
[[panes]]
id = "capture"
title = "herdr capture"
command = ["./target/release/herdr-nav", "capture"]
placement = "popup"
width = "80%"
height = "80%"

[[actions]]
id = "nav-capture"
title = "nav: capture workspace as template"
command = ["herdr", "plugin", "pane", "open", "--plugin", "herdr-nav", "--entrypoint", "capture", "--placement", "popup"]
```

The binary gains a `capture` subcommand (arg) that runs the wizard
instead of the switcher. The user binds a key in their
`~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "Ctrl S"          # user's choice; informational only
action = "nav-capture"
type = "plugin_action"
```

Herdr owns all keybindings; the plugin never binds its own keys (same
rule as `nav-open`, doc/keybinding.md).

## 4. Capture algorithm

1. `workspace.list` → find the **focused** workspace → its `workspace_id`
   and `label`. (Scope is the active workspace, all tabs — §1.)
2. `tab.list` → filter tabs whose `workspace_id` matches; sort by
   `number`.
3. For each tab: `layout.export {tab_id}` → the recursive split tree.
4. `pane.list` → join by `pane_id` for `agent`/`agent_status`/`label`
   (the export already carries `cwd` and `label`; `pane.list` is the
   source for agent detection only).
5. For each leaf pane: `pane.process_info {pane_id}` → best-effort
   command (§5).
6. Derive the **workspace base cwd** = the `cwd` of the first pane of
   the first tab (§6).
7. Apply the user's two global policies (command policy §5, cwd policy
   §6) to every pane.
8. Map the per-tab trees onto `Template`/`TemplateTab`/`Layout`/
   `PaneNode` (§7).
9. Serialize to YAML, write to `~/.config/herdr/templates/<name>.yaml`
   (§8), optionally `$EDITOR` (§9).

**Socket discipline:** one fresh connection per request
(socket_client.rs). `layout.export` and `pane.process_info` are one
request each; for a workspace with T tabs and P panes that's
1 + 1 + T + P calls. Acceptable for a one-shot capture.

## 5. Command capture (`pane.process_info`)

`pane.process_info` returns `{shell_pid,
foreground_process_group_id, foreground_processes:[{pid, name, argv0,
argv[], cmdline, cwd}]}`. It is the **whole foreground process group**
with **no `ppid`**, so the user's originally-launched command cannot be
identified with certainty when multiple non-shell processes are present.

**Reasonable-default policy:**

1. **Plain shell** — if the only foreground process is a known shell
   (`fish`/`bash`/`zsh`/`sh`/…), `command` = `null` (plain login shell,
   no nested shell). High confidence.
2. **Non-shell** — pick the non-shell foreground process whose `cwd`
   matches the pane's cwd (preferring the smallest `pid` as a
   tiebreak); use its `cmdline` as `command:`. Annotate the YAML:
   ```yaml
   # best-effort: captured from pane wD:p1 process bun; verify
   command: bun /path/to/server.bundle.mjs
   ```
3. **Ambiguous / no match** — `command` = `null` with the same
   `# best-effort: …; no confident match` comment.
4. **Agent panes** are captured like any pane (no special-casing). A
   `pi` agent pane will best-effort to `command: pi`. The editor step
   is where the user decides whether a bare `pi` is the intended
   startup or should be blanked.

The editor step (§9) is the verification surface for all guessed
commands. The generated YAML is honest about which commands are guessed
vs. confirmed-blank via the `# best-effort:` comments.

## 6. cwd policy and the workspace base cwd

The daemon exposes no workspace-level `cwd`. The **base cwd** is
**derived** = the `cwd` of the first pane of the first tab (tab.list
sorted by `number`; within that tab, `layout.export`'s root → first leaf
pane). This is the pane that existed before any splits — the effective
"where the workspace was opened" path.

The wizard asks a **global cwd policy** (one choice, all panes):

| Choice | Behavior |
| --- | --- |
| **Relative to base** (default) | Panes whose cwd is under the base → relativized (`./…`, `…`). Panes whose cwd is **not** under the base ("far distant") → kept **absolute** automatically. A pane whose cwd equals the base → `.` (or blanked to inherit). |
| **Absolute** | Keep every pane cwd absolute as captured. Machine-specific. |
| **Inherit (blank)** | Blank every pane cwd (`null`) → each pane inherits the new workspace's cwd on apply. |

**Per-pane cwd clearing** (blanking one pane to inherit the ws default)
is done in the `$EDITOR` fine-tune step, **not** the wizard. The wizard
is global; the editor is the per-pane surface.

Relativization uses the existing `resolve_cwd`/`expand_path` helpers
(source.rs) — reversed: `base`-relative path for `cwd`. `~`/`$HOME`
expansion is preserved on write (the template stores `~/…` where the
base is under HOME).

## 7. Mapping `layout.export` → `Template`

| `layout.export` | `Template` schema | Conversion |
| --- | --- | --- |
| split `direction: "right"` | `Layout.direction: "v"` | side-by-side (left \| right) |
| split `direction: "down"` | `Layout.direction: "h"` | stacked (top / bottom) |
| split `ratio` (0.0–1.0) | `Layout.ratio` (0–100, 0 = even) | `round(ratio * 100)`; `0.0` → `0` |
| split `first` / `second` | `Layout.panes[0]` / `panes[1]` | binary → 2-element list |
| pane `cwd` | `PaneNode::Pane.cwd` | after cwd policy (§6) |
| pane `label` | `PaneNode::Pane.name` | direct |
| pane `pane_id` | — (not in template) | dropped; optionally a `# pane: <id>` comment |
| tab `label` (from `tab.list`) | `TemplateTab.name` | pre-filled, editable in wizard (§8 step 6) |
| pane `label` (from `layout.export`) | `PaneNode::Pane.name` | pre-filled, editable in wizard (§8 step 6); blank → `None` (no `name:` field) |
| `focused_pane_id` | — | dropped (template has no focus concept) |
| `zoomed` | — | dropped (view state, not structure) |

**Single-pane tab:** `layout.export`'s root is a `pane` node, not a
split. Emit a `TemplateTab` with a one-pane `Layout` (`direction: "v"`,
`ratio: 0`, one `Pane` child) — same shape as
`hardcoded_default_template`.

**Nested splits:** recurse `first`/`second` → `PaneNode::Nested{layout}`
vs `PaneNode::Pane{…}`. The existing untagged enum already deserializes
both; serialization writes the same shape.

## 8. The wizard (step-by-step)

A step-by-step ratatui wizard, one question per screen, reusing the
theme stack. `Esc` at any step aborts (no write). `Enter` advances.

1. **Scope confirm** — read-only summary: workspace label, `N` tabs,
   `M` panes, `K` panes with a guessed (non-shell) command. `Enter`
   proceeds; `Esc` aborts.
2. **Template name** (required) — default = the workspace label.
   Filename stem and `name:`. Sanitized to a safe filename.
3. **match globs + default flag** (optional) — zero or more glob
   patterns for `match:` (auto-preselect) and a toggle for
   `default: true` (fallback template). Blank = no match globs;
   `default` off by default.
4. **Command policy** (global) — *keep best-effort guesses* (with
   `# best-effort:` comments) / *blank all to plain shells*.
5. **cwd policy** (global) — *relative to base* (default) / *absolute*
   / *inherit (blank)*. See §6.
6. **Names (tabs + panes)** — one editable field per tab **and per
   pane**, each pre-filled from the live label (`tab.list` for tabs,
   the export tree's `label` for panes; fallback `tab<N>` when a tab
   label is empty or a bare number like `1`). All rows on one screen as
   a flat, indented list: each tab header row is followed by its pane
   rows. `↑↓` focuses a row, typing edits the focused row. A **blank
   pane name means "no name"** — the `name:` field is omitted (None),
   so the pane is not renamed on apply. A non-empty entry overrides
   the live label; an absent entry (defensive) falls back to the live
   label.
7. **Review** — a rendered YAML preview (read-only) of the final
   template. `Enter` writes; `Esc` goes back.
8. **Name clash** — if `~/.config/herdr/templates/<name>.yaml` exists:
   prompt *overwrite / cancel / rename* (rename loops back to step 2
   with the name pre-filled).
9. **Write** — serialize and write to
   `~/.config/herdr/templates/<name>.yaml`. Create the `templates/`
   dir if missing.
10. **Editor prompt** — *Open in $EDITOR now?* yes/no.
    - **yes** → exec `$VISUAL`/`$EDITOR` (fallback `vi`) on the
      written path; the popup pane process is replaced by the editor.
      When the editor exits, the pane process ends and the popup
      closes.
    - **no** → toast the written path and close the popup.

## 9. Editor handoff

- `$VISUAL` preferred, then `$EDITOR`, then `vi`.
- The editor runs **in place** on the written file (§8 step 9). Edits
  land directly in `~/.config/herdr/templates/<name>.yaml`.
- **No post-edit validation** by the plugin (exec model — the plugin
  process is replaced). A malformed YAML is surfaced by `read_templates`
  on the next `^t` use, which already logs parse errors to stderr
  (source.rs). The editor is the validation surface.

## 10. Edge cases

| Case | Behavior |
| --- | --- |
| Single-pane tab | root is a pane node → one-pane `Layout` (§7). |
| Zoomed tab | zoom is a view state; capture the full structure, ignore `zoomed`. |
| `focused_pane_id` | dropped (no focus in templates). |
| `ratio: 0.0` | `Layout.ratio: 0` (even). `0.5` → `50`. |
| `templates/` dir missing | created on write. |
| Name clash | prompt overwrite / cancel / rename (§8 step 8). |
| Agent pane | captured like any pane; best-effort `command:` (§5). |
| Pane cwd outside base | kept absolute under "relative" policy (§6). |
| `pane.layout` used by mistake | never — it ignores `tab_id` (spike §3). Use `layout.export`. |
| No templates exist after write | the new file is the only one; `^t` now lists it. |

## 11. Open / deferred

- **match-glob pre-suggestion**: leave blank by default. A future
  nicety could pre-suggest `**/<basename-of-base-cwd>/**` when the base
  is inside a git repo. Not in v1.
- **`layout.apply` round-trip**: `layout.apply` exists and accepts the
  same tree shape — a future "apply template via layout.apply" path
  could replace the current `build_workspace_from_template` split
  loop. Out of scope here; capture only writes the YAML.
- **Per-pane command review screen**: the wizard stays global for
  command policy; per-command fixing is in `$EDITOR`. If real usage
  shows the best-effort guesses are too often wrong, revisit a per-pane
  review screen.

## 12. Decisions log (from grilling)

1. Capture scope: **active workspace, all tabs**.
2. Command capture: **probe `pane.process_info`**, reasonable defaults,
   best-effort + annotation, editor verifies.
3. Entry point: **second ratatui popup** (`nav-capture` action), separate
   binding, symmetric to `nav-open`.
4. Form questions: **name (required), match globs + default flag,
   command policy, cwd policy**.
5. Base cwd: **first pane of first tab**.
6. cwd policy: **relative by default, absolute when distant, blank to
   inherit (per-pane in editor)**.
7. Write + edit: **write in-place, then `$EDITOR`**.
8. Name clash: **prompt overwrite / cancel / rename**.
9. Agent panes: **captured like any pane** (no special-casing).
10. Form layout: **step-by-step wizard**.
11. Tab names: **ask per-tab, pre-filled from live labels**.
11b. Pane names: **editable in the same Names step as tabs** (combined);
     a blank pane name means `None` (no `name:` field, not renamed on
     apply). Pre-filled from the live pane label.
12. Per-pane cwd: **global in wizard, per-pane in editor**.
13. After editor: **exec, no validation**.
