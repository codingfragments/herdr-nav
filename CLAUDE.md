# herdr-nav — Claude Code conventions

This is a new Herdr plugin (not a port), so the gitflow below is adapted
from the sister `herdr-flash` / `herdr-zextract` repos — same shape, minus
the migration-from-Zellij angle.

## Gitflow

- **Always create a branch before making any change or commit** — even one-line fixes.
- Branch prefixes:
  - `bug/` — bug fixes
  - `feature/` — new features
  - `phase/` — larger milestone / multi-commit work (e.g. each item in
    PLANNING.md §17 Implementation phases)
  - `release/<version>` — release prep (version bump + CHANGELOG)
- If the type is unclear, ask before creating the branch.
- Every branch lands via PR to `main` — no direct commits to `main`.
- Stay on the working branch until the PR is explicitly merged; switch back
  to `main` only after merge.

## Workflow

- Commit frequently within a branch as work progresses.
- Do not push without user approval — summarise what's testable and wait
  for "push" / "looks good".
- End each phase with: what to test, how to trigger it, what works vs
  what's still a stub.
- Implementation follows the phase sequence in PLANNING.md §17, one
  `phase/` branch/PR per phase, in order. There are **15 phases**: each
  is a vertical, end-to-end testable slice delivering one independent
  improvement and focused on a single aspect. The last three are
  hardening (edge cases + performance), then docs + public-facing, then
  release/CI-CD — deliberately last. Open questions in §15 are resolved
  in the Phase 1 spike (session daemon IPC) and per-provider phases
  (agents/zoxide/plugins data sources) before the phases that depend on
  them.

## Release process

Two-step merge flow, same shape as the sister plugins:

1. **Code PRs** (`bug/`, `feature/`, `phase/`) — code changes only; merge
   first.
2. **Release PR** (`release/<x.y.z>`) — separate branch/PR containing:
   - `Cargo.toml` version bump (semver: patch/minor/major)
   - `CHANGELOG.md` entry
3. Merge the release PR to `main`.
4. Tag the resulting merge commit: `git tag v<x.y.z> && git push origin v<x.y.z>`
5. Pushing the tag triggers `.github/workflows/release.yml`, which builds
   release binaries for all target triples (see PLANNING.md §8), computes
   SHA-256 checksums, and publishes the GitHub release automatically.

> Never push a `v*.*.*` tag from a feature branch or before the release PR
> is merged.

## Project conventions

- Target platforms: macOS (arm64 + x86_64) and Linux (x86_64 + aarch64) —
  no Windows support planned. No Intel Mac release binary (build from
  source there).
- Versioning follows semver.
- Once a manifest exists, plugin registration/config lives in Herdr's own
  config, not this repo — this repo owns the binary and its
  `herdr-plugin.toml` only.
- The socket client opens a **fresh connection per request** — Herdr's
  socket server closes after one request; reusing a connection yields
  `BrokenPipe`. Never add a persistent-connection path.
- The popup PTY runs in legacy keyboard mode: every key event arrives as
  `KeyEventKind::Press`, so a single tap can double-fire. The event loop
  debounces identical consecutive presses within `KEY_DEBOUNCE` (40ms).
