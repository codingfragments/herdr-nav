//! Theme palette — auto-follows Herdr's `[theme]` setting.
#![allow(clippy::doc_lazy_continuation, clippy::doc_markdown)]
//!
//! Reads `~/.config/herdr/config.toml` at launch, resolves the theme
//! name to one of the 18 built-in palettes Herdr ships (catppuccin,
//! catppuccin-latte, terminal, tokyo-night, …, vesper), and applies
//! `[theme.custom]` overrides. Falls back to Herdr's default
//! (catppuccin) if the file is missing or malformed — never crashes.
//!
//! The palette mirrors Herdr's own `Palette` struct
//! (`src/app/state.rs` in the herdr repo) so the switcher's colors
//! match the host exactly. The "terminal" theme uses ANSI named
//! colors so the terminal resolves them — the popup automatically
//! matches whatever the terminal is themed to.
//!
//! Spec §9 amendment: the palette is no longer fixed to Catppuccin
//! Macchiato; it follows Herdr's theme. The kind-glyph colour mapping
//! uses the theme's palette tokens (mauve/teal/blue/yellow/green/red
//! + accent), with workspace/group sharing mauve and tab/zox sharing
//! the teal token. Disambiguated by glyph + label, never colour alone (spec 9).

use ratatui::style::Color;

// ── Palette ───────────────────────────────────────────────────────────────────

/// The resolved colour palette, mirroring Herdr's `Palette`. Every
/// slot has a Catppuccin (Mocha) default matching Herdr's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Primary accent (highlight, active borders, plugin glyph).
    pub accent: Color,
    /// Deepest background (dialog/popup bg).
    pub panel_bg: Color,
    /// Selection row background.
    pub selection_bg: Color,
    /// Dim surface (twisties, meta, borders).
    pub surface0: Color,
    /// Brighter surface.
    pub surface1: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text.
    pub subtext0: Color,
    /// Overlay/dim text.
    pub overlay0: Color,
    pub mauve: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub teal: Color,
    pub peach: Color,
}

impl Palette {
    /// Herdr's default theme is "catppuccin" (Mocha).
    pub fn default() -> Self {
        Self::catppuccin()
    }

