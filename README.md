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
| [`doc/rex.md`](doc/rex.md) | Running under Rex: install, bindings, the delegated jump |
| [`doc/host-layer.md`](doc/host-layer.md) | nav's host glue (`host_ext`) and the Rex jump; links to the [muxhost reference](https://github.com/codingfragments/muxhost/tree/main/doc) for the shared layer |
| [`doc/env-vars.md`](doc/env-vars.md) | The environment variables involved |

## Configuration

Optional — the plugin works with zero config, using built-in defaults.
Config is looked up at `$NAV_CONFIG`, then `~/.config/nav/config.toml`,
then the legacy plugin dir (rex-migration R2)
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

### Herdr (the native host)

Requires [Herdr](https://herdr.dev) itself. On a tagged version, **no
Rust toolchain is needed** — the `[[build]]` step downloads the release
binary for your platform (sha256-checked) and only builds from source on
untagged commits or if the download fails:

```sh
herdr plugin install codingfragments/herdr-nav --ref v0.6.0
```

Pin `--ref` to a tagged version for a reproducible install, or `--ref
latest` to track the newest tagged release (a rolling tag, force-moved
on every release — see [CHANGELOG.md](CHANGELOG.md) for what's in it).
Omit `--ref` entirely to track `main` (builds from source). Add `--yes`
to skip the confirmation prompt (non-interactive/scripted installs,
e.g. from dotfiles). To update, re-run the same command — it re-resolves
the ref and rebuilds in place; there is no `herdr plugin update`.

Once installed, bind a key — see [`doc/keybinding.md`](doc/keybinding.md).

### Rex

Requires the `herdr-nav` binary on the login `PATH` and the Rex app's
**Remote Control** enabled once (needed for the switcher to make the app
follow your jumps — see [`doc/rex.md`](rex.md) for the details):

```sh
mise use -g github:codingfragments/herdr-nav   # pins the newest release
```

Then copy the launcher — [`rex/nav.lua`](rex/nav.lua) →
`~/.config/rex/plugins/nav.lua` — and add `require("plugins.nav")` to
`init.lua`. Bindings: `ctrl+s>s` / `cmd+k` (switcher), `ctrl+s>a` /
`cmd+shift+k` (agents), `ctrl+s>ctrl+t` (capture). Full Rex guide:
[`doc/rex.md`](doc/rex.md).

### Build from source (either host)

```sh
cargo install --git https://github.com/codingfragments/herdr-nav --tag v0.6.0
```

## Hosts

`herdr-nav` runs under [Herdr](https://herdr.dev) (its native host) and
[Rex](https://rex.dev) (from the v0.7.0 Rex migration on): the same
binary, one host layer ([`muxhost`](https://github.com/codingfragments/muxhost)
+ nav's `src/host_ext/`). Under Rex the Plugins group is hidden (Rex has
no plugin system), and jumps go through Remote Control. Install:
[`doc/rex.md`](doc/rex.md); the host layer in detail:
[`doc/host-layer.md`](doc/host-layer.md).

## Platform support

Built and tested for macOS (Apple Silicon) and Linux (x86_64 + aarch64).
No Windows support planned. No Intel Mac (`x86_64-apple-darwin`) release
binary — build from source with `cargo build --release` if needed on that
architecture.

## License

MIT.
