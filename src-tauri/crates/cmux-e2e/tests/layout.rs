//! Rearranging a tab's panes — `mira swap`, `rotate`, `equalize`,
//! `layout` — in the running app. Skipped unless `MIRA_E2E=1` (see the
//! crate docs for what to build first).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::{Request, SplitDir};
use serde_json::Value;

const LONG: Duration = Duration::from_secs(45);

fn split(sb: &Sandbox, pane: &str, dir: SplitDir) -> String {
    sb.call(Request::SplitPane { pane_id: Some(pane.to_string()), dir, command: None })["paneId"]
        .as_str()
        .unwrap()
        .to_string()
}

/// `mira <args>` from `pane`, which must succeed.
fn mira(sb: &Sandbox, pane: &str, args: &[&str]) -> String {
    let (code, out, err) = sb.mira(Some(pane), args);
    if code != 0 {
        sb.fail(&format!("mira {} exited {code}: {err}{out}", args.join(" ")));
    }
    out
}

/// `mira <args>` from `pane`, which must be refused: its error.
fn refused(sb: &Sandbox, pane: &str, args: &[&str]) -> String {
    let (code, out, err) = sb.mira(Some(pane), args);
    if code == 0 {
        sb.fail(&format!("mira {} was not refused: {out}", args.join(" ")));
    }
    err
}

/// The first tab's split tree.
fn root(sb: &Sandbox) -> Value {
    sb.snapshot()["tabs"][0]["root"].clone()
}

/// Echoes a marker the shell has to compute, so only its output, never
/// the typed line, reads `mark-<name>-2`.
fn mark(sb: &Sandbox, pane: &str, name: &str) {
    sb.type_into(pane, &format!("echo mark-{name}-$((1+1))\r"));
    sb.wait_for_screen(pane, &format!("{name}'s marker"), LONG, |s| has_mark(s, name));
}

fn has_mark(screen: &str, name: &str) -> bool {
    screen.lines().any(|l| l.trim() == format!("mark-{name}-2"))
}

/// Every pane shows its own marker and no other: its terminal went with
/// it, still drawn and still holding what it printed.
fn each_shows_its_own(sb: &Sandbox, panes: &[(&str, &str)]) {
    for (pane, name) in panes {
        let screen = sb.wait_for_screen(pane, &format!("{name} after the move"), LONG, |s| has_mark(s, name));
        for (_, other) in panes.iter().filter(|(_, o)| o != name) {
            assert!(!has_mark(&screen, other), "{name}'s pane shows {other}'s marker:\n{screen}");
        }
    }
}