    /// Resolve a theme by name (canonicalised). Returns None for unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        match canonical_theme_name(name)? {
            "catppuccin" => Some(Self::catppuccin()),
            "catppuccin-latte" => Some(Self::catppuccin_latte()),
            "terminal" => Some(Self::terminal()),
            "tokyo-night" => Some(Self::tokyo_night()),
            "tokyo-night-day" => Some(Self::tokyo_night_day()),
            "dracula" => Some(Self::dracula()),
            "nord" => Some(Self::nord()),
            "gruvbox" => Some(Self::gruvbox()),
            "gruvbox-light" => Some(Self::gruvbox_light()),
            "one-dark" => Some(Self::one_dark()),
            "one-light" => Some(Self::one_light()),
            "solarized" => Some(Self::solarized()),
            "solarized-light" => Some(Self::solarized_light()),
            "kanagawa" => Some(Self::kanagawa()),
            "kanagawa-lotus" => Some(Self::kanagawa_lotus()),
            "rose-pine" => Some(Self::rose_pine()),
            "rose-pine-dawn" => Some(Self::rose_pine_dawn()),
            "vesper" => Some(Self::vesper()),
            _ => None,
        }
    }

    // ── Built-in themes (values from herdr/src/app/state.rs) ────────────────

    pub fn catppuccin() -> Self {
        Self {
            accent: Color::Rgb(137, 180, 250),
            panel_bg: Color::Rgb(24, 24, 37),
            selection_bg: Color::Rgb(49, 50, 68),
            surface0: Color::Rgb(49, 50, 68),
            surface1: Color::Rgb(69, 71, 90),
            text: Color::Rgb(205, 214, 244),
            subtext0: Color::Rgb(166, 173, 200),
            overlay0: Color::Rgb(108, 112, 134),
            mauve: Color::Rgb(203, 166, 247),
            green: Color::Rgb(166, 227, 161),
            yellow: Color::Rgb(249, 226, 175),
            red: Color::Rgb(243, 139, 168),
            blue: Color::Rgb(137, 180, 250),
            teal: Color::Rgb(148, 226, 213),
            peach: Color::Rgb(250, 179, 135),
        }
    }

    pub fn catppuccin_latte() -> Self {
        Self {
            accent: Color::Rgb(30, 102, 245),
            panel_bg: Color::Rgb(239, 241, 245),
            selection_bg: Color::Rgb(189, 208, 245),
            surface0: Color::Rgb(204, 208, 218),
            surface1: Color::Rgb(188, 192, 204),
            text: Color::Rgb(76, 79, 105),
            subtext0: Color::Rgb(108, 111, 133),
            overlay0: Color::Rgb(156, 160, 176),
            mauve: Color::Rgb(136, 57, 239),
            green: Color::Rgb(64, 160, 43),
            yellow: Color::Rgb(223, 142, 29),
            red: Color::Rgb(210, 15, 57),
            blue: Color::Rgb(30, 102, 245),
            teal: Color::Rgb(23, 146, 153),
            peach: Color::Rgb(254, 100, 11),
        }
    }

    /// "terminal" — ANSI named colors; the terminal resolves them, so
    /// the popup automatically matches whatever the terminal is themed to.
    pub fn terminal() -> Self {
        Self {
            accent: Color::Blue,
            panel_bg: Color::Reset,
            selection_bg: Color::Reset,
            surface0: Color::Reset,
            surface1: Color::DarkGray,
            text: Color::Reset,
            subtext0: Color::Gray,
            overlay0: Color::Gray,
            mauve: Color::Gray,
            green: Color::Green,
            yellow: Color::Yellow,
            red: Color::LightRed,
            blue: Color::Blue,
            teal: Color::Cyan,
            peach: Color::Yellow,
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            accent: Color::Rgb(122, 162, 247),
            panel_bg: Color::Rgb(26, 27, 38),
            selection_bg: Color::Rgb(45, 54, 80),
            surface0: Color::Rgb(36, 40, 59),
            surface1: Color::Rgb(65, 72, 104),
            text: Color::Rgb(192, 202, 245),
            subtext0: Color::Rgb(169, 177, 214),
            overlay0: Color::Rgb(86, 95, 137),
            mauve: Color::Rgb(187, 154, 247),
            green: Color::Rgb(158, 206, 106),
            yellow: Color::Rgb(224, 175, 104),
            red: Color::Rgb(247, 118, 142),
            blue: Color::Rgb(122, 162, 247),
            teal: Color::Rgb(125, 207, 255),
            peach: Color::Rgb(255, 158, 100),
        }
    }

    pub fn tokyo_night_day() -> Self {
        Self {
            accent: Color::Rgb(46, 125, 233),
            panel_bg: Color::Rgb(225, 226, 231),
            selection_bg: Color::Rgb(182, 202, 231),
            surface0: Color::Rgb(196, 200, 218),
            surface1: Color::Rgb(168, 174, 203),
            text: Color::Rgb(55, 96, 191),
            subtext0: Color::Rgb(97, 114, 176),
            overlay0: Color::Rgb(137, 144, 179),
            mauve: Color::Rgb(120, 71, 189),
            green: Color::Rgb(88, 117, 57),
            yellow: Color::Rgb(140, 108, 62),
            red: Color::Rgb(245, 42, 101),
            blue: Color::Rgb(46, 125, 233),
            teal: Color::Rgb(17, 140, 116),
            peach: Color::Rgb(177, 92, 0),
        }
    }

    pub fn dracula() -> Self {
        Self {
            accent: Color::Rgb(189, 147, 249),
            panel_bg: Color::Rgb(40, 42, 54),
            selection_bg: Color::Rgb(70, 63, 93),
            surface0: Color::Rgb(68, 71, 90),
            surface1: Color::Rgb(98, 114, 164),
            text: Color::Rgb(248, 248, 242),
            subtext0: Color::Rgb(210, 210, 220),
            overlay0: Color::Rgb(98, 114, 164),
            mauve: Color::Rgb(255, 121, 198),
            green: Color::Rgb(80, 250, 123),
            yellow: Color::Rgb(241, 250, 140),
            red: Color::Rgb(255, 85, 85),
            blue: Color::Rgb(139, 233, 253),
            teal: Color::Rgb(139, 233, 253),
            peach: Color::Rgb(255, 184, 108),
        }
    }

    pub fn nord() -> Self {
        Self {
            accent: Color::Rgb(136, 192, 208),
            panel_bg: Color::Rgb(46, 52, 64),
            selection_bg: Color::Rgb(64, 80, 93),
            surface0: Color::Rgb(59, 66, 82),
            surface1: Color::Rgb(67, 76, 94),
            text: Color::Rgb(236, 239, 244),
            subtext0: Color::Rgb(216, 222, 233),
            overlay0: Color::Rgb(76, 86, 106),
            mauve: Color::Rgb(180, 142, 173),
            green: Color::Rgb(163, 190, 140),
            yellow: Color::Rgb(235, 203, 139),
            red: Color::Rgb(191, 97, 106),
            blue: Color::Rgb(129, 161, 193),
            teal: Color::Rgb(143, 188, 187),
            peach: Color::Rgb(208, 135, 112),
        }
    }

    pub fn gruvbox() -> Self {
        Self {
            accent: Color::Rgb(215, 153, 33),
            panel_bg: Color::Rgb(40, 40, 40),
            selection_bg: Color::Rgb(75, 63, 39),
            surface0: Color::Rgb(60, 56, 54),
            surface1: Color::Rgb(80, 73, 69),
            text: Color::Rgb(235, 219, 178),
            subtext0: Color::Rgb(213, 196, 161),
            overlay0: Color::Rgb(146, 131, 116),
            mauve: Color::Rgb(211, 134, 155),
            green: Color::Rgb(184, 187, 38),
            yellow: Color::Rgb(250, 189, 47),
            red: Color::Rgb(251, 73, 52),
            blue: Color::Rgb(131, 165, 152),
            teal: Color::Rgb(142, 192, 124),
            peach: Color::Rgb(254, 128, 25),
        }
    }

    pub fn gruvbox_light() -> Self {
        Self {
            accent: Color::Rgb(7, 102, 120),
            panel_bg: Color::Rgb(251, 241, 199),
            selection_bg: Color::Rgb(235, 219, 178),
            surface0: Color::Rgb(235, 219, 178),
            surface1: Color::Rgb(213, 196, 161),
            text: Color::Rgb(60, 56, 54),
            subtext0: Color::Rgb(80, 73, 69),
            overlay0: Color::Rgb(146, 131, 116),
            mauve: Color::Rgb(143, 63, 113),
            green: Color::Rgb(121, 116, 14),
            yellow: Color::Rgb(181, 118, 20),
            red: Color::Rgb(157, 0, 6),
            blue: Color::Rgb(7, 102, 120),
            teal: Color::Rgb(66, 123, 88),
            peach: Color::Rgb(175, 58, 3),
        }
    }

    pub fn one_dark() -> Self {
        Self {
            accent: Color::Rgb(97, 175, 239),
            panel_bg: Color::Rgb(40, 44, 52),
            selection_bg: Color::Rgb(51, 70, 89),
            surface0: Color::Rgb(44, 49, 58),
            surface1: Color::Rgb(62, 68, 81),
            text: Color::Rgb(171, 178, 191),
            subtext0: Color::Rgb(150, 156, 168),
            overlay0: Color::Rgb(92, 99, 112),
            mauve: Color::Rgb(198, 120, 221),
            green: Color::Rgb(152, 195, 121),
            yellow: Color::Rgb(229, 192, 123),
            red: Color::Rgb(224, 108, 117),
            blue: Color::Rgb(97, 175, 239),
            teal: Color::Rgb(86, 182, 194),
            peach: Color::Rgb(209, 154, 102),
        }
    }

    pub fn one_light() -> Self {
        Self {
            accent: Color::Rgb(64, 120, 242),
            panel_bg: Color::Rgb(250, 250, 250),
            selection_bg: Color::Rgb(205, 219, 248),
            surface0: Color::Rgb(240, 240, 241),
            surface1: Color::Rgb(229, 229, 230),
            text: Color::Rgb(56, 58, 66),
            subtext0: Color::Rgb(104, 107, 119),
            overlay0: Color::Rgb(160, 161, 167),
            mauve: Color::Rgb(166, 38, 164),
            green: Color::Rgb(80, 161, 79),
            yellow: Color::Rgb(193, 132, 1),
            red: Color::Rgb(228, 86, 73),
            blue: Color::Rgb(64, 120, 242),
            teal: Color::Rgb(1, 132, 188),
            peach: Color::Rgb(152, 104, 1),
        }
    }

    pub fn solarized() -> Self {
        Self {
            accent: Color::Rgb(38, 139, 210),
            panel_bg: Color::Rgb(0, 43, 54),
            selection_bg: Color::Rgb(8, 62, 85),
            surface0: Color::Rgb(7, 54, 66),
            surface1: Color::Rgb(88, 110, 117),
            text: Color::Rgb(147, 161, 161),
            subtext0: Color::Rgb(131, 148, 150),
            overlay0: Color::Rgb(88, 110, 117),
            mauve: Color::Rgb(211, 54, 130),
            green: Color::Rgb(133, 153, 0),
            yellow: Color::Rgb(181, 137, 0),
            red: Color::Rgb(220, 50, 47),
            blue: Color::Rgb(38, 139, 210),
            teal: Color::Rgb(42, 161, 152),
            peach: Color::Rgb(203, 75, 22),
        }
    }

    pub fn solarized_light() -> Self {
        Self {
            accent: Color::Rgb(38, 139, 210),
            panel_bg: Color::Rgb(253, 246, 227),
            selection_bg: Color::Rgb(201, 220, 223),
            surface0: Color::Rgb(238, 232, 213),
            surface1: Color::Rgb(147, 161, 161),
            text: Color::Rgb(101, 123, 131),
            subtext0: Color::Rgb(131, 148, 150),
            overlay0: Color::Rgb(147, 161, 161),
            mauve: Color::Rgb(211, 54, 130),
            green: Color::Rgb(133, 153, 0),
            yellow: Color::Rgb(181, 137, 0),
            red: Color::Rgb(220, 50, 47),
            blue: Color::Rgb(38, 139, 210),
            teal: Color::Rgb(42, 161, 152),
            peach: Color::Rgb(203, 75, 22),
        }
    }

    pub fn kanagawa() -> Self {
        Self {
            accent: Color::Rgb(126, 156, 216),
            panel_bg: Color::Rgb(31, 31, 40),
            selection_bg: Color::Rgb(50, 56, 75),
            surface0: Color::Rgb(42, 42, 55),
            surface1: Color::Rgb(54, 54, 70),
            text: Color::Rgb(220, 215, 186),
            subtext0: Color::Rgb(200, 195, 170),
            overlay0: Color::Rgb(114, 113, 105),
            mauve: Color::Rgb(149, 127, 184),
            green: Color::Rgb(118, 148, 106),
            yellow: Color::Rgb(192, 163, 110),
            red: Color::Rgb(195, 64, 67),
            blue: Color::Rgb(126, 156, 216),
            teal: Color::Rgb(127, 180, 202),
            peach: Color::Rgb(255, 160, 102),
        }
    }

    pub fn kanagawa_lotus() -> Self {
        Self {
            accent: Color::Rgb(77, 105, 155),
            panel_bg: Color::Rgb(242, 236, 188),
            selection_bg: Color::Rgb(220, 213, 172),
            surface0: Color::Rgb(220, 213, 172),
            surface1: Color::Rgb(201, 203, 209),
            text: Color::Rgb(84, 84, 100),
            subtext0: Color::Rgb(67, 67, 108),
            overlay0: Color::Rgb(160, 156, 172),
            mauve: Color::Rgb(98, 76, 131),
            green: Color::Rgb(111, 137, 78),
            yellow: Color::Rgb(119, 113, 63),
            red: Color::Rgb(200, 64, 83),
            blue: Color::Rgb(77, 105, 155),
            teal: Color::Rgb(78, 140, 162),
            peach: Color::Rgb(204, 109, 0),
        }
    }

    pub fn rose_pine() -> Self {
        Self {
            accent: Color::Rgb(196, 167, 231),
            panel_bg: Color::Rgb(25, 23, 36),
            selection_bg: Color::Rgb(59, 52, 75),
            surface0: Color::Rgb(31, 29, 46),
            surface1: Color::Rgb(38, 35, 58),
            text: Color::Rgb(224, 222, 244),
            subtext0: Color::Rgb(200, 197, 220),
            overlay0: Color::Rgb(110, 106, 134),
            mauve: Color::Rgb(196, 167, 231),
            green: Color::Rgb(49, 116, 143),
            yellow: Color::Rgb(246, 193, 119),
            red: Color::Rgb(235, 111, 146),
            blue: Color::Rgb(49, 116, 143),
            teal: Color::Rgb(156, 207, 216),
            peach: Color::Rgb(234, 154, 151),
        }
    }

    pub fn rose_pine_dawn() -> Self {
        Self {
            accent: Color::Rgb(144, 122, 169),
            panel_bg: Color::Rgb(250, 244, 237),
            selection_bg: Color::Rgb(242, 233, 225),
            surface0: Color::Rgb(242, 233, 225),
            surface1: Color::Rgb(255, 250, 243),
            text: Color::Rgb(70, 66, 97),
            subtext0: Color::Rgb(121, 117, 147),
            overlay0: Color::Rgb(152, 147, 165),
            mauve: Color::Rgb(144, 122, 169),
            green: Color::Rgb(40, 105, 131),
            yellow: Color::Rgb(234, 157, 52),
            red: Color::Rgb(180, 99, 122),
            blue: Color::Rgb(40, 105, 131),
            teal: Color::Rgb(86, 148, 159),
            peach: Color::Rgb(215, 130, 126),
        }
    }

    pub fn vesper() -> Self {
        Self {
            accent: Color::Rgb(255, 199, 153),
            panel_bg: Color::Rgb(26, 26, 26),
            selection_bg: Color::Rgb(35, 35, 35),
            surface0: Color::Rgb(35, 35, 35),
            surface1: Color::Rgb(40, 40, 40),
            text: Color::Rgb(255, 255, 255),
            subtext0: Color::Rgb(160, 160, 160),
            overlay0: Color::Rgb(92, 92, 92),
            mauve: Color::Rgb(255, 209, 168),
            green: Color::Rgb(153, 255, 228),
            yellow: Color::Rgb(255, 199, 153),
            red: Color::Rgb(255, 128, 128),
            blue: Color::Rgb(176, 176, 176),
            teal: Color::Rgb(102, 221, 204),
            peach: Color::Rgb(255, 199, 153),
        }
    }

    /// Apply `[theme.custom]` overrides (hex/rgb/named/reset).
    pub fn with_override(mut self, field: &str, value: &str) -> Self {
        let c = parse_color(value);
        match field {
            "accent" => self.accent = c,
            "panel_bg" => self.panel_bg = c,
            "selection_bg" => self.selection_bg = c,
            "surface0" => self.surface0 = c,
            "surface1" => self.surface1 = c,
            "text" => self.text = c,
            "subtext0" => self.subtext0 = c,
            "overlay0" => self.overlay0 = c,
            "mauve" => self.mauve = c,
            "green" => self.green = c,
            "yellow" => self.yellow = c,
            "red" => self.red = c,
            "blue" => self.blue = c,
            "teal" => self.teal = c,
            "peach" => self.peach = c,
            _ => {}
        }
        self
    }
}

