//! Importer for wezterm color schemes — the iTerm2-Color-Schemes TOML
//! format (`[colors]` with `ansi`/`brights` arrays). We deliberately do not
//! parse `wezterm.lua`; schemes are plain TOML files.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{normalize_color, ImportedTheme};

#[derive(Deserialize)]
struct SchemeFile {
    colors: SchemeColors,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SchemeColors {
    ansi: Vec<String>,
    brights: Vec<String>,
    background: Option<String>,
    foreground: Option<String>,
    cursor_bg: Option<String>,
    selection_bg: Option<String>,
}

/// `scheme` is either a path to a .toml file or a scheme name resolved in
/// `~/.config/wezterm/colors/<name>.toml`.
pub fn import(scheme: &str) -> Option<ImportedTheme> {
    let path = if scheme.ends_with(".toml") && Path::new(scheme).is_file() {
        PathBuf::from(scheme)
    } else {
        colors_dir()?.join(format!("{scheme}.toml"))
    };
    import_from(&path)
}

pub fn import_from(path: &Path) -> Option<ImportedTheme> {
    let text = std::fs::read_to_string(path).ok()?;
    let parsed: SchemeFile = toml::from_str(&text).ok()?;
    let c = parsed.colors;

    let mut theme = ImportedTheme {
        background: c.background.as_deref().and_then(normalize_color),
        foreground: c.foreground.as_deref().and_then(normalize_color),
        cursor: c.cursor_bg.as_deref().and_then(normalize_color),
        selection_background: c.selection_bg.as_deref().and_then(normalize_color),
        ..Default::default()
    };
    for (i, color) in c.ansi.iter().take(8).enumerate() {
        theme.palette[i] = normalize_color(color);
    }
    for (i, color) in c.brights.iter().take(8).enumerate() {
        theme.palette[8 + i] = normalize_color(color);
    }
    Some(theme)
}

fn colors_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/wezterm/colors"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iterm2_scheme_toml() {
        let dir = std::env::temp_dir().join(format!("cmux-wez-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Dracula.toml");
        std::fs::write(
            &path,
            r##"
[colors]
ansi = ["#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2"]
brights = ["#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff"]
background = "#282a36"
foreground = "#f8f8f2"
cursor_bg = "#f8f8f2"
selection_bg = "#44475a"

[metadata]
name = "Dracula"
"##,
        )
        .unwrap();

        let theme = import_from(&path).unwrap();
        assert_eq!(theme.background.as_deref(), Some("#282a36"));
        assert_eq!(theme.palette[1].as_deref(), Some("#ff5555"));
        assert_eq!(theme.palette[8].as_deref(), Some("#6272a4"));
        assert_eq!(theme.selection_background.as_deref(), Some("#44475a"));
    }
}
