//! Config loading for `herdr-nav` (spec §13).
//!
//! Config lookup order (rex-migration.md R2): `$NAV_CONFIG`, then
//! `~/.config/cf-nav/config.toml`, then the legacy plugin dir
//! (`$HERDR_PLUGIN_CONFIG_DIR/config.toml`) — the first existing file
//! wins. A missing or malformed config never crashes the plugin —
//! parse errors are reported on stderr and built-in defaults are used.
//!
//! `[host.rex]` / `[host.herdr]` tables override `groups` and `theme`
//! per host. The palette (§9) is resolved by `theme.rs` (nav's config
//! `[theme]` → herdr's config → the default).
//!
//! **Phase 10:** the schema is now wired. `groups` controls display
//! order; `bias` feeds the search ranking; `zoxide_limit` caps the
//! zoxide provider; `preview`/`expand`/`scoring` are parsed and
//! available (some take effect in later phases).
#![allow(dead_code)]

use serde::Deserialize;

// ── Built-in defaults (spec §13) ──────────────────────────────────────────────

/// The spec §4 fixed group order.
pub const DEFAULT_GROUPS: &[&str] = &["session", "agents", "pinned", "zoxide", "plugins"];

/// The spec §6.3 default bias values.
pub const DEFAULT_BIAS: BiasCfg = BiasCfg {
    agent_waiting: 6.0,
    pane: 4.0,
    pinned: 3.0,
    agent: 2.0,
    zoxide: 0.0,
    plugin: -2.0,
};

/// The spec §13 default preview config.
pub const DEFAULT_PREVIEW: PreviewCfg = PreviewCfg {
    enabled: true,
    width_pct: 56,
    min_cols: 60,
};

/// The spec §13 default expand config.
pub const DEFAULT_EXPAND: ExpandCfg = ExpandCfg {
    session_default: String::new(), // "active"
    restore_ttl_secs: 600,
};

/// The spec §6.2 default scoring weights.
pub const DEFAULT_SCORING: ScoringCfg = ScoringCfg {
    consecutive: 8.0,
    gap: 0.4,
    prefix: 0.6,
    word_boundary: 4.0,
};

pub const DEFAULT_ZOXIDE_LIMIT: u32 = 50;

/// Default cap for the "extend zoxide" keybind (Phase 16): the larger
/// limit used when a search finds no directory results and the user
/// presses `Tab` to surface deeper frecency dirs. Configurable via
/// `zoxide_extend_limit`.
pub const DEFAULT_ZOXIDE_EXTEND_LIMIT: u32 = 1000;

// ── Config structs ───────────────────────────────────────────────────────────

/// Top-level config. All fields optional — every key falls back to a
/// built-in default so the plugin works with zero config.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub log_level: String,
    /// Root target groups in display order (spec §4/§13). Empty →
    /// the spec default order.
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub open_key: String,
    #[serde(default = "default_zoxide_limit")]
    pub zoxide_limit: u32,
    /// Cap for the "extend zoxide" keybind (Phase 16). Default 1000.
    #[serde(default = "default_zoxide_extend_limit")]
    pub zoxide_extend_limit: u32,
    #[serde(default = "default_preview")]
    pub preview: PreviewCfg,
    #[serde(default = "default_expand")]
    pub expand: ExpandCfg,
    #[serde(default = "default_scoring")]
    pub scoring: ScoringCfg,
    #[serde(default = "default_bias")]
    pub bias: BiasCfg,
    /// Per-host overrides (rex-migration.md R2): `[host.rex]` and
    /// `[host.herdr]` tables, each optionally carrying `groups` and
    /// `theme`. Applied on top of the top-level keys for that host.
    #[serde(default)]
    pub host: HostOverrides,
}

/// One host's override table (`[host.rex]` / `[host.herdr]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HostCfg {
    /// Override the display order / set of root groups for this host.
    #[serde(default)]
    pub groups: Vec<String>,
    /// A theme name (resolved by `theme.rs`; overrides the `[theme]`
    /// sections for this host).
    #[serde(default)]
    pub theme: Option<String>,
}