// ── Theme name canonicalisation (from herdr/src/config/theme.rs) ────────────

/// Canonicalise a theme name (lowercase, replace spaces/underscores
/// with hyphens, then match known aliases). Returns None for unknown.
pub fn canonical_theme_name(name: &str) -> Option<&'static str> {
    let key = name.to_lowercase().replace([' ', '_'], "-");
    match key.as_str() {
        "catppuccin" | "catppuccin-mocha" => Some("catppuccin"),
        "catppuccin-latte" | "latte" | "light" => Some("catppuccin-latte"),
        "terminal" => Some("terminal"),
        "tokyo-night" | "tokyonight" => Some("tokyo-night"),
        "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => Some("tokyo-night-day"),
        "dracula" => Some("dracula"),
        "nord" => Some("nord"),
        "gruvbox" | "gruvbox-dark" => Some("gruvbox"),
        "gruvbox-light" => Some("gruvbox-light"),
        "one-dark" | "onedark" => Some("one-dark"),
        "one-light" | "onelight" => Some("one-light"),
        "solarized" | "solarized-dark" => Some("solarized"),
        "solarized-light" => Some("solarized-light"),
        "kanagawa" => Some("kanagawa"),
        "kanagawa-lotus" | "lotus" => Some("kanagawa-lotus"),
        "rose-pine" | "rosepine" => Some("rose-pine"),
        "rose-pine-dawn" | "rosepine-dawn" | "dawn" => Some("rose-pine-dawn"),
        "vesper" => Some("vesper"),
        _ => None,
    }
}

