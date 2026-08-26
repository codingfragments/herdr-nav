# Plan: Capture workspace as template — implementation phases

> **Status:** detailed plan, 2026-08-26. Implements
> [`spec/capture-template-spec.md`](capture-template-spec.md), backed by
> the live spikes in [`spike-layout-export.md`](spike-layout-export.md).
> Follows the PLANNING.md §17 phase discipline: each phase is a vertical,
> end-to-end testable slice, one `phase/` branch/PR per phase, in order.
> Each phase ends with: what to test, how to trigger it, what works vs
> what's still a stub.
>
> This is a **plan only** — no implementation without explicit consent
> (global AGENTS.md §1). When approved, these land as `phase/capture-*`
> branches off `main`, one PR per phase.

## Integration surface (what already exists, what's new)

**Reuse (no change):**
- `socket_client::request` — one-shot socket calls (fresh connection per
  request; the contract that makes `layout.export`/`pane.process_info`
  safe to call per-tab/per-pane).
- `source::Template` / `TemplateTab` / `Layout` / `PaneNode` — the
  target schema. `PaneNode` is already an untagged enum matching
  `layout.export`'s leaf/branch shape.
- `source::read_templates` — reads back what we write (used by `^t`).
- `source::resolve_cwd` / `expand_path` — `~`/`$HOME` and relative-path
  expansion. Reused for the relativization inverse.
- `source::workspace_name_default` — fallback name from a path.
- `theme::Theme` (Catppuccin Macchiato) — wizard colours.
- `serde_yaml` (already a dep) — YAML serialization.

**New code:**
- `src/capture.rs` — the capture algorithm (socket calls, mapping,
  cwd policy, command best-effort, YAML write).
- `src/capture_ui.rs` — the ratatui wizard (step-by-step form).
- `main.rs` — subcommand dispatch (`capture` vs the default switcher).
- `herdr-plugin.toml` — the `capture` entrypoint + `nav-capture` action.
- `source::Template` etc. — add `Serialize` derive (currently
  `Deserialize`-only) so we can write YAML. **Non-breaking** (adding a
  derive; existing deserialization unchanged).

**Vertical integration point (the spine):** every phase from C2 onward
produces a YAML file that the **existing `^t` open-with-template flow**
(`source::read_templates` → `build_workspace_from_template`) can
immediately read and apply. The capture feature plugs into the apply
feature at the file-format boundary — no new socket path, no change to
`^t`. That is the single most important integration contract.

## Phase C1 — Subcommand dispatch + capture spine (no UI)

**Aspect:** make `herdr-nav` a multi-entrypoint binary and prove the
daemon calls work end-to-end. The thinnest vertical slice: no wizard,
no YAML, just "read the current workspace and print it."

- `main.rs`: parse `args()`; if first arg is `capture`, run the capture
  path; else run the existing switcher (unchanged). No `clap` — a
  hand-rolled `match` on `std::env::args().nth(1)` keeps the binary
  zero-dep-increase and matches the popup's one-shot nature.
- `src/capture.rs`: `capture_summary(socket) -> Summary` —
  `workspace.list` (find focused ws) → `tab.list` (filter + sort by
  `number`) → per tab `layout.export {tab_id}` → collect
  `{workspace_label, tabs: [{tab_label, pane_count, split_depth}]}`.
- `capture` subcommand prints the summary as plain text to stdout and
  exits 0. No terminal setup, no ratatui — this is a CLI probe, not a
  popup yet.
- **Gotcha locked in:** use `layout.export`, never `pane.layout` (the
  latter ignores its `tab_id` param — spike §3).
- **Exit criteria:** `./target/release/herdr-nav capture` against a live
  herdr prints the active workspace's tabs and per-tab pane counts,
  matching what `pane.list` shows. A multi-split tab shows the right
  pane count. No wizard, no file written.

## Phase C2 — layout.export → Template mapping + write a usable YAML

**Aspect:** the core value — turn the live tree into a `Template` YAML
that the existing `^t` flow can read and apply. Still no wizard; name
and policies come from CLI flags so the slice is scriptable/testable.

- `source.rs`: add `#[derive(Serialize)]` to `Template`, `TemplateTab`,
  `Layout`, `PaneNode` (alongside the existing `Deserialize`). Verify
  round-trip: `read_templates` parses what we write.
