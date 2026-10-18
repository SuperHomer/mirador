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
    assert_eq!(card(&after, &second), serde_json::json!({ "paneId": second, "x": 900.0, "y": 600.0, "w": 420.0, "h": 260.0 }));
    assert_eq!(card(&after, &first), card(&before, &first));
    // The splits were never touched by any of it.
    assert_eq!(after["tabs"][0]["root"], before["tabs"][0]["root"]);
    assert_panes_answer(&sb, &[first, second]);
}
