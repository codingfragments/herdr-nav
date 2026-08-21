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