- `src/capture.rs`:
  - `map_export_to_template(export_root, tab_label, base_cwd, cwd_policy)
    -> TemplateTab` — recurse the `layout.export` tree:
    - `split{direction:"right"}` → `Layout{direction:"v"}`;
      `"down"` → `"h"`.
    - `ratio` (0.0–1.0) → `Layout.ratio` (`round(ratio*100)`; `0.0` → `0`).
    - `first`/`second` → `panes[0]`/`panes[1]`.
    - `pane{cwd,label}` → `PaneNode::Pane{cwd, name: label, command: None}`
      (command lands in C3).
  - `derive_base_cwd(first_tab_export) -> String` — the first pane's cwd
    (root → first leaf), per spec §6.
  - `apply_cwd_policy(pane_cwd, base, policy)` —
    `Relative`: under base → `./…`-relative; not under base → absolute;
    equals base → `.`. `Absolute`: as-is. `Inherit`: `None`.
    Reuse `expand_path` for `~`/`$HOME`.
  - `capture_to_yaml(socket, name, cwd_policy) -> String` — assemble the
    `Template` (name, no match globs, `default: false`, one tab per
    `tab.list` entry) and `serde_yaml::to_string`.
- `main.rs` `capture` subcommand: accept `--name <name>` (default =
  workspace label) and `--cwd-policy relative|absolute|inherit`
  (default `relative`). Write to
  `~/.config/herdr/templates/<name>.yaml` (create `templates/` if
  missing). Print the written path. No clash handling yet (C5); a
  clash overwrites silently in this phase (acceptable for the slice —
  flagged in the exit criteria).
- **Vertical integration test:** after writing, run the existing `^t`
  flow on a dir and confirm the new template appears in the picker and
  `Enter` builds a workspace whose structure matches the source.
- **Exit criteria:** `herdr-nav capture --name rust-dev` writes
  `~/.config/herdr/templates/rust-dev.yaml`; the file round-trips
  through `read_templates`; `^t` lists it and builds a workspace with
  the right tab/split structure and cwds. All pane `command:`s are
  blank (C3 fills them). Silent overwrite on clash (C5 fixes).

## Phase C3 — pane.process_info command capture (best-effort)

**Aspect:** fill each pane's `command:` from the running process group,
with honest annotation. The fidelity ceiling acknowledged in the spec.

- `src/capture.rs`:
  - `best_effort_command(socket, pane_id, pane_cwd) -> (Option<String>,
    Option<String>)` → `(command, annotation)`:
    - `pane.process_info {pane_id}` → `foreground_processes[]`.
    - **Plain shell:** if the only foreground process `name` is in
      `{fish, bash, zsh, sh, dash, ksh, tcsh, csh}` → `(None, None)`.
    - **Non-shell:** pick the non-shell process whose `cwd` == pane cwd
      (smallest `pid` tiebreak); `command` = its `cmdline`;
      `annotation` = `format!("best-effort: captured from pane {pane_id}
        process {name}; verify")`.
    - **No match:** `(None, Some("best-effort: …; no confident match"))`.
  - Wire into the mapper: each `PaneNode::Pane` gets `command` + the
    annotation emitted as a `# <annotation>` comment line above it.