/// The `[host]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HostOverrides {
    #[serde(default)]
    pub rex: HostCfg,
    #[serde(default)]
    pub herdr: HostCfg,
}

fn default_zoxide_limit() -> u32 {
    DEFAULT_ZOXIDE_LIMIT
}
fn default_zoxide_extend_limit() -> u32 {
    DEFAULT_ZOXIDE_EXTEND_LIMIT
}
fn default_preview() -> PreviewCfg {
    DEFAULT_PREVIEW
}
fn default_expand() -> ExpandCfg {
    ExpandCfg {
        session_default: String::new(),
        restore_ttl_secs: 600,
    }
}
fn default_scoring() -> ScoringCfg {
    DEFAULT_SCORING
}
fn default_bias() -> BiasCfg {
    DEFAULT_BIAS
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: String::new(),
            groups: Vec::new(),
            open_key: String::new(),
            zoxide_limit: DEFAULT_ZOXIDE_LIMIT,
            zoxide_extend_limit: DEFAULT_ZOXIDE_EXTEND_LIMIT,
            preview: DEFAULT_PREVIEW,
            expand: default_expand(),
            scoring: DEFAULT_SCORING,
            bias: DEFAULT_BIAS,
            host: HostOverrides::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_width_pct")]
    pub width_pct: u32,
    #[serde(default = "default_min_cols")]
    pub min_cols: u32,
}

fn default_true() -> bool {
    true
}
fn default_width_pct() -> u32 {
    56
}
fn default_min_cols() -> u32 {
    60
}

