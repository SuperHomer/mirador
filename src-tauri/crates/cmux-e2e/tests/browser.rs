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
<select id="color" onchange="document.getElementById('out').textContent = 'picked ' + this.value"><option value="red">Red</option><option value="blue">Blue</option><option value="g">Green</option></select>
<button id="go" onclick="document.getElementById('out').textContent = 'hello ' + document.getElementById('name').value + ' ' + document.getElementById('color').value">Go</button>
<input type="checkbox" id="agree">
<p id="out"></p>
<a href="/page2">Page two</a>"#;

const SUBMIT: &str = r#"<!doctype html><title>Submit</title>
<form action="/page2"><input name="q" value="sent"><button>Send</button></form>"#;

const PAGE2: &str = "<!doctype html><title>Page two</title><h1>Second page</h1>";

const ERRORS: &str = r#"<!doctype html><title>Errors</title><h1>Errors</h1>
<img src="/missing.png" alt="broken">
<script>
console.log("fixture-log", { a: 1 });
console.warn("fixture-warn");
console.error("fixture-error");
Promise.reject(new Error("fixture-rejected"));
throw new Error("fixture-boom");
</script>"#;

const KEYS: &str = r#"<!doctype html><title>Keys</title>
<form action="/page2">
<input id="q" name="q" onkeyup="document.getElementById('mirror').textContent = this.value">
<input id="next">
<button id="go">Go</button>
</form>
<p id="mirror"></p><p id="esc"></p>
<script>document.addEventListener("keydown", (e) => { if (e.key === "Escape") document.getElementById("esc").textContent = "escaped"; });</script>"#;

/// Forms for Enter's implicit submission, each with a different shape.
/// Each records a `submit` event; one that navigates lands on /page2.
fn implicit_form(path: &str) -> Option<String> {
    let fields = match path {
        // The default button is clicked: its handler and its name/value
        // go with the submission.
        "/form-button" => r#"<input id="f" name="q" value="b"><input id="g">
            <input type="hidden" name="clicked">
            <button name="via" value="button" onclick="this.form.clicked.value = 'yes'">Send</button>"#,
        // An image is a submit button too.
        "/form-image" => r#"<input id="f" name="q" value="i"><input id="g">
            <input type="hidden" name="clicked">
            <input type="image" name="img" alt="Send" onclick="this.form.clicked.value = 'yes'">"#,
        "/form-one" => r#"<input id="f" name="q" value="one">"#,
        "/form-two" => r#"<input id="f" name="q" value="two"><input id="g" name="r">"#,
        "/form-disabled" => r#"<input id="f" name="q" value="d"><button disabled>Send</button>"#,
        _ => return None,
    };
    Some(format!(
        r#"<!doctype html><title>Implicit</title>
<form action="/page2" onsubmit="window.submitted = true">{fields}</form>"#
    ))
}

const SPAM: &str = r#"<!doctype html><title>Spam</title>
<script>for (let i = 0; i < 5000; i++) console.log("line " + i);</script>"#;