// ── Color parser (from herdr/src/config/theme.rs) ────────────────────────────

/// Parse a color string: `#rrggbb`, `#rgb`, `rgb(r,g,b)`, named ANSI,
/// or `reset`/`default`/`none`/`transparent` → `Color::Reset`.
/// Unknown → `Color::Cyan` (matches Herdr's fallback).
pub fn parse_color(s: &str) -> Color {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "reset" | "default" | "none" | "transparent" => return Color::Reset,
        _ => {}
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        } else if hex.len() == 3 {
            let chars: Vec<u8> = hex
                .chars()
                .filter_map(|c| u8::from_str_radix(&c.to_string(), 16).ok())
                .collect();
            if chars.len() == 3 {
                return Color::Rgb(chars[0] * 17, chars[1] * 17, chars[2] * 17);
            }
        }
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].trim().parse::<u8>(),
                parts[1].trim().parse::<u8>(),
                parts[2].trim().parse::<u8>(),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    match s.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::Cyan,
    }
}

// ── Load from ~/.config/herdr/config.toml ───────────────────────────────────

/// Load the palette from `~/.config/herdr/config.toml`. Reads
/// `[theme].name` and `[theme.custom]` overrides. Falls back to
/// Herdr's default (catppuccin) if the file is missing, the theme
/// is unknown, or parsing fails — never crashes.
pub fn load() -> Palette {
    let Some(home) = std::env::var("HOME").ok() else {
        return Palette::default();
    };
    let path = format!("{home}/.config/herdr/config.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Palette::default();
    };
    parse_config(&content).unwrap_or_else(|_| Palette::default())
}

