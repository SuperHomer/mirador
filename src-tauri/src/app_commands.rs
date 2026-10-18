// Every Tauri command the app registers, by name: the app's ACL manifest.
// `build.rs` includes this file — so no inner doc comments here — to have
// Tauri generate an `allow-<command>` permission for each, and to write
// the one permission set (`permissions/app-commands.toml`) the capability
// grants the host webview. With a manifest, Tauri checks the ACL for the
// app's own commands even for a local page; without one, it never did.
//
// A command missing here is refused at run time, in the app's own UI. The
// test below fails first, naming it.

pub const APP_COMMANDS: &[&str] = &[
    "workspace_snapshot",
    "get_config",
    "new_tab",
    "close_tab",
    "quit_ending_sessions",
    "claude_integration_offer",
    "setup_claude_integration",
    "dismiss_claude_integration",
    "set_active_tab",
    "rename_tab",
    "move_tab",
    "split_pane",
    "close_pane",
    "focus_pane",
    "focus_direction",
    "set_split_ratios",
    "set_view",
    "set_camera",
    "set_card_rect",
    "set_frame_position",
    "attach_pane",
    "list_notifications",
    "mark_all_notifications_read",
    "resolve_screen_read",
    "store_scrollback",
    "load_scrollback",
    "open_browser",
    "open_diff",
    "load_diff",
    "set_diff_spec",
    "list_worktrees",
    "set_diff_worktree",
    "open_graph",
    "open_agent",
    "agent_roles",
    "open_agent_editor",
    "agent_editor_catalog",
    "agent_file_read",
    "agent_file_check",
    "agent_file_template",
    "agent_file_save",
    "agent_file_delete",
    "start_team",
    "list_teams",
    "team_add",
    "team_restart",
    "open_agent_wall",
    "get_agent_activity",
    "mark_pane_read",
    "zoom_pane",
    "load_graph",
    "graph_show_commit",
    "whats_new",
    "release_image",
    "open_whats_new",
    "open_ssh",
    "ssh_hosts",
    "set_browser_bounds",
    "set_browser_visible",
    "browser_navigate",
    "browser_history",
    "write_pty",
    "resize_pty",
    "ack_pty",
    "app_version",
    "available_update",
    "check_update",
    "install_update",
];

#[cfg(test)]
mod tests {
    use super::APP_COMMANDS;
    use std::collections::BTreeSet;

    /// The commands `generate_handler!` registers in lib.rs, by name.
    fn registered() -> BTreeSet<String> {
        let lib = include_str!("lib.rs");
        let start = lib.find("generate_handler![").expect("generate_handler! in lib.rs");
        let list = &lib[start + "generate_handler![".len()..];
        let list = &list[..list.find(']').expect("the end of generate_handler!")];
        list.split(',')
            .map(|path| path.trim())
            .filter(|path| !path.is_empty())
            .map(|path| path.rsplit("::").next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn the_manifest_lists_exactly_the_registered_commands() {
        let registered = registered();
        let listed: BTreeSet<String> = APP_COMMANDS.iter().map(|c| c.to_string()).collect();
        assert_eq!(listed.len(), APP_COMMANDS.len(), "a command listed twice in app_commands.rs");
        let missing: Vec<_> = registered.difference(&listed).collect();
        let extra: Vec<_> = listed.difference(&registered).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "src/app_commands.rs and generate_handler! in src/lib.rs disagree.\n\
             registered but not in APP_COMMANDS (the app's own UI would be refused it): {missing:?}\n\
             in APP_COMMANDS but not registered: {extra:?}"
        );
    }
}
