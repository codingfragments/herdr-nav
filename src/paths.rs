//! Host-neutral config and data paths (rex-migration.md, Phase R2).
//!
//! nav used to keep everything under `~/.config/herdr/` because it only
//! ran as a herdr plugin. With Rex coming (R3), the files move to
//! nav's own dir — `~/.config/nav/` — with the herdr locations kept as
//! legacy fallbacks so existing setups keep working unchanged:
//!
//! | File | Lookup order (read) | Write target |
//! |---|---|---|
//! | `config.toml` | `$NAV_CONFIG` → `~/.config/nav/config.toml` → `$HERDR_PLUGIN_CONFIG_DIR/config.toml` | — |
//! | `targets.toml` | `~/.config/nav/` → `~/.config/herdr/` → `$HERDR_PLUGIN_CONFIG_DIR/` | nav path, unless a legacy file is the only one that exists (then there — no data fork) |
//! | `templates/*.yaml` | `~/.config/nav/templates/` → `~/.config/herdr/templates/` (first dir that exists) | first dir that exists, else nav's (created) |
//!
//! `targets.toml` (the pinned dirs) is shared by both hosts — it is
//! pure user data with nothing host-specific in it.

use std::path::PathBuf;

/// nav's own config/data dir: `~/.config/nav`. (`HOME` is expected to
/// be set in every real launch context; the relative degenerate case
/// just fails file ops later instead of panicking here.)
pub fn nav_dir() -> PathBuf {
    home().join(".config").join("nav")
}

/// herdr's config dir: `~/.config/herdr` (the legacy location).
pub fn herdr_dir() -> PathBuf {
    home().join(".config").join("herdr")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

// ── config.toml ──────────────────────────────────────────────────────────────

/// Every `config.toml` candidate, in lookup order: `$NAV_CONFIG` (a
/// full path, for launchers/tests), `~/.config/nav/config.toml`, then
/// the legacy plugin dir. The first existing file wins.
pub fn config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(p) = std::env::var_os("NAV_CONFIG") {
        if !p.is_empty() {
            candidates.push(PathBuf::from(p));
        }
    }
    candidates.push(nav_dir().join("config.toml"));
    if let Some(d) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        if !d.is_empty() {
            candidates.push(PathBuf::from(d).join("config.toml"));
        }
    }
    candidates
}

/// The config file to read: the first existing `config_candidates()`
/// entry, or None (defaults are used).
pub fn config_file() -> Option<PathBuf> {
    config_candidates().into_iter().find(|p| p.is_file())
}

// ── targets.toml (pinned dirs — shared by both hosts) ────────────────────────

/// Every `targets.toml` candidate, in lookup order: nav's dir, the
/// legacy herdr dir, then the plugin dir.
pub fn targets_read_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        nav_dir().join("targets.toml"),
        herdr_dir().join("targets.toml"),
    ];
    if let Some(d) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        if !d.is_empty() {
            candidates.push(PathBuf::from(d).join("targets.toml"));
        }
    }
    candidates
}

/// Where pin writes (`^p`) and unpin rewrites (`^u`) go: nav's file when
/// it exists; else the legacy herdr file when it is the only one that
/// exists (so an existing setup keeps its data instead of forking it);
/// else nav's path (created on first write).
pub fn targets_write_path() -> PathBuf {
    let nav = nav_dir().join("targets.toml");
    if nav.is_file() {
        return nav;
    }
    let herdr = herdr_dir().join("targets.toml");
    if herdr.is_file() {
        return herdr;
    }
    nav
}

// ── templates ───────────────────────────────────────────────────────────────
//
// Host-scoped (2026-11-20, R5 direction): Rex's templates are its OWN
// set — `~/.config/nav/rex/templates/` — deliberately NOT falling back
// to the herdr dirs. The herdr flow keeps the R2 chain
// (`~/.config/nav/templates/` → the legacy `~/.config/herdr/templates/`).

/// The template dir to READ for a host ("rex" gets its own set; every
/// other host follows the nav → herdr chain).
pub fn templates_read_dir_for(host: &str) -> PathBuf {
    let rex = nav_dir().join("rex").join("templates");
    if host == "rex" {
        return rex;
    }
    let nav = nav_dir().join("templates");
    if nav.is_dir() {
        return nav;
    }
    herdr_dir().join("templates")
}

/// The template dir to WRITE (capture) for a host: the first existing
/// dir in that host's read chain, else its preferred dir (created by
/// the writer).
pub fn templates_write_dir_for(host: &str) -> PathBuf {
    let chain = |host: &str| match host {
        "rex" => vec![nav_dir().join("rex").join("templates")],
        _ => vec![nav_dir().join("templates"), herdr_dir().join("templates")],
    };
    let dirs = chain(host);
    for d in &dirs {
        if d.is_dir() {
            return d.clone();
        }
    }
    dirs[0].clone()
}

