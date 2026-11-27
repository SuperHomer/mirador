//! The macOS menu bar, laid out from the palette's action table and the
//! config's keybindings: which menu each action goes in, what it is called
//! there, and the shortcut it shows. Built from the same table and the same
//! bindings as the palette, so a key rebound in `mirador.json` shows in both.

use std::collections::HashMap;

use cmux_protocol::MenuAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Action {
        id: String,
        label: String,
        /// In the spelling Tauri's menu items parse ("CmdOrCtrl+Shift+D").
        accelerator: Option<String>,
    },
    Separator,
}

/// Action group → the menu it goes in, in the menu bar's order. "Mirador"
/// is the app menu, which the app merges with About, Hide and Quit.
const MENUS: &[(&str, &str)] = &[
    ("app", "Mirador"),
    ("tabs", "File"),
    ("view", "View"),
    ("pane", "Pane"),
    ("agents", "Agents"),
    ("git", "Git"),
];

/// Left to the Edit menu, whose Copy and Paste roles are these on macOS.
const LEFT_OUT: &[&str] = &["copy", "paste"];

/// The menus for `actions`, in the menu bar's order, each listing its
/// actions in the table's order. An item is labelled with its title less
/// the palette's "Area: " prefix, and a separator falls wherever that
/// prefix changes, so the Layout items stand apart under Pane. A group
/// with no menu is left out, and so is a menu with nothing in it.
pub fn menus(actions: &[MenuAction], keybindings: &HashMap<String, String>) -> Vec<Menu> {
    let shortcuts = pick_accelerators(keybindings);
    MENUS
        .iter()
        .filter_map(|(group, title)| {
            let mut items = Vec::new();
            let mut area: Option<&str> = None;
            for action in actions
                .iter()
                .filter(|a| a.group == *group && !LEFT_OUT.contains(&a.id.as_str()))
            {
                let (prefix, label) = match action.title.split_once(": ") {
                    Some((prefix, label)) => (prefix, label),
                    None => ("", action.title.as_str()),
                };
                if area.is_some_and(|area| area != prefix) {
                    items.push(MenuItem::Separator);
                }
                area = Some(prefix);
                items.push(MenuItem::Action {
                    id: action.id.clone(),
                    label: label.to_string(),
                    accelerator: shortcuts.get(&action.id).and_then(|a| accelerator(a)),
                });
            }
            (!items.is_empty()).then(|| Menu {
                title: title.to_string(),
                items,
            })
        })
        .collect()
}

/// action id → the one accelerator to show for it, as the config spells it.
///
/// Must agree with `acceleratorsByAction` in `src/keymap/accelerator.ts`,
/// which picks the palette's: the fewest modifiers, then the shortest, then
/// alphabetical. A total order, because the config is a JSON object whose
/// key order is not stable between runs, and a shortcut that changed on
/// every launch would be worse than none.
pub fn pick_accelerators(keybindings: &HashMap<String, String>) -> HashMap<String, String> {
    let mut best: HashMap<String, &str> = HashMap::new();
    for (accel, action) in keybindings {
        if action.is_empty() || action == "none" {
            continue;
        }
        let rank = |a: &str| (a.split('+').count(), a.len(), a.to_string());
        match best.get(action) {
            Some(current) if rank(current) <= rank(accel) => {}
            _ => {
                best.insert(action.clone(), accel);
            }
        }
    }
    best.into_iter()
        .map(|(action, accel)| (action, accel.to_string()))
        .collect()
}

