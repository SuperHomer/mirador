//! The release app with the real agents. Skipped unless `MIRA_E2E_LIVE=1`
//! — they spend real tokens, so never in CI — and needs the release bundle
//! (`npm run tauri build`) and `claude` and `codex` on the PATH.

use std::time::Duration;

use cmux_e2e::{live, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);
/// A real model at work: minutes, not seconds.
const AGENT: Duration = Duration::from_secs(240);

/// The release bundle starts in a sandbox and its terminal echoes: what
/// every live test stands on, and free.
#[test]
fn release_app_starts() {
    live!();
    let mut sb = Sandbox::live("relstart");
    sb.launch();
    let pane = sb.first_pane();
    sb.type_into(&pane, "echo ok\r");
    sb.wait_for("the shell's answer", LONG, || sb.screen(&pane).filter(|s| s.lines().any(|l| l.trim() == "ok")));
}

/// Both real agents start unattended, each on its role's model: no
/// first-run, login, trust or update screen stops them. No prompt is sent,
/// so it costs next to nothing.
#[test]
fn real_agents_start() {
    live!();
    let mut sb = Sandbox::live("realstart");
    sb.write(".mirador/roles/haiku.md", "---\nmodel: claude-haiku-5-5\n---\nYou wait.\n");
    sb.write(".mirador/roles/luna.md", "---\nharness: codex\nmodel: gpt-6-luna\n---\nYou wait.\n");
    // A repository, where Codex asks whether to trust the folder.
    std::process::Command::new("git").arg("init").current_dir(&sb.project).output().unwrap();
    sb.launch();
    let first = sb.first_pane();
    let start = |role: &str| {
        sb.call(Request::AgentNew { role: Some(role.into()), harness: None, task: None, target: None, pane_id: Some(first.clone()) })
            ["paneId"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let claude = start("haiku");
    let codex = start("luna");

    // Each at its prompt: had a first-run, login or trust screen stopped
    // it, that is what the screen would show instead.
    // The session's header, model and billing: no dialog or welcome shows it.
    sb.wait_for_screen(&claude, "claude's prompt", LONG, |s| s.contains("Haiku 5.5 · "));
    sb.wait_for_screen(&codex, "codex's prompt", LONG, |s| s.contains("Ask Codex"));

    let agent = |pane: &str| {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        agents.into_iter().find(|a| a["paneId"] == pane).unwrap_or_else(|| sb.fail(&format!("{pane} is not an agent")))
    };
    let codex = agent(&codex);
    assert_eq!((codex["harness"].as_str(), codex["model"].as_str()), (Some("codex"), Some("gpt-6-luna")), "{codex}");
    // Claude Code's hooks reached the app: it knows what the session is doing.
    let claude = sb.wait_for("claude's hooks", LONG, || {
        Some(agent(&claude)).filter(|a| matches!(a["status"].as_str(), Some("idle" | "working")))
    });
    assert_eq!((claude["harness"].as_str(), claude["model"].as_str()), (Some("claude"), Some("claude-haiku-5-5")), "{claude}");
}

/// A real team hands off real work: the Claude lead, which cannot edit,
/// gives the Codex member one job over `mira agent send`, Codex does it and
/// says so the same way — through its sandbox, to Mirador's socket.
/// Only what anyone could observe is checked — never what a model said.
#[test]
fn mirador_test_team_hands_off() {
    live!();
    let mut sb = Sandbox::live("handoff");
    sb.write(
        ".mirador/roles/claude.md",
        "---\nmodel: claude-haiku-5-5\ntools:\n  deny: [\"Edit\", \"Write\", \"NotebookEdit\"]\n---\n\
         You lead. You never write files yourself — your edit tools are off — you hand work to codex.\n",
    );
    sb.write(
        ".mirador/roles/codex.md",
        "---\nharness: codex\nmodel: gpt-6-luna\n---\nYou do what you are sent, briefly. When it is done, \
         tell the lead with `mira agent send claude \"done\"`.\n",
    );
    sb.write(
        ".mirador/teams/mirador-test.md",
        "---\nmembers:\n  - role: claude\n    lead: true\n  - role: codex\n---\nA test team: keep every turn short.\n",
    );
    // A repository, where Codex asks whether to trust the folder.
    std::process::Command::new("git").arg("init").current_dir(&sb.project).output().unwrap();
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::TeamStart {
        team: "mirador-test".into(),
        task: Some(
            "Hand codex one job with `mira agent send codex`: create pong.txt in the project directory \
             containing exactly the word pong. Do not write it yourself, and do not wait for a reply."
                .into(),
        ),
        pane_id: Some(first),
    });

    // Each member, by its pane, is the agent its role says: `team list`
    // names the members, `agent list` says what runs in each.
    let pane_of = |name: &str| {
        let member = sb.wait_for(&format!("{name} in team list"), LONG, || sb.member(name));
        member["pane"].as_str().unwrap().to_string()
    };
    for (name, harness, model) in [("claude", "claude", "claude-haiku-5-5"), ("codex", "codex", "gpt-6-luna")] {
        let pane = pane_of(name);
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        let agent = agents.iter().find(|a| a["paneId"] == pane.as_str()).unwrap_or_else(|| sb.fail(&format!("{name} is not an agent")));
        assert_eq!((agent["harness"].as_str(), agent["model"].as_str()), (Some(harness), Some(model)), "{agent}");
    }

    let codex_pane = pane_of("codex");
    // Codex was started allowed this sandbox's socket, which nothing else is.
    let socket = sb.runtime.join("mira.sock");
    let allowed = format!("codex .*unix_sockets=.*{}", socket.display());
    sb.wait_for(&format!("codex allowed {}", socket.display()), LONG, || cmux_e2e::process_running(&allowed).then_some(()));
    sb.wait_for_screen(&codex_pane, "the lead's message", AGENT, |s| s.contains("[message from claude"));
    let pong = sb.project.join("pong.txt");
    sb.wait_for("codex to write pong.txt", AGENT, || {
        std::fs::read_to_string(&pong).ok().filter(|t| t.trim() == "pong")
    });
    sb.wait_for_screen(&pane_of("claude"), "codex's reply", AGENT, |s| s.contains("[message from codex"));
}