impl Default for PreviewCfg {
    fn default() -> Self {
        DEFAULT_PREVIEW
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExpandCfg {
    #[serde(default)]
    pub session_default: String,
    #[serde(default = "default_restore_ttl")]
    pub restore_ttl_secs: u32,
}

fn default_restore_ttl() -> u32 {
    600
}

impl Default for ExpandCfg {
    fn default() -> Self {
        ExpandCfg {
            session_default: String::new(),
            restore_ttl_secs: 600,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScoringCfg {
    #[serde(default = "default_consecutive")]
    pub consecutive: f64,
    #[serde(default = "default_gap")]
    pub gap: f64,
    #[serde(default = "default_prefix")]
    pub prefix: f64,
    #[serde(default = "default_word_boundary")]
    pub word_boundary: f64,
}

fn default_consecutive() -> f64 {
    8.0
}
fn default_gap() -> f64 {
    0.4
}
fn default_prefix() -> f64 {
    0.6
}
fn default_word_boundary() -> f64 {
    4.0
}

impl Default for ScoringCfg {
    fn default() -> Self {
        DEFAULT_SCORING
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BiasCfg {
    #[serde(default = "default_agent_waiting")]
    pub agent_waiting: f64,
    #[serde(default = "default_pane_bias")]
    pub pane: f64,
    #[serde(default = "default_pinned_bias")]
    pub pinned: f64,
    #[serde(default = "default_agent_bias")]
    pub agent: f64,
    #[serde(default = "default_zoxide_bias")]
    pub zoxide: f64,
    #[serde(default = "default_plugin_bias")]
    pub plugin: f64,
}

fn default_agent_waiting() -> f64 {
    6.0
}
fn default_pane_bias() -> f64 {
    4.0
}
fn default_pinned_bias() -> f64 {
    3.0
}
fn default_agent_bias() -> f64 {
    2.0
}
fn default_zoxide_bias() -> f64 {
    0.0
}
fn default_plugin_bias() -> f64 {
    -2.0
}

impl Default for BiasCfg {
    fn default() -> Self {
        DEFAULT_BIAS
    }
}

// ── Loading ─────────────────────────────────────────────────────────────────

impl Config {
    /// Load config from the R2 lookup order (`$NAV_CONFIG` →
    /// `~/.config/cf-nav/config.toml` → the legacy plugin dir), falling
    /// back to built-in defaults on any error (missing file, parse
    /// error). Parse errors are reported on stderr.
    pub fn load() -> Self {
        let Some(path) = crate::paths::config_file() else {
            return Config::default();
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Config::default(),
        };
        match toml::from_str::<Config>(&text) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("herdr-nav: {} parse error: {e}", path.display());
                Config::default()
            }
        }
    }

    /// The `[host.<host>]` override table for a host name ("herdr",
    /// "rex"); unknown hosts get an empty table (no overrides).
    fn host_cfg(&self, host: &str) -> &HostCfg {
        match host {
            "rex" => &self.host.rex,
            "herdr" => &self.host.herdr,
            _ => &EMPTY_HOST_CFG,
        }
    }

    /// The theme name override for a host (`[host.<host>].theme`),
    /// when set.
    pub fn host_theme(&self, host: &str) -> Option<String> {
        self.host_cfg(host).theme.clone()
    }

    /// Resolve the `groups` list for a host to a validated set of
    /// group names in display order: the `[host.<host>].groups`
    /// override when set, else the top-level `groups`, else the spec
    /// default order. Unknown names are dropped (stderr warning).
    pub fn resolved_groups(&self, host: &str) -> Vec<String> {
        let host_groups = self.host_cfg(host).groups.clone();
        let configured: &[String] = if host_groups.is_empty() {
            &self.groups
        } else {
            &host_groups
        };
        if configured.is_empty() {
            return DEFAULT_GROUPS.iter().map(|s| s.to_string()).collect();
        }
        let valid: Vec<String> = configured
            .iter()
            .filter(|g| DEFAULT_GROUPS.contains(&g.as_str()))
            .cloned()
            .collect();
        if valid.len() != configured.len() {
            eprintln!(
                "herdr-nav: config has unknown groups in `groups` (ignored); valid: {:?}",
                DEFAULT_GROUPS
            );
        }
        if valid.is_empty() {
            DEFAULT_GROUPS.iter().map(|s| s.to_string()).collect()
        } else {
            valid
        }
    }
}

static EMPTY_HOST_CFG: HostCfg = HostCfg {
    groups: Vec::new(),
    theme: None,
};

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;

    #[test]
    fn defaults_match_spec() {
        assert_eq!(
            DEFAULT_GROUPS,
            &["session", "agents", "pinned", "zoxide", "plugins"]
        );
        assert_eq!(DEFAULT_BIAS.pane, 4.0);
        assert_eq!(DEFAULT_BIAS.plugin, -2.0);
        assert_eq!(DEFAULT_ZOXIDE_LIMIT, 50);
        assert_eq!(DEFAULT_ZOXIDE_EXTEND_LIMIT, 1000);
        assert_eq!(DEFAULT_PREVIEW.width_pct, 56);
        assert_eq!(DEFAULT_SCORING.consecutive, 8.0);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
groups = ["session", "agents"]
zoxide_limit = 10
[preview]
enabled = false
width_pct = 40
[scoring]
consecutive = 10.0
[bias]
pane = 8
plugin = -5
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.groups, vec!["session", "agents"]);
        assert_eq!(c.zoxide_limit, 10);
        assert_eq!(c.zoxide_extend_limit, DEFAULT_ZOXIDE_EXTEND_LIMIT); // unspecified
        assert!(!c.preview.enabled);
        assert_eq!(c.preview.width_pct, 40);
        assert_eq!(c.scoring.consecutive, 10.0);
        assert_eq!(c.bias.pane, 8.0);
        assert_eq!(c.bias.plugin, -5.0);
        // unspecified bias fields keep defaults
        assert_eq!(c.bias.agent, 2.0);
    }

    #[test]
    fn parse_empty_uses_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.groups.is_empty()); // resolved_groups() fills defaults
        assert_eq!(c.zoxide_limit, DEFAULT_ZOXIDE_LIMIT);
        assert_eq!(c.zoxide_extend_limit, DEFAULT_ZOXIDE_EXTEND_LIMIT);
        assert_eq!(c.bias.pane, 4.0);
    }

    #[test]
    fn resolved_groups_drops_unknown() {
        let mut c = Config::default();
        c.groups = vec![
            "session".to_string(),
            "bogus".to_string(),
            "agents".to_string(),
        ];
        let r = c.resolved_groups("herdr");
        assert_eq!(r, vec!["session", "agents"]);
    }

    #[test]
    fn resolved_groups_empty_uses_default_order() {
        let c = Config::default();
        let r = c.resolved_groups("herdr");
        assert_eq!(r, DEFAULT_GROUPS);
    }

    #[test]
    fn host_override_wins_for_that_host_only() {
        let toml = r#"
groups = ["session", "agents"]
[host.rex]
groups = ["session", "agents", "pinned", "zoxide"]
theme = "tokyo-night"
[host.herdr]
theme = "vesper"
"#;
        let c: Config = toml::from_str(toml).unwrap();
        // rex: the override groups, not the top-level ones.
        assert_eq!(
            c.resolved_groups("rex"),
            vec!["session", "agents", "pinned", "zoxide"]
        );
        // herdr: the top-level groups; the rex override doesn't leak.
        assert_eq!(c.resolved_groups("herdr"), vec!["session", "agents"]);
        // theme overrides are host-scoped too.
        assert_eq!(c.host_theme("rex").as_deref(), Some("tokyo-night"));
        assert_eq!(c.host_theme("herdr").as_deref(), Some("vesper"));
        assert_eq!(c.host_theme("none"), None);
        // unknown hosts get the top-level groups.
        assert_eq!(c.resolved_groups("none"), vec!["session", "agents"]);
    }

    #[test]
    fn parse_partial_preview_keeps_defaults() {
        let toml = r#"
[preview]
width_pct = 70
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.preview.width_pct, 70);
        assert!(c.preview.enabled); // default
        assert_eq!(c.preview.min_cols, 60); // default
    }

