//! What a browser pane may load: the web, never Mirador itself.
//!
//! Tauri treats a page from the app's own origin as local, and grants
//! local pages its commands. A browser pane is a webview in Mirador's
//! window, so a pane showing `tauri://localhost` — or the dev server a
//! debug build loads its frontend from — would hold every command the
//! app's own UI does. These are the origins it must never show.

/// Whether `url` is one of the app's own origins: its custom protocols
/// (`tauri:`, `asset:`, `ipc:`), the `http(s)://<protocol>.localhost` form
/// Windows serves them under, or `dev_origin`, the debug build's frontend.
/// `url` should be normalized first (lowercase host, no default port), as
/// `url::Url` prints it.
pub fn is_app_origin(url: &str, dev_origin: Option<&str>) -> bool {
    let Some((scheme, host, port)) = origin(url) else {
        // No `scheme://`: `about:`, `data:`, `blob:`, `javascript:` — none
        // is the app's.
        return false;
    };
    if matches!(scheme.as_str(), "tauri" | "asset" | "ipc") {
        return true;
    }
    if matches!(scheme.as_str(), "http" | "https")
        && matches!(host.trim_end_matches('.'), "tauri.localhost" | "asset.localhost" | "ipc.localhost")
    {
        return true;
    }
    dev_origin.and_then(origin).is_some_and(|dev| dev == (scheme, host, port))
}

/// Whether a pane may be sent to `url` — what `open`, `navigate` and the
/// URL bar ask for: an http(s) page that is not the app's, or a blank one.
pub fn may_open(url: &str, dev_origin: Option<&str>) -> Result<(), String> {
    if url == "about:blank" {
        return Ok(());
    }
    let scheme = url.split(':').next().unwrap_or_default().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("a browser pane opens http(s) pages, not {scheme}: ({url})"));
    }
    if is_app_origin(url, dev_origin) {
        return Err(format!("a browser pane never opens Mirador's own pages ({url})"));
    }
    Ok(())
}

/// Scheme, host and port of `scheme://[user@]host[:port]/…`, lowercase,
/// the port defaulted for http(s).
fn origin(url: &str) -> Option<(String, String, Option<u16>)> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // `user@host`: the host is after the last `@`.
    let authority = authority.rsplit('@').next().unwrap_or_default().to_ascii_lowercase();
    let (host, port) = match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => (authority[..i].to_string(), authority[i + 1..].parse().ok()),
        _ => (authority, None),
    };
    let port = port.or(match scheme.as_str() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    });
    Some((scheme, host, port))
}

#[cfg(test)]
mod tests {
    use super::{is_app_origin, may_open};

    const DEV: Option<&str> = Some("http://localhost:1420/");

    #[test]
    fn the_apps_own_protocols_are_its_origins() {
        for url in [
            "tauri://localhost/",
            "TAURI://localhost/index.html",
            "asset://localhost/x.png",
            "ipc://localhost/cmd",
            "http://tauri.localhost/",
            "https://tauri.localhost/x",
            "http://ipc.localhost/cmd",
            "http://asset.localhost/x",
            "http://tauri.localhost./",
            "http://user@tauri.localhost/",
        ] {
            assert!(is_app_origin(url, None), "{url}");
        }
    }

    #[test]
    fn the_dev_frontend_is_one_in_a_debug_build() {
        assert!(is_app_origin("http://localhost:1420/", DEV));
        assert!(is_app_origin("http://localhost:1420/some/route?x#y", DEV));
        assert!(is_app_origin("http://evil@localhost:1420/", DEV));
        // Another port, host or scheme is another origin.
        assert!(!is_app_origin("http://localhost:1421/", DEV));
        assert!(!is_app_origin("http://127.0.0.1:1420/", DEV));
        assert!(!is_app_origin("https://localhost:1420/", DEV));
        // And without a dev frontend, it is just a page.
        assert!(!is_app_origin("http://localhost:1420/", None));
    }

    #[test]
    fn the_web_is_not() {
        for url in [
            "https://example.com/",
            "http://localhost:3000/",
            "http://tauri.localhost.example.com/",
            "http://example.com/tauri://localhost",
            "about:blank",
            "about:srcdoc",
            "data:text/html,hi",
            "blob:https://example.com/1",
        ] {
            assert!(!is_app_origin(url, DEV), "{url}");
        }
    }

    #[test]
    fn a_pane_opens_web_pages_only() {
        assert!(may_open("https://example.com/", DEV).is_ok());
        assert!(may_open("http://localhost:3000/", DEV).is_ok());
        assert!(may_open("about:blank", DEV).is_ok());
        for url in [
            "tauri://localhost/",
            "http://tauri.localhost/",
            "http://localhost:1420/",
            "file:///etc/passwd",
            "data:text/html,hi",
            "javascript:alert(1)",
            "about:config",
        ] {
            assert!(may_open(url, DEV).is_err(), "{url}");
        }
    }
}
