# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Scaffold

- Initial repo structure: `Cargo.toml`, `herdr-plugin.toml`, `justfile`,
  `CLAUDE.md`, `README.md`, `PLANNING.md`, `config.example.toml`,
  `LICENSE`, `.gitignore`.
- Source modules (stubs): `main.rs`, `socket_client.rs`, `config.rs`,
  `nav.rs`, `source.rs`, `preview.rs`, `render.rs`.
- Docs (stubs): `doc/config-reference.md`, `doc/env-vars.md`,
  `doc/keybinding.md`, `doc/navigation.md`, `doc/use-cases.md`.
- CI: `ci.yml` (fmt/clippy/test on macOS + Linux) and `release.yml`
  (tag-triggered, 3 target triples, SHA-256, rolling `latest` release).
- Manifest with one `[[actions]]` (`nav-open`), `[[build]]` step, one
  popup pane (`switcher`) at 80%x80%, clamped to 100x34 cells.
- Placeholder event loop: renders a scaffold banner, exits on `Esc`.
  No switcher functionality yet — lands in the 15-phase sequence in
  PLANNING.md §17 (popup shell + Session tree browse is Phase 1).
- Normative spec, interactive prototype, and reference figures stored in
  `spec/` so implementation does not depend on files in `~/Downloads`.
