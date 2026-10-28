//! The mirador.json keys as a list a person can read and edit: each key's
//! group, kind, default and description, the values a file sets them to,
//! and the one function that writes a key back.

use std::path::Path;

use cmux_protocol::{ChoiceOption, SettingKind, SettingRow};
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

fn option(value: Value) -> ChoiceOption {
    ChoiceOption { value, available: true, note: None }
}

fn choice(values: &[&str]) -> SettingKind {
    SettingKind::Choice { options: values.iter().map(|v| option(json!(v))).collect() }
}

fn setting(key: &'static str, group: &'static str, kind: SettingKind, default: &str, description: &'static str) -> Setting {
    Setting { key, group, kind, default: default.into(), description }
}

/// Every key of [`Config`], in the order the Settings pane shows them.
pub fn settings() -> Vec<Setting> {
    use SettingKind::{Bool, Number, ReadOnly, Text};
    let shell = if cfg!(windows) { "PowerShell 7, Windows PowerShell or cmd.exe" } else { "$SHELL" };
    vec![
        setting("themeSource", "Appearance", choice(&["auto", "ghostty", "wezterm", "builtin", "none", "devaslife"]), "auto",
            "Where colors come from: auto uses Ghostty's config when there is one; builtin (or none) is Mirador's own colors."),
        setting("weztermScheme", "Appearance", Text, "none",
            "The wezterm scheme a wezterm theme source reads: a name in ~/.config/wezterm/colors, or a .toml path."),
        setting("fontFamily", "Appearance", Text, default_font_family(), "The terminal font, as a CSS font list."),
        setting("fontSize", "Appearance", Number { min: 6.0, max: 72.0 }, "13", "The terminal font size, in points."),
        setting("colors", "Appearance", ReadOnly, "the theme's",
            "Colors that override the theme's: background, foreground, cursor, selection, palette."),
        setting("scrollback", "Terminal", Number { min: 0.0, max: u32::MAX as f64 }, "10000", "Lines each terminal keeps above the screen."),
        setting("macOptionIsMeta", "Terminal", Bool, "false",
            "macOS only: Option+key sends Meta instead of typing its character, which non-US layouts need for [ ] { } |."),
        setting("shell", "Terminal", Text, shell, "The program panes run."),
        setting("shellArgs", "Terminal", ReadOnly, "none", "Arguments for a custom shell; when set, Mirador adds none of its own."),
        setting("defaultCwd", "Terminal", Text, "home directory",
            "Where a new tab's first pane starts; splits inherit their pane's directory."),
        setting("persistSessions", "Terminal", Bool, "true",
            "Keep terminals running when Mirador quits, and reattach to them next launch. Applies to new panes."),
        setting("agentsPerTab", "Agents", SettingKind::Choice { options: vec![option(json!(4)), option(json!(6)), option(json!(9))] }, "4",
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
        SettingKind::Choice { options } => options.iter().any(|o| o.value == *value),
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
    settings_view_in(&config_path(), &super::ghostty::user_config_path())
}

/// Every setting with the value the file at `path` gives it, and whether
/// that value is valid. A file that does not parse is that error, not a
/// view of the defaults, which would show as if nothing were set.
/// `ghostty` is where a Ghostty config would be, for themeSource's notes.
fn settings_view_in(path: &Path, ghostty: &Path) -> Result<Vec<SettingRow>, String> {
    let obj = read_object(path)?;
    Ok(settings()
        .into_iter()
        .map(|s| {
            let value = obj.get(s.key).cloned();
            let kind = match s.kind.clone() {
                SettingKind::Choice { options } if s.key == "themeSource" => {
                    SettingKind::Choice { options: theme_options(options, &obj, ghostty) }
                }
                kind => kind,
            };
            SettingRow {
                set: obj.contains_key(s.key),
                valid: value.as_ref().is_none_or(|v| accepts(&s, v)),
                key: s.key.into(),
                group: s.group.into(),
                kind,
                default: s.default,
                description: s.description.into(),
                value,
            }
        })
        .collect())
}

/// themeSource's options as the pane offers them, from the file and the
/// disk: whether a source has anything to read, and what auto does here.
/// "none" is accepted, an alias of builtin that older files use, but not
/// offered: a file that has it still shows it, set and valid.
fn theme_options(options: Vec<ChoiceOption>, obj: &serde_json::Map<String, Value>, ghostty: &Path) -> Vec<ChoiceOption> {
    let has_ghostty = ghostty.exists();
    let scheme = obj.get("weztermScheme").and_then(Value::as_str).filter(|s| !s.trim().is_empty());
    options
        .into_iter()
        .filter(|o| o.value != "none")
        .map(|o| {
            let (available, note) = match o.value.as_str() {
                Some("auto") if has_ghostty => (true, Some("Uses your Ghostty config".to_string())),
                Some("auto") => (true, Some("No Ghostty config: builtin colors".to_string())),
                Some("ghostty") if !has_ghostty => (false, Some(format!("No Ghostty config at {}", ghostty.display()))),
                Some("wezterm") => match scheme {
                    None => (false, Some("Set weztermScheme first".to_string())),
                    Some(name) if super::wezterm::import(name).is_none() => (false, Some(format!("No wezterm scheme {name}"))),
                    Some(_) => (true, None),
                },
                _ => (true, None),
            };
            ChoiceOption { available, note, ..o }
        })
        .collect()
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
        // No cap on scrollback beyond what a u32 holds.
        set_config_value_in(&path, "scrollback", Some(json!(2_000_000))).unwrap();
        let cfg = read(&path);
        let keys: Vec<&String> = cfg.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "fontSize", "themeSource", "somethingNew", "persistSessions", "scrollback"]);
        assert_eq!(cfg["fontSize"], 12.5);
        assert_eq!(cfg["somethingNew"], json!({ "a": [1, 2] }));

        // None removes the key, and the others keep their order.
        set_config_value_in(&path, "fontSize", None).unwrap();
        let cfg = read(&path);
        let keys: Vec<&String> = cfg.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "themeSource", "somethingNew", "persistSessions", "scrollback"]);
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
            ("scrollback", json!(u64::from(u32::MAX) + 1)),
            ("agentsPerTab", json!(5)),
            ("agentsPerTab", json!("6")), // a choice of numbers
            ("fontSize", json!(200)),     // out of range
            ("fontSize", json!(73)),
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
        let rows = settings_view_in(&path, &no_ghostty(&path)).unwrap();
        assert_eq!(rows.len(), settings().len());
        assert!(rows.iter().all(|r| r.value.is_none() && r.valid));

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "agentsPerTab": 5, "fontSize": 14, "keybindings": { "mod+t": "new_tab" } }"#).unwrap();
        let rows = settings_view_in(&path, &no_ghostty(&path)).unwrap();
        let row = |key: &str| rows.iter().find(|r| r.key == key).unwrap();
        assert_eq!((row("agentsPerTab").value.clone(), row("agentsPerTab").valid), (Some(json!(5)), false));
        assert_eq!((row("fontSize").value.clone(), row("fontSize").valid), (Some(json!(14)), true));
        assert!(row("keybindings").valid);
        assert_eq!(row("themeSource").value, None);

        // A file that does not parse is an error, not the defaults.
        std::fs::write(&path, "{ broken").unwrap();
        assert!(settings_view_in(&path, &no_ghostty(&path)).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_key_set_to_null_is_set_invalid_and_removable() {
        let path = temp_config("null");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "fontSize": null, "zeta": 1 }"#).unwrap();
        let rows = settings_view_in(&path, &no_ghostty(&path)).unwrap();
        let row = rows.iter().find(|r| r.key == "fontSize").unwrap();
        assert!(row.set, "the key is in the file");
        assert!(!row.valid, "null is not a size");
        assert!(!rows.iter().find(|r| r.key == "scrollback").unwrap().set);

        set_config_value_in(&path, "fontSize", None).unwrap();
        assert_eq!(read(&path), json!({ "zeta": 1 }));
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    /// Where no Ghostty config is, beside the test's mirador.json.
    fn no_ghostty(path: &Path) -> std::path::PathBuf {
        path.parent().unwrap().join("no-ghostty").join("config")
    }

    /// themeSource's options as the view gives them: value → (available, note).
    fn theme_choices(path: &Path, ghostty: &Path) -> Vec<(String, bool, Option<String>)> {
        let rows = settings_view_in(path, ghostty).unwrap();
        let row = rows.into_iter().find(|r| r.key == "themeSource").unwrap();
        let SettingKind::Choice { options } = row.kind else { panic!("themeSource is a choice") };
        options.into_iter().map(|o| (o.value.as_str().unwrap().to_string(), o.available, o.note)).collect()
    }

    fn choice_of(choices: &[(String, bool, Option<String>)], value: &str) -> (bool, Option<String>) {
        let (_, available, note) = choices.iter().find(|(v, _, _)| v == value).unwrap_or_else(|| panic!("{value} offered"));
        (*available, note.clone())
    }

    #[test]
    fn theme_sources_say_whether_they_can_do_anything() {
        let path = temp_config("sources");
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{}").unwrap();

        // No Ghostty config, no weztermScheme.
        let ghostty = no_ghostty(&path);
        let c = theme_choices(&path, &ghostty);
        assert_eq!(choice_of(&c, "auto"), (true, Some("No Ghostty config: builtin colors".into())));
        assert_eq!(choice_of(&c, "ghostty"), (false, Some(format!("No Ghostty config at {}", ghostty.display()))));
        assert_eq!(choice_of(&c, "wezterm"), (false, Some("Set weztermScheme first".into())));
        assert_eq!(choice_of(&c, "builtin"), (true, None));
        assert_eq!(choice_of(&c, "devaslife"), (true, None));
        // "none" is an alias kept for old files, offered nowhere.
        assert!(c.iter().all(|(v, _, _)| v != "none"));

        // A Ghostty config there: auto uses it, ghostty is available.
        let ghostty = dir.join("ghostty").join("config");
        std::fs::create_dir_all(ghostty.parent().unwrap()).unwrap();
        std::fs::write(&ghostty, "background = 101010\n").unwrap();
        let c = theme_choices(&path, &ghostty);
        assert_eq!(choice_of(&c, "auto"), (true, Some("Uses your Ghostty config".into())));
        assert_eq!(choice_of(&c, "ghostty"), (true, None));

        // A weztermScheme that resolves to nothing, then one that does.
        let missing = dir.join("missing.toml");
        std::fs::write(&path, json!({ "weztermScheme": missing.to_str().unwrap() }).to_string()).unwrap();
        let c = theme_choices(&path, &ghostty);
        assert_eq!(choice_of(&c, "wezterm"), (false, Some(format!("No wezterm scheme {}", missing.display()))));
        let scheme = dir.join("light.toml");
        std::fs::write(&scheme, "[colors]\nbackground = \"#fdf6e3\"\nforeground = \"#657b83\"\n").unwrap();
        std::fs::write(&path, json!({ "weztermScheme": scheme.to_str().unwrap() }).to_string()).unwrap();
        assert_eq!(choice_of(&theme_choices(&path, &ghostty), "wezterm"), (true, None));

        // Other choices carry no notes.
        let rows = settings_view_in(&path, &ghostty).unwrap();
        for row in rows.iter().filter(|r| r.key != "themeSource") {
            if let SettingKind::Choice { options } = &row.kind {
                assert!(options.iter().all(|o| o.available && o.note.is_none()), "{}", row.key);
            }
        }
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_hidden_or_unavailable_theme_source_is_still_valid_and_written() {
        let path = temp_config("hidden");
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        // A file with the hidden alias shows it, set and valid.
        std::fs::write(&path, r#"{ "themeSource": "none" }"#).unwrap();
        let rows = settings_view_in(&path, &no_ghostty(&path)).unwrap();
        let row = rows.iter().find(|r| r.key == "themeSource").unwrap();
        assert!(row.set && row.valid);
        // An unavailable one is written all the same: the file may be
        // shared with a machine where it does something.
        set_config_value_in(&path, "themeSource", Some(json!("ghostty"))).unwrap();
        set_config_value_in(&path, "themeSource", Some(json!("wezterm"))).unwrap();
        set_config_value_in(&path, "themeSource", Some(json!("none"))).unwrap();
        assert_eq!(read(&path), json!({ "themeSource": "none" }));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
