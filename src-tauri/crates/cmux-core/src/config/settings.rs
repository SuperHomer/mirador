//! The mirador.json keys as a list a person can read and edit: each key's
//! group, kind, default and description, the values a file sets them to,
//! and the one function that writes a key back.

use std::path::Path;

use cmux_protocol::{SettingKind, SettingRow};
use serde_json::{json, Value};

use super::{config_path, default_font_family, Config};

/// One mirador.json key, as the Settings pane describes it.
pub struct Setting {
    pub key: &'static str,
    pub group: &'static str,
    pub kind: SettingKind,
    pub default: String,
    pub description: &'static str,
}

fn choice(values: &[&str]) -> SettingKind {
    SettingKind::Choice { values: values.iter().map(|v| json!(v)).collect() }
}

fn setting(key: &'static str, group: &'static str, kind: SettingKind, default: &str, description: &'static str) -> Setting {
    Setting { key, group, kind, default: default.into(), description }
}

/// Every key of [`Config`], in the order the Settings pane shows them.
pub fn settings() -> Vec<Setting> {
    use SettingKind::{Bool, Number, ReadOnly, Text};
    let shell = if cfg!(windows) { "PowerShell 7, Windows PowerShell or cmd.exe" } else { "$SHELL" };
    vec![
        setting("themeSource", "Appearance", choice(&["auto", "ghostty", "wezterm", "builtin", "devaslife"]), "auto",
            "Where colors come from: auto uses Ghostty's config when there is one."),
        setting("weztermScheme", "Appearance", Text, "none",
            "The wezterm scheme a wezterm theme source reads: a name in ~/.config/wezterm/colors, or a .toml path."),
        setting("fontFamily", "Appearance", Text, default_font_family(), "The terminal font, as a CSS font list."),
        setting("fontSize", "Appearance", Number { min: 6.0, max: 72.0 }, "13", "The terminal font size, in points."),
        setting("colors", "Appearance", ReadOnly, "the theme's",
            "Colors that override the theme's: background, foreground, cursor, selection, palette."),
        setting("scrollback", "Terminal", Number { min: 0.0, max: 1_000_000.0 }, "10000", "Lines each terminal keeps above the screen."),
        setting("macOptionIsMeta", "Terminal", Bool, "false",
            "macOS only: Option+key sends Meta instead of typing its character, which non-US layouts need for [ ] { } |."),
        setting("shell", "Terminal", Text, shell, "The program panes run."),
        setting("shellArgs", "Terminal", ReadOnly, "none", "Arguments for a custom shell; when set, Mirador adds none of its own."),
        setting("defaultCwd", "Terminal", Text, "home directory",
            "Where a new tab's first pane starts; splits inherit their pane's directory."),
        setting("persistSessions", "Terminal", Bool, "true",
            "Keep terminals running when Mirador quits, and reattach to them next launch. Applies to new panes."),
        setting("agentsPerTab", "Agents", SettingKind::Choice { values: vec![json!(4), json!(6), json!(9)] }, "4",
            "How many agents a page of the wall's Grid layout holds."),
        setting("agentClick", "Agents", choice(&["wall", "pane"]), "wall",
            "What clicking an agent in the sidebar does: its tab's wall, or the agent alone."),
        setting("agentRoles", "Agents", ReadOnly, "none",
            "Named ways to start a Claude Code agent, each with its own model and system prompt."),
        setting("editorOpensIn", "Panes", choice(&["tab", "split"]), "tab", "Where the roles, ways and teams editor opens."),
        setting("diffOpensIn", "Panes", choice(&["tab", "split"]), "split", "Where a diff opens."),
        setting("graphOpensIn", "Panes", choice(&["tab", "split"]), "split", "Where the commit graph opens."),
        setting("keybindings", "Advanced", ReadOnly, "the built-in bindings",
            "Accelerators mapped to actions; \"none\" removes a built-in one."),
        setting("customCommands", "Advanced", ReadOnly, "none", "Commands of your own in the palette."),
    ]
}

/// Whether `value` is one `setting` takes: its kind allows it, and
/// [`Config`] reads it, so writing it can never stop the file parsing
/// (a fraction in `scrollback` is a number in range, and not a `u32`).
fn accepts(setting: &Setting, value: &Value) -> bool {
    let kind_allows = match &setting.kind {
        SettingKind::Choice { values } => values.contains(value),
        SettingKind::Bool => value.is_boolean(),
        SettingKind::Number { min, max } => value.as_f64().is_some_and(|n| n >= *min && n <= *max),
        SettingKind::Text => value.is_string(),
        SettingKind::ReadOnly => true,
    };
    kind_allows && serde_json::from_value::<Config>(json!({ setting.key: value })).is_ok()
}

/// Reads the file at `path` as JSON; a missing file is an empty object.
fn read_object(path: &Path) -> Result<serde_json::Map<String, Value>, String> {
    let value = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text).map_err(|e| format!("mirador.json: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("mirador.json: {e}")),
    };
    match value {
        Value::Object(obj) => Ok(obj),
        _ => Err("mirador.json is not a JSON object".into()),
    }
}

/// Every setting with the value mirador.json gives it.
pub fn settings_view() -> Result<Vec<SettingRow>, String> {
    settings_view_in(&config_path())
}

