# Query filter reference

> **Status: stub.** The full syntax, composition rules, and worked examples
> land in Phase 11 (PLANNING.md §17). This file exists now so the repo
> layout is complete; the real content is written during that phase.

`herdr-nav` supports a lightweight filter syntax layered on top of the
fuzzy search. Filters restrict which targets appear before the fuzzy
scorer ranks them. They are **Search-mode only** — typing any filter
token implicitly enters Search.

## Operators (quick reference)

| Syntax | Meaning |
| --- | --- |
| `agents …` | Group scope — restrict to one group (leading position only) |
| `@pane` / `kind:pane` | Kind filter — show only pane leaves |
| `@dir` / `kind:dir` | Union alias — pinned dirs + zoxide entries |
| `@zox` / `kind:zox` | Specific kind — zoxide only |
| `!plugin` | Negation — exclude all plugin leaves |
| `!zox` | Negation — exclude zoxide entries |

`@` is shorthand for `kind:`. Available kinds: `pane`, `agent`, `dir`
(union: pinned+zoxide), `zox`, `plugin`, `tab`, `workspace`.

## Composition

```text
result =
    initial_group_scope
    ∩ union(positive_kind_filters)   ← multiple @/kind: are OR
    − union(negative_filters)
    |› nucleo(fuzzy_needle)          ← remaining plain text
```

See PLANNING.md §17 Phase 11 for the full composition semantics, rules,
and examples table.

## Examples

| Query | Effect |
| --- | --- |
| `nvim` | plain fuzzy across all leaves |
| `@pane nvim` | pane leaves fuzzy-matched with `nvim` |
| `@pane @dir` | panes OR directories |
| `session @pane nvim` | panes within Session, fuzzy `nvim` |
| `@dir !zox` | pinned dirs only (directory-like minus zoxide) |
| `!plugin nvim` | everything except plugins, fuzzy `nvim` |

Unrecognised `@`/`kind:` tokens (e.g. `@pnae`) are treated as ordinary
fuzzy text, not errors. Contradictory filters (e.g. `agents @pane`)
produce the normal no-match state.

---

## Templates (open-with-template, §8.4)

`^t` on a directory or zoxide entry opens a template picker.
Templates are **one YAML file per template** in
`~/.config/herdr/templates/`. With no dir (or an empty
one), `^t` is unbound.

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
| `layout: { ... }` | a nested split (recursive — same shape). |

So a child is either a leaf (`command`) or a branch (`layout`).

### `cwd` resolution

`cwd` is valid on three levels, most-specific wins:

1. **Pane** `cwd:` — this pane only.
2. **Tab** `cwd:` — every pane in the tab (unless a pane overrides).
3. **Workspace** path — the dir you jumped to (unset = herdr's default).

`cwd` is passed to `pane.split`/`tab.create` natively — no `cd` command.

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
