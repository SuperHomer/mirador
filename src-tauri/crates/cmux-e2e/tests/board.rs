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
    // source, the same size (its frame has no room inside); a new tab a
    // frame clear of the first.
    let second = split(&sb, &first);
    sb.call(Request::NewTab { command: None });
    let snap = sb.snapshot();
    assert_every_pane_has_a_card(&snap);
    let (src, new) = (card(&snap, &first), card(&snap, &second));
    assert_eq!(new["x"].as_f64().unwrap(), src["x"].as_f64().unwrap() + src["w"].as_f64().unwrap() + CARD_GAP);
    assert_eq!((&new["y"], &new["w"], &new["h"]), (&src["y"], &src["w"], &src["h"]));
    let frames: Vec<&Value> = snap["tabs"].as_array().unwrap().iter().map(|t| &t["frame"]).collect();
    assert_nothing_overlaps(&snap);
    // A frame dropped onto another stays where it was.
    let second_tab = snap["tabs"][1]["id"].as_str().unwrap().to_string();
    let at = sb.call(Request::SetFramePosition { tab_id: second_tab, x: 100.0, y: 100.0 });
    assert_eq!((&at["x"], &at["y"]), (&frames[1]["x"], &frames[1]["y"]));

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
    // Below the first card: a free place.
    sb.call(Request::SetCardRect { pane_id: second.clone(), x: 16.0, y: 900.0, w: 420.0, h: 260.0 });
    let tab = sb.snapshot()["tabs"][0]["id"].as_str().unwrap().to_string();
    sb.call(Request::SetFramePosition { tab_id: tab.clone(), x: -300.0, y: 150.0 });
    // Zoom is held to what the board can show.
    sb.call(Request::SetCamera { x: 40.0, y: -25.0, zoom: 9.0, width: None, height: None });
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
    assert_eq!((&moved["x"], &moved["y"], &moved["w"], &moved["h"]), (&16.0.into(), &900.0.into(), &420.0.into(), &260.0.into()));
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

/// A page for a browser card, served on an ephemeral port for the life of
/// the test.
fn serve_page() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let body = "<!doctype html><title>board-page</title><p id=hi>on a card</p>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}/")
}

/// A browser card the board is not showing — its page hidden, off in a
/// frame out of view — still answers every `mira browser` command: hidden
/// is not gone. And the page is laid out at its card's size.
#[test]
fn a_hidden_browser_card_still_answers() {
    e2e!();
    let mut sb = Sandbox::new("boardbrowser");
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });
    let (code, out, err) = sb.mira(Some(&first), &["browser", "open", &serve_page(), "--tab"]);
    assert_eq!(code, 0, "{err}");
    let pane = out.trim().to_string();
    // Its frame moved far out of view: the page hidden.
    let tab = sb.snapshot()["activeTab"].as_str().unwrap().to_string();
    sb.call(Request::SetFramePosition { tab_id: tab, x: 50000.0, y: 50000.0 });
    let browser = |args: &[&str]| -> String {
        let mut all = vec!["browser"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--pane", &pane]);
        let (code, out, err) = sb.mira(None, &all);
        assert_eq!(code, 0, "{all:?}: {err}");
        out.trim().to_string()
    };
    // Out of view, so hidden — and the page knows it.
    assert_eq!(browser(&["eval", "document.visibilityState"]), "\"hidden\"");
    assert_eq!(browser(&["eval", "document.title"]), "\"board-page\"");
    assert!(browser(&["text"]).contains("on a card"));
    browser(&["click", "#hi"]);
    let width: f64 = browser(&["eval", "innerWidth"]).parse().unwrap();
    let card = card(&sb.snapshot(), &pane);
    let card_width = card["w"].as_f64().unwrap();
    assert!(width > card_width - 40.0 && width <= card_width, "page {width}px wide on a {card_width}px card");
}

