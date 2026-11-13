//! The mirador.json keys as a list a person can read and edit: each key's
//! group, kind, default and description, the values a file sets them to,
//! and the one function that writes a key back.

use std::ffi::OsStr;
use std::path::Path;

use cmux_protocol::{ChoiceOption, SettingKind, SettingRow};
use serde_json::{json, Value};

use super::{config_path, default_font_family, Computer, Config, FOCUS_RAIL_WIDTH, SIDEBAR_WIDTH, SIDEBAR_WIDTH_DEFAULT};
use crate::pty::conpty;

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

/// A width in whole pixels within `range`. The kind allows a fraction in
/// range; [`accepts`] refuses it, since [`Config`] reads a `u32`.
fn pixels(range: &std::ops::RangeInclusive<u32>) -> SettingKind {
    SettingKind::Number { min: f64::from(*range.start()), max: f64::from(*range.end()) }
}

/// What themeSource takes: the sources, then every named theme, so a
/// theme added to the table is a choice without touching this list.
fn theme_sources() -> Vec<&'static str> {
    let mut sources = vec!["auto", "ghostty", "wezterm", "builtin", "none"];
    sources.extend(super::NAMED_THEMES.iter().map(|t| t.name));
    sources
}

/// conpty's options as the pane offers them. [`settings`] gives auto and
/// system; a folder anywhere is taken too ([`accepts`]). Only here, for the
/// pane, is PATH (`path`) looked at: [`settings`] runs on every config
/// write, a zoom step or a sidebar drop included, and one PATH folder on a
/// drive that does not answer would hold each of them up. On Windows the
/// pane offers each folder on PATH that holds a conpty.dll, the ConPTYs
/// this machine already has, and says what auto loads here and which
/// folders hold a ConPTY that works (a conpty.dll without the
/// OpenConsole.exe it starts does not). The file's own folder, when it is
/// not one of those, shows as itself.
fn conpty_view_options(mut options: Vec<ChoiceOption>, value: Option<&Value>, path: Option<&OsStr>) -> Vec<ChoiceOption> {
    let on_path = match path {
        Some(path) if cfg!(windows) => conpty::on_path(path),
        _ => Vec::new(),
    };
    let first_on_path = on_path.first().map(|dir| dir.to_string_lossy().into_owned());
    options.extend(on_path.iter().map(|dir| option(json!(dir.to_string_lossy()))));
    if let Some(v) = value.and_then(Value::as_str) {
        let named = matches!(conpty::parse(Some(v)), Ok(conpty::Conpty::Auto | conpty::Conpty::System));
        if !named && !options.iter().any(|o| o.value == *v) {
            options.push(option(json!(v)));
        }
    }
    if !cfg!(windows) {
        return options;
    }
    options
        .into_iter()
        .map(|o| {
            let (available, note) = match o.value.as_str() {
                Some("auto") => match &first_on_path {
                    Some(dir) => (true, Some(format!("Loads the conpty.dll in {dir}"))),
                    None => (true, Some("No conpty.dll on PATH: Windows' own".to_string())),
                },
                Some("system") => (true, Some("Windows' own ConPTY".to_string())),
                Some(dir) => match conpty::folder_problem(Path::new(dir)) {
                    None => (true, None),
                    Some(problem) => (false, Some(problem.to_string())),
                },
                None => (true, None),
            };
            ChoiceOption { available, note, ..o }
        })
        .collect()
}

