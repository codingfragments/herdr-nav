# herdr-nav

[Releases](https://github.com/codingfragments/herdr-nav/releases) ·
[PLANNING.md](PLANNING.md) for the full design and phase plan.

A [Herdr](https://herdr.dev) plugin — a popup target switcher: one
keystroke opens it, you aim, Enter moves you, it closes. Two derived
modes (Browse = tree, Search = flat fuzzy-ranked leaves), five target
groups (Session, Agents, Pinned dirs, zoxide, Plugins), a single-shape
live preview for every kind. The palette auto-follows Herdr's `[theme]`
setting — no per-plugin color config needed.

## What it does

The switcher answers one question — *where do I want to be?* — for every
kind of place Herdr knows about: a live pane, an agent waiting on you, a
directory you visit often, a plugin you want to poke. It is a modal popup,
not a persistent panel. Browse a tree of target groups, or type to flip
into a fuzzy-ranked flat list of every leaf; the preview always shows
what's under the cursor; Enter moves you and closes.

Typical workflows:
- Jump to a pane you can't see by name, not by tab-hunting.
- Switch to an agent and see its transcript / blocked question first.
- Open a recently-used directory (zoxide) or a pinned dir as a fresh
  workspace — a worktree-space inside a git repo.
- Jump to a plugin and pick one of its declared actions.

See [`doc/navigation.md`](doc/navigation.md) for the two modes and the
five groups, and [`doc/keybinding.md`](doc/keybinding.md) for the full
keymap.

## Docs

| Doc | Covers |
|---|---|
| [`doc/config-reference.md`](doc/config-reference.md) | Full `config.toml` schema (sources, filters, theme, preview) |
| [`doc/keybinding.md`](doc/keybinding.md) | Shipped actions, binding a key, adding your own |
| [`doc/navigation.md`](doc/navigation.md) | The two modes, the five groups, the switch action |
| [`doc/query-filters.md`](doc/query-filters.md) | Query filter syntax: `@pane`, `kind:`, `!`, group scope, composition |
| [`doc/templates.md`](doc/templates.md) | Workspace template file syntax + capturing a workspace as a template |
| [`doc/use-cases.md`](doc/use-cases.md) | Worked walkthroughs |
| [`doc/env-vars.md`](doc/env-vars.md) | The environment variables involved |

## Configuration

Optional — the plugin works with zero config, using built-in defaults
(the five groups in spec order, default scoring/bias, preview on). To
change group order, zoxide limit, preview width, search scoring, or
provider bias, copy [`config.example.toml`](config.example.toml) to:

```sh
herdr plugin config-dir herdr-nav   # prints the target directory
```

as `config.toml`. Full schema: [`doc/config-reference.md`](doc/config-reference.md).

## Keybinding

The plugin ships two actions — `nav-open` (the switcher) and
`nav-capture` (capture the current workspace as a template) — bound
via `[[keys.command]]` entries with `type = "plugin_action"` in your
own `~/.config/herdr/config.toml`. Herdr owns all keybindings; the
plugin never binds its own keys. Full binding reference is in
[`doc/keybinding.md`](doc/keybinding.md).

```toml
[[keys.command]]
key = "Ctrl k"
action = "nav-open"
type = "plugin_action"

[[keys.command]]
key = "prefix+ctrl+t"
action = "nav-capture"
type = "plugin_action"
description = "capture workspace as template"
```

Press `?` inside the switcher popup for the full keybinding dialog.

## Build

A native Rust binary — no WASM target involved.

**Requires a working Rust/`cargo` toolchain** (e.g. via
[rustup](https://rustup.rs)) on the machine doing the build.

```sh
git clone https://github.com/codingfragments/herdr-nav
cd herdr-nav
cargo build --release
```

Supported targets: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`. Tagged releases ship prebuilt binaries for
all three via GitHub Actions — see
[PLANNING.md §8](PLANNING.md#8-ci--release-plan-github-actions).

## Install

Requires [Herdr](https://herdr.dev/install.sh) itself, and a working
Rust/`cargo` toolchain on the machine running the install command.

**Option A — `herdr plugin install` (recommended):**
```sh
herdr plugin install codingfragments/herdr-nav --ref v0.1.0
```
Clones the repo at that ref, runs the `[[build]]` step
(`cargo build --release`) `herdr-plugin.toml` declares, and registers the
plugin — one command, no separate build step. Pin `--ref` to a tagged
version (`v0.1.0`) for a reproducible install, or use `--ref latest` to
track the newest tagged release (a rolling tag, force-moved on every
release — see [CHANGELOG.md](CHANGELOG.md) for what's in it). Omit `--ref`
entirely to track `main`. Add `--yes` to skip the confirmation prompt
(needed for non-interactive/scripted installs, e.g. from dotfiles).

To update later, just re-run the same command — it re-resolves the ref
and rebuilds in place; there's no separate `herdr plugin update`.

Once installed, bind a key — see [`doc/keybinding.md`](doc/keybinding.md).

## Platform support

Built and tested for macOS (Apple Silicon) and Linux (x86_64 + aarch64).
No Windows support planned. No Intel Mac (`x86_64-apple-darwin`) release
binary — build from source with `cargo build --release` if needed on that
architecture.

## License

MIT.