/// Swapping and rotating move the panes in the tree and their terminals
/// with them; a grid and then even sizes give the tree those shapes.
#[test]
fn panes_move_with_their_terminals() {
    e2e!();
    let mut sb = Sandbox::new("layout");
    sb.launch();
    // [a | [b over c]]
    let a = sb.first_pane();
    let b = split(&sb, &a, SplitDir::Row);
    let c = split(&sb, &b, SplitDir::Column);
    for (pane, name) in [(&a, "a"), (&b, "b"), (&c, "c")] {
        mark(&sb, pane, name);
    }

    // b is a's neighbor on the right (the tie with c goes to the first).
    mira(&sb, &a, &["swap", &a, "--dir", "right"]);
    assert_eq!(sb.panes(), vec![b.clone(), a.clone(), c.clone()]);
    each_shows_its_own(&sb, &[(&a, "a"), (&b, "b"), (&c, "c")]);
    // And it still takes typing where it went.
    sb.type_into(&a, "echo moved-$((6*7))\r");
    sb.wait_for_screen(&a, "typing after the swap", LONG, |s| s.lines().any(|l| l.trim() == "moved-42"));

    let err = refused(&sb, &a, &["swap", &b, "--dir", "left"]);
    assert!(err.contains("no pane on its left"), "{err}");

    mira(&sb, &a, &["rotate"]);
    assert_eq!(sb.panes(), vec![c.clone(), b.clone(), a.clone()]);
    mira(&sb, &a, &["rotate", "--back"]);
    assert_eq!(sb.panes(), vec![b.clone(), a.clone(), c.clone()]);
    each_shows_its_own(&sb, &[(&a, "a"), (&b, "b"), (&c, "c")]);

    // Grid of three: [b a] over c.
    mira(&sb, &a, &["layout", "grid"]);
    let tree = root(&sb);
    assert_eq!(tree["dir"], "column", "{tree}");
    assert_eq!(tree["children"][0]["dir"], "row", "{tree}");
    assert_eq!(tree["children"][0]["children"].as_array().unwrap().len(), 2, "{tree}");
    assert_eq!(tree["children"][1]["paneId"], c.as_str(), "{tree}");
    assert_eq!(sb.panes(), vec![b.clone(), a.clone(), c.clone()]);
    let err = refused(&sb, &a, &["layout", "grid"]);
    assert!(err.contains("already laid out grid"), "{err}");
    each_shows_its_own(&sb, &[(&a, "a"), (&b, "b"), (&c, "c")]);

    // A split halves a's share of the top row: 0.5, 0.25, 0.25.
    let d = split(&sb, &a, SplitDir::Row);
    mira(&sb, &a, &["equalize"]);
    let tree = root(&sb);
    let ratios: Vec<f64> = tree["children"][0]["ratios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_f64().unwrap())
        .collect();
    assert_eq!(ratios.len(), 3, "{tree}");
    assert!(ratios.iter().all(|r| (r - 1.0 / 3.0).abs() < 1e-4), "{tree}");
    assert_eq!(sb.panes(), vec![b, a.clone(), d, c]);
    let err = refused(&sb, &a, &["equalize"]);
    assert!(err.contains("already even"), "{err}");

    let tab = sb.call(Request::NewTab { command: None });
    let lone = tab["tabId"].as_str().unwrap_or_else(|| sb.fail(&format!("new tab: {tab}"))).to_string();
    let err = refused(&sb, &a, &["rotate", "--tab", &lone]);
    assert!(err.contains("nothing to rotate"), "{err}");
}

/// A browser pane laid out again keeps its page, and its page takes the
/// pane's new size: full width stacked, about half side by side.
#[test]
fn a_page_keeps_loaded_and_takes_its_new_size() {
    e2e!();
    let mut sb = Sandbox::new("layoutweb");
    sb.launch();
    let first = sb.first_pane();
    let url = serve_page();
    let page = mira(&sb, &first, &["browser", "open", &url]).trim().to_string();
    let eval = |js: &str| -> Option<String> {
        let (code, out, _) = sb.mira(Some(&first), &["browser", "eval", "--pane", &page, js]);
        (code == 0).then(|| out.trim().to_string())
    };
    let width = |what: &str, check: &dyn Fn(f64) -> bool| -> f64 {
        sb.wait_for(what, LONG, || {
            let w: f64 = eval("innerWidth")?.parse().ok()?;
            check(w).then_some(w)
        })
    };
    sb.wait_for("the page to load", LONG, || eval("document.readyState").filter(|s| s == "\"complete\""));
    // Gone if the page is ever loaded again.
    assert_eq!(eval("window.stayed = 42").as_deref(), Some("42"));

    // `browser open` splits the pane beside it: side by side already, so
    // stack them first.
    mira(&sb, &first, &["layout", "stacked"]);
    let full = width("the stacked page's width", &|w| w > 0.0);
    mira(&sb, &first, &["layout", "side-by-side"]);
    let half = width("the side-by-side page's width", &|w| w < full * 0.7);
    assert!(half > full * 0.3, "side by side, {half} of {full}");
    mira(&sb, &first, &["layout", "stacked"]);
    width("the page's width stacked again", &|w| (w - full).abs() < 2.0);
    assert_eq!(eval("document.readyState").as_deref(), Some("\"complete\""));
    assert_eq!(eval("window.stayed").as_deref(), Some("42"), "the page was never loaded again");
}

/// Serves one page on a port of its own; its URL.
fn serve_page() -> String {
    const PAGE: &str = "<!doctype html><title>Layout</title><h1>Layout</h1>";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}/")
}