/// Every key of [`Config`], in the order the Settings pane shows them.
pub fn settings() -> Vec<Setting> {
    use SettingKind::{Bool, Number, ReadOnly, Text};
    let shell = if cfg!(windows) { "PowerShell 7, Windows PowerShell or cmd.exe" } else { "$SHELL" };
    vec![
        setting("themeSource", "Appearance", choice(&theme_sources()), "auto",
            "Where colors come from: auto uses Ghostty's config when there is one; builtin (or none) is Mirador's own colors."),
        setting("weztermScheme", "Appearance", Text, "none",
            "The wezterm scheme a wezterm theme source reads: a name in ~/.config/wezterm/colors, or a .toml path."),
        setting("fontFamily", "Appearance", Text, default_font_family(), "The terminal font, as a CSS font list."),
        setting("fontSize", "Appearance", Number { min: 6.0, max: 72.0 }, "13", "The terminal font size, in points."),
        setting("colors", "Appearance", ReadOnly, "the theme's",
            "Colors that override the theme's: background, foreground, cursor, selection, palette."),
        setting("sidebarWidth", "Appearance", pixels(&SIDEBAR_WIDTH), &SIDEBAR_WIDTH_DEFAULT.to_string(),
            "The sidebar's width, in pixels. Dragging its edge sets it; a double-click on the edge clears it."),
        setting("scrollback", "Terminal", Number { min: 0.0, max: u32::MAX as f64 }, "10000", "Lines each terminal keeps above the screen."),
        setting("macOptionIsMeta", "Terminal", Bool, "false",
            "macOS only: Option+key sends Meta instead of typing its character, which non-US layouts need for [ ] { } |."),
        setting("shell", "Terminal", Text, shell, "The program panes run."),
        setting("shellArgs", "Terminal", ReadOnly, "none", "Arguments for a custom shell; when set, Mirador adds none of its own."),
        setting("defaultCwd", "Terminal", Text, "home directory",
            "Where a new tab's first pane starts; splits inherit their pane's directory."),
        setting("persistSessions", "Terminal", Bool, "true",
            "Keep terminals running when Mirador quits, and reattach to them next launch. Applies to new panes."),
        setting("conpty", "Terminal", choice(&["auto", "system"]), "auto",
            "Windows only: the ConPTY panes run under. auto loads the first conpty.dll Windows finds, one on PATH included; system is Windows' own; or a folder with conpty.dll and OpenConsole.exe. Applies to new panes; with persistSessions off, after a restart."),
        setting("agentsPerTab", "Agents", SettingKind::Choice { options: vec![option(json!(4)), option(json!(6)), option(json!(9))] }, "4",
            "How many agents a page of the wall's Grid layout holds."),
        setting("focusRailWidth", "Agents", pixels(&FOCUS_RAIL_WIDTH), "a third of the wall",
            "The width of Focus's column beside the agent shown large, in pixels. Dragging its edge sets it; a double-click on the edge clears it."),
        setting("teamCostWarnUsd", "Agents", Number { min: f64::MIN_POSITIVE, max: f64::MAX }, "0.5",
            "What starting a team may cost, in US dollars, before its weight in the editor warns."),
        setting("agentClick", "Agents", choice(&["wall", "pane"]), "wall",
            "What clicking an agent in the sidebar does: its tab's wall, or the agent alone."),
        setting("agentRoles", "Agents", ReadOnly, "none",
            "Named ways to start a Claude Code agent, each with its own model and system prompt."),
        setting("computers", "Agents", ReadOnly, "none",
            "Computers agents can work on over SSH: a name, an ssh host and the directory there. Added from the palette."),
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
        // Only what conpty::parse takes: auto, system, or a folder holding a
        // ConPTY, wherever it is. Not by being offered: a folder on PATH is
        // offered for its conpty.dll alone, and may lack the OpenConsole.exe.
        SettingKind::Choice { .. } if setting.key == "conpty" => {
            value.as_str().is_some_and(|v| conpty::parse(Some(v)).is_ok())
        }
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
    settings_view_in(&config_path(), &super::ghostty::user_config_path(), super::wezterm::colors_dir().as_deref())
}

/// Every setting with the value the file at `path` gives it, and whether
/// that value is valid. A file that does not parse is that error, not a
/// view of the defaults, which would show as if nothing were set.
/// `ghostty` is where a Ghostty config would be and `wezterm_colors` where
/// wezterm schemes are by name, for themeSource's notes. On Windows the
/// folders on PATH are looked at too, for conpty's ([`conpty_view_options`]):
/// a PATH folder on a drive that does not answer slows this view, never a
/// write. Nothing else on disk is looked at.
fn settings_view_in(path: &Path, ghostty: &Path, wezterm_colors: Option<&Path>) -> Result<Vec<SettingRow>, String> {
    let obj = read_object(path)?;
    Ok(settings()
        .into_iter()
        .map(|s| {
            let value = obj.get(s.key).cloned();
            let kind = match s.kind.clone() {
                SettingKind::Choice { options } if s.key == "themeSource" => {
                    SettingKind::Choice { options: theme_options(options, &obj, ghostty, wezterm_colors) }
                }
                SettingKind::Choice { options } if s.key == "conpty" => {
                    SettingKind::Choice {
                        options: conpty_view_options(options, value.as_ref(), std::env::var_os("PATH").as_deref()),
                    }
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
fn theme_options(
    options: Vec<ChoiceOption>,
    obj: &serde_json::Map<String, Value>,
    ghostty: &Path,
    wezterm_colors: Option<&Path>,
) -> Vec<ChoiceOption> {
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
                    Some(name)
                        if super::wezterm::scheme_path(name, wezterm_colors)
                            .and_then(|p| super::wezterm::import_from(&p))
                            .is_none() =>
                    {
                        (false, Some(format!("No wezterm scheme {name}")))
                    }
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
    write_object(path, obj)
}

fn write_object(path: &Path, obj: serde_json::Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(obj)).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| e.to_string())
}

/// Adds a computer to mirador.json's `computers`.
pub fn add_computer(name: &str, host: &str, dir: &str) -> Result<Computer, String> {
    add_computer_in(&config_path(), name, host, dir)
}

/// Appends a computer to `computers` in the file at `path`, as
/// [`set_config_value_in`] edits a key: as JSON, so nothing else in the
/// file changes. `computers` is read-only in the Settings pane, since it
/// takes no single value, so this is the one way the app writes it. The
/// name goes on command lines and in palette entries, so it is a role's
/// kind of name (`roles::is_name`), and no two computers share one.
pub(super) fn add_computer_in(path: &Path, name: &str, host: &str, dir: &str) -> Result<Computer, String> {
    let computer = Computer { name: name.trim().into(), host: host.trim().into(), dir: dir.trim().into() };
    if !crate::roles::is_name(&computer.name) {
        return Err(format!(
            "`{}` can't name a computer: letters, digits, `-`, `_` and `.`, not starting with `-` or `.`",
            computer.name
        ));
    }
    if computer.host.is_empty() {
        return Err("a computer needs an ssh host".into());
    }
    if computer.dir.is_empty() {
        return Err("a computer needs the directory where the work is".into());
    }
    let mut obj = read_object(path)?;
    let mut list = match obj.get("computers") {
        None => Vec::new(),
        Some(Value::Array(list)) => list.clone(),
        Some(_) => return Err("computers in mirador.json is not a list".into()),
    };
    if list.iter().any(|c| c["name"].as_str().map(str::trim) == Some(computer.name.as_str())) {
        return Err(format!("there is already a computer `{}`", computer.name));
    }
    list.push(serde_json::to_value(&computer).map_err(|e| e.to_string())?);
    // As set_config_value_in: never write what Config cannot read back.
    if serde_json::from_value::<Config>(json!({ "computers": list })).is_err() {
        return Err("computers in mirador.json has an entry that is not a computer".into());
    }
    obj.insert("computers".into(), Value::Array(list));
    write_object(path, obj)?;
    Ok(computer)
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
    fn a_computer_is_added_and_nothing_else_changes() {
        let path = temp_config("computers");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"fontSize": 14, "someFutureKey": {"kept": true}}"#).unwrap();
        let added = add_computer_in(&path, " box ", "devbox", "~/work/app").unwrap();
        assert_eq!(added, Computer { name: "box".into(), host: "devbox".into(), dir: "~/work/app".into() });
        add_computer_in(&path, "gpu", "-p 2222 me@gpu.example", "/srv/app").unwrap();
        let file = read(&path);
        assert_eq!(file["fontSize"], 14);
        assert_eq!(file["someFutureKey"], json!({ "kept": true }), "a key Config does not know survives");
        assert_eq!(
            file["computers"],
            json!([
                { "name": "box", "host": "devbox", "dir": "~/work/app" },
                { "name": "gpu", "host": "-p 2222 me@gpu.example", "dir": "/srv/app" },
            ])
        );
        let config: Config = serde_json::from_value(file).unwrap();
        assert_eq!(config.computers.len(), 2);
    }

    #[test]
    fn a_computer_needs_a_new_shell_safe_name_a_host_and_a_dir() {
        let path = temp_config("computers-bad");
        add_computer_in(&path, "box", "devbox", "~/work").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let dup = add_computer_in(&path, "box", "other", "~/x").unwrap_err();
        assert_eq!(dup, "there is already a computer `box`");
        for bad in ["", "my box", "-box", ".box", "box;rm", "bo$x"] {
            let err = add_computer_in(&path, bad, "devbox", "~/work").unwrap_err();
            assert!(err.contains("can't name a computer"), "{bad}: {err}");
        }
        assert!(add_computer_in(&path, "b2", " ", "~/work").unwrap_err().contains("ssh host"));
        assert!(add_computer_in(&path, "b2", "devbox", "").unwrap_err().contains("directory"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "a refused computer changes nothing");

        std::fs::write(&path, r#"{"computers": ["nope"]}"#).unwrap();
        assert!(add_computer_in(&path, "b2", "devbox", "~/w").unwrap_err().contains("not a computer"));
        std::fs::write(&path, r#"{"computers": "nope"}"#).unwrap();
        assert!(add_computer_in(&path, "b2", "devbox", "~/w").unwrap_err().contains("not a list"));
        // The Settings pane does not write it: it takes no single value.
        assert!(set_config_value_in(&path, "computers", Some(json!([]))).is_err());
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
    fn widths_take_whole_pixels_in_their_range() {
        let path = temp_config("widths");
        // What a drag writes, and the Settings pane.
        set_config_value_in(&path, "sidebarWidth", Some(json!(300))).unwrap();
        set_config_value_in(&path, "focusRailWidth", Some(json!(520))).unwrap();
        assert_eq!(read(&path), json!({ "sidebarWidth": 300, "focusRailWidth": 520 }));
        for (key, value) in [
            ("sidebarWidth", json!(159)),
            ("sidebarWidth", json!(601)),
            ("sidebarWidth", json!(300.5)), // in range, but no u32
            ("focusRailWidth", json!(149)),
            ("focusRailWidth", json!(1601)),
            ("focusRailWidth", json!("wide")),
        ] {
            assert!(set_config_value_in(&path, key, Some(value.clone())).is_err(), "{key} = {value}");
        }
        assert_eq!(read(&path), json!({ "sidebarWidth": 300, "focusRailWidth": 520 }));
        // A double-click on the edge: the key goes, the default is back.
        set_config_value_in(&path, "focusRailWidth", None).unwrap();
        assert_eq!(read(&path), json!({ "sidebarWidth": 300 }));
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn the_view_shows_what_the_file_sets_and_whether_it_is_valid() {
        let path = temp_config("view");
        // No file: every setting, none set.
        let rows = settings_view_in(&path, &no_ghostty(&path), Some(&wezterm_colors(&path))).unwrap();
        assert_eq!(rows.len(), settings().len());
        assert!(rows.iter().all(|r| r.value.is_none() && r.valid));

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "agentsPerTab": 5, "fontSize": 14, "keybindings": { "mod+t": "new_tab" } }"#).unwrap();
        let rows = settings_view_in(&path, &no_ghostty(&path), Some(&wezterm_colors(&path))).unwrap();
        let row = |key: &str| rows.iter().find(|r| r.key == key).unwrap();
        assert_eq!((row("agentsPerTab").value.clone(), row("agentsPerTab").valid), (Some(json!(5)), false));
        assert_eq!((row("fontSize").value.clone(), row("fontSize").valid), (Some(json!(14)), true));
        assert!(row("keybindings").valid);
        assert_eq!(row("themeSource").value, None);

        // A file that does not parse is an error, not the defaults.
        std::fs::write(&path, "{ broken").unwrap();
        assert!(settings_view_in(&path, &no_ghostty(&path), Some(&wezterm_colors(&path))).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_key_set_to_null_is_set_invalid_and_removable() {
        let path = temp_config("null");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "fontSize": null, "zeta": 1 }"#).unwrap();
        let rows = settings_view_in(&path, &no_ghostty(&path), Some(&wezterm_colors(&path))).unwrap();
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

    /// Where wezterm schemes by name are for the test, beside its
    /// mirador.json: never the real home's.
    fn wezterm_colors(path: &Path) -> std::path::PathBuf {
        path.parent().unwrap().join("wezterm-colors")
    }

    /// themeSource's options as the view gives them: value → (available, note).
    fn theme_choices(path: &Path, ghostty: &Path) -> Vec<(String, bool, Option<String>)> {
        let rows = settings_view_in(path, ghostty, Some(&wezterm_colors(path))).unwrap();
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

        // A scheme by name is looked for in the colors directory passed in,
        // and only there.
        std::fs::write(&path, json!({ "weztermScheme": "Light" }).to_string()).unwrap();
        assert_eq!(choice_of(&theme_choices(&path, &ghostty), "wezterm"), (false, Some("No wezterm scheme Light".into())));
        let colors = wezterm_colors(&path);
        std::fs::create_dir_all(&colors).unwrap();
        std::fs::copy(&scheme, colors.join("Light.toml")).unwrap();
        assert_eq!(choice_of(&theme_choices(&path, &ghostty), "wezterm"), (true, None));

        // Other choices carry no notes, but conpty's, which has its own test.
        let rows = settings_view_in(&path, &ghostty, Some(&wezterm_colors(&path))).unwrap();
        for row in rows.iter().filter(|r| r.key != "themeSource" && r.key != "conpty") {
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
        let rows = settings_view_in(&path, &no_ghostty(&path), Some(&wezterm_colors(&path))).unwrap();
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

    /// The probe the test below runs in a process of its own, with a PATH
    /// it chose: prints the conpty options of the list every config write
    /// goes through. Run alone, without the variable, it does nothing.
    #[test]
    #[ignore = "run in a process of its own by writing_a_setting_does_not_look_at_path"]
    fn probe_conpty_options_of_a_write() {
        if std::env::var_os("MIRADOR_SETTINGS_PROBE").is_none() {
            return;
        }
        let row = settings().into_iter().find(|s| s.key == "conpty").unwrap();
        let SettingKind::Choice { options } = row.kind else { panic!("conpty is a choice") };
        println!("\noptions: {}", json!(options.into_iter().map(|o| o.value).collect::<Vec<_>>()));
    }

    /// What every config write goes through offers conpty auto and system
    /// only: it does not look at PATH, where one folder on a drive that does
    /// not answer would hold up each write. The pane's view does. Checked in
    /// a process whose PATH is one folder holding a conpty.dll, so the
    /// result does not depend on this machine's PATH.
    #[test]
    fn writing_a_setting_does_not_look_at_path() {
        let dir = std::env::temp_dir().join(format!("mirador-settings-write-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(conpty::DLL), b"").unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "config::settings::tests::probe_conpty_options_of_a_write", "--ignored", "--nocapture", "--test-threads=1"])
            .env("MIRADOR_SETTINGS_PROBE", "1")
            .env("PATH", std::env::join_paths([&dir]).unwrap())
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let options = stdout.lines().find_map(|l| l.strip_prefix("options: ")).unwrap_or_else(|| panic!("no probe line: {stdout}"));
        assert_eq!(serde_json::from_str::<Value>(options).unwrap(), json!(["auto", "system"]), "{stdout}");
    }

    /// A folder on PATH is offered, in the pane, for its conpty.dll alone.
    /// Offered is not taken: without its OpenConsole.exe it is refused, and
    /// shown so.
    #[test]
    fn an_offered_conpty_folder_without_its_host_is_refused_all_the_same() {
        let dir = std::env::temp_dir().join(format!("mirador-settings-offered-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(conpty::DLL), b"").unwrap();
        let named = json!(dir.to_str().unwrap());
        let setting = settings().into_iter().find(|s| s.key == "conpty").unwrap();
        assert!(!accepts(&setting, &named), "a folder without OpenConsole.exe is taken for being offered");
        if cfg!(windows) {
            let SettingKind::Choice { options } = setting.kind.clone() else { unreachable!() };
            let path = std::env::join_paths([&dir]).unwrap();
            let shown = conpty_view_options(options, None, Some(&path));
            let offered = shown.iter().find(|o| o.value == named).expect("a folder on PATH with a conpty.dll is offered");
            assert!(!offered.available);
            assert_eq!(offered.note.as_deref(), Some("no OpenConsole.exe beside its conpty.dll"));
            let auto = shown.iter().find(|o| o.value == "auto").unwrap();
            assert_eq!(auto.note, Some(format!("Loads the conpty.dll in {}", dir.display())));
        }
        std::fs::write(dir.join(conpty::HOST), b"").unwrap();
        assert!(accepts(&setting, &named), "with both files it is a ConPTY");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn conpty_takes_auto_system_or_a_folder_with_a_conpty_in_it() {
        let path = temp_config("conpty");
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let row = |path: &Path| {
            let rows = settings_view_in(path, &no_ghostty(path), Some(&wezterm_colors(path))).unwrap();
            rows.into_iter().find(|r| r.key == "conpty").unwrap()
        };
        let options = |row: &SettingRow| match &row.kind {
            SettingKind::Choice { options } => options.clone(),
            other => panic!("conpty is a choice, not {other:?}"),
        };

        // A folder with a ConPTY in it, on no PATH: auto, system and it are taken.
        let folder = dir.join("openconsole");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(conpty::DLL), b"").unwrap();
        std::fs::write(folder.join(conpty::HOST), b"").unwrap();
        let named = json!(folder.to_str().unwrap());
        for value in [json!("auto"), json!("system"), named.clone()] {
            set_config_value_in(&path, "conpty", Some(value.clone())).unwrap();
            assert_eq!(read(&path)["conpty"], value);
        }
        // The file's folder shows as itself, set, valid and available.
        let shown = row(&path);
        assert!(shown.set && shown.valid);
        let own = options(&shown).into_iter().find(|o| o.value == named).expect("the file's folder is offered");
        assert!(own.available);
        assert!(options(&shown).iter().any(|o| o.value == "auto") && options(&shown).iter().any(|o| o.value == "system"));

        // Without the OpenConsole.exe it starts, the folder is no ConPTY:
        // refused, and so are a bare name and a number.
        std::fs::remove_file(folder.join(conpty::HOST)).unwrap();
        for value in [named.clone(), json!("WezTerm"), json!(1)] {
            assert!(set_config_value_in(&path, "conpty", Some(value.clone())).is_err(), "{value}");
        }
        assert_eq!(read(&path)["conpty"], named, "the file is left alone");
        // A file that still names it shows it invalid, and on Windows why.
        let shown = row(&path);
        assert!(shown.set && !shown.valid);
        if cfg!(windows) {
            let own = options(&shown).into_iter().find(|o| o.value == named).unwrap();
            assert!(!own.available);
            assert_eq!(own.note.as_deref(), Some("no OpenConsole.exe beside its conpty.dll"));
        }
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