/// Every setting with the value the file at `path` gives it, and whether
/// that value is valid. A file that does not parse is that error, not a
/// view of the defaults, which would show as if nothing were set.
fn settings_view_in(path: &Path) -> Result<Vec<SettingRow>, String> {
    let obj = read_object(path)?;
    Ok(settings()
        .into_iter()
        .map(|s| {
            let value = obj.get(s.key).cloned();
            SettingRow {
                valid: value.as_ref().is_none_or(|v| accepts(&s, v)),
                key: s.key.into(),
                group: s.group.into(),
                kind: s.kind,
                default: s.default,
                description: s.description.into(),
                value,
            }
        })
        .collect())
}

/// Sets `key` in mirador.json, or removes it with `None`.
pub fn set_config_value(key: &str, value: Option<Value>) -> Result<(), String> {
    set_config_value_in(&config_path(), key, value)
}

/// Sets `key` to `value` in the file at `path`, creating it if absent, or
/// removes the key with `None`. Only that key changes: the file is edited
/// as JSON, not read into [`Config`] and written back, which would drop
/// keys Config does not know and every value it reads loosely. Unknown
/// and read-only keys and values the key does not take are refused, and
/// a file that does not parse is left alone, not replaced.
pub(super) fn set_config_value_in(path: &Path, key: &str, value: Option<Value>) -> Result<(), String> {
    let setting = settings()
        .into_iter()
        .find(|s| s.key == key)
        .ok_or_else(|| format!("mirador.json has no setting {key}"))?;
    if setting.kind == SettingKind::ReadOnly {
        return Err(format!("{key} is edited in mirador.json itself"));
    }
    if let Some(v) = &value {
        if !accepts(&setting, v) {
            return Err(format!("{key} does not take {v}"));
        }
    }
    let mut obj = read_object(path)?;
    match value {
        Some(v) => {
            obj.insert(key.into(), v);
        }
        None => {
            obj.shift_remove(key);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(obj)).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("mirador").join("mirador.json")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn every_config_key_has_a_setting() {
        // Config serializes every field, unset ones as null, so a field
        // added without a setting shows here.
        let config = serde_json::to_value(Config::default()).unwrap();
        let mut fields: Vec<&str> = config.as_object().unwrap().keys().map(String::as_str).collect();
        let mut keys: Vec<&str> = settings().iter().map(|s| s.key).collect();
        fields.sort();
        keys.sort();
        assert_eq!(fields, keys);
    }

    #[test]
    fn setting_a_key_keeps_every_other_key_in_its_place() {
        let path = temp_config("keep");
        // No file yet: it is made, with the one key.
        set_config_value_in(&path, "fontSize", Some(json!(14))).unwrap();
        assert_eq!(read(&path), json!({ "fontSize": 14 }));

        std::fs::write(&path, r#"{ "zeta": 1, "fontSize": 15, "themeSource": "auto", "somethingNew": { "a": [1, 2] } }"#).unwrap();
        set_config_value_in(&path, "fontSize", Some(json!(12.5))).unwrap();
        set_config_value_in(&path, "persistSessions", Some(json!(false))).unwrap();
        let cfg = read(&path);
        let keys: Vec<&String> = cfg.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "fontSize", "themeSource", "somethingNew", "persistSessions"]);
        assert_eq!(cfg["fontSize"], 12.5);
        assert_eq!(cfg["somethingNew"], json!({ "a": [1, 2] }));

        // None removes the key, and the others keep their order.
        set_config_value_in(&path, "fontSize", None).unwrap();
        let cfg = read(&path);
        let keys: Vec<&String> = cfg.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "themeSource", "somethingNew", "persistSessions"]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn values_a_key_does_not_take_are_refused_and_the_file_left_alone() {
        let path = temp_config("refuse");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = r#"{ "fontSize": 13 }"#;
        std::fs::write(&path, text).unwrap();
        for (key, value) in [
            ("themeSource", json!("solarized")), // not a choice
            ("agentsPerTab", json!(5)),
            ("agentsPerTab", json!("6")), // a choice of numbers
            ("fontSize", json!(200)),     // out of range
            ("scrollback", json!(1.5)),   // in range, but not a whole number
            ("persistSessions", json!("yes")), // wrong type
            ("shell", json!(7)),
            ("keybindings", json!({})), // read-only
            ("noSuchKey", json!(1)),    // unknown
        ] {
            assert!(set_config_value_in(&path, key, Some(value.clone())).is_err(), "{key} = {value}");
        }
        assert!(set_config_value_in(&path, "colors", None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

        // A file that does not parse is not replaced.
        std::fs::write(&path, "{ broken").unwrap();
        assert!(set_config_value_in(&path, "fontSize", Some(json!(14))).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn the_view_shows_what_the_file_sets_and_whether_it_is_valid() {
        let path = temp_config("view");
        // No file: every setting, none set.
        let rows = settings_view_in(&path).unwrap();
        assert_eq!(rows.len(), settings().len());
        assert!(rows.iter().all(|r| r.value.is_none() && r.valid));

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "agentsPerTab": 5, "fontSize": 14, "keybindings": { "mod+t": "new_tab" } }"#).unwrap();
        let rows = settings_view_in(&path).unwrap();
        let row = |key: &str| rows.iter().find(|r| r.key == key).unwrap();
        assert_eq!((row("agentsPerTab").value.clone(), row("agentsPerTab").valid), (Some(json!(5)), false));
        assert_eq!((row("fontSize").value.clone(), row("fontSize").valid), (Some(json!(14)), true));
        assert!(row("keybindings").valid);
        assert_eq!(row("themeSource").value, None);

        // A file that does not parse is an error, not the defaults.
        std::fs::write(&path, "{ broken").unwrap();
        assert!(settings_view_in(&path).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
