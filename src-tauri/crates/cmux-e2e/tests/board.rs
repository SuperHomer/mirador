//! The board: every pane of every tab as a card, a second layout of the
//! same panes. Skipped unless `MIRA_E2E=1` (see the crate docs for what to
//! build first). These see the model the board draws from — what it looks
//! like needs a person.

use std::time::Duration;

use cmux_core::board::CARD_GAP;
use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::{Request, SplitDir};
use serde_json::Value;

const LONG: Duration = Duration::from_secs(45);

fn leaves(node: &Value, out: &mut Vec<String>) {
    if let Some(p) = node["paneId"].as_str() {
        out.push(p.to_string());
    }
    for c in node["children"].as_array().into_iter().flatten() {
        leaves(c, out);
    }
}

/// Every tab's cards name exactly its panes, in its pane order.
fn assert_every_pane_has_a_card(snapshot: &Value) {
    for tab in snapshot["tabs"].as_array().unwrap() {
        let mut panes = Vec::new();
        leaves(&tab["root"], &mut panes);
        let cards: Vec<String> = tab["cards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["paneId"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(cards, panes, "{tab}");
    }
}

fn card(snapshot: &Value, pane: &str) -> Value {
    snapshot["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["cards"].as_array().unwrap())
        .find(|c| c["paneId"] == pane)
        .cloned()
        .unwrap_or_else(|| panic!("no card for {pane}: {snapshot}"))
}

fn split(sb: &Sandbox, pane: &str) -> String {
    sb.call(Request::SplitPane { pane_id: Some(pane.to_string()), dir: SplitDir::Row, command: None })["paneId"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Each pane's shell answers what is typed into it.
fn assert_panes_answer(sb: &Sandbox, panes: &[String]) {
    for (i, pane) in panes.iter().enumerate() {
        sb.wait_for("the pane to mount", LONG, || sb.screen(pane));
        sb.wait_for("the pane to take input", LONG, || {
            sb.request(Request::SendInput { pane_id: Some(pane.clone()), data: format!("echo board-{i}-$((6*7))\r") })
                .ok()
        });
        let want = format!("board-{i}-42");
        sb.wait_for("the pane's shell", LONG, || {
            sb.screen(pane).filter(|s| s.lines().any(|l| l.trim() == want))
        });
    }
}

#[test]
fn the_board_shows_every_pane_and_none_stops_answering() {
    e2e!();
    let mut sb = Sandbox::new("board");
    sb.launch();
    let first = sb.first_pane();

    let (code, out, err) = sb.mira(None, &["view"]);
    assert_eq!((code, out.trim()), (0, "tabs"), "{err}");
    let (code, _, err) = sb.mira(None, &["view", "board"]);
    assert_eq!(code, 0, "{err}");
    let (_, out, _) = sb.mira(None, &["view"]);
    assert_eq!(out.trim(), "board");
    let (code, _, err) = sb.mira(None, &["view", "grid"]);
    assert_ne!(code, 0, "an unknown view is refused");
    assert!(err.contains("tabs|board"), "{err}");
    assert_eq!(sb.snapshot()["view"], "board");

    // Panes opened while the board is shown get cards: a split beside its
    // source, the same size; a new tab a frame clear of the first.
    let second = split(&sb, &first);
    sb.call(Request::NewTab { command: None });
    let snap = sb.snapshot();
    assert_every_pane_has_a_card(&snap);
    let (src, new) = (card(&snap, &first), card(&snap, &second));
    assert_eq!(new["x"].as_f64().unwrap(), src["x"].as_f64().unwrap() + src["w"].as_f64().unwrap() + CARD_GAP);
    assert_eq!((&new["y"], &new["w"], &new["h"]), (&src["y"], &src["w"], &src["h"]));
    let frames: Vec<&Value> = snap["tabs"].as_array().unwrap().iter().map(|t| &t["frame"]).collect();
    let f = |v: &Value, k: &str| v[k].as_f64().unwrap();
    assert!(f(frames[1], "x") >= f(frames[0], "x") + f(frames[0], "w"), "{frames:?}");

    // Every pane, in every tab, still takes input and shows its output
    // with the board shown.
    let panes = sb.panes();
    assert_eq!(panes.len(), 3);
    assert_panes_answer(&sb, &panes);

    // Closing a pane takes its card with it.
    sb.call(Request::ClosePane { pane_id: second.clone() });
    let snap = sb.snapshot();
    assert_every_pane_has_a_card(&snap);
    assert!(!snap.to_string().contains(&second));

    // And back to tabs.
    let (code, _, err) = sb.mira(None, &["view", "tabs"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(sb.snapshot()["view"], "tabs");
}

#[test]
fn the_board_comes_back_as_it_was_left() {
    e2e!();
    let mut sb = Sandbox::new("boardkeep");
    sb.launch();
    let first = sb.first_pane();
    let second = split(&sb, &first);
    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });
    sb.call(Request::SetCardRect { pane_id: second.clone(), x: 900.0, y: 600.0, w: 420.0, h: 260.0 });
    let tab = sb.snapshot()["tabs"][0]["id"].as_str().unwrap().to_string();
    sb.call(Request::SetFramePosition { tab_id: tab.clone(), x: -300.0, y: 150.0 });
    // Zoom is held to what the board can show.
    sb.call(Request::SetCamera { x: 40.0, y: -25.0, zoom: 9.0 });
    // Garbage is refused, not saved.
    assert!(sb.request(Request::SetCardRect { pane_id: second.clone(), x: 1e300, y: 0.0, w: 1.0, h: 1.0 }).is_err());
    assert!(sb.request(Request::SetFramePosition { tab_id: "nope".into(), x: 0.0, y: 0.0 }).is_err());

    let before = sb.snapshot();
    assert_eq!(before["camera"], serde_json::json!({ "x": 40.0, "y": -25.0, "zoom": 4.0 }));

    sb.quit();
    sb.launch();
    let after = sb.snapshot();
    assert_eq!(after["view"], "board");
    assert_eq!(after["camera"], before["camera"]);
    assert_eq!(after["tabs"][0]["frame"], before["tabs"][0]["frame"]);
    assert_eq!(after["tabs"][0]["frame"]["x"], -300.0);
    let moved = card(&after, &second);
    assert_eq!((&moved["x"], &moved["y"], &moved["w"], &moved["h"]), (&900.0.into(), &600.0.into(), &420.0.into(), &260.0.into()));
    assert_eq!(card(&after, &first), card(&before, &first));
    // The splits were never touched by any of it.
    assert_eq!(after["tabs"][0]["root"], before["tabs"][0]["root"]);
    assert_panes_answer(&sb, &[first, second]);
}

/// A terminal shows the same thing after a trip to the board and back: the
/// board draws the same xterm, never a new one, so nothing is lost or
/// replayed. (Its size changes on the way, and it reflows, so only lines
/// that fit either width are compared.)
#[test]
fn switching_views_keeps_what_a_terminal_shows() {
    e2e!();
    let mut sb = Sandbox::new("boardkeepscreen");
    sb.launch();
    let first = sb.first_pane();
    let other = sb.call(Request::NewTab { command: None })["paneId"].as_str().unwrap().to_string();
    for (pane, word) in [(&first, "alpha"), (&other, "beta")] {
        sb.wait_for("the pane to take input", LONG, || sb.screen(pane));
        sb.type_into(pane, &format!("for i in 1 2 3; do echo {word}-$((i*7)); done\r"));
        let last = format!("{word}-21");
        sb.wait_for("the shell's output", LONG, || sb.screen(pane).filter(|s| s.lines().any(|l| l.trim() == last)));
    }
    let lines = |pane: &str| -> Vec<String> {
        let text = sb.request(Request::ReadScreen { pane_id: Some(pane.to_string()), lines: Some(200) }).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string();
        text.lines().filter(|l| l.contains("alpha-") || l.contains("beta-")).map(|l| l.trim().to_string()).collect()
    };
    let before = (lines(&first), lines(&other));
    assert_eq!(before.0.len(), 4, "the command line and three answers: {:?}", before.0);

    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });
    // Both terminals are drawn on the board, the other tab's included.
    sb.wait_for("the board to settle", LONG, || (lines(&first) == before.0 && lines(&other) == before.1).then_some(()));
    sb.call(Request::View { view: Some(cmux_protocol::View::Tabs) });
    sb.wait_for("the tabs to settle", LONG, || (lines(&first) == before.0 && lines(&other) == before.1).then_some(()));
    // And they still take input after the round trip.
    assert_panes_answer(&sb, &[first, other]);
}

/// On the board a terminal is as big as its card: its PTY follows the
/// card's size, not the split it has in its tab.
#[test]
fn a_card_sizes_its_terminal() {
    e2e!();
    let mut sb = Sandbox::new("boardsize");
    sb.launch();
    let pane = sb.first_pane();
    let stty = || -> Option<(u32, u32)> {
        sb.type_into(&pane, "clear; stty size\r");
        let screen = sb.screen(&pane)?;
        let line = screen
            .lines()
            .find(|l| l.split_whitespace().count() == 2 && l.chars().all(|c| c.is_ascii_digit() || c == ' '))?;
        let mut it = line.split_whitespace().map(|n| n.parse::<u32>().unwrap());
        Some((it.next()?, it.next()?))
    };
    // The terminal's size once it differs from `from`: the card is laid
    // out, xterm refits, and the shell hears of it.
    let size = |w: f64, h: f64, from: Option<(u32, u32)>| -> (u32, u32) {
        sb.call(Request::SetCardRect { pane_id: pane.clone(), x: 16.0, y: 36.0, w, h });
        sb.wait_for("the terminal to take the card's size", LONG, || stty().filter(|s| Some(*s) != from))
    };
    // Each size is read once it has changed from the one before, so a
    // reading never belongs to the previous layout: the tab's, then the
    // card's default, then each card size asked for.
    let in_tab = sb.wait_for("the terminal's size", LONG, stty);
    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });
    let default_card = sb.wait_for("the board's size", LONG, || stty().filter(|s| *s != in_tab));
    let (rows_big, cols_big) = size(1000.0, 600.0, Some(default_card));
    let (rows_small, cols_small) = size(500.0, 300.0, Some((rows_big, cols_big)));
    assert!(cols_big > cols_small * 18 / 10, "{cols_big} columns at 1000px, {cols_small} at 500px");
    assert!(rows_big > rows_small * 18 / 10, "{rows_big} rows at 600px, {rows_small} at 300px");
}
