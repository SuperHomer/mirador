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

/// A tile that goes away while its pane lives on — the agent exits, its
/// shell stays, under the wall — gives the terminal back, so the next tile
/// for that pane takes it and sizes it. It once lent it to itself again on
/// the way out, to a node leaving the page, and the pane's terminal stayed
/// that size.
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

    // Two tiles, side by side: the wall has A's terminal once it is sized.
    let (code, out, err) = sb.mira(Some(&a), &["agent", "wall"]);
    assert_eq!((code, out.trim()), (0, "wall on"), "{err}");
    let full = sb.wait_for("A's tile", LONG, || size(&sb, &a).filter(|s| *s != in_tab));

    // A's tile goes; its shell stays, its pane hidden under the wall.
    sb.type_into(&a, "/exit\r");
    sb.wait_for("A to stop being an agent", LONG, || agent(&sb, &a).is_none().then_some(()));

    // Three tiles, two rows: every tile half the height.
    let c = split(&sb, &b);
    start_agent(&sb, &c);
    start_agent(&sb, &a);
    let half = sb.wait_for("B's tile to shrink", LONG, || size(&sb, &b).filter(|s| s.0 < full.0));
    sb.wait_for("A's tile to be sized like B's", LONG, || (size(&sb, &a) == Some(half)).then_some(()));
}