/// Switching views resizes a terminal once each way — its card and its
/// split differ in size — and never more: no storm of resizes, each of
/// which an agent's TUI would redraw for.
#[test]
fn switching_views_resizes_a_terminal_once_each_way() {
    e2e!();
    let mut sb = Sandbox::new("boardwinch");
    sb.launch();
    let pane = sb.first_pane();
    let log = sb.project.join("winch.log");
    sb.type_into(&pane, "TRAPWINCH() { echo w >> winch.log }; echo trap-$((6*7))\r");
    sb.wait_for("the trap", LONG, || sb.screen(&pane).filter(|s| s.lines().any(|l| l.trim() == "trap-42")));
    let resizes = || std::fs::read_to_string(&log).map(|s| s.lines().count()).unwrap_or(0);
    let start = resizes();
    for round in 0..3 {
        for (i, view) in [cmux_protocol::View::Board, cmux_protocol::View::Tabs].into_iter().enumerate() {
            sb.call(Request::View { view: Some(view) });
            let want = start + round * 2 + i + 1;
            sb.wait_for("the resize", LONG, || (resizes() >= want).then_some(()));
        }
    }
    // A round trip through the shell after the last switch: anything
    // still coming has had time to arrive.
    sb.type_into(&pane, "echo settled-$((6*7))\r");
    sb.wait_for("the shell", LONG, || sb.screen(&pane).filter(|s| s.lines().any(|l| l.trim() == "settled-42")));
    assert_eq!(resizes() - start, 6, "one resize per switch, six switches");
}

/// A browser card shows its page when the board can show it — at 100%,
/// wholly in view, still — and hides it once its card leaves the view. A
/// card cannot be put over it: that move is refused.
#[test]
fn a_browser_card_shows_its_page_only_when_in_view() {
    e2e!();
    let mut sb = Sandbox::new("boardshow");
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });
    let (code, out, err) = sb.mira(Some(&first), &["browser", "open", &serve_page()]);
    assert_eq!(code, 0, "{err}");
    let pane = out.trim().to_string();
    let visibility = || {
        let (_, out, _) = sb.mira(None, &["browser", "eval", "document.visibilityState", "--pane", &pane]);
        out.trim().to_string()
    };
    let place = |p: &str, x: f64, y: f64| -> Value {
        sb.call(Request::SetCardRect { pane_id: p.to_string(), x, y, w: 400.0, h: 260.0 })
    };
    // Both cards small and apart. Focusing a pane brings its card into
    // view if it is not (the board pans to it) — even the pane that has
    // the focus already, as this one does since it opened.
    let focus = |p: &str| sb.call(Request::FocusPane { pane_id: p.to_string() });
    place(&first, 16.0, 340.0);
    place(&pane, 16.0, 36.0);
    focus(&pane);
    sb.wait_for("the page to be shown", LONG, || (visibility() == "\"visible\"").then_some(()));
    // The terminal's card cannot go over it: it stays where it was.
    let refused = place(&first, 100.0, 100.0);
    assert_eq!((&refused["x"], &refused["y"]), (&16.0.into(), &340.0.into()), "{refused}");
    // Out of view: hidden; back: shown.
    place(&pane, 3000.0, 36.0);
    sb.wait_for("the page to be hidden", LONG, || (visibility() == "\"hidden\"").then_some(()));
    place(&pane, 16.0, 36.0);
    focus(&pane);
    sb.wait_for("the page to be shown again", LONG, || (visibility() == "\"visible\"").then_some(()));
    // And in the tabs view it is the app's to show, as before.
    sb.call(Request::View { view: Some(cmux_protocol::View::Tabs) });
    sb.wait_for("the page in its tab", LONG, || (visibility() == "\"visible\"").then_some(()));
}

/// Nothing on the board overlaps: no two frames, no two cards.
fn assert_nothing_overlaps(snapshot: &Value) {
    let rect = |v: &Value, dx: f64, dy: f64| {
        let f = |k: &str| v[k].as_f64().unwrap();
        (f("x") + dx, f("y") + dy, f("w"), f("h"))
    };
    let apart = |a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)| {
        a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1
    };
    let tabs = snapshot["tabs"].as_array().unwrap();
    let frames: Vec<_> = tabs.iter().map(|t| rect(&t["frame"], 0.0, 0.0)).collect();
    let cards: Vec<_> = tabs
        .iter()
        .flat_map(|t| {
            let (fx, fy) = (t["frame"]["x"].as_f64().unwrap(), t["frame"]["y"].as_f64().unwrap());
            t["cards"].as_array().unwrap().iter().map(move |c| rect(c, fx, fy))
        })
        .collect();
    for rects in [frames, cards] {
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(apart(*a, *b), "{a:?} overlaps {b:?}");
            }
        }
    }
}

