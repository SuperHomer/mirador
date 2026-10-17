//! What a Claude Code role may use: its MCP servers, plugins, skills and
//! permission rules, given to Claude Code as one settings file
//! (`--settings`), the highest-priority settings it reads.
//!
//! - `mcp: [a, b]` → `allowedMcpServers`, which keeps every other server
//!   from loading, wherever it is configured: the project's `.mcp.json`,
//!   the user's, a plugin's, a claude.ai connector. An empty list leaves
//!   none.
//! - `plugins: [a]` → `enabledPlugins`, every other installed plugin set
//!   to `false`.
//! - `skills: [a]` → `skillOverrides`, every other skill set to
//!   `user-invocable-only`: the agent no longer sees it, and you can still
//!   type `/name` yourself.
//! - `tools` → `permissions.allow` / `permissions.deny`.
//!
//! The last two lists name what to *keep*, but Claude Code's settings only
//! turn things off one name at a time — so Mirador needs every name there
//! is. It asks Claude Code itself: a `-p` session in `stream-json` prints
//! an `init` event listing its skills and plugins before it calls a model,
//! and is killed on reading it ([`probe`]). Reading them off disk instead
//! would miss the skills that are built in or come from an account.

use std::path::Path;

use cmux_protocol::{AgentRole, Harness};
use serde_json::{json, Map, Value};

/// The skills and plugins a Claude Code would load in some directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inventory {
    pub skills: Vec<String>,
    /// `(name, source)`: `source` is `name@marketplace`, the key
    /// `enabledPlugins` takes. Claude Code's own (`@builtin`) are left out:
    /// they are not the user's to turn off.
    pub plugins: Vec<(String, String)>,
}

/// Whether starting `role` needs an [`Inventory`].
pub fn needs_inventory(role: &AgentRole) -> bool {
    role.harness == Harness::Claude && (role.skills.is_some() || role.plugins.is_some())
}

/// Whether `role` limits anything, and so needs a settings file.
pub fn limits(role: &AgentRole) -> bool {
    role.harness == Harness::Claude
        && (role.mcp.is_some() || role.plugins.is_some() || role.skills.is_some() || role.tools.is_some())
}

/// The inventory in one line of `stream-json` output, if it is the `init`
/// event.
pub fn parse_init(line: &str) -> Option<Inventory> {
    let event: Value = serde_json::from_str(line).ok()?;
    if event["type"] != "system" || event["subtype"] != "init" {
        return None;
    }
    let skills = event["skills"]
        .as_array()?
        .iter()
        .filter_map(|s| s.as_str().map(String::from))
        .collect();
    let plugins = event["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let source = p["source"].as_str()?;
            (p["path"] != "builtin" && !source.ends_with("@builtin"))
                .then(|| (p["name"].as_str().unwrap_or(source).to_string(), source.to_string()))
        })
        .collect();
    Some(Inventory { skills, plugins })
}

/// The settings that hold `role` to what it lists. `inventory` is needed
/// when it lists skills or plugins ([`needs_inventory`]). A name that
/// matches nothing is an error, not a quiet "none": it is a typo, or
/// something this machine does not have, and either way the role is not
/// what its author meant.
pub fn settings(role: &AgentRole, inventory: Option<&Inventory>) -> Result<Value, String> {
    let mut out = Map::new();
    if let Some(servers) = &role.mcp {
        let allowed: Vec<Value> = servers.iter().map(|s| json!({ "serverName": s })).collect();
        out.insert("allowedMcpServers".into(), Value::Array(allowed));
    }
    if let Some(wanted) = &role.plugins {
        let inventory = inventory.ok_or("no inventory of plugins")?;
        let keep = |(name, source): &(String, String)| wanted.iter().any(|w| w == name || w == source);
        if let Some(missing) = wanted
            .iter()
            .find(|w| !inventory.plugins.iter().any(|(n, s)| *w == n || *w == s))
        {
            return Err(unknown("plugin", missing, inventory.plugins.iter().map(|(_, s)| s.as_str())));
        }
        let off: Map<String, Value> = inventory
            .plugins
            .iter()
            .filter(|p| !keep(p))
            .map(|(_, source)| (source.clone(), Value::Bool(false)))
            .collect();
        out.insert("enabledPlugins".into(), Value::Object(off));
    }
    if let Some(wanted) = &role.skills {
        let inventory = inventory.ok_or("no inventory of skills")?;
        // `pdf` names `anthropic-skills:pdf` too: the qualified name is the
        // account's, and a role shared through a repository cannot know it.
        let named = |skill: &str, w: &str| skill == w || skill.rsplit_once(':').is_some_and(|(_, bare)| bare == w);
        if let Some(missing) = wanted.iter().find(|w| !inventory.skills.iter().any(|s| named(s, w))) {
            return Err(unknown("skill", missing, inventory.skills.iter().map(String::as_str)));
        }
        let hidden: Map<String, Value> = inventory
            .skills
            .iter()
            .filter(|s| !wanted.iter().any(|w| named(s, w)))
            .map(|s| (s.clone(), Value::from("user-invocable-only")))
            .collect();
        out.insert("skillOverrides".into(), Value::Object(hidden));
    }
    if let Some(tools) = &role.tools {
        let mut permissions = Map::new();
        if !tools.allow.is_empty() {
            permissions.insert("allow".into(), json!(tools.allow));
        }
        if !tools.deny.is_empty() {
            permissions.insert("deny".into(), json!(tools.deny));
        }
        if !permissions.is_empty() {
            out.insert("permissions".into(), Value::Object(permissions));
        }
    }
    Ok(Value::Object(out))
}

