# Query filters

herdr-nav supports an advanced query language in **search mode** that
layers on top of the Phase 4 fuzzy scorer. It lets you restrict the
haystack by group, kind, and negation — without a second matcher or a
scoring-model change.

## When it's active

Filters are a **search-mode concept**. Typing any character (including
a filter token like `@pane`) enters search mode, exactly as plain text
does. Browse mode is structure; filters are search.

## Operators

### Group scope (leading token)

A **leading** token (first word) that matches a group name restricts
the haystack to that group:

| Token | Group |
|-------|-------|
| `session` | live panes in the current session |
| `agents` | agent panes, sorted by status |
| `pinned` | pinned directories |
| `zoxide` | zoxide frecency directories |
| `plugins` | installed plugins |

Only **one** group scope is allowed. A second group-name token is
treated as fuzzy text, not a scope.

```
agents nvim    → Agents leaves only, fuzzy "nvim"
session cargo  → Session leaves only, fuzzy "cargo"
```

### Kind filter (`kind:X` or `@X`)

Position-independent tokens that restrict to leaves of a specific
`Kind`. `@` is sugar for `kind:` — they're identical.

| Token | Kind |
|-------|------|
| `@pane` / `kind:pane` | `Kind::Pane` |
| `@agent` / `kind:agent` | `Kind::Agent` |
| `@dir` / `kind:dir` | `Kind::Dir` **and** `Kind::Zox` (union alias) |
| `@zox` / `kind:zox` | `Kind::Zox` only |
| `@plugin` / `kind:plugin` | `Kind::Plugin` |
| `@tab` / `kind:tab` | `Kind::Tab` |
| `@workspace` / `kind:workspace` | `Kind::Workspace` |

`dir` is a **union alias**: `@dir` matches both pinned dirs (`Kind::Dir`)
and zoxide entries (`Kind::Zox`), because users think of them as
"directories" and rarely care about provenance when filtering. Use
`@zox` for zoxide only; use the `pinned` group scope for pinned only.

Multiple positive kind filters are **OR** (a node has exactly one Kind):

```
@pane @dir       → panes OR directory-like entries
@pane @agent     → panes OR agents
```

### Negation (`!X`)

Excludes a kind or group from the results:

| Token | Excludes |
|-------|----------|
| `!pane` | `Kind::Pane` |
| `!agent` | `Kind::Agent` |
| `!dir` | `Kind::Dir` **and** `Kind::Zox` (union alias) |
| `!zox` | `Kind::Zox` |
| `!plugin` | `Kind::Plugin` |
| `!session` | the Session group (panes) |
| `!agents` | the Agents group |
| `!pinned` | the Pinned group (dir leaves) |
| `!zoxide` | the Zoxide group (zox leaves) |
| `!plugins` | the Plugins group |

The singular/plural difference is the user-visible cue: `!agent`
(kind) and `!agents` (group) exclude the same leaves here, but `!dir`
(kind union) excludes both Dir+Zox while `!pinned` (group) excludes
Dir only.

## Composition

The parser splits the raw query into filter tokens + a fuzzy needle.
Filters are set operations applied in this order:

```
result = group_scope ∩ union(positive_kinds) − union(negations) |› nucleo(needle)
```

- **Group scope** restricts the starting set (one allowed).
- **Positive kinds** are OR'd, then intersected with the scope.
- **Negations** subtract afterward.
- The **needle** (remaining plain text) is fuzzy-matched by nucleo.

### Rules

- Only **one** positive group scope. A second group token → fuzzy text.
- Multiple positive kinds = **OR**.
- Group scope **intersects** with the kind union.
- Negations **subtract** from the result.
- **Contradictory** filters (e.g. `agents @pane`) → no matches, not
  an error.
- **Unrecognised** `@`/`kind:` tokens (e.g. `@pnae`) → treated as
  fuzzy text, not errors.
- **Repeated** filters are deduplicated silently.
- **Empty needle** after filters (e.g. just `@pane`) → show all matching
  leaves, ranked by haystack order.

## Examples

| Query | Meaning |
|-------|---------|
| `nvim` | plain fuzzy across all leaves |
| `agents nvim` | group scope Agents, fuzzy `nvim` |
| `@pane` | all pane leaves, no fuzzy filter |
| `@pane nvim` | pane leaves fuzzy-matched with `nvim` |
| `@pane @dir` | panes OR directory-like entries (pinned+zoxide) |
| `session @pane nvim` | panes within Session only, fuzzy `nvim` |
| `@dir !zox` | directory-like minus zoxide = pinned dirs only |
| `!plugin !zox nvim` | everything except plugins and zoxide, fuzzy `nvim` |
| `agents @pane` | contradiction → no matches (Agents has no Pane leaves) |
| `@pnae nvim` | `@pnae` unrecognised → needle = `@pnae nvim` |

## Status strip

The status strip (bottom of the list) confirms the active filters on
every keystroke:

```
 agents · pane · !zox · fuzzy    3/47
```

When no filters are active, it shows `flat leaves · fuzzy` as before.

## What's NOT supported

- **Quoted exact** (`"src/switcher"`) — deferred to v0.2.
- **AND-tokens** (space-separated, all-must-match) — deferred to v0.2.
- **Field targets** (`path:herdr name:nvim`) — cut from v0.1 (drifts
  toward a command palette, which is a non-goal per spec §1).