const LATE: &str = r#"<!doctype html><title>Late</title>
<script>setTimeout(() => { document.body.innerHTML = "<h1 id='arrived'>Arrived late</h1>"; }, 1500);</script>
<div id="spinner">Loading…</div>"#;

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
    // `/slow/<page>` is `<page>`, half a second late: on a fast machine a
    // navigation's new document lands before the next command, on a slow
    // one (CI) it does not, and these make every machine a slow one.
    let path = match path.strip_prefix("/slow") {
        Some(rest) if rest.starts_with('/') => {
            std::thread::sleep(std::time::Duration::from_millis(500));
            rest
        }
        _ => path,
    };
    // Redirects to Mirador's own pages, which a pane must not follow.
    let location = match path {
        "/to-app" => Some("tauri://localhost/"),
        "/to-dev" => Some(DEV_FRONTEND),
        _ => None,
    };
    if let Some(location) = location {
        let _ = write!(stream, "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    if let Some(body) = implicit_form(path) {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        return;
    }
    let (status, body) = match path {
        "/" => ("200 OK", FORM),
        "/page2" => ("200 OK", PAGE2),
        "/submit" => ("200 OK", SUBMIT),
        "/errors" => ("200 OK", ERRORS),
        "/spam" => ("200 OK", SPAM),
        "/keys" => ("200 OK", KEYS),
        "/late" => ("200 OK", LATE),
        _ => ("404 Not Found", "not found"),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Where the debug app under test loads its frontend from (`devUrl`).
const DEV_FRONTEND: &str = "http://localhost:1420/";

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
    // Nothing to say on success: not the pane id, which read as a result.
    assert_eq!(browser(&sb, &first, &["fill", "#name", "Grace"]), "");
    assert_eq!(browser(&sb, &first, &["click", "#go"]), "");
    let out = browser(&sb, &first, &["eval", "document.getElementById('out').textContent"]);
    assert_eq!(out.trim(), "\"hello Grace red\"");
    let (code, _, err) = sb.mira(Some(&first), &["browser", "click", "#missing"]);
    assert_ne!(code, 0);
    assert!(err.contains("element not found: #missing"), "{err}");
}

#[test]
fn fill_a_select_by_value_or_text() {
    e2e!();
    let (sb, first) = launched("bselect");
    open(&sb, &first, &page("/"));
    let out = |sb: &Sandbox| browser(sb, &first, &["eval", "document.getElementById('out').textContent"]);
    // The snapshot shows the choices, and which is chosen.
    let snap = browser(&sb, &first, &["snapshot"]);
    for line in [
        "<select> \"Red\" type=select-one value=\"red\"",
        "      option \"Red\" value=\"red\" selected\n",
        "      option \"Blue\" value=\"blue\"\n",
        "      option \"Green\" value=\"g\"\n",
    ] {
        assert!(snap.contains(line), "no {line:?} in:\n{snap}");
    }
    // By value, and the page hears the change.
    assert_eq!(browser(&sb, &first, &["fill", "#color", "blue"]), "");
    assert_eq!(out(&sb).trim(), "\"picked blue\"");
    // By the text a person sees.
    browser(&sb, &first, &["fill", "#color", "Green"]);
    assert_eq!(out(&sb).trim(), "\"picked g\"");
    assert!(browser(&sb, &first, &["snapshot"]).contains("option \"Green\" value=\"g\" selected"));
    // No such option: the error lists them.
    let (code, _, err) = sb.mira(Some(&first), &["browser", "fill", "#color", "purple"]);
    assert_ne!(code, 0);
    assert!(err.contains("no option \"purple\"") && err.contains("\"g\" (Green)"), "{err}");
    // A checkbox is clicked, and the error says so.
    let (code, _, err) = sb.mira(Some(&first), &["browser", "fill", "#agree", "yes"]);
    assert_ne!(code, 0);
    assert!(err.contains("mira browser click"), "{err}");
    browser(&sb, &first, &["click", "#agree"]);
    assert_eq!(browser(&sb, &first, &["eval", "document.getElementById('agree').checked"]).trim(), "true");
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
    // No value is `null`, never the pane id.
    assert_eq!(browser(&sb, &first, &["eval", "undefined"]).trim(), "null");
    assert_eq!(browser(&sb, &first, &["eval", "window.noSuchThing"]).trim(), "null");
}

#[test]
fn eval_awaits_a_promise() {
    e2e!();
    let (sb, first) = launched("bpromise");
    open(&sb, &first, &page("/"));
    let out = browser(&sb, &first, &["eval", "new Promise(r => setTimeout(() => r(42), 300))"]);
    assert_eq!(out.trim(), "42");
    let out = browser(&sb, &first, &["eval", "fetch('/page2').then(r => r.text()).then(t => t.length > 0)"]);
    assert_eq!(out.trim(), "true");
    let (code, _, err) = sb.mira(Some(&first), &["browser", "eval", "Promise.reject(new Error('nah'))"]);
    assert_ne!(code, 0);
    assert!(err.contains("nah"), "a rejection is the command's error: {err}");
}

/// From a page in a pane, every command of the app that matters, through
/// the IPC entry point Tauri's plugins inject: a read, writes to the
/// sandbox's own panes, and a plugin's. Each answer is "refused: …" or
/// "ALLOWED".
fn try_the_apps_commands(sb: &Sandbox, from: &str, pane: &str, victim: &str) -> Vec<String> {
    let js = format!(
        r#"Promise.all([
            ["workspace_snapshot", {{}}],
            ["write_pty", {{ paneId: "{victim}", data: "echo pwned-$((6*7))\r" }}],
            ["open_browser", {{ paneId: null, tab: true, url: "about:blank" }}],
            ["plugin:notification|notify", {{ options: {{ title: "pwned" }} }}],
            ["close_pane", {{ paneId: "{victim}" }}],
        ].map(([cmd, args]) => window.__TAURI_INTERNALS__.invoke(cmd, args).then(
            () => cmd + " ALLOWED",
            (e) => cmd + " refused: " + String(e).slice(0, 80),
        )))"#
    );
    serde_json::from_str(&browser(sb, from, &["eval", "--pane", pane, &js])).unwrap()
}

/// Every one refused by Tauri's ACL — not failing for some other reason,
/// like a wrong argument, which would pass for a refusal — and nothing
/// done in the sandbox: no new pane, no pane closed, nothing typed.
fn assert_all_refused(sb: &Sandbox, answers: &[String], victim: &str, panes: &[String]) {
    assert_eq!(answers.len(), 5, "{answers:?}");
    for answer in answers {
        assert!(answer.contains(" refused: ") && answer.contains("not allowed"), "{answers:?}");
    }
    assert_eq!(sb.panes(), panes, "no pane opened or closed");
    assert!(!sb.screen(victim).unwrap_or_default().contains("pwned-42"), "nothing typed");
}