/// Parse the config TOML content into a palette. Minimal line-based
/// parser (avoids a full TOML dep just for the theme block): finds
/// `[theme]` name and `[theme.custom]` overrides.
fn parse_config(content: &str) -> Result<Palette, String> {
    let mut name = String::new();
    let mut custom: Vec<(String, String)> = Vec::new();
    let mut in_theme = false;
    let mut in_theme_custom = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match trimmed {
            "[theme]" => {
                in_theme = true;
                in_theme_custom = false;
            }
            "[theme.custom]" => {
                in_theme = false;
                in_theme_custom = true;
            }
            s if s.starts_with('[') => {
                in_theme = false;
                in_theme_custom = false;
            }
            _ => {
                if in_theme {
                    if let Some(v) = trimmed
                        .strip_prefix("name")
                        .and_then(|s| s.trim().strip_prefix('='))
                    {
                        name = v.trim().trim_matches('"').to_string();
                    }
                } else if in_theme_custom {
                    if let Some((k, v)) = trimmed.split_once('=') {
                        custom.push((k.trim().to_string(), v.trim().trim_matches('"').to_string()));
                    }
                }
            }
        }
    }
    let mut palette = if name.is_empty() {
        Palette::default()
    } else {
        Palette::from_name(&name).unwrap_or_else(Palette::default)
    };
    for (k, v) in custom {
        palette = palette.with_override(&k, &v);
    }
    Ok(palette)
}

