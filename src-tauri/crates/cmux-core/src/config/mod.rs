//! Mirador configuration: `~/.config/mirador/mirador.json` is the primary config;
//! themes can additionally be imported from Ghostty configs or wezterm
//! color-scheme TOML files. Resolution order (later wins):
//! builtin defaults → imported theme source → explicit mirador.json values.

pub mod ghostty;
pub mod wezterm;

use std::collections::HashMap;
use std::path::PathBuf;

use cmux_protocol::{CustomCommand, ResolvedColors, ResolvedConfig};
use serde::{Deserialize, Serialize};

/// Partial theme extracted from an import source; `None` fields fall back.
#[derive(Debug, Default, Clone)]
pub struct ImportedTheme {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection_background: Option<String>,
    pub palette: [Option<String>; 16],
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
}

/// On-disk mirador.json shape. Everything optional; defaults fill the gaps.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// "auto" (ghostty config if present), "ghostty", "wezterm", "builtin"
    pub theme_source: Option<String>,
    /// wezterm scheme name (in ~/.config/wezterm/colors) or a .toml path.
    pub wezterm_scheme: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub scrollback: Option<u32>,
    pub colors: Option<ColorOverrides>,
    /// Program panes run. Defaults to `$SHELL` on macOS/Linux and
    /// PowerShell 7 → Windows PowerShell → cmd.exe on Windows. Point it at
    /// e.g. `C:\Program Files\Git\bin\bash.exe` for a POSIX shell.
    pub shell: Option<String>,
    /// Arguments for a custom `shell`. When set, Mirador adds none of its
    /// own (no `-l`, no PowerShell prompt hook).
    pub shell_args: Option<Vec<String>>,
    pub keybindings: HashMap<String, String>,
    pub custom_commands: Vec<CustomCommand>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorOverrides {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection_background: Option<String>,
    pub palette: Option<Vec<String>>,
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("mirador")
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".config")
            })
            .join("mirador")
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("mirador.json")
}

/// The user's home directory (`%USERPROFILE%` on Windows).
pub fn home_dir() -> Option<String> {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok()
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok()
    }
}

/// Loads mirador.json (missing file → defaults; parse errors are reported but
/// fall back to defaults so a typo can't brick the terminal).
pub fn load() -> (Config, Option<String>) {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(cfg) => (cfg, None),
            Err(e) => (Config::default(), Some(format!("mirador.json: {e}"))),
        },
        Err(_) => (Config::default(), None),
    }
}

/// Writes a starter config if none exists, so users can discover options.
pub fn write_default_if_missing() {
    let path = config_path();
    if path.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(config_dir());
    let starter = serde_json::json!({
        "themeSource": "auto",
        "keybindings": default_keybindings(),
        "customCommands": [],
    });
    let _ = std::fs::write(path, serde_json::to_string_pretty(&starter).unwrap());
}

/// macOS: Cmd is free, so single-modifier bindings are safe.
#[cfg(target_os = "macos")]
const DEFAULT_KEYBINDINGS: &[(&str, &str)] = &[
    ("mod+t", "new_tab"),
    ("mod+w", "close_pane"),
    ("mod+shift+w", "close_tab"),
    ("mod+d", "split_right"),
    ("mod+shift+d", "split_down"),
    ("mod+alt+left", "focus_left"),
    ("mod+alt+right", "focus_right"),
    ("mod+alt+up", "focus_up"),
    ("mod+alt+down", "focus_down"),
    ("mod+b", "toggle_sidebar"),
    ("mod+k", "command_palette"),
    ("mod+i", "notifications"),
    ("mod+shift+]", "next_tab"),
    ("mod+shift+[", "prev_tab"),
    ("mod+1", "tab_1"),
    ("mod+2", "tab_2"),
    ("mod+3", "tab_3"),
    ("mod+4", "tab_4"),
    ("mod+5", "tab_5"),
    ("mod+6", "tab_6"),
    ("mod+7", "tab_7"),
    ("mod+8", "tab_8"),
    ("mod+9", "tab_9"),
];

/// Windows/Linux: plain Ctrl belongs to the program in the pane — Ctrl+C
/// interrupts, Ctrl+D is EOF, Ctrl+W kills a word. So the app's own
/// bindings live on Ctrl+Shift, as in every other terminal on these
/// platforms, and copy/paste get the usual Ctrl+Shift+C/V (there is no app
/// menu to provide them, unlike macOS).
///
/// Ctrl+Alt is avoided for letters: it *is* AltGr on international
/// keyboards, so those combos both fail to arrive as the expected letter
/// and would steal character input from anyone typing @ or [ with it. The
/// pairs macOS spells with Shift therefore take a second letter here
/// (split down is Ctrl+Shift+E) rather than a second modifier.
#[cfg(not(target_os = "macos"))]
const DEFAULT_KEYBINDINGS: &[(&str, &str)] = &[
    ("ctrl+shift+t", "new_tab"),
    ("ctrl+shift+w", "close_pane"),
    ("ctrl+shift+q", "close_tab"),
    ("ctrl+shift+d", "split_right"),
    ("ctrl+shift+e", "split_down"),
    ("ctrl+alt+left", "focus_left"),
    ("ctrl+alt+right", "focus_right"),
    ("ctrl+alt+up", "focus_up"),
    ("ctrl+alt+down", "focus_down"),
    ("ctrl+shift+b", "toggle_sidebar"),
    ("ctrl+shift+k", "command_palette"),
    ("ctrl+shift+i", "notifications"),
    ("ctrl+shift+c", "copy"),
    ("ctrl+shift+v", "paste"),
    ("ctrl+tab", "next_tab"),
    ("ctrl+shift+tab", "prev_tab"),
    ("alt+1", "tab_1"),
    ("alt+2", "tab_2"),
    ("alt+3", "tab_3"),
    ("alt+4", "tab_4"),
    ("alt+5", "tab_5"),
    ("alt+6", "tab_6"),
    ("alt+7", "tab_7"),
    ("alt+8", "tab_8"),
    ("alt+9", "tab_9"),
];