/// The boundary in CLAUDE.md, *Security boundaries*: a page in a browser
/// pane gets none of the app's commands.
#[test]
fn a_page_in_a_pane_gets_no_command_of_the_app() {
    e2e!();
    let (sb, first) = launched("bipc");
    let pane = open(&sb, &first, &page("/"));
    let panes = sb.panes();
    // The entry point is there — Tauri's plugins put it in every webview.
    assert_eq!(browser(&sb, &first, &["eval", "typeof window.__TAURI_INTERNALS__"]).trim(), "\"object\"");
    let answers = try_the_apps_commands(&sb, &first, &pane, &first);
    assert_all_refused(&sb, &answers, &first, &panes);
}

/// Nor can a pane be made one of the app's own pages, which Tauri trusts
/// as local: not by `mira`, not by the page itself, not by a redirect.
#[test]
fn a_pane_never_shows_the_apps_own_pages() {
    e2e!();
    let (sb, first) = launched("bipcnav");
    for url in ["tauri://localhost/", DEV_FRONTEND, "http://tauri.localhost/"] {
        let (code, _, err) = sb.mira(Some(&first), &["browser", "open", "--tab", url]);
        assert_ne!(code, 0, "open {url}");
        assert!(err.contains("never opens Mirador's own pages") || err.contains("opens http(s) pages"), "{err}");
    }
    let pane = open(&sb, &first, &page("/"));
    let panes = sb.panes();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "navigate", "tauri://localhost/"]);
    assert_ne!(code, 0, "{err}");

    let stays = |how: &str, leaves_for: &str| {
        // Refused, the navigation never lands: the page waited for never
        // comes, and the form is still there to answer.
        let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "--url", leaves_for, "--timeout", "2"]);
        assert_ne!(code, 0, "{how} reached {leaves_for}: {err}");
        let at = browser(&sb, &first, &["eval", "location.href"]);
        assert_eq!(at.trim(), format!("{:?}", page("/")), "{how} left the page");
        let answers = try_the_apps_commands(&sb, &first, &pane, &first);
        assert_all_refused(&sb, &answers, &first, &panes);
    };
    for target in ["tauri://localhost/", DEV_FRONTEND] {
        browser(&sb, &first, &["eval", &format!("location.href = {target:?}; 0")]);
        stays(&format!("location.href = {target}"), target);
    }
    for (path, target) in [("/to-app", "tauri://"), ("/to-dev", DEV_FRONTEND)] {
        browser(&sb, &first, &["eval", &format!("location.href = {path:?}; 0")]);
        stays(&format!("a redirect from {path}"), target);
    }
}

/// Typing and keys, the way a person uses a form: the page's key handlers
/// see each key.
#[test]
fn type_and_press_keys_like_a_person() {
    e2e!();
    let (sb, first) = launched("bkeys");
    open(&sb, &first, &page("/keys"));
    let text = |sb: &Sandbox, id: &str| browser(sb, &first, &["eval", &format!("document.getElementById('{id}').textContent")]);
    let focused = |sb: &Sandbox| browser(sb, &first, &["eval", "document.activeElement.id"]);

    // keyup mirrors the field into the page: it saw every key.
    assert_eq!(browser(&sb, &first, &["type", "#q", "hello"]), "");
    assert_eq!(text(&sb, "mirror").trim(), "\"hello\"");
    browser(&sb, &first, &["type", "#q", " world"]);
    assert_eq!(text(&sb, "mirror").trim(), "\"hello world\"", "type appends");

    assert_eq!(browser(&sb, &first, &["press", "Backspace"]), "");
    assert_eq!(text(&sb, "mirror").trim(), "\"hello worl\"");
    browser(&sb, &first, &["press", "Meta+A", "--target", "#q"]);
    browser(&sb, &first, &["press", "Backspace"]);
    assert_eq!(text(&sb, "mirror").trim(), "\"\"", "select all, then delete it");

    browser(&sb, &first, &["press", "Tab"]);
    assert_eq!(focused(&sb).trim(), "\"next\"");
    browser(&sb, &first, &["press", "Shift+Tab"]);
    assert_eq!(focused(&sb).trim(), "\"q\"");
    // No wrapping: past the last control, focus is the document's.
    browser(&sb, &first, &["press", "Tab", "--target", "#go"]);
    assert_eq!(browser(&sb, &first, &["eval", "document.activeElement === document.body"]).trim(), "true");
    browser(&sb, &first, &["press", "Shift+Tab", "--target", "#q"]);
    assert_eq!(browser(&sb, &first, &["eval", "document.activeElement === document.body"]).trim(), "true");

    browser(&sb, &first, &["press", "Escape"]);
    assert_eq!(text(&sb, "esc").trim(), "\"escaped\"");

    let (code, _, err) = sb.mira(Some(&first), &["browser", "press", "Hyper"]);
    assert_ne!(code, 0);
    assert!(err.contains("unknown key \"Hyper\"") && err.contains("Enter, Tab, Escape"), "{err}");

    // A field `type` or `press` focuses gets the cursor at its end, as a
    // click after its text would — even a pre-filled one never focused,
    // which WebKit's own focus() would select whole.
    let pre = |sb: &Sandbox| browser(sb, &first, &["eval", "document.getElementById('pre').value"]).trim().to_string();
    browser(&sb, &first, &["eval", "document.body.insertAdjacentHTML('beforeend', '<input id=pre value=pre>'); 0"]);
    browser(&sb, &first, &["type", "#pre", "fix"]);
    assert_eq!(pre(&sb), "\"prefix\"");
    // A focused field keeps its cursor: put at the start, `type` inserts there.
    browser(&sb, &first, &["eval", "document.getElementById('pre').setSelectionRange(0, 0); 0"]);
    browser(&sb, &first, &["type", "#pre", ">"]);
    assert_eq!(pre(&sb), "\">prefix\"");
    // An editable element too: never focused, its text gets the cursor at
    // its end.
    browser(&sb, &first, &["eval", "document.body.insertAdjacentHTML('beforeend', '<div id=ed contenteditable>note</div>'); 0"]);
    browser(&sb, &first, &["type", "#ed", "book"]);
    assert_eq!(browser(&sb, &first, &["eval", "document.getElementById('ed').textContent"]).trim(), "\"notebook\"");
    // And `press --target` on a field not focused edits at its end.
    browser(&sb, &first, &["press", "Backspace", "--target", "#q"]);
    browser(&sb, &first, &["press", "Backspace", "--target", "#pre"]);
    assert_eq!(pre(&sb), "\">prefi\"");

    // Enter in a form's field submits it, as a browser does.
    browser(&sb, &first, &["type", "#q", "sent"]);
    browser(&sb, &first, &["press", "Enter", "--target", "#q"]);
    browser(&sb, &first, &["wait", "--url", "/page2?q=sent"]);
}

