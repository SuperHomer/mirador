//! Read-only importer for Ghostty's `key = value` config format: extracts
//! theme colors and font settings so cmux matches the user's Ghostty look.
//! Lenient by design — unknown keys are ignored, errors skip the line.

use std::path::{Path, PathBuf};

use super::{normalize_color, ImportedTheme};

pub fn user_config_path() -> PathBuf {
    super::config_dir()
        .parent()
        .map(|p| p.join("ghostty").join("config"))
        .unwrap_or_else(|| PathBuf::from("ghostty-config-missing"))
}

pub fn import() -> Option<ImportedTheme> {
    import_from(&user_config_path())
}

pub fn import_from(path: &Path) -> Option<ImportedTheme> {
    let entries = parse_file(path, 0)?;
    Some(build_theme(&entries))
}

/// One parsed `key = value` occurrence, in file order (later wins, except
/// `palette` where every occurrence contributes an entry).
type Entries = Vec<(String, String)>;

fn parse_file(path: &Path, depth: usize) -> Option<Entries> {
    if depth > 8 {
        return None; // include cycle guard
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut entries = Entries::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim().trim_matches('"').to_string();
        if key == "config-file" && !value.is_empty() {
            let include = if Path::new(&value).is_absolute() {
                PathBuf::from(&value)
            } else {
                path.parent().unwrap_or(Path::new(".")).join(&value)
            };
            if let Some(nested) = parse_file(&include, depth + 1) {
                entries.extend(nested);
            }
            continue;
        }
        entries.push((key, value));
    }
    Some(entries)
}

fn build_theme(entries: &Entries) -> ImportedTheme {
    let mut theme = ImportedTheme::default();

    // `theme = name` acts as a base layer: resolve and apply it first.
    if let Some(name) = last(entries, "theme") {
        if let Some(theme_entries) = resolve_theme_file(&name).and_then(|p| parse_file(&p, 0)) {
            apply_entries(&mut theme, &theme_entries);
        }
    }
    // Explicit keys in the config override the named theme.
    apply_entries(&mut theme, entries);
    theme
}

fn apply_entries(theme: &mut ImportedTheme, entries: &Entries) {
    for (key, value) in entries {
        match key.as_str() {
            "background" => theme.background = normalize_color(value),
            "foreground" => theme.foreground = normalize_color(value),
            "cursor-color" => theme.cursor = normalize_color(value),
            "selection-background" => theme.selection_background = normalize_color(value),
            // Ghostty allows multiple font-family lines (fallbacks); first wins.
            "font-family" => {
                if theme.font_family.is_none() && !value.is_empty() {
                    theme.font_family = Some(value.clone());
                }
            }
            "font-size" => theme.font_size = value.parse().ok(),
            // palette = N=#rrggbb
            "palette" => {
                if let Some((idx, color)) = value.split_once('=') {
                    if let (Ok(i), Some(c)) =
                        (idx.trim().parse::<usize>(), normalize_color(color))
                    {
                        if i < 16 {
                            theme.palette[i] = Some(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn last(entries: &Entries, key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Ghostty themes are key=value files searched in the user themes dir and
/// the app bundle's resources.
fn resolve_theme_file(name: &str) -> Option<PathBuf> {
    let candidates = [
        user_config_path().parent()?.join("themes").join(name),
        PathBuf::from("/Applications/Ghostty.app/Contents/Resources/ghostty/themes").join(name),
        PathBuf::from("/usr/share/ghostty/themes").join(name),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Extracts (font_family, font_size, key→value map) — exposed for tests.
#[cfg(test)]
fn parse_str(text: &str) -> Entries {
    let dir = std::env::temp_dir().join(format!("cmux-ghostty-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config");
    std::fs::write(&path, text).unwrap();
    parse_file(&path, 0).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_keys_and_palette() {
        let entries = parse_str(
            r##"
# a comment
font-family = JetBrains Mono
font-family = Symbols Nerd Font
font-size = 14
background = 282c34
foreground = #abb2bf
cursor-color = #528bff
palette = 0=#1e2127
palette = 1=#e06c75
palette = 15=#ffffff
"##,
        );
        let mut theme = ImportedTheme::default();
        apply_entries(&mut theme, &entries);
        assert_eq!(theme.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(theme.font_size, Some(14.0));
        assert_eq!(theme.background.as_deref(), Some("#282c34"));
        assert_eq!(theme.foreground.as_deref(), Some("#abb2bf"));
        assert_eq!(theme.cursor.as_deref(), Some("#528bff"));
        assert_eq!(theme.palette[0].as_deref(), Some("#1e2127"));
        assert_eq!(theme.palette[1].as_deref(), Some("#e06c75"));
        assert_eq!(theme.palette[15].as_deref(), Some("#ffffff"));
        assert!(theme.palette[2].is_none());
    }

    #[test]
    fn config_file_include_and_override() {
        let dir = std::env::temp_dir().join(format!("cmux-ghostty-inc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base"), "background = #111111\nfont-size = 12\n").unwrap();
        std::fs::write(
            dir.join("config"),
            "config-file = base\nbackground = #222222\n",
        )
        .unwrap();
        let theme = import_from(&dir.join("config")).unwrap();
        // Explicit key after include wins; include's other keys survive.
        assert_eq!(theme.background.as_deref(), Some("#222222"));
        assert_eq!(theme.font_size, Some(12.0));
    }

    #[test]
    fn named_theme_is_base_layer() {
        let dir = std::env::temp_dir().join(format!("cmux-ghostty-theme-{}", std::process::id()));
        let themes = dir.join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(
            themes.join("mytheme"),
            "background = #333333\nforeground = #eeeeee\n",
        )
        .unwrap();
        std::fs::write(dir.join("config"), "theme = mytheme\nbackground = #444444\n").unwrap();

        // resolve_theme_file searches relative to the *user* config dir, so
        // exercise the layering through parse+build with a patched resolver:
        let entries = parse_file(&dir.join("config"), 0).unwrap();
        let mut theme = ImportedTheme::default();
        let theme_entries = parse_file(&themes.join("mytheme"), 0).unwrap();
        apply_entries(&mut theme, &theme_entries);
        apply_entries(&mut theme, &entries);
        assert_eq!(theme.background.as_deref(), Some("#444444"));
        assert_eq!(theme.foreground.as_deref(), Some("#eeeeee"));
    }
}
