//! Config loading for `herdr-nav` (spec §13).
//!
//! Config lives at `$HERDR_PLUGIN_CONFIG_DIR/config.toml`. A missing or
//! malformed config never crashes the plugin — parse errors are reported
//! on stderr and built-in defaults are used. The Catppuccin Macchiato
//! palette (§9) is fixed and not configurable.
//!
//! **Status: scaffold only.** The real schema (groups order, zoxide
//! limit, preview, expand, scoring, bias) is wired in Phase 10
//! (PLANNING.md §17); earlier phases use the built-in defaults.

use serde::Deserialize;

/// Top-level config. All fields optional — every key falls back to a
/// built-in default so the plugin works with zero config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub log_level: String,
    /// Root target groups in display order (spec §4/§13).
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub zoxide_limit: u32,
    #[serde(default)]
    pub preview: PreviewCfg,
    #[serde(default)]
    pub expand: ExpandCfg,
    #[serde(default)]
    pub scoring: ScoringCfg,
    #[serde(default)]
    pub bias: BiasCfg,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PreviewCfg {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub width_pct: u32,
    #[serde(default)]
    pub min_cols: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExpandCfg {
    #[serde(default)]
    pub session_default: String,
    #[serde(default)]
    pub restore_ttl_secs: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScoringCfg {
    #[serde(default)]
    pub consecutive: f64,
    #[serde(default)]
    pub gap: f64,
    #[serde(default)]
    pub prefix: f64,
    #[serde(default)]
    pub word_boundary: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BiasCfg {
    #[serde(default)]
    pub agent_waiting: f64,
    #[serde(default)]
    pub pane: f64,
    #[serde(default)]
    pub pinned: f64,
    #[serde(default)]
    pub agent: f64,
    #[serde(default)]
    pub zoxide: f64,
    #[serde(default)]
    pub plugin: f64,
}

impl Config {
    /// Load config from `$HERDR_PLUGIN_CONFIG_DIR/config.toml`, falling
    /// back to built-in defaults on any error (missing file, parse error).
    pub fn load() -> Self {
        // TODO Phase 10: read $HERDR_PLUGIN_CONFIG_DIR/config.toml, parse,
        // report parse errors on stderr, fall back to defaults.
        Config::default()
    }
}