/// Enter in a field submits a form only as a browser's would: through
/// its default button, or — without one — when it has a single text
/// field.
#[test]
fn enter_submits_a_form_as_a_browser_would() {
    e2e!();
    let (sb, first) = launched("bimplicit");
    let pane = open(&sb, &first, &page("/form-button"));
    browser(&sb, &first, &["press", "Enter", "--target", "#f"]);
    browser(&sb, &first, &["wait", "--url", "/page2?q=b&clicked=yes&via=button"]);

    // Its handler ran, and it was the submitter: an image sends where it
    // was clicked.
    browser(&sb, &first, &["navigate", "/form-image"]);
    browser(&sb, &first, &["wait", "#f"]);
    browser(&sb, &first, &["press", "Enter", "--target", "#f"]);
    browser(&sb, &first, &["wait", "--url", "/page2?q=i&clicked=yes&img.x=0&img.y=0"]);

    browser(&sb, &first, &["navigate", "/form-one"]);
    browser(&sb, &first, &["wait", "#f"]);
    browser(&sb, &first, &["press", "Enter", "--target", "#f"]);
    browser(&sb, &first, &["wait", "--url", "/page2?q=one"]);

    for path in ["/form-two", "/form-disabled"] {
        browser(&sb, &first, &["navigate", path]);
        browser(&sb, &first, &["wait", "--url", path]);
        browser(&sb, &first, &["press", "Enter", "--target", "#f"]);
        let (code, _, _) = sb.mira(Some(&first), &["browser", "wait", "--url", "/page2", "--timeout", "2"]);
        assert_ne!(code, 0, "{path} submitted");
        assert_eq!(reported_url(&sb, &pane), Some(page(path)), "{path} left the page");
        assert_eq!(browser(&sb, &first, &["eval", "window.submitted === true"]).trim(), "false", "{path}: a submit event");
    }
}

/// What a person reads on the page, which a snapshot leaves out.
#[test]
fn text_gives_what_the_page_says() {
    e2e!();
    let (sb, first) = launched("btext");
    open(&sb, &first, &page("/"));
    assert_eq!(browser(&sb, &first, &["text", "h1"]).trim(), "Form");
    let page_text = browser(&sb, &first, &["text"]);
    assert!(page_text.contains("Form") && page_text.contains("Page two"), "{page_text}");
    browser(&sb, &first, &["click", "#go"]);
    // As laid out: the empty name collapses into one space.
    assert_eq!(browser(&sb, &first, &["text", "#out"]).trim(), "hello red");

    let (code, _, err) = sb.mira(Some(&first), &["browser", "text", "#missing"]);
    assert_ne!(code, 0);
    assert!(err.contains("nothing matches #missing"), "{err}");

    // A long page stops, and says how much more there was.
    browser(&sb, &first, &["eval", "document.body.innerText = 'x'.repeat(250000); 0"]);
    let long = browser(&sb, &first, &["text"]);
    assert!(long.trim_end().ends_with("… 150000 more characters"), "{}", &long[long.len().saturating_sub(80)..]);
    assert!(long.len() < 110_000, "{}", long.len());
}

