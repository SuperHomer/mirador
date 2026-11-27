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
        sb.call(Request::AgentNew { role: Some(role.into()), harness: None, title: None, task: None, target: None, pane_id: Some(first.clone()), host: None, dir: None, on: None })
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
    sb.call(Request::TeamStart { title: None,
        team: "mirador-test".into(),
        task: Some(
            "Hand codex one job with `mira agent send codex`: create pong.txt in the project directory \
             containing exactly the word pong. Do not write it yourself, and do not wait for a reply."
                .into(),
        ),
        pane_id: Some(first),
        host: None,
        dir: None,
        on: None,
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

/// A send tells the person's draft from an empty input box by the cursor
/// alone (`cmux_core::handoff::has_draft`): Claude Code and Codex park it
/// right after their prompt's glyph when the box is empty, placeholder or
/// not. Only the fake agent had shown that. Were a real one to park it
/// anywhere else, every box typed into would read as a draft, and messages
/// to it would wait forever — so each case is checked here: an empty box
/// idle, a draft, a draft with the cursor moved to its start (told by the
/// placeholder being dim), and an empty box while the agent works — where
/// Codex's message waits for the turn instead (#246), the turn finishing
/// first.
#[test]
fn real_claude_shows_a_draft_by_its_cursor() {
    live!();
    drafts_seen_in("claude", "---\nmodel: claude-haiku-5-5\ntools:\n  allow: [\"Bash(sleep *)\"]\n---\nYou do what you are asked, briefly.\n");
}

/// [`real_claude_shows_a_draft_by_its_cursor`], for Codex.
#[test]
fn real_codex_shows_a_draft_by_its_cursor() {
    live!();
    drafts_seen_in("codex", "---\nharness: codex\nmodel: gpt-6-luna\n---\nYou do what you are asked, briefly.\n");
}

fn drafts_seen_in(name: &str, role: &str) {
    let mut sb = Sandbox::live(&format!("draft{name}"));
    // A role of the person's own, not the repository's: only those may
    // allow a tool, and Claude Code must run `sleep` without asking.
    let roles = sb.config.join("mirador/roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(roles.join(format!("{name}.md")), role).unwrap();
    // A repository, where Codex asks whether to trust the folder.
    std::process::Command::new("git").arg("init").current_dir(&sb.project).output().unwrap();
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew { role: Some(name.into()), harness: None, title: None, task: None, target: None, pane_id: Some(first.clone()), host: None, dir: None, on: None })
        ["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for_screen(&pane, "its prompt", LONG, |s| s.contains("Haiku 5.5 · ") || s.contains("Ask Codex"));

    // What a failure shows: the screen, and where its cursor is.
    let look = || {
        let read = sb.call(Request::ReadScreen { pane_id: Some(pane.clone()), lines: None });
        format!("cursor {} on:\n{}", read["cursor"], read["text"].as_str().unwrap_or_default())
    };
    let send = |message: &str| {
        sb.request(Request::AgentSend { title: None, to: pane.clone(), message: message.into(), pane_id: Some(first.clone()), force: false, card: None })
            .unwrap_or_else(|e| sb.fail(&format!("send: {e}; {}", look())))
    };
    // Past the window in which typing alone holds a message back
    // (`TYPING_QUIET`, 2 s): nothing on screen says it is over, so this
    // waits on the clock.
    let quiet = || std::thread::sleep(Duration::from_millis(2500));

    // Idle, the box empty, but typed into: only the cursor can say that
    // it holds no draft.
    sb.type_into(&pane, "x");
    sb.type_into(&pane, "\x7f");
    quiet();
    let sent = send("Reply with the sum of 20 and 22, digits only.");
    assert!(sent.get("queued").is_none(), "an empty box read as a draft: {sent}; {}", look());
    sb.wait_for_screen(&pane, "the first answer", AGENT, |s| answered(s, "42"));

    // A draft: the message waits for it, past the typing window, and goes
    // in on its own once the person clears it.
    sb.type_into(&pane, "pa");
    sb.wait_for_screen(&pane, "the draft", LONG, |s| s.lines().any(|l| l.trim_end().ends_with(" pa")));
    quiet();
    let sent = send("Reply with the sum of 30 and 33, digits only.");
    let why = sent["queued"]["why"].as_str().unwrap_or_default();
    assert!(why.starts_with("the person has a draft"), "a draft not seen: {sent}; {}", look());
    let id = sent["queued"]["id"].as_u64().unwrap();
    quiet();
    let at = sb.call(Request::AgentQueued { message_id: id });
    assert!(at["deliveredMs"].is_null(), "typed into the draft: {at}; {}", look());
    sb.type_into(&pane, "\x7f\x7f");
    sb.wait_for("the message, once the draft is gone", LONG, || {
        sb.call(Request::AgentQueued { message_id: id })["deliveredMs"].as_u64()
    });
    sb.wait_for_screen(&pane, "the second answer", AGENT, |s| answered(s, "63"));
    let prompts = prompts_with(&sb, "the sum of 30 and 33");
    assert!(!prompts.is_empty(), "the message is in no transcript under {}", sb.home.display());
    assert!(prompts.iter().all(|p| !p.contains("paReply")), "sent with the draft: {prompts:?}");

    // A draft with the cursor moved back to its start, where an empty
    // box parks it: only dimness tells it from the placeholder.
    sb.type_into(&pane, "pa\x01");
    sb.wait_for("the cursor at the draft's start", LONG, || {
        let read = sb.call(Request::ReadScreen { pane_id: Some(pane.clone()), lines: None });
        let text = read["text"].as_str().unwrap_or_default().to_string();
        let row = text.lines().position(|l| l.trim_end().ends_with(" pa"))?;
        let glyph = text.lines().nth(row)?.chars().take_while(|c| c.is_whitespace()).count();
        (read["cursor"] == serde_json::json!([row, glyph + 2])).then_some(())
    });
    quiet();
    let sent = send("Reply with the sum of 60 and 39, digits only.");
    let why = sent["queued"]["why"].as_str().unwrap_or_default();
    assert!(why.starts_with("the person has a draft"), "a draft at its start not seen: {sent}; {}", look());
    let id = sent["queued"]["id"].as_u64().unwrap();
    sb.type_into(&pane, "\x05\x7f\x7f");
    sb.wait_for("the message, once the draft is gone", LONG, || {
        sb.call(Request::AgentQueued { message_id: id })["deliveredMs"].as_u64()
    });
    sb.wait_for_screen(&pane, "the answer after the draft at its start", AGENT, |s| answered(s, "99"));
    let prompts = prompts_with(&sb, "the sum of 60 and 39");
    assert!(prompts.iter().all(|p| !p.contains("paReply") && !p.contains("only.pa")), "sent with the draft: {prompts:?}");

    // At work, the box empty: never taken for a draft. Codex's message
    // waits for the turn to end (#246): typed into it, Codex took it as a
    // steer and dropped its sleep. Claude Code's goes in at once, to be
    // read at its next tool call.
    let sent = send("Run `sleep 15` in the shell, then reply with the sum of 5 and 6, digits only.");
    assert!(sent.get("queued").is_none(), "{sent}; {}", look());
    sb.wait_for_screen(&pane, "the agent at work on its sleep", AGENT, |s| working(s) && s.contains("sleep 15"));
    let sent = send("Reply with the sum of 50 and 38, digits only.");
    if name == "codex" {
        let why = sent["queued"]["why"].as_str().unwrap_or_default();
        assert_eq!(why, "it is at work; it goes in once its turn ends", "{sent}; {}", look());
        let id = sent["queued"]["id"].as_u64().unwrap();
        // The turn finishes what it was asked, then the message is answered.
        sb.wait_for_screen(&pane, "both answers, in turn", AGENT, |s| {
            let line = |sum: &str| s.lines().position(|l| l.trim() == sum || l.trim().ends_with(&format!(" {sum}")));
            answered(s, "88") && line("11").zip(line("88")).is_some_and(|(first, then)| first < then)
        });
        assert!(sb.call(Request::AgentQueued { message_id: id })["deliveredMs"].as_u64().is_some());
    } else {
        assert!(sent.get("queued").is_none(), "an empty box at work read as a draft: {sent}; {}", look());
        sb.wait_for_screen(&pane, "the last answer", AGENT, |s| answered(s, "88"));
    }
}

fn working(screen: &str) -> bool {
    screen.contains("esc to interrupt")
}

/// The agent's answer on a line of its own, its turn over.
fn answered(screen: &str, sum: &str) -> bool {
    !working(screen) && screen.lines().any(|l| l.trim() == sum || l.trim().ends_with(&format!(" {sum}")))
}

/// Every line of the agents' transcripts — Claude Code's under
/// `~/.claude/projects`, Codex's under `~/.codex/sessions` — holding
/// `text`: what was submitted to them, as they recorded it.
fn prompts_with(sb: &Sandbox, text: &str) -> Vec<String> {
    fn walk(dir: &std::path::Path, text: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                walk(&path, text, out);
            } else if path.extension().is_some_and(|e| e == "jsonl") {
                let lines = std::fs::read_to_string(&path).unwrap_or_default();
                out.extend(lines.lines().filter(|l| l.contains(text)).map(String::from));
            }
        }
    }
    let mut out = Vec::new();
    for dir in [sb.home.join(".claude/projects"), sb.home.join(".codex/sessions")] {
        walk(&dir, text, &mut out);
    }
    out
}