pub fn default_keybindings() -> HashMap<String, String> {
    DEFAULT_KEYBINDINGS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A monospace stack that exists on the platform without asking the user to
/// install anything.
fn default_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Menlo, Monaco, 'Courier New', monospace"
    }
    #[cfg(target_os = "windows")]
    {
        "'Cascadia Mono', Consolas, 'Courier New', monospace"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "'DejaVu Sans Mono', 'Liberation Mono', monospace"
    }
}

/// Catppuccin Mocha — the builtin fallback theme.
fn builtin_colors() -> ResolvedColors {
    ResolvedColors {
        background: "#1e1e2e".into(),
        foreground: "#cdd6f4".into(),
        cursor: "#f5e0dc".into(),
        selection_background: "#585b70".into(),
        palette: [
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
            "#bac2de", "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7",
            "#94e2d5", "#a6adc8",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    }
}

/// Resolves the user config into the runtime config the frontend consumes.
pub fn resolve(cfg: &Config) -> ResolvedConfig {
    let imported = import_theme(cfg).unwrap_or_default();

    let mut colors = builtin_colors();
    apply_imported(&mut colors, &imported);
    if let Some(o) = &cfg.colors {
        apply_overrides(&mut colors, o);
    }

    let mut keybindings = default_keybindings();
    for (accel, action) in &cfg.keybindings {
        if action.is_empty() || action == "none" {
            keybindings.remove(accel);
        } else {
            keybindings.insert(accel.clone(), action.clone());
        }
    }

    ResolvedConfig {
        font_family: cfg
            .font_family
            .clone()
            .or(imported.font_family)
            .unwrap_or_else(|| default_font_family().into()),
        font_size: cfg.font_size.or(imported.font_size).unwrap_or(13.0),
        scrollback: cfg.scrollback.unwrap_or(10_000),
        colors,
        keybindings,
        custom_commands: cfg.custom_commands.clone(),
    }
}

fn import_theme(cfg: &Config) -> Option<ImportedTheme> {
    match cfg.theme_source.as_deref().unwrap_or("auto") {
        "ghostty" => ghostty::import(),
        "wezterm" => wezterm::import(cfg.wezterm_scheme.as_deref()?),
        "builtin" | "none" => None,
        // auto: use Ghostty's config when the user has one (cmux-parity behavior)
        _ => {
            if ghostty::user_config_path().exists() {
                ghostty::import()
            } else {
                None
            }
        }
    }
}

fn apply_imported(colors: &mut ResolvedColors, t: &ImportedTheme) {
    if let Some(v) = &t.background {
        colors.background = v.clone();
    }
    if let Some(v) = &t.foreground {
        colors.foreground = v.clone();
    }
    if let Some(v) = &t.cursor {
        colors.cursor = v.clone();
    }
    if let Some(v) = &t.selection_background {
        colors.selection_background = v.clone();
    }
    for (i, c) in t.palette.iter().enumerate() {
        if let Some(c) = c {
            colors.palette[i] = c.clone();
        }
    }
}

fn apply_overrides(colors: &mut ResolvedColors, o: &ColorOverrides) {
    if let Some(v) = &o.background {
        colors.background = v.clone();
    }
    if let Some(v) = &o.foreground {
        colors.foreground = v.clone();
    }
    if let Some(v) = &o.cursor {
        colors.cursor = v.clone();
    }
    if let Some(v) = &o.selection_background {
        colors.selection_background = v.clone();
    }
    if let Some(p) = &o.palette {
        for (i, c) in p.iter().enumerate().take(16) {
            colors.palette[i] = c.clone();
        }
    }
}

/// Normalizes color forms found in terminal configs to `#rrggbb`.
pub(crate) fn normalize_color(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("#{}", hex.to_lowercase()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_resolve_without_config() {
        let resolved = resolve(&Config {
            theme_source: Some("builtin".into()),
            ..Default::default()
        });
        assert_eq!(resolved.colors.background, "#1e1e2e");
        assert_eq!(resolved.colors.palette.len(), 16);
        assert_eq!(resolved.font_size, 13.0);
        // The accelerator differs per platform (Cmd vs Ctrl+Shift); the
        // action set does not.
        assert!(resolved.keybindings.values().any(|a| a == "new_tab"));
    }

    #[test]
    fn explicit_overrides_beat_defaults() {
        let cfg = Config {
            theme_source: Some("builtin".into()),
            font_size: Some(15.0),
            colors: Some(ColorOverrides {
                background: Some("#000000".into()),
                ..Default::default()
            }),
            keybindings: [
                ("mod+t".to_string(), "split_right".to_string()),
                ("mod+w".to_string(), "none".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let resolved = resolve(&cfg);
        assert_eq!(resolved.font_size, 15.0);
        assert_eq!(resolved.colors.background, "#000000");
        assert_eq!(resolved.keybindings["mod+t"], "split_right");
        assert!(!resolved.keybindings.contains_key("mod+w"));
    }

    #[test]
    fn color_normalization() {
        assert_eq!(normalize_color("1e1e2e"), Some("#1e1e2e".into()));
        assert_eq!(normalize_color("#AABBCC"), Some("#aabbcc".into()));
        assert_eq!(normalize_color("red"), None);
    }
}
