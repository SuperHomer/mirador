//! The browser pane, driven the way an agent drives it: `mira browser …`
//! from a pane, against pages this test serves itself. Skipped unless
//! `MIRA_E2E=1` (see the crate docs for what to build first).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};

const LONG: Duration = Duration::from_secs(45);

const FORM: &str = r#"<!doctype html><title>Form</title>
<h1>Form</h1>
<input id="name" placeholder="name">
<select id="color"><option value="red">Red</option><option value="blue">Blue</option></select>
<button id="go" onclick="document.getElementById('out').textContent = 'hello ' + document.getElementById('name').value + ' ' + document.getElementById('color').value">Go</button>
<p id="out"></p>
<a href="/page2">Page two</a>"#;

const PAGE2: &str = "<!doctype html><title>Page two</title><h1>Second page</h1>";

const ERRORS: &str = r#"<!doctype html><title>Errors</title><h1>Errors</h1>
<script>console.log("fixture-log"); throw new Error("fixture-boom");</script>"#;

const LATE: &str = r#"<!doctype html><title>Late</title>
<script>setTimeout(() => { document.body.innerHTML = "<h1>Arrived late</h1>"; }, 1500);</script>"#;

/// Serves the fixture pages on an ephemeral port for the life of the test
/// process, and gives the port.
fn serve_fixture() -> u16 {
    static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve_page(stream));
            }
        });
        port
    })
}