    /// The shipped `config.example.toml` must parse cleanly and every
    /// value must match the built-in defaults. This is the "keep the
    /// example in sync with the code" guard — if a default changes in
    /// `config.rs`, this test fails until the example is updated.
    #[test]
    fn example_file_matches_defaults() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read config.example.toml: {e}"));
        let c: Config =
            toml::from_str(&text).unwrap_or_else(|e| panic!("parse config.example.toml: {e}"));

        // Top-level defaults.
        assert_eq!(c.log_level, "info");
        assert_eq!(
            c.groups,
            DEFAULT_GROUPS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(c.open_key, "ctrl-k");
        assert_eq!(c.zoxide_limit, DEFAULT_ZOXIDE_LIMIT);
        assert_eq!(c.zoxide_extend_limit, DEFAULT_ZOXIDE_EXTEND_LIMIT);

        // [preview]
        assert_eq!(c.preview.enabled, DEFAULT_PREVIEW.enabled);
        assert_eq!(c.preview.width_pct, DEFAULT_PREVIEW.width_pct);
        assert_eq!(c.preview.min_cols, DEFAULT_PREVIEW.min_cols);

        // [expand]
        assert_eq!(c.expand.session_default, "active");
        assert_eq!(c.expand.restore_ttl_secs, DEFAULT_EXPAND.restore_ttl_secs);

        // [scoring]
        assert_eq!(c.scoring.consecutive, DEFAULT_SCORING.consecutive);
        assert_eq!(c.scoring.gap, DEFAULT_SCORING.gap);
        assert_eq!(c.scoring.prefix, DEFAULT_SCORING.prefix);
        assert_eq!(c.scoring.word_boundary, DEFAULT_SCORING.word_boundary);

        // [bias]
        assert_eq!(c.bias.agent_waiting, DEFAULT_BIAS.agent_waiting);
        assert_eq!(c.bias.pane, DEFAULT_BIAS.pane);
        assert_eq!(c.bias.pinned, DEFAULT_BIAS.pinned);
        assert_eq!(c.bias.agent, DEFAULT_BIAS.agent);
        assert_eq!(c.bias.zoxide, DEFAULT_BIAS.zoxide);
        assert_eq!(c.bias.plugin, DEFAULT_BIAS.plugin);
    }
}