/// What a page logs and throws — while it loads, too — with its level and
/// where it came from.
#[test]
fn console_shows_what_the_page_logged_and_threw() {
    e2e!();
    let (sb, first) = launched("bconsole");
    open(&sb, &first, &page("/errors"));
    // The failed image and the rejection come in on their own time.
    let out = browser_until(&sb, &first, &["console"], "every entry", |s| {
        s.contains("Failed to load <img>") && s.contains("Unhandled rejection")
    });
    for (level, at, message) in [
        ("log  ", "errors:4:", "fixture-log {\"a\":1}"),
        ("warn ", "errors:5:", "fixture-warn"),
        ("error", "errors:6:", "fixture-error"),
        ("error", "errors:8:", "Uncaught Error: fixture-boom"),
    ] {
        assert!(
            out.lines().any(|l| l.starts_with(&format!("{level}  {at}")) && l.ends_with(message)),
            "no {level} at {at} saying {message:?} in:\n{out}"
        );
    }
    assert!(out.contains(&format!("error  missing.png  Failed to load <img> {}", page("/missing.png"))), "{out}");
    assert!(out.contains("error  Unhandled rejection: Error: fixture-rejected"), "{out}");

    let errors = browser(&sb, &first, &["console", "--errors"]);
    assert_eq!(errors.lines().count(), 4, "{errors}");
    assert!(errors.lines().all(|l| l.starts_with("error")), "{errors}");
    // Of the ACL's refusals, only the notification plugin's own check on
    // load is left out: one the page provokes is the page's, and shows.
    browser(&sb, &first, &[
        "eval",
        "window.__TAURI_INTERNALS__.invoke('plugin:notification|notify', { options: { title: 'x' } }); 0",
    ]);
    browser_until(&sb, &first, &["console", "--errors"], "the page's own refused call", |s| {
        s.contains("Unhandled rejection: notification.notify not allowed on window")
    });
    // Even the very call the plugin makes on load: the page's own, later,
    // shows — only the plugin's, once, is left out.
    browser(&sb, &first, &["eval", "window.__TAURI_INTERNALS__.invoke('plugin:notification|is_permission_granted'); 0"]);
    let errors = browser_until(&sb, &first, &["console", "--errors"], "the page's own permission check", |s| {
        s.contains("Unhandled rejection: notification.is_permission_granted not allowed on window")
    });
    assert_eq!(errors.matches("is_permission_granted").count(), 1, "the plugin's own still left out:\n{errors}");

    let raw: serde_json::Value = serde_json::from_str(&browser(&sb, &first, &["--json", "console"])).unwrap();
    assert_eq!(raw["console"][0]["level"], "log", "{raw}");
    assert!(raw["console"][0]["time"].is_string(), "{raw}");

    // The page's own console still works, and returns what it did.
    assert_eq!(browser(&sb, &first, &["eval", "console.log('from-eval')"]).trim(), "null");
    assert!(browser(&sb, &first, &["console"]).contains("from-eval"));

    // --clear prints, then empties.
    assert!(browser(&sb, &first, &["console", "--clear"]).contains("fixture-boom"));
    // Empty, it says so, rather than print nothing.
    assert_eq!(browser(&sb, &first, &["console"]), "(no console output)\n");
    assert_eq!(browser(&sb, &first, &["console", "--errors"]), "(no errors)\n");
    let raw: serde_json::Value = serde_json::from_str(&browser(&sb, &first, &["--json", "console"])).unwrap();
    assert_eq!(raw["console"], serde_json::json!([]), "--json unchanged: {raw}");
}