#[cfg(test)]
pub(crate) mod test_support {
    /// Serializes tests that mutate process-global env vars (`HOME`,
    /// `NAV_CONFIG`, …) — cargo test runs them in parallel threads, and
    /// an unlocked env writer races every other env reader/writer.
    /// Lock this for the whole body of such a test.
    pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A throwaway HOME under the temp dir, so the path logic can be
    /// tested without touching the real `~/.config`.
    struct TempHome(PathBuf);
    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("herdr-nav-paths-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn nav(&self) -> PathBuf {
            self.0.join(".config").join("nav")
        }
        fn herdr(&self) -> PathBuf {
            self.0.join(".config").join("herdr")
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // SAFETY(tests): env-var mutation is process-global; ENV_LOCK
    // serializes every env-touching test in the binary.
    fn with_home(tag: &str, f: impl FnOnce(&TempHome)) {
        let _guard = test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let home = TempHome::new(tag);
        std::env::set_var("HOME", &home.0);
        std::env::remove_var("NAV_CONFIG");
        std::env::remove_var("HERDR_PLUGIN_CONFIG_DIR");
        f(&home);
        std::env::remove_var("HOME");
        // TempHome::drop removes the tree — done under the lock via guard order.
        drop(_guard);
    }

    #[test]
    fn targets_write_prefers_nav_when_it_exists() {
        with_home("tw-nav", |home| {
            std::fs::create_dir_all(home.nav()).unwrap();
            std::fs::write(home.nav().join("targets.toml"), "").unwrap();
            std::fs::create_dir_all(home.herdr()).unwrap();
            std::fs::write(home.herdr().join("targets.toml"), "").unwrap();
            assert_eq!(targets_write_path(), home.nav().join("targets.toml"));
        });
    }

    #[test]
    fn targets_write_stays_on_legacy_when_only_it_exists() {
        with_home("tw-legacy", |home| {
            std::fs::create_dir_all(home.herdr()).unwrap();
            std::fs::write(home.herdr().join("targets.toml"), "").unwrap();
            assert_eq!(targets_write_path(), home.herdr().join("targets.toml"));
        });
    }

    #[test]
    fn targets_write_creates_nav_when_nothing_exists() {
        with_home("tw-new", |_home| {
            assert_eq!(targets_write_path(), nav_dir().join("targets.toml"));
        });
    }

    #[test]
    fn templates_read_prefers_nav_dir_when_it_exists() {
        with_home("tr-nav", |home| {
            std::fs::create_dir_all(home.nav().join("templates")).unwrap();
            std::fs::create_dir_all(home.herdr().join("templates")).unwrap();
            assert_eq!(
                templates_read_dir_for("herdr"),
                home.nav().join("templates")
            );
        });
    }

    #[test]
    fn templates_read_falls_back_to_herdr() {
        with_home("tr-legacy", |home| {
            std::fs::create_dir_all(home.herdr().join("templates")).unwrap();
            assert_eq!(
                templates_read_dir_for("herdr"),
                home.herdr().join("templates")
            );
        });
    }

    #[test]
    fn templates_are_host_scoped() {
        with_home("tr-rex", |home| {
            // Under Rex: its own dir, never the herdr chain — even when
            // both herdr dirs exist and the rex dir doesn't.
            std::fs::create_dir_all(home.nav().join("templates")).unwrap();
            std::fs::create_dir_all(home.herdr().join("templates")).unwrap();
            assert_eq!(
                templates_read_dir_for("rex"),
                home.nav().join("rex").join("templates")
            );
            assert_eq!(
                templates_write_dir_for("rex"),
                home.nav().join("rex").join("templates")
            );
        });
    }

    #[test]
    fn templates_write_to_first_existing_else_nav() {
        with_home("tw2-legacy", |home| {
            // Only herdr's exists → write there.
            std::fs::create_dir_all(home.herdr().join("templates")).unwrap();
            assert_eq!(
                templates_write_dir_for("herdr"),
                home.herdr().join("templates")
            );
        });
        with_home("tw2-new", |_home| {
            // Neither exists → nav's (created by the writer).
            assert_eq!(
                templates_write_dir_for("herdr"),
                nav_dir().join("templates")
            );
        });
    }

    #[test]
    fn config_candidates_order() {
        with_home("cfg", |home| {
            std::fs::create_dir_all(home.nav()).unwrap();
            std::env::set_var("NAV_CONFIG", "/tmp/nav-config-override.toml");
            std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", "/tmp/plugin-dir");
            let c = config_candidates();
            assert_eq!(c.len(), 3);
            assert_eq!(c[0], Path::new("/tmp/nav-config-override.toml"));
            assert_eq!(c[1], home.nav().join("config.toml"));
            assert_eq!(c[2], Path::new("/tmp/plugin-dir/config.toml"));
            // Without the explicit override: nav's file, then the plugin dir.
            std::env::remove_var("NAV_CONFIG");
            let c = config_candidates();
            assert_eq!(c.len(), 2);
            assert_eq!(c[0], home.nav().join("config.toml"));
            assert_eq!(c[1], Path::new("/tmp/plugin-dir/config.toml"));
        });
    }
}
