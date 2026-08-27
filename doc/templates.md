# Workspace templates reference

> **Status:** the template file syntax. See [`doc/query-filters.md`](query-filters.md) for the query-filter syntax, [`doc/keybinding.md`](keybinding.md) for the `Enter` keybind.

---

---

## Templates (open-with-template, §8.4)

`Enter` on a directory, zoxide, or DirNav entry opens a template
picker (when templates are configured); the default is preselected via
match-glob → `default: true` → first. Confirm the picker → name prompt
→ build + open the workspace. With no templates dir (or an empty one),
`Enter` skips the picker and opens the name prompt directly with the
hardcoded 1-tab/1-pane default.

### File layout

```
~/.config/herdr/templates/
├── rust-dev.yaml
├── plain.yaml
└── notes.yaml
```

The filename stem is the default `name` (unless `name:` overrides).
`default: true` marks the fallback (used when no `match` glob fits).

### Schema

```yaml
# Template metadata
name: rust-dev          # optional; defaults to the filename stem
default: false          # true = the fallback when no match glob fits
match:                   # optional; globs that auto-preselect this template
  - "**/Cargo.toml"

# Tabs — each becomes a herdr tab
tabs:
  - name: editor
    cwd: ~/code           # TAB-level cwd: every pane in this tab starts here
    layout:                # a layout is a split (see below)
      direction: v          # v = side-by-side (left | right), h = stacked (top / bottom)
      ratio: 60             # split ratio (0–100; 0 = even)
      panes:                # list of children — each is a leaf or a nested split
        - command: nvim .   # a leaf pane running that command
        - command: cargo watch -x test
```

### The recursive `layout`

A `layout` is a **split**: a `direction`, an optional `ratio`, and a
list of `panes`. Each child is one of:

| Child | Meaning |
| --- | --- |
| `command: "..."` | a leaf pane running that command. Empty/omitted = plain login shell (no nested shell). |
| `name: "..."` | optional pane label (set via `pane.rename`; herdr's `pane.split` doesn't accept a label). |
| `layout: { ... }` | a nested split (recursive — same shape). |

So a child is either a leaf (`command`) or a branch (`layout`).

> **Schema caveat:** a pane child is **either** `command:` (leaf) **or**
> `layout:` (nested split), not both. If a child has both keys,
> `serde` matches the `Nested` variant (because it has `layout:`) and
> silently ignores `command:`. This is a schema ambiguity, not a YAML
> error — YAML itself is valid with both keys (each `- ` starts
> a new list item), but the template schema treats them as
> mutually exclusive. Use one or the other, never both.

### `cwd` resolution

`cwd` is valid on three levels, most-specific wins:

1. **Pane** `cwd:` — this pane only.
2. **Tab** `cwd:` — every pane in the tab (unless a pane overrides).
3. **Workspace** path — the dir you jumped to (unset = herdr's default).

`cwd` is passed to `pane.split`/`tab.create` natively — no `cd` command.

Relative paths (`./...`, `../...`, `foo`) expand against the workspace
path (the dir you jumped to); `~`/`$HOME` expand to HOME. herdr's socket
API does NOT resolve relative `cwd` (it falls back to HOME), so the plugin
expands them before passing to the socket.

### Examples

#### Simple (single-level)

```yaml
# plain.yaml
default: true
tabs:
  - name: shell
    layout:
      panes:
        - {}              # plain login shell, no command
```

#### Multi-level split

> left = pane A, right = B stacked over C

```yaml
# dev.yaml
name: dev
match: ["**/Cargo.toml"]
tabs:
  - name: main
    cwd: ~/code
    layout:
      direction: v         # side-by-side: left | right
      panes:
        - command: nvim .   # left = pane A
        - layout:            # right = a nested split
            direction: h      # stacked: top / bottom
            panes:
              - command: cargo watch -x test   # top = B
              - command: zsh                   # bottom = C
```

#### Per-pane cwd override

```yaml
tabs:
  - name: dev
    cwd: ~/code            # tab default
    layout:
      direction: v
      panes:
        - command: nvim .
        - command: cargo watch -x test
          cwd: ~/code/watch   # this pane overrides the tab cwd
```

---

## Capturing a workspace as a template

The `nav-capture` action (bound to `prefix+ctrl+t` by recommendation;
see [`doc/keybinding.md`](keybinding.md)) opens a wizard that captures
the **current** workspace's live structure and writes a template YAML
— the inverse of `Enter` (which applies a template). See
[`spec/capture-template-spec.md`](../spec/capture-template-spec.md) for
the full spec.

### What it captures

- **Structure**: every tab's recursive split tree (`layout.export`),
  mapped to the `Template` schema (`direction`, `ratio`, nested
  `layout`).
- **Per-pane `cwd`**: from the live pane. The wizard's cwd policy
  controls how it's written:
  - `relative` (default) — relativize under the workspace base cwd;
    keep absolute when a pane cwd is "far distant" (not under the base).
  - `absolute` — keep every cwd as captured (machine-specific).
  - `inherit` — blank every cwd; each pane inherits the new workspace's
    cwd on apply.
- **Per-pane `command`**: best-effort from `pane.process_info`. A
  plain shell (only foreground process is `fish`/`bash`/`zsh`) → blank
  (high confidence). A non-shell pane → the detected command, with a
  `# best-effort:` comment so you can verify in the editor step. The
  `blank` command policy forces all commands to plain shells.
- **Per-pane `name`** (label): from the live pane, editable in the
  wizard's Names step; a blank pane name means no `name:` field (the
  pane is not renamed on apply).

### The wizard

A step-by-step form with a **live YAML preview** on every step:

1. **Confirm workspace** — read-only summary (label, tab count, pane
   count, per-tab breakdown).
2. **Template name** (required) — defaults to the workspace label.
3. **Match globs** — space-separated glob patterns for auto-preselect;
   `Tab` toggles the `default: true` flag.
4. **Command policy** — `keep` (best-effort) or `blank` (plain shells).
5. **cwd policy** — `relative`, `absolute`, or `inherit`.
6. **Names** — tab **and pane** names, pre-filled from live labels,
   shown as a flat indented list (each tab header followed by its pane
   rows); `↑↓` focuses a row, typing edits the focused one. A blank
   pane name means "no name" (the `name:` field is omitted).
7. **Review & write** — live YAML preview; `Enter` writes.

If the name clashes with an existing template, a **Clash prompt** offers
overwrite / rename / cancel before writing. After a successful write,
an **Editor prompt** offers to open `$EDITOR` on the file for fine-tuning.

### Non-interactive (CLI)

For scripts and tests, the wizard can be bypassed with flags:

```sh
herdr-nav capture --name my-tmpl --cwd-policy relative --command-policy keep
herdr-nav capture --summary   # print the workspace summary without writing
```

### Fidelity caveat

`pane.process_info` returns the **whole foreground process group** with
no `ppid`, so the user's originally-launched command can't be
identified with certainty when multiple non-shell processes are present
(e.g. an agent's MCP-server children). The `# best-effort:` comments
flag every guessed command; the `$EDITOR` step is the verification
surface. Plain-shell detection is reliable; non-shell capture is
best-effort.