#[test]
fn console_keeps_the_last_500_entries() {
    e2e!();
    let (sb, first) = launched("bspam");
    open(&sb, &first, &page("/spam"));
    let out = browser(&sb, &first, &["console"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.first(), Some(&"(4500 earlier entries dropped)"));
    assert_eq!(lines.len(), 501);
    assert!(lines[1].ends_with("line 4500") && lines[500].ends_with("line 4999"), "{out}");
}

/// `wait` instead of a sleep: for an element, for text, for a spinner to
/// go, for a URL — each on a page that gets there only after a while.
#[test]
fn wait_until_the_page_is_ready() {
    e2e!();
    let (sb, first) = launched("bwait");
    open(&sb, &first, &page("/"));

    browser(&sb, &first, &["navigate", "/late"]);
    // Met, it says what it waited for; `--json` stays `null`.
    assert_eq!(browser(&sb, &first, &["wait", "#arrived"]), "#arrived is shown\n");
    assert_eq!(browser(&sb, &first, &["--json", "wait", "#arrived"]).trim(), "null");
    assert!(browser(&sb, &first, &["snapshot"]).contains("<h1> \"Arrived late\""));

    browser(&sb, &first, &["reload"]);
    assert_eq!(browser(&sb, &first, &["wait", "--text", "Arrived late"]), "text \"Arrived late\" found\n");
    assert!(browser(&sb, &first, &["snapshot"]).contains("<h1> \"Arrived late\""));

    // Seen first, so the wait for it to go is on this page and not met
    // by the last one.
    browser(&sb, &first, &["reload"]);
    browser(&sb, &first, &["wait", "#spinner"]);
    assert_eq!(browser(&sb, &first, &["wait", "--gone", "#spinner"]), "#spinner is gone\n");
    assert_eq!(browser(&sb, &first, &["eval", "document.querySelector('#spinner')"]).trim(), "null");

    // Across a navigation the page starts by itself, a second from now.
    browser(&sb, &first, &["navigate", "/"]);
    browser(&sb, &first, &["wait", "--url", &page("/")]);
    browser(&sb, &first, &["eval", "setTimeout(() => document.querySelector('a').click(), 1000); 0"]);
    assert_eq!(browser(&sb, &first, &["wait", "--url", "/page2"]), "url contains /page2\n");
    assert!(browser(&sb, &first, &["snapshot"]).starts_with("Page two — "));
}

#[test]
fn wait_times_out_naming_what_it_waited_for() {
    e2e!();
    let (sb, first) = launched("bwaitout");
    open(&sb, &first, &page("/"));
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "#never", "--timeout", "1"]);
    assert_ne!(code, 0);
    assert!(err.contains("timed out after 1s waiting for #never to be shown"), "{err}");
    assert!(err.contains("the page shows: \"Form"), "a glimpse of the page: {err}");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());

    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "--text", "Never there", "--timeout", "1"]);
    assert_ne!(code, 0);
    assert!(err.contains("waiting for the text \"Never there\""), "{err}");
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "--gone", "h1", "--timeout", "1"]);
    assert_ne!(code, 0);
    assert!(err.contains("h1 to be gone; still shown: \"Form\""), "{err}");
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "--url", "/nowhere", "--timeout", "1"]);
    assert_ne!(code, 0);
    assert!(err.contains(&format!("a URL containing \"/nowhere\"; the page is at {}", page("/"))), "{err}");

    // A selector that cannot match anything fails at once, not on timeout.
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "[[", "--timeout", "10"]);
    assert_ne!(code, 0);
    assert!(started.elapsed() < Duration::from_secs(5), "{err}");
    // And one thing at a time.
    let (code, _, _) = sb.mira(Some(&first), &["browser", "wait", "h1", "--text", "Form"]);
    assert_ne!(code, 0);
}

/// `--timeout` holds while the page is still loading — one that never
/// will — and not only once it has.
#[test]
fn wait_keeps_its_timeout_before_the_page_has_loaded() {
    e2e!();
    let (sb, first) = launched("bwaitload");
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{closed}/");
    // `open` waits up to 10 s for that page; ask while it is still waiting.
    let mut opening = sb.mira_command(Some(&first), &["browser", "open", "--tab", &url])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pane = sb.wait_for("the pane to open", LONG, || {
        sb.snapshot()["browserPanes"]
            .as_array()?
            .iter()
            .find(|b| b["url"] == url.as_str())
            .and_then(|b| b["paneId"].as_str().map(String::from))
    });
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "h1", "--timeout", "1", "--pane", &pane]);
    let took = started.elapsed();
    assert_ne!(code, 0);
    assert!(took < Duration::from_secs(3), "--timeout 1 took {took:?}: {err}");
    assert!(err.contains("timed out after 1s waiting for h1 to be shown") && err.contains("has not loaded yet"), "{err}");
    opening.wait().unwrap();
}

/// And before the pane is even drawn: a pane under 2 px wide never is,
/// so its page never gets a webview.
#[test]
fn wait_keeps_its_timeout_before_the_pane_is_drawn() {
    e2e!();
    let (sb, first) = launched("bundrawn");
    // Each split halves the last pane: twelve leave it a fraction of a pixel.
    let mut tiny = first.clone();
    for _ in 0..12 {
        tiny = sb.call(cmux_protocol::Request::SplitPane {
            pane_id: Some(tiny.clone()),
            dir: cmux_protocol::SplitDir::Row,
            command: None,
        })["paneId"]
            .as_str()
            .unwrap()
            .to_string();
    }
    sb.call(cmux_protocol::Request::FocusPane { pane_id: tiny.clone() });
    let (code, _, err) = sb.mira(Some(&tiny), &["browser", "open", &page("/")]);
    assert_ne!(code, 0, "no webview, so `open` says so: {err}");
    let pane = sb.snapshot()["browserPanes"][0]["paneId"].as_str().unwrap().to_string();

    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "wait", "h1", "--timeout", "1", "--pane", &pane]);
    let took = started.elapsed();
    assert_ne!(code, 0);
    assert!(took < Duration::from_secs(3), "--timeout 1 took {took:?}: {err}");
    assert!(err.contains("timed out after 1s waiting for h1 to be shown") && err.contains("has no page yet"), "{err}");
}