/// The config's spelling of a shortcut ("mod+shift+d") in Tauri's
/// ("CmdOrCtrl+Shift+D"), with the same modifier aliases the keymap
/// accepts. None for what a menu item cannot carry — an unknown modifier
/// or key, or no modifier at all — rather than a guess: a shortcut that
/// cannot fire must not be advertised, as `formatAccel` already holds.
///
/// "plus" is one of those. Tauri parses a menu's accelerator into a
/// physical key, and the + the keymap answers to is a character, made by
/// Shift and = on one layout and another key on the next.
pub fn accelerator(accel: &str) -> Option<String> {
    let lower = accel.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let (key, mods) = parts.split_last()?;
    if mods.is_empty() {
        return None;
    }

    // The order Tauri's examples write them in; the parse ignores order.
    const ORDER: &[&str] = &["CmdOrCtrl", "Cmd", "Ctrl", "Alt", "Shift"];
    let mut names = Vec::new();
    for m in mods {
        let name = match *m {
            "mod" => "CmdOrCtrl",
            "cmd" | "super" | "meta" => "Cmd",
            "ctrl" | "control" => "Ctrl",
            "alt" | "opt" | "option" => "Alt",
            "shift" => "Shift",
            _ => return None,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort_by_key(|n| ORDER.iter().position(|o| o == n));

    let key = key_name(key)?;
    Some(format!("{}+{key}", names.join("+")))
}

/// A key as the config names it (`KeyboardEvent.key`, lowercased, or the
/// physical key's name) → Tauri's name for it.
fn key_name(key: &str) -> Option<String> {
    if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(key.to_ascii_uppercase());
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=12).contains(&n).then(|| format!("F{n}"));
    }
    let name = match key {
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "minus" | "-" => "Minus",
        "=" => "Equal",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "\\" => "Backslash",
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "'" => "Quote",
        "`" => "Backquote",
        "space" => "Space",
        "tab" => "Tab",
        "enter" => "Enter",
        "escape" => "Escape",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => return None,
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn action(id: &str, title: &str, group: &str) -> MenuAction {
        MenuAction {
            id: id.into(),
            title: title.into(),
            group: group.into(),
        }
    }

    fn item(id: &str, label: &str, accelerator: Option<&str>) -> MenuItem {
        MenuItem::Action {
            id: id.into(),
            label: label.into(),
            accelerator: accelerator.map(String::from),
        }
    }

    #[test]
    fn mod_is_cmd_or_ctrl_and_the_aliases_are_the_keymaps() {
        assert_eq!(accelerator("mod+shift+d").as_deref(), Some("CmdOrCtrl+Shift+D"));
        assert_eq!(accelerator("cmd+k").as_deref(), Some("Cmd+K"));
        assert_eq!(accelerator("super+k").as_deref(), Some("Cmd+K"));
        assert_eq!(accelerator("meta+k").as_deref(), Some("Cmd+K"));
        assert_eq!(accelerator("opt+k").as_deref(), Some("Alt+K"));
        assert_eq!(accelerator("option+k").as_deref(), Some("Alt+K"));
        assert_eq!(accelerator("control+k").as_deref(), Some("Ctrl+K"));
        assert_eq!(accelerator("Ctrl+Alt+K").as_deref(), Some("Ctrl+Alt+K"));
    }

    #[test]
    fn modifiers_come_out_in_one_order_whatever_the_configs() {
        assert_eq!(
            accelerator("shift+alt+mod+left").as_deref(),
            Some("CmdOrCtrl+Alt+Shift+ArrowLeft")
        );
    }

    #[test]
    fn named_keys_take_tauris_names() {
        for (config, tauri) in [
            ("mod+alt+left", "CmdOrCtrl+Alt+ArrowLeft"),
            ("mod+alt+down", "CmdOrCtrl+Alt+ArrowDown"),
            ("mod+shift+]", "CmdOrCtrl+Shift+BracketRight"),
            ("mod+shift+[", "CmdOrCtrl+Shift+BracketLeft"),
            ("mod+=", "CmdOrCtrl+Equal"),
            ("mod+minus", "CmdOrCtrl+Minus"),
            ("mod+-", "CmdOrCtrl+Minus"),
            ("ctrl+tab", "Ctrl+Tab"),
            ("ctrl+shift+space", "Ctrl+Shift+Space"),
            ("mod+1", "CmdOrCtrl+1"),
            ("mod+f5", "CmdOrCtrl+F5"),
        ] {
            assert_eq!(accelerator(config).as_deref(), Some(tauri), "{config}");
        }
    }

    #[test]
    fn what_a_menu_cannot_carry_has_no_accelerator() {
        // + is a character, not a key, and a menu item names a key.
        assert_eq!(accelerator("mod+plus"), None);
        assert_eq!(accelerator("mod+nosuchkey"), None);
        assert_eq!(accelerator("hyper+d"), None);
        assert_eq!(accelerator("d"), None);
        assert_eq!(accelerator(""), None);
        assert_eq!(accelerator("mod+"), None);
    }

    #[test]
    fn the_pick_is_fewest_modifiers_then_shortest_then_alphabetical() {
        let picked = pick_accelerators(&bindings(&[
            ("mod+=", "zoom_in"),
            ("mod+plus", "zoom_in"),
            ("mod+shift+plus", "zoom_in"),
            ("mod+shift+b", "two"),
            ("mod+a", "two"),
            ("mod+y", "three"),
            ("mod+x", "three"),
        ]));
        assert_eq!(picked["zoom_in"], "mod+=");
        assert_eq!(picked["two"], "mod+a");
        assert_eq!(picked["three"], "mod+x");
    }

    #[test]
    fn the_pick_does_not_depend_on_the_maps_order() {
        let pairs = [
            ("ctrl+y", "act"),
            ("ctrl+x", "act"),
            ("alt+x", "act"),
            ("ctrl+shift+a", "act"),
        ];
        // A HashMap's order changes with its seed: build it many times.
        for _ in 0..50 {
            assert_eq!(pick_accelerators(&bindings(&pairs))["act"], "alt+x");
        }
    }

    #[test]
    fn none_and_empty_bindings_advertise_nothing() {
        let picked = pick_accelerators(&bindings(&[("mod+d", "none"), ("mod+e", "")]));
        assert!(picked.is_empty());
    }

    #[test]
    fn a_rebound_key_shows_and_an_unbound_one_does_not() {
        let actions = [action("split_right", "Pane: Split Right…", "pane")];
        let rebound = menus(&actions, &bindings(&[("mod+alt+r", "split_right")]));
        assert_eq!(
            rebound[0].items,
            vec![item("split_right", "Split Right…", Some("CmdOrCtrl+Alt+R"))]
        );
        let unbound = menus(&actions, &bindings(&[("mod+d", "none")]));
        assert_eq!(unbound[0].items, vec![item("split_right", "Split Right…", None)]);
    }

    #[test]
    fn menus_follow_the_menu_bar_and_items_the_table() {
        let actions = [
            action("new_tab", "Tab: New", "tabs"),
            action("commit_graph", "Git: Commit Graph", "git"),
            action("settings", "App: Settings", "app"),
            action("toggle_sidebar", "View: Toggle Sidebar", "view"),
            action("notifications", "Agents: Open Inbox", "agents"),
            action("close_tab", "Tab: Close", "tabs"),
            action("mystery", "Odd: Thing", "nowhere"),
        ];
        let menus = menus(&actions, &bindings(&[("mod+t", "new_tab")]));
        let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
        // No Pane menu: nothing went in it.
        assert_eq!(titles, ["Mirador", "File", "View", "Agents", "Git"]);
        assert_eq!(
            menus[1].items,
            vec![
                item("new_tab", "New", Some("CmdOrCtrl+T")),
                item("close_tab", "Close", None),
            ]
        );
    }

    #[test]
    fn a_change_of_area_within_a_menu_is_a_separator() {
        let actions = [
            action("close_pane", "Pane: Close", "pane"),
            action("rotate_forward", "Pane: Rotate Forward", "pane"),
            action("layout_even", "Layout: Even Sizes", "pane"),
            action("layout_columns", "Layout: Columns", "pane"),
            action("toggle_zoom", "Pane: Toggle Zoom", "pane"),
        ];
        let menus = menus(&actions, &HashMap::new());
        assert_eq!(
            menus[0].items,
            vec![
                item("close_pane", "Close", None),
                item("rotate_forward", "Rotate Forward", None),
                MenuItem::Separator,
                item("layout_even", "Even Sizes", None),
                item("layout_columns", "Columns", None),
                MenuItem::Separator,
                item("toggle_zoom", "Toggle Zoom", None),
            ]
        );
    }

    #[test]
    fn copy_and_paste_are_left_to_the_edit_menu() {
        let actions = [
            action("copy", "Pane: Copy Selection", "pane"),
            action("paste", "Pane: Paste", "pane"),
            action("close_pane", "Pane: Close", "pane"),
        ];
        let menus = menus(&actions, &HashMap::new());
        assert_eq!(menus[0].items, vec![item("close_pane", "Close", None)]);
    }
}