fn serve_page(mut stream: std::net::TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
    let (status, body) = match path {
        "/" => ("200 OK", FORM),
        "/page2" => ("200 OK", PAGE2),
        "/errors" => ("200 OK", ERRORS),
        "/late" => ("200 OK", LATE),
        _ => ("404 Not Found", "not found"),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// `http://127.0.0.1:<port><path>` on the fixture.
fn page(path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", serve_fixture())
}

/// A launched sandbox and the pane an agent would run `mira` from.
fn launched(name: &str) -> (Sandbox, String) {
    let mut sb = Sandbox::new(name);
    sb.launch();
    let first = sb.first_pane();
    (sb, first)
}

/// `mira browser <args>` from `pane`, which must succeed: its stdout.
fn browser(sb: &Sandbox, pane: &str, args: &[&str]) -> String {
    let mut all = vec!["browser"];
    all.extend_from_slice(args);
    let (code, out, err) = sb.mira(Some(pane), &all);
    if code != 0 {
        sb.fail(&format!("mira {} exited {code}: {err}{out}", all.join(" ")));
    }
    out
}

/// `mira browser <args>`, retried until it succeeds and `check` passes on
/// its stdout.
fn browser_until(sb: &Sandbox, pane: &str, args: &[&str], what: &str, check: impl Fn(&str) -> bool) -> String {
    let mut all = vec!["browser"];
    all.extend_from_slice(args);
    sb.wait_for(what, LONG, || {
        let (code, out, _) = sb.mira(Some(pane), &all);
        (code == 0 && check(&out)).then_some(out)
    })
}

/// Opens `url` in a split and waits for its page to answer a snapshot.
fn open(sb: &Sandbox, pane: &str, url: &str) -> String {
    let browser_pane = browser(sb, pane, &["open", url]).trim().to_string();
    browser_until(sb, pane, &["snapshot"], "the page to load", |s| s.contains(url));
    browser_pane
}

/// The URL the workspace reports for `pane`, as the URL bar shows it.
fn reported_url(sb: &Sandbox, pane: &str) -> Option<String> {
    sb.snapshot()["browserPanes"]
        .as_array()?
        .iter()
        .find(|b| b["paneId"] == pane)
        .and_then(|b| b["url"].as_str().map(String::from))
}

/// The snapshot id of the first element whose line contains `needle`.
fn id_of(snapshot: &str, needle: &str) -> String {
    let line = snapshot
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in the snapshot:\n{snapshot}"));
    line.trim_start_matches('[').split(']').next().unwrap().to_string()
}

#[test]
fn open_splits_the_pane_and_shows_the_page() {
    e2e!();
    let (sb, first) = launched("bopen");
    let url = page("/");
    let pane = open(&sb, &first, &url);
    assert_eq!(sb.panes(), [first.clone(), pane.clone()], "a split beside the calling pane");
    let browsers = &sb.snapshot()["browserPanes"];
    assert!(
        browsers.as_array().unwrap().iter().any(|b| b["paneId"] == pane.as_str() && b["url"] == url.as_str()),
        "{browsers}"
    );
}

#[test]
fn open_with_tab_opens_a_new_tab() {
    e2e!();
    let (sb, first) = launched("btab");
    let url = page("/page2");
    let pane = browser(&sb, &first, &["open", "--tab", &url]).trim().to_string();
    let tabs = sb.snapshot()["tabs"].as_array().unwrap().len();
    assert_eq!(tabs, 2, "a tab of its own");
    assert_eq!(sb.panes(), [first.clone(), pane.clone()]);
    browser_until(&sb, &first, &["snapshot", "--pane", &pane], "the page in the new tab", |s| {
        s.starts_with("Page two — ") && s.contains("<h1> \"Second page\"")
    });
}

#[test]
fn snapshot_lists_the_page_with_ids() {
    e2e!();
    let (sb, first) = launched("bsnap");
    open(&sb, &first, &page("/"));
    let snap = browser(&sb, &first, &["snapshot"]);
    assert!(snap.starts_with(&format!("Form — {}", page("/"))), "{snap}");
    for line in ["<h1> \"Form\"", "<input> \"name\" type=text value=\"\"", "<button> \"Go\" type=submit", "<a> \"Page two\""] {
        assert!(snap.contains(line), "no {line:?} in:\n{snap}");
    }
    assert!(snap.contains(&format!("href={}", page("/page2"))), "{snap}");
    // Ids are stable: the same element keeps its id in the next snapshot.
    assert_eq!(id_of(&browser(&sb, &first, &["snapshot"]), "<button>"), id_of(&snap, "<button>"));
}

#[test]
fn fill_and_click_by_snapshot_id() {
    e2e!();
    let (sb, first) = launched("bclickid");
    open(&sb, &first, &page("/"));
    let snap = browser(&sb, &first, &["snapshot"]);
    browser(&sb, &first, &["fill", &id_of(&snap, "<input>"), "Ada"]);
    browser(&sb, &first, &["click", &id_of(&snap, "<button>")]);
    let out = browser(&sb, &first, &["eval", "document.getElementById('out').textContent"]);
    assert_eq!(out.trim(), "\"hello Ada red\"");
    // And a snapshot shows what was typed.
    assert!(browser(&sb, &first, &["snapshot"]).contains("value=\"Ada\""));
}

#[test]
fn fill_and_click_by_css_selector() {
    e2e!();
    let (sb, first) = launched("bclickcss");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["fill", "#name", "Grace"]);
    browser(&sb, &first, &["click", "#go"]);
    let out = browser(&sb, &first, &["eval", "document.getElementById('out').textContent"]);
    assert_eq!(out.trim(), "\"hello Grace red\"");
    let (code, _, err) = sb.mira(Some(&first), &["browser", "click", "#missing"]);
    assert_ne!(code, 0);
    assert!(err.contains("element not found: #missing"), "{err}");
}

#[test]
fn eval_gives_a_value_as_json() {
    e2e!();
    let (sb, first) = launched("beval");
    open(&sb, &first, &page("/"));
    assert_eq!(browser(&sb, &first, &["eval", "document.title"]).trim(), "\"Form\"");
    assert_eq!(browser(&sb, &first, &["eval", "6 * 7"]).trim(), "42");
    let obj: serde_json::Value = serde_json::from_str(&browser(&sb, &first, &["eval", "({a: [1, 2]})"])).unwrap();
    assert_eq!(obj, serde_json::json!({ "a": [1, 2] }));
    let (code, _, err) = sb.mira(Some(&first), &["browser", "eval", "nope()"]);
    assert_ne!(code, 0);
    assert!(err.contains("nope"), "a thrown error is the command's error: {err}");
}

#[test]
fn navigate_to_an_absolute_url() {
    e2e!();
    let (sb, first) = launched("bnavabs");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", &page("/page2")]);
    browser_until(&sb, &first, &["snapshot"], "page two", |s| s.starts_with("Page two — "));
}

#[test]
fn navigate_to_a_relative_path() {
    e2e!();
    let (sb, first) = launched("bnavrel");
    let pane = open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", "/page2"]);
    browser_until(&sb, &first, &["snapshot"], "page two on the same site", |s| {
        s.starts_with(&format!("Page two — {}", page("/page2")))
    });
    sb.wait_for("the reported url", LONG, || reported_url(&sb, &pane).filter(|u| *u == page("/page2")));
}

#[test]
fn navigate_to_localhost_without_a_scheme() {
    e2e!();
    let (sb, first) = launched("bnavlocal");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", &format!("localhost:{}/page2", serve_fixture())]);
    let url = format!("http://localhost:{}/page2", serve_fixture());
    browser_until(&sb, &first, &["snapshot"], "page two over http", |s| s.starts_with(&format!("Page two — {url}")));
}

#[test]
fn open_localhost_without_a_scheme() {
    e2e!();
    let (sb, first) = launched("bopenlocal");
    let pane = browser(&sb, &first, &["open", "--tab", &format!("localhost:{}", serve_fixture())]).trim().to_string();
    let url = format!("http://localhost:{}/", serve_fixture());
    browser_until(&sb, &first, &["snapshot", "--pane", &pane], "the form over http", |s| {
        s.starts_with(&format!("Form — {url}"))
    });
    assert_eq!(reported_url(&sb, &pane), Some(url));
}

/// A navigation that fails leaves the pane on the page it was showing,
/// and the reported URL says so — not the one that never loaded.
#[test]
fn a_failed_navigation_reports_the_page_still_shown() {
    e2e!();
    let (sb, first) = launched("bnavfail");
    let pane = open(&sb, &first, &page("/"));
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    browser(&sb, &first, &["navigate", &format!("http://127.0.0.1:{closed}/")]);
    // Nothing listens there, so the load fails at once; the page still
    // answering says it is over.
    browser_until(&sb, &first, &["eval", "location.href"], "the form, still", |s| s.trim() == format!("{:?}", page("/")));
    assert_eq!(reported_url(&sb, &pane), Some(page("/")));
}

#[test]
fn back_forward_and_reload() {
    e2e!();
    let (sb, first) = launched("bhistory");
    let pane = open(&sb, &first, &page("/"));
    // Followed a link, as a person would. Deferred: a link clicked within
    // the bridge's own call does not navigate yet.
    browser(&sb, &first, &["eval", "setTimeout(() => document.querySelector('a').click(), 50); 0"]);
    browser_until(&sb, &first, &["snapshot"], "page two after the link", |s| s.starts_with("Page two — "));
    sb.wait_for("the url bar after the link", LONG, || reported_url(&sb, &pane).filter(|u| *u == page("/page2")));
    browser(&sb, &first, &["back"]);
    browser_until(&sb, &first, &["snapshot"], "the form after back", |s| s.starts_with("Form — "));
    sb.wait_for("the url bar after back", LONG, || reported_url(&sb, &pane).filter(|u| *u == page("/")));
    browser(&sb, &first, &["forward"]);
    browser_until(&sb, &first, &["snapshot"], "page two after forward", |s| s.starts_with("Page two — "));
    sb.wait_for("the url bar after forward", LONG, || reported_url(&sb, &pane).filter(|u| *u == page("/page2")));

    // A reload is a new document: what the page held is gone.
    browser(&sb, &first, &["eval", "window.marker = 1"]);
    browser(&sb, &first, &["reload"]);
    browser_until(&sb, &first, &["eval", "typeof window.marker"], "a fresh document", |s| s.trim() == "\"undefined\"");
}

/// `navigate` returns before the page loads, yet the next command already
/// reaches the new document: twenty in a row did, not one the old page.
#[test]
fn snapshot_right_after_navigate() {
    e2e!();
    let (sb, first) = launched("bnavrace");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", &page("/page2")]);
    let snap = browser(&sb, &first, &["snapshot"]);
    assert!(snap.starts_with("Page two — "), "the new page, not the old one:\n{snap}");
}