/// An answer has a size limit, and it holds even against a page that
/// rewrites the bridge — as it holds the time limit against one that
/// never answers.
#[test]
fn a_too_large_or_endless_answer_is_refused() {
    e2e!();
    let (sb, first) = launched("blarge");
    open(&sb, &first, &page("/"));
    let (code, _, err) = sb.mira(Some(&first), &["browser", "eval", "'x'.repeat(2000000)"]);
    assert_ne!(code, 0);
    assert!(err.contains("result too large") && err.contains("max 1000000"), "{err}");
    // Just under the limit is fine.
    let out = browser(&sb, &first, &["eval", "'x'.repeat(900000).length"]);
    assert_eq!(out.trim(), "900000");

    // A hostile page: its own __miraRun, answering 2 MB.
    browser(&sb, &first, &["eval", "window.__miraRun = () => 'x'.repeat(2000000); 0"]);
    let (code, _, err) = sb.mira(Some(&first), &["browser", "snapshot"]);
    assert_ne!(code, 0);
    assert!(err.contains("result too large") && err.contains("max 1000000"), "{err}");

    // And one that says "pending" forever still meets the timeout.
    browser(&sb, &first, &["reload"]);
    browser_until(&sb, &first, &["eval", "typeof window.__miraTake"], "a fresh bridge", |s| s.trim() == "\"function\"");
    browser(&sb, &first, &[
        "eval",
        "window.__miraRun = window.__miraTake = () => JSON.stringify({ pending: true }); 0",
    ]);
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "snapshot"]);
    assert_ne!(code, 0);
    assert!(err.contains("timed out"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
}

#[test]
fn click_a_link_or_a_submit_button_navigates() {
    e2e!();
    let (sb, first) = launched("bclicknav");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["click", "a"]);
    browser_until(&sb, &first, &["snapshot"], "page two after the link", |s| s.starts_with("Page two — "));
    browser(&sb, &first, &["navigate", "/submit"]);
    browser_until(&sb, &first, &["snapshot"], "the form to submit", |s| s.starts_with("Submit — "));
    browser(&sb, &first, &["click", "button"]);
    browser_until(&sb, &first, &["snapshot"], "page two after the submit", |s| {
        s.starts_with(&format!("Page two — {}", page("/page2?q=sent")))
    });
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
    browser(&sb, &first, &["click", "a"]);
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

/// `open` returns once its page is there: the next command needs no wait.
/// Ten in a row, since a race shows only sometimes.
#[test]
fn snapshot_right_after_open() {
    e2e!();
    let (sb, first) = launched("bopenrace");
    let url = page("/");
    for i in 0..10 {
        let tab = if i % 2 == 0 { None } else { Some("--tab") };
        let mut args = vec!["open"];
        args.extend(tab);
        args.push(&url);
        let pane = browser(&sb, &first, &args).trim().to_string();
        let snap = browser(&sb, &first, &["snapshot", "--pane", &pane]);
        assert!(snap.starts_with(&format!("Form — {url}")), "open #{i} ({args:?}):\n{snap}");
        // Splits halve the pane each time: close it before the next.
        let (code, _, err) = sb.mira(None, &["close-pane", &pane]);
        assert_eq!(code, 0, "{err}");
    }
}

/// `navigate`, `reload`, `back` and `forward` answer once the page they
/// went to has loaded — even when it is slow to come — so the next
/// command finds it, not the page before.
#[test]
fn history_and_navigation_wait_for_the_new_page() {
    e2e!();
    let (sb, first) = launched("bslow");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", "/slow/page2"]);
    let snap = browser(&sb, &first, &["snapshot"]);
    assert!(snap.starts_with("Page two — "), "navigate: the new page, not the old one:\n{snap}");

    // A reload's old document must not answer for the new one.
    browser(&sb, &first, &["navigate", "/slow/late"]);
    browser(&sb, &first, &["wait", "#arrived"]);
    browser(&sb, &first, &["reload"]);
    let (code, out, err) = sb.mira(Some(&first), &["browser", "eval", "document.getElementById('spinner') !== null"]);
    assert_eq!((code, out.trim()), (0, "true"), "reload: the fresh document, spinner and all: {err}");
    browser(&sb, &first, &["wait", "--text", "Arrived late"]);
    assert!(browser(&sb, &first, &["snapshot"]).contains("<h1> \"Arrived late\""));

    browser(&sb, &first, &["back"]);
    let snap = browser(&sb, &first, &["snapshot"]);
    assert!(snap.starts_with("Page two — "), "back: the page before:\n{snap}");
    browser(&sb, &first, &["forward"]);
    let (_, out, _) = sb.mira(Some(&first), &["browser", "eval", "location.pathname"]);
    assert_eq!(out.trim(), "\"/slow/late\"", "forward");

    // What loads no new page answers at once: a #fragment, and back with
    // nowhere to go.
    browser(&sb, &first, &["navigate", "/"]);
    let started = std::time::Instant::now();
    browser(&sb, &first, &["navigate", "#section"]);
    assert!(started.elapsed() < Duration::from_secs(3), "a fragment: {:?}", started.elapsed());
    let fresh = browser(&sb, &first, &["open", "--tab", &page("/")]).trim().to_string();
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "back", "--pane", &fresh]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "no note for nowhere to go: {err}");
    assert!(started.elapsed() < Duration::from_secs(5), "back with no history: {:?}", started.elapsed());
}