/// What `mira` creates while the board is shown lands by the board's rules,
/// as what the board's own controls create does — they are the same calls:
/// a pane beside another gets a card in that frame, a new tab a frame where
/// the board is looked at, and nothing ever overlaps.
#[test]
fn what_mira_creates_lands_by_the_boards_rules() {
    e2e!();
    let mut sb = Sandbox::new("boardcreate");
    // A repository with a change, for `diff` and `graph`.
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=e2e", "-c", "user.email=e2e@example.com"])
            .args(args)
            .current_dir(&sb.project)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    sb.write("notes.txt", "one\n");
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    sb.write("notes.txt", "one\ntwo\n");
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::View { view: Some(cmux_protocol::View::Board) });

    let url = serve_page();
    for args in [
        vec!["split"],
        vec!["browser", "open", url.as_str()],
        vec!["agent", "new"],
        vec!["diff"],
        vec!["graph"],
    ] {
        let (code, _, err) = sb.mira(Some(&first), &args);
        assert_eq!(code, 0, "{args:?}: {err}");
        let snap = sb.snapshot();
        assert_every_pane_has_a_card(&snap);
        assert_nothing_overlaps(&snap);
    }
    let snap = sb.snapshot();
    assert_eq!(snap["tabs"].as_array().unwrap().len(), 1, "every one joined the first frame");
    assert_eq!(snap["tabs"][0]["cards"].as_array().unwrap().len(), 6);

    // Closing a pane takes its card; the rest stay where they were.
    let panes = sb.panes();
    let before = snap["tabs"][0]["cards"].clone();
    sb.call(Request::ClosePane { pane_id: panes[1].clone() });
    let after = sb.snapshot()["tabs"][0]["cards"].clone();
    let kept: Vec<&Value> = before.as_array().unwrap().iter().filter(|c| c["paneId"] != panes[1].as_str()).collect();
    assert_eq!(after.as_array().unwrap().iter().collect::<Vec<_>>(), kept);

    // A new tab's frame goes where the board is looked at. The app saves
    // where that is a moment after the board moves (each pane made above
    // brought its card into view), so let it settle before saying where.
    sb.type_into(&first, "sleep 1; echo settled-$((6*7))\r");
    sb.wait_for("the shell", LONG, || sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "settled-42")));
    sb.call(Request::SetCamera { x: -20000.0, y: -20000.0, zoom: 1.0, width: Some(1000.0), height: Some(800.0) });
    let (code, _, err) = sb.mira(None, &["new-tab"]);
    assert_eq!(code, 0, "{err}");
    let snap = sb.snapshot();
    let frame = &snap["tabs"].as_array().unwrap().iter().find(|t| t["id"] == snap["activeTab"]).unwrap()["frame"];
    let f = |k: &str| frame[k].as_f64().unwrap();
    assert_eq!((f("x") + f("w") / 2.0, f("y") + f("h") / 2.0), (-19500.0, -19600.0), "centred in view: {frame}");
    assert_nothing_overlaps(&snap);

    // What else a frame's buttons do: open the wall (a frame of its own),
    // and close a tab, its frame with it.
    let (code, _, err) = sb.mira(Some(&first), &["agent", "wall"]);
    assert_eq!(code, 0, "{err}");
    let snap = sb.snapshot();
    assert_eq!(snap["tabs"].as_array().unwrap().len(), 3);
    assert_nothing_overlaps(&snap);
    let wall_tab = snap["activeTab"].as_str().unwrap().to_string();
    let (code, _, err) = sb.mira(None, &["close-tab", &wall_tab]);
    assert_eq!(code, 0, "{err}");
    let snap = sb.snapshot();
    assert_eq!(snap["tabs"].as_array().unwrap().len(), 2);
    assert!(!snap.to_string().contains(&wall_tab));
}