fn unknown<'a>(kind: &str, name: &str, have: impl Iterator<Item = &'a str>) -> String {
    let mut have: Vec<&str> = have.collect();
    have.sort_unstable();
    have.dedup();
    let list = if have.is_empty() { "none".to_string() } else { have.join(", ") };
    format!("no {kind} `{name}` here (Claude Code has: {list})")
}

/// Asks the Claude Code on `path` (the PATH to find it on; the app's own
/// when `None`) what it would load in `cwd`. No hooks run, no MCP server
/// starts, no session is saved, and no model is called: the process is
/// killed on its first event.
pub fn probe(cwd: Option<&Path>, path: Option<&str>) -> Result<Inventory, String> {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let mut cmd = crate::proc::command("claude");
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "list",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if let Some(cwd) = cwd.filter(|c| c.is_dir()) {
        cmd.current_dir(cwd);
    }
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run `claude` to list its skills and plugins: {e}"))?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(inventory) = parse_init(&line) {
                let _ = tx.send(inventory);
                return;
            }
        }
    });
    let found = rx.recv_timeout(std::time::Duration::from_secs(20));
    let _ = child.kill();
    let _ = child.wait();
    found.map_err(|_| {
        let mut err = String::new();
        let _ = stderr.read_to_string(&mut err);
        let err = err.trim();
        format!(
            "`claude` did not list its skills and plugins{}",
            if err.is_empty() { String::new() } else { format!(": {}", err.lines().last().unwrap_or(err)) }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_protocol::ToolRules;

    const INIT: &str = r#"{"type":"system","subtype":"init","skills":["banana","dataviz","loop","anthropic-skills:pdf","rust:fmt"],"plugins":[{"name":"rust-analyzer-lsp","path":"/p/rust","source":"rust-analyzer-lsp@claude-plugins-official"},{"name":"cc-plugin-telemetry","path":"builtin","source":"cc-plugin-telemetry@builtin"},{"name":"rust","path":"/p/r","source":"rust@market"}],"mcp_servers":[]}"#;

    fn role() -> AgentRole {
        AgentRole { name: "r".into(), ..Default::default() }
    }

    #[test]
    fn reads_the_init_event() {
        let inv = parse_init(INIT).unwrap();
        assert_eq!(inv.skills.len(), 5);
        assert_eq!(
            inv.plugins,
            [
                ("rust-analyzer-lsp".to_string(), "rust-analyzer-lsp@claude-plugins-official".to_string()),
                ("rust".to_string(), "rust@market".to_string())
            ]
        );
        assert_eq!(parse_init(r#"{"type":"assistant"}"#), None);
        assert_eq!(parse_init("not json"), None);
    }

    #[test]
    fn keeps_only_what_the_role_lists() {
        let inv = parse_init(INIT).unwrap();
        let r = AgentRole {
            mcp: Some(vec!["github".into()]),
            plugins: Some(vec!["rust".into()]),
            skills: Some(vec!["pdf".into(), "banana".into()]),
            tools: Some(ToolRules { allow: vec![], deny: vec!["Bash(git push *)".into()] }),
            ..role()
        };
        let s = settings(&r, Some(&inv)).unwrap();
        assert_eq!(s["allowedMcpServers"], json!([{ "serverName": "github" }]));
        // Claude Code's own plugins are not the role's to turn off.
        assert_eq!(s["enabledPlugins"], json!({ "rust-analyzer-lsp@claude-plugins-official": false }));
        // `pdf` keeps the account's `anthropic-skills:pdf`; the rest — the
        // plugin's skill included — the agent no longer sees.
        assert_eq!(
            s["skillOverrides"],
            json!({ "dataviz": "user-invocable-only", "loop": "user-invocable-only", "rust:fmt": "user-invocable-only" })
        );
        assert_eq!(s["permissions"], json!({ "deny": ["Bash(git push *)"] }));
    }

    #[test]
    fn an_empty_list_is_none_and_an_absent_one_is_everything() {
        let inv = parse_init(INIT).unwrap();
        let none = AgentRole { mcp: Some(vec![]), plugins: Some(vec![]), skills: Some(vec![]), ..role() };
        let s = settings(&none, Some(&inv)).unwrap();
        assert_eq!(s["allowedMcpServers"], json!([]));
        assert_eq!(s["enabledPlugins"].as_object().unwrap().len(), 2);
        assert_eq!(s["skillOverrides"].as_object().unwrap().len(), 5);
        assert_eq!(settings(&role(), None).unwrap(), json!({}));
        assert!(!limits(&role()));
        assert!(limits(&AgentRole { tools: Some(ToolRules::default()), ..role() }));
        assert!(!needs_inventory(&AgentRole { mcp: Some(vec![]), ..role() }));
        assert!(needs_inventory(&AgentRole { skills: Some(vec![]), ..role() }));
    }

    #[test]
    fn a_name_that_matches_nothing_is_an_error() {
        let inv = parse_init(INIT).unwrap();
        let typo = AgentRole { skills: Some(vec!["pfd".into()]), ..role() };
        let err = settings(&typo, Some(&inv)).unwrap_err();
        assert!(err.contains("no skill `pfd`") && err.contains("anthropic-skills:pdf"), "{err}");
        let gone = AgentRole { plugins: Some(vec!["nope".into()]), ..role() };
        assert!(settings(&gone, Some(&inv)).unwrap_err().contains("no plugin `nope`"));
        // By its full name too.
        let full = AgentRole { plugins: Some(vec!["rust@market".into()]), ..role() };
        assert_eq!(
            settings(&full, Some(&inv)).unwrap()["enabledPlugins"],
            json!({ "rust-analyzer-lsp@claude-plugins-official": false })
        );
    }
}