/// A navigation sent while the pane's webview is still being created —
/// its own first page loading slowly — answers for the page it was sent
/// to, not the first one.
#[test]
fn navigate_while_the_pane_is_being_created_lands_where_it_was_sent() {
    e2e!();
    let (sb, first) = launched("bnavearly");
    let url = page("/slow/late");
    let mut opening = sb
        .mira_command(Some(&first), &["browser", "open", "--tab", &url])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pane = sb.wait_for("the pane to be listed", LONG, || {
        sb.snapshot()["browserPanes"]
            .as_array()?
            .iter()
            .find(|b| b["url"] == url.as_str())
            .and_then(|b| b["paneId"].as_str().map(String::from))
    });
    browser(&sb, &first, &["navigate", "--pane", &pane, &page("/slow/page2")]);
    let snap = browser(&sb, &first, &["snapshot", "--pane", &pane]);
    assert!(snap.starts_with("Page two — "), "the page it was sent to:\n{snap}");
    opening.wait().unwrap();
}

/// Without `--pane`, a command goes to the active tab's browser pane;
/// with none there, to the one opened or driven last — never an arbitrary
/// one.
#[test]
fn a_command_without_pane_picks_a_browser_pane_predictably() {
    e2e!();
    let (sb, first) = launched("bchoice");
    let split = open(&sb, &first, &page("/"));
    let tab = browser(&sb, &first, &["open", "--tab", &page("/page2")]).trim().to_string();
    let title = |sb: &Sandbox| browser(sb, &first, &["eval", "document.title"]).trim().to_string();
    assert_eq!(title(&sb), "\"Page two\"", "the active tab's");
    sb.call(cmux_protocol::Request::FocusPane { pane_id: first.clone() });
    assert_eq!(title(&sb), "\"Form\"", "the active tab's, after switching back");
    // A tab with no browser pane: the one driven last.
    sb.call(cmux_protocol::Request::NewTab { command: None });
    for _ in 0..3 {
        assert_eq!(title(&sb), "\"Form\"", "still the one driven last");
    }
    browser(&sb, &first, &["snapshot", "--pane", &tab]);
    assert_eq!(title(&sb), "\"Page two\"", "now this one, driven by --pane");
    browser(&sb, &first, &["snapshot", "--pane", &split]);
    assert_eq!(title(&sb), "\"Form\"");
}

/// A pane whose page never loaded says so, at once — not that the page
/// is busy, after a timeout.
#[test]
fn a_page_that_never_loaded_says_so() {
    e2e!();
    let (sb, first) = launched("bnoload");
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let pane = browser(&sb, &first, &["open", "--tab", &format!("http://127.0.0.1:{closed}/")]).trim().to_string();
    let started = std::time::Instant::now();
    let (code, _, err) = sb.mira(Some(&first), &["browser", "snapshot", "--pane", &pane]);
    assert_ne!(code, 0);
    assert!(err.contains("never loaded") && err.contains(&closed.to_string()), "{err}");
    assert!(started.elapsed() < Duration::from_secs(3), "at once, not after a wait: {:?}", started.elapsed());
    // And navigating it somewhere that answers brings it back.
    browser(&sb, &first, &["navigate", "--pane", &pane, &page("/page2")]);
    browser_until(&sb, &first, &["snapshot", "--pane", &pane], "page two", |s| s.starts_with("Page two — "));
}

/// `navigate` returns once the new page has loaded, so the next command
/// reaches it. On a fast machine this held even when `navigate` returned
/// at once (twenty in a row); CI's slower runner showed it does not, and
/// `history_and_navigation_wait_for_the_new_page` makes every machine slow.
#[test]
fn snapshot_right_after_navigate() {
    e2e!();
    let (sb, first) = launched("bnavrace");
    open(&sb, &first, &page("/"));
    browser(&sb, &first, &["navigate", &page("/page2")]);
    let snap = browser(&sb, &first, &["snapshot"]);
    assert!(snap.starts_with("Page two — "), "the new page, not the old one:\n{snap}");
}
