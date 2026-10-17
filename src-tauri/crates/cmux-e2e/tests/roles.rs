//! Roles, end to end: what a role starts with, what refuses to start, and
//! the files a launch leaves for the shell's history. Skipped unless
//! `MIRA_E2E=1`.

use cmux_e2e::{e2e, flag, Sandbox};
use cmux_protocol::Request;
use serde_json::Value;

const WAY: &str = "---\ndescription: Tests first\n---\nWrite the failing test first.\nNever push.\n";

fn agent_new(sb: &Sandbox, role: &str, from: &str) -> Result<String, String> {
    sb.request(Request::AgentNew {
        role: Some(role.into()),
        task: None,
        target: None,
        pane_id: Some(from.into()),
    })
    .map(|v| v["paneId"].as_str().unwrap().to_string())
}

/// A Claude role gets its own prompt then its way's, by file; a Codex role
/// gets the same as `developer_instructions`; a role that names a missing
/// way opens nothing and writes nothing; and the prompt file is still
/// there after a relaunch, so the command in the shell's history still
/// works (#117).
#[test]
fn a_role_starts_with_its_prompt_and_way() {
    e2e!();
    let mut sb = Sandbox::new("roles");
    sb.write(".mirador/ways/our-way.md", WAY);
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\nway: our-way\n---\nYou review.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\nmodel: gpt-5\nway: our-way\n---\nYou write \"the\" code.\n");
    sb.write(".mirador/roles/lost.md", "---\nway: nowhere\n---\nLost.\n");
    sb.launch();
    let first = sb.first_pane();

    let reviewer = agent_new(&sb, "reviewer", &first).unwrap();
    let args = sb.nth_start(&reviewer, 1);
    assert_eq!(flag(&args, "--model").as_deref(), Some("haiku"), "{args:?}");
    let prompt_file = flag(&args, "--append-system-prompt-file").expect("a prompt file");
    let prompt = std::fs::read_to_string(&prompt_file).unwrap();
    assert_eq!(prompt, "You review.\n\nWrite the failing test first.\nNever push.");

    // Codex: the same prompt, as a TOML string that survived being typed.
    let coder = agent_new(&sb, "coder", &first).unwrap();
    let args = sb.nth_start(&coder, 1);
    assert_eq!(args[0], "--model", "{args:?}");
    assert_eq!(flag(&args, "--model").as_deref(), Some("gpt-5"));
    let config: toml::Table = toml::from_str(&flag(&args, "-c").expect("developer_instructions")).unwrap();
    assert_eq!(
        config["developer_instructions"].as_str(),
        Some("You write \"the\" code.\n\nWrite the failing test first.\nNever push.")
    );
    // Codex has no hooks: it is an agent because the pane was marked so.
    let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap();
    let codex = agents.iter().find(|a| a["paneId"] == coder.as_str()).expect("listed");
    assert_eq!((codex["harness"].as_str(), codex["role"].as_str()), (Some("codex"), Some("coder")));

    // A role that cannot start opens nothing and leaves no file.
    let panes = sb.panes().len();
    let err = agent_new(&sb, "lost", &first).unwrap_err();
    assert!(err.contains("nowhere"), "{err}");
    assert_eq!(sb.panes().len(), panes);
    let prompts = std::path::Path::new(&prompt_file).parent().unwrap().to_path_buf();
    let names: Vec<String> = std::fs::read_dir(&prompts)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.starts_with("lost-")), "{names:?}");

    // The launch's files outlive the app: ↑ Enter still finds them.
    sb.quit();
    sb.launch();
    assert!(std::path::Path::new(&prompt_file).is_file(), "{prompt_file} was removed at startup");
}

/// A role that keeps only some skills and no plugins: Mirador asks the
/// agent's Claude Code what it has (the probe), and switches the rest off
/// — skills hidden from the agent, plugins disabled, Claude Code's own
/// plugins left alone (#118). A typo is refused with what exists, and a
/// repository's role may not hand out tools.
#[test]
fn a_limited_role_keeps_only_what_it_lists() {
    e2e!();
    let mut sb = Sandbox::new("limits");
    sb.set_env("E2E_SKILLS", "dataviz,loop,anthropic-skills:pdf");
    sb.write(".mirador/roles/charts.md", "---\nskills: [pdf]\nplugins: []\nmcp: [github]\n---\nCharts.\n");
    sb.write(".mirador/roles/typo.md", "---\nskills: [pfd]\n---\nTypo.\n");
    sb.write(".mirador/roles/greedy.md", "---\ntools:\n  allow: [\"Bash(*)\"]\n---\nGreedy.\n");
    sb.launch();
    let first = sb.first_pane();

    let charts = agent_new(&sb, "charts", &first).unwrap();
    let args = sb.nth_start(&charts, 1);
    let probes: Vec<Value> = sb.agent_events().into_iter().filter(|e| e["event"] == "probe").collect();
    assert_eq!(probes.len(), 1, "one probe, for the one limited launch");
    assert!(
        probes[0]["cwd"].as_str().is_some_and(|c| c.ends_with("proj")),
        "the probe runs where the agent will: {}",
        probes[0]["cwd"]
    );
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(flag(&args, "--settings").expect("settings")).unwrap()).unwrap();
    assert_eq!(
        settings["skillOverrides"],
        serde_json::json!({ "dataviz": "user-invocable-only", "loop": "user-invocable-only" })
    );
    assert_eq!(settings["enabledPlugins"], serde_json::json!({ "rust@market": false }));
    assert_eq!(settings["allowedMcpServers"], serde_json::json!([{ "serverName": "github" }]));

    let typo = agent_new(&sb, "typo", &first).unwrap_err();
    assert!(typo.contains("no skill `pfd`") && typo.contains("anthropic-skills:pdf"), "{typo}");
    let greedy = agent_new(&sb, "greedy", &first).unwrap_err();
    assert!(greedy.contains("cannot allow tools"), "{greedy}");
}
