plugin_id := "herdr-nav"

# list available recipes
default:
    @just --list

# build the release binary
build:
    cargo build --release

# format, lint, and test - run before committing
check: fmt-check clippy test

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

clippy:
    cargo clippy --release -- -D warnings

test:
    cargo test

# register this checkout with a running local Herdr as a linked (dev) plugin
link: build
    herdr plugin link .

# remove the linked dev plugin
unlink:
    herdr plugin unlink {{ plugin_id }}

# rebuild and re-link in one step, e.g. after pulling changes
relink: unlink link

# open the plugin's popup pane directly, bypassing any keybind
open:
    herdr plugin pane open --plugin {{ plugin_id }} --entrypoint switcher --placement popup

# swap this checkout's release build into the mise-managed herdr-nav
# (the binary the Rex launcher resolves) — one-time backup to *.orig
rex-swap: build
    #!/bin/sh
    dst=$(mise which herdr-nav) || exit 1
    case "$dst" in "$HOME"/.local/share/mise/installs/*) ;; *)
      echo "not a mise install: $dst"; exit 1 ;; esac
    if [ ! -f "$dst.orig" ]; then
      cp "$dst" "$dst.orig" && echo "backed up release binary → $dst.orig"
    fi
    cp target/release/herdr-nav "$dst" \
      && echo "dev build → $dst ($(./target/release/herdr-nav --version))"

# restore the mise-managed release binary (undo rex-swap)
rex-restore:
    #!/bin/sh
    dst=$(mise which herdr-nav) || exit 1
    if [ ! -f "$dst.orig" ]; then
      echo "no backup at $dst.orig — nothing to restore"; exit 1
    fi
    cp "$dst.orig" "$dst" && echo "release binary restored → $dst"
