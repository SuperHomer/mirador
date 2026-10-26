//! The agent wall: a tab's agents' terminals, lent to its tiles. Skipped
//! unless `MIRA_E2E=1`. What a tile looks like needs a person; how big the
//! terminal in it is, the agent itself can say.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::{Request, SplitDir};

const LONG: Duration = Duration::from_secs(45);

fn split(sb: &Sandbox, pane: &str) -> String {
    sb.call(Request::SplitPane { pane_id: Some(pane.to_string()), dir: SplitDir::Row, command: None })["paneId"]
        .as_str()
        .unwrap()
        .to_string()
}

fn agent(sb: &Sandbox, pane: &str) -> Option<serde_json::Value> {
    let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
    agents.into_iter().find(|a| a["paneId"] == pane)
}

/// Starts the fake `claude` in `pane`, and waits for Mirador to see its
/// process: its model comes from there. Its hooks alone make it an agent
/// first, and one that exits before its process is seen is not forgotten.
fn start_agent(sb: &Sandbox, pane: &str) {
    sb.type_into(pane, "claude --model e2e\r");
    sb.wait_for("the agent's process", LONG, || agent(sb, pane).filter(|a| a["model"] == "e2e"));
}

/// The size of `pane`'s terminal, `(rows, cols)`, as its agent's `!size`
/// prints it: the last rows-and-columns line on its screen.
fn size(sb: &Sandbox, pane: &str) -> Option<(u32, u32)> {
    sb.type_into(pane, "!size\r");
    let screen = sb.screen(pane)?;
    let line = screen
        .lines()
        .rev()
        .find(|l| l.split_whitespace().count() == 2 && l.chars().all(|c| c.is_ascii_digit() || c == ' '))?;
    let mut it = line.split_whitespace().map(|n| n.parse::<u32>().unwrap());
    Some((it.next()?, it.next()?))
}

/// Sets `agentsPerTab` in the sandbox's mirador.json, which the app picks
/// up like any edit: the Grid's pages, and so its tiles, change size.
fn set_agents_per_tab(sb: &Sandbox, n: u8) {
    let path = sb.config.join("mirador/mirador.json");
    let mut cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    cfg["agentsPerTab"] = n.into();
    std::fs::write(&path, cfg.to_string()).unwrap();
}

/// A tile that goes away while its pane lives on — the agent exits, its
/// shell stays, under the wall — gives the terminal back, so the next tile
/// for that pane takes it and sizes it. It once lent it to itself again on
/// the way out, to a node leaving the page, and the pane's terminal stayed
/// that size. In Grid, where every tile fits its terminal; the Grid's tiles
/// keep their size as agents come and go, so the page is made smaller in
/// between, or the next tile would be the size the last one was.
#[test]
fn a_tile_that_goes_gives_its_terminal_back() {
    e2e!();
    let mut sb = Sandbox::new("walltile");
    sb.launch();
    let a = sb.first_pane();
    let b = split(&sb, &a);
    start_agent(&sb, &a);
    start_agent(&sb, &b);
    let in_tab = sb.wait_for("A's size", LONG, || size(&sb, &a));

    // Two tiles of a page of four: the wall has A's terminal once it is sized.
    let (code, out, err) = sb.mira(Some(&a), &["agent", "wall", "--grid"]);
    assert_eq!((code, out.trim()), (0, "wall on: grid"), "{err}");
    let full = sb.wait_for("A's tile", LONG, || size(&sb, &a).filter(|s| *s != in_tab));

    // A's tile goes; its shell stays, its pane hidden under the wall.
    sb.type_into(&a, "/exit\r");
    sb.wait_for("A to stop being an agent", LONG, || agent(&sb, &a).is_none().then_some(()));

    // Pages of nine: three columns, three rows, every tile smaller.
    set_agents_per_tab(&sb, 9);
    let small = sb.wait_for("B's tile to shrink", LONG, || size(&sb, &b).filter(|s| s.0 < full.0 && s.1 < full.1));
    start_agent(&sb, &a);
    sb.wait_for("A's tile to be sized like B's", LONG, || (size(&sb, &a) == Some(small)).then_some(()));
}