// ── Kind-glyph colour mapping (§9) ───────────────────────────────────────────

/// The colour for a kind's glyph, derived from the active palette.
/// Two pairs share a colour (workspace/group → mauve, tab/zox →
/// teal) — disambiguated by glyph + label, never colour alone (§9).
pub fn kind_color(p: &Palette, kind: crate::nav::Kind) -> Color {
    use crate::nav::Kind;
    match kind {
        Kind::Workspace | Kind::Group => p.mauve,
        Kind::Tab | Kind::Zox => p.teal,
        Kind::Pane => p.blue,
        Kind::Dir => p.yellow,
        Kind::Plugin => p.accent,
        Kind::Agent => p.green,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::Kind;

    #[test]
    fn resolves_known_themes() {
        assert_eq!(canonical_theme_name("catppuccin"), Some("catppuccin"));
        assert_eq!(canonical_theme_name("Catppuccin Mocha"), Some("catppuccin"));
        assert_eq!(canonical_theme_name("tokyo_night"), Some("tokyo-night"));
        assert_eq!(
            canonical_theme_name("rose-pine-dawn"),
            Some("rose-pine-dawn")
        );
        assert_eq!(canonical_theme_name("unknown"), None);
    }

    #[test]
    fn from_name_returns_palette() {
        let p = Palette::from_name("terminal").unwrap();
        assert_eq!(p.accent, Color::Blue);
        let p2 = Palette::from_name("catppuccin").unwrap();
        assert_eq!(p2.text, Color::Rgb(205, 214, 244));
    }

    #[test]
    fn unknown_name_falls_back_to_default() {
        let p = Palette::from_name("nonexistent").unwrap_or_else(Palette::default);
        assert_eq!(p, Palette::catppuccin());
    }

    #[test]
    fn override_replaces_token() {
        let p = Palette::catppuccin().with_override("red", "#ff0000");
        assert_eq!(p.red, Color::Rgb(255, 0, 0));
    }

    #[test]
    fn override_reset_clears_color() {
        let p = Palette::catppuccin().with_override("panel_bg", "reset");
        assert_eq!(p.panel_bg, Color::Reset);
    }

    #[test]
    fn parse_color_hex_and_named() {
        assert_eq!(parse_color("#f5a97f"), Color::Rgb(245, 169, 127));
        assert_eq!(parse_color("#abc"), Color::Rgb(170, 187, 204));
        assert_eq!(parse_color("blue"), Color::Blue);
        assert_eq!(parse_color("rgb(1,2,3)"), Color::Rgb(1, 2, 3));
        assert_eq!(parse_color("reset"), Color::Reset);
    }

    #[test]
    fn kind_colors_distinct_enough() {
        let p = Palette::catppuccin();
        // workspace/group share mauve.
        assert_eq!(kind_color(&p, Kind::Workspace), p.mauve);
        assert_eq!(kind_color(&p, Kind::Group), p.mauve);
        // tab/zox share teal.
        assert_eq!(kind_color(&p, Kind::Tab), p.teal);
        assert_eq!(kind_color(&p, Kind::Zox), p.teal);
        // the rest are distinct.
        assert_eq!(kind_color(&p, Kind::Pane), p.blue);
        assert_eq!(kind_color(&p, Kind::Dir), p.yellow);
        assert_eq!(kind_color(&p, Kind::Plugin), p.accent);
        assert_eq!(kind_color(&p, Kind::Agent), p.green);
    }

    #[test]
    fn parse_config_reads_theme_name() {
        let toml = r##"
[theme]
name = "tokyo-night"
[theme.custom]
red = "#ff0000"
"##;
        let p = parse_config(toml).unwrap();
        assert_eq!(p.text, Color::Rgb(192, 202, 245)); // tokyo-night text
        assert_eq!(p.red, Color::Rgb(255, 0, 0)); // overridden
    }

    #[test]
    fn parse_config_missing_theme_uses_default() {
        let toml = "[other]\nkey = \"value\"\n";
        let p = parse_config(toml).unwrap();
        assert_eq!(p, Palette::catppuccin());
    }
}