- `serde_yaml` does not emit comments natively. **Decision:** emit the
  annotation as a YAML comment via a small post-processing pass
  (string replace before write) OR as a `# best-effort:` line in a
  top-of-file header keyed by pane id. **Recommended:** a per-pane
  comment immediately above the `command:` line, inserted by a
  serializer wrapper that knows the pane order. Keep it simple: build
  the YAML string, then inject comments by pane index. (Exact mechanism
  is an implementation detail for the phase PR; the contract is "the
  comment is adjacent to the guessed command.")
- `main.rs`: add `--command-policy keep|blank` (default `keep`).
  `blank` → force all `command:` to `None` (no process_info calls).
- **Exit criteria:** a workspace with a `pi` agent pane and a plain
  `fish` pane captures `command: pi …` (annotated) and `command: null`
  respectively; the YAML carries `# best-effort:` comments on guesses;
  `--command-policy blank` emits all-blank commands with no
  process_info calls. `^t` still round-trips the file.

## Phase C4 — The wizard (step-by-step ratatui form)

**Aspect:** the interactive surface — the "ask some questions" flow.
Replaces the CLI flags as the default; flags remain as a non-interactive
escape hatch. The biggest UI phase.

- `src/capture_ui.rs`: a ratatui wizard, one question per screen,
  reusing `theme::Theme` and the popup geometry (80%×80%, same as the
  switcher). `crossterm` backend, same `KEY_DEBOUNCE` discipline as
  `main.rs` (legacy keyboard mode double-fire guard).
- **Steps** (spec §8):
  1. **Scope confirm** — read-only: workspace label, `N` tabs, `M`
     panes, `K` panes with a guessed command. `Enter` proceeds, `Esc`
     aborts (no write).
  2. **Template name** (required) — text field, default = workspace
     label. Sanitize to a safe filename stem.
  3. **match globs + default flag** — repeatable glob entry + a
     `default` toggle. Blank = no globs.
  4. **Command policy** — `keep` / `blank` (default `keep`).
  5. **cwd policy** — `relative` / `absolute` / `inherit` (default
     `relative`).
  6. **Per-tab names** — one editable field per tab, pre-filled from
     `tab.list` labels (fallback `tab<N>` for empty/bare-number
     labels). All tabs on one screen.
  7. **Review** — read-only YAML preview. `Enter` writes, `Esc` back.
- The wizard holds the in-progress answers in a `CaptureForm` struct;
  on the review step's `Enter`, it calls into `capture.rs`
  (`capture_to_yaml` with the assembled policies) and writes the file.
  No clash handling yet (C5).
- `main.rs`: `capture` with **no** flags → run the wizard; with
  `--name` etc. → non-interactive (C2/C3 path, for scripts/tests).
- **Exit criteria:** `herdr-nav capture` opens the wizard; walk all 7
  steps, `Enter` writes the YAML to `~/.config/herdr/templates/<name>.yaml`;
  `Esc` at any step aborts without writing; the written file round-trips
  through `^t`. CLI-flag mode still works.

## Phase C5 — Name clash + editor handoff

**Aspect:** the safety and finish — don't clobber existing templates,
and the optional in-flow `$EDITOR` fine-tune.

- `src/capture_ui.rs` (or `capture.rs`): after the review step's write
  attempt, if `~/.config/herdr/templates/<name>.yaml` exists, insert a
  **clash prompt** screen: *overwrite / cancel / rename*.
  - `overwrite` → write (replace).
  - `cancel` → abort, no write.
  - `rename` → loop back to the name step with the current name
    pre-filled.
  - Clash check happens **before** write, so a cancel never leaves a
    half-written file.
- After a successful write (clash-resolved or no clash), an **editor
  prompt** screen: *Open in $EDITOR now?* yes/no.
  - **yes** → `exec` `$VISUAL` (fallback `$EDITOR`, fallback `vi`) on
    the written path. The popup pane process is **replaced** by the
    editor (no fork, no wait, no post-edit validation — spec §9). When
    the editor exits, the pane process ends and the popup closes.
  - **no** → toast the written path, close the popup.
- `exec` (not `spawn`) is the contract: the plugin process becomes the
  editor. This is why there's no post-edit validation — the plugin
  isn't alive after `exec`. A malformed YAML is surfaced by
  `read_templates` on the next `^t` (it already logs parse errors to
  stderr — `source.rs`).
- **Exit criteria:** capturing with an existing name shows the clash
  prompt; overwrite replaces, cancel aborts, rename loops back; after
  write, the editor prompt offers `$EDITOR`; "yes" opens the editor on
  the file (edit in place), "no" toasts the path and closes. A
  deliberately-malformed hand-edit is reported by `^t` on next use, not
  by the capture flow.

## Cross-phase: tests

Each phase adds tests under `tests/` (house style: fixtures +
integration per phase):
- **C1:** a `capture_summary` unit test with a mocked socket response
  (reuse the spike's raw JSON as a fixture) asserting the tab/pane
  counts parse correctly.
- **C2:** a `map_export_to_template` unit test with the spike's nested
  `layout.export` fixture (wP:t2: right-split over a down-split)
  asserting the `Template` shape; a round-trip test
  (serialize → `read_templates` → assert equal).
- **C3:** `best_effort_command` unit tests: plain-shell → `None`,
  multi-process non-shell → picks the cwd-matching one, no-match →
  `None` + annotation.
- **C4:** wizard is UI — tested via the existing manual-trigger path
  (`just link` + bind key). State transitions (step advance/back/abort)
  can be unit-tested on `CaptureForm` without ratatui.
- **C5:** clash resolution logic unit-tested (overwrite/cancel/rename
  branching) on a `resolve_clash` pure function; editor `exec` is
  manual (can't unit-test `exec`).

## Cross-phase: docs

- `doc/keybinding.md`: add the `nav-capture` action row and the
  (informational) capture keybind note.
- `doc/templates.md`: add a "Capturing a workspace as a template"
  section pointing at the new action, with the caveat that `command:`
  is best-effort.
- `spec/capture-template-spec.md` is already committed; no change.

## What is explicitly NOT in these phases

- No change to the `^t` apply path (`build_workspace_from_template`).
- No `layout.apply` round-trip (spec §11 deferred).
- No per-pane command review screen in the wizard (spec §11 deferred;
  per-pane command fixing is in `$EDITOR`).
- No match-glob auto-suggestion (spec §11 deferred).
- No capture of scrollback, agent transcripts, or pixel sizes.
- No special-casing of agent panes (captured like any pane — spec §5).

## Sequencing and dependencies

```
C1 (spine)  →  C2 (mapping + write)  →  C3 (commands)
                                          ↓
                          C4 (wizard)  →  C5 (clash + editor)
```

C2 is the load-bearing vertical integration point: it's where the
capture feature first meets the existing `^t` apply feature at the
file-format boundary. C3 enriches the same file. C4 swaps the CLI
surface for the wizard. C5 adds safety + finish. Each phase is
independently mergeable and each leaves the binary working.