/// The size `pane`'s terminal has now: `!size`'s own answer, waited for —
/// unlike `size`, which may read the last one printed, and so cannot say a
/// size did not change.
fn fresh_size(sb: &Sandbox, pane: &str) -> (u32, u32) {
    let answers = || -> Vec<(u32, u32)> {
        let all = sb.request(Request::ReadScreen { pane_id: Some(pane.to_string()), lines: Some(100_000) });
        let text = all.ok().and_then(|v| v["text"].as_str().map(String::from)).unwrap_or_default();
        text.lines()
            .filter(|l| l.split_whitespace().count() == 2 && l.chars().all(|c| c.is_ascii_digit() || c == ' '))
            .map(|l| {
                let mut it = l.split_whitespace().map(|n| n.parse::<u32>().unwrap());
                (it.next().unwrap(), it.next().unwrap())
            })
            .collect()
    };
    let before = answers().len();
    sb.type_into(pane, "!size\r");
    sb.wait_for("a fresh size", LONG, || {
        let now = answers();
        (now.len() > before).then(|| *now.last().unwrap())
    })
}

/// The same size twice in a row: a pane's status bar can still be
/// settling (its text wraps or not) when the agent first starts.
fn settled_size(sb: &Sandbox, pane: &str) -> (u32, u32) {
    let mut last = fresh_size(sb, pane);
    for _ in 0..10 {
        let now = fresh_size(sb, pane);
        if now == last {
            return now;
        }
        last = now;
    }
    sb.fail(&format!("{pane}'s size never settled"))
}

/// Focus shows one agent large, its terminal fitted to the large tile, and
/// the rest down a rail as thumbnails: the same terminals, drawn smaller,
/// never resized — focus moves often, and each resize would reflow the
/// agent's screen. Whoever needs you is shown large.
#[test]
fn focus_fits_the_agent_shown_large_and_never_resizes_the_rail() {
    e2e!();
    let mut sb = Sandbox::new("wallfocus");
    sb.launch();
    let a = sb.first_pane();
    let b = split(&sb, &a);
    let c = split(&sb, &b);
    start_agent(&sb, &a);
    start_agent(&sb, &c);
    // B is a Codex: no hooks, so no status, and the rail draws an agent
    // whose status is unknown as a thumbnail, with the working. (A working
    // fake agent would not do: it reads nothing mid-turn, `!size` included.)
    sb.type_into(&b, "codex --model e2e\r");
    sb.wait_for("B's process", LONG, || agent(&sb, &b).filter(|a| a["model"] == "e2e"));
    // Three panes side by side, each a third of the tab across.
    let in_a = settled_size(&sb, &a);
    let in_b = settled_size(&sb, &b);

    // A and C are idle, B's status unknown: A, first of the idle in the
    // tab's order, is shown large, wider than a third of the tab. B goes to
    // the rail at the size it had.
    let (code, out, err) = sb.mira(Some(&a), &["agent", "wall", "--focus"]);
    assert_eq!((code, out.trim()), (0, "wall on: focus"), "{err}");
    let large = sb.wait_for("A's terminal to take the large tile", LONG, || {
        size(&sb, &a).filter(|s| s.1 > in_a.1)
    });
    assert_eq!(fresh_size(&sb, &b), in_b, "a thumbnail is scaled, not resized");

    // C asks for permission: it is shown large, fitted the same way, and
    // B, still on the rail, keeps its size.
    sb.type_into(&c, "!ask\r");
    sb.wait_for("C to need you", LONG, || agent(&sb, &c).filter(|a| a["status"] == "needsYou"));
    sb.wait_for("C's terminal to take the large tile", LONG, || {
        size(&sb, &c).filter(|s| s.1 == large.1)
    });
    assert_eq!(fresh_size(&sb, &b), in_b, "the rail never resizes its terminals");
}
