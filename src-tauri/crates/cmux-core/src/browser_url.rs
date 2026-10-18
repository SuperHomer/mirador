//! What a browser pane loads for what someone typed: `mira browser open`,
//! `navigate`, or the pane's URL bar. People type a dev server the way
//! they say it — `localhost:3000`, `/checkout` — not as a full URL.

/// The URL to load for `input`, given the URL the pane shows now.
///
/// A URL with a scheme loads as it is. A loopback host (`localhost`,
/// `127.0.0.1`, `[::1]`, `0.0.0.0`) gets `http://`: a dev server rarely
/// speaks TLS, and https to it fails. A path (`/page`), query (`?q`) or
/// fragment (`#f`) is taken against `current`, which must be an http(s)
/// page — there is no host to guess. Anything else gets `https://`.
pub fn resolve(input: &str, current: Option<&str>) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("no url given".into());
    }
    if input.contains("://") || input.starts_with("about:") {
        return Ok(input.to_string());
    }
    if input.starts_with(['/', '?', '#']) {
        return relative(input, current);
    }
    if is_loopback(host_of(input)) {
        Ok(format!("http://{input}"))
    } else {
        Ok(format!("https://{input}"))
    }
}

fn relative(input: &str, current: Option<&str>) -> Result<String, String> {
    let current = current
        .filter(|c| c.starts_with("http://") || c.starts_with("https://"))
        .ok_or_else(|| format!("{input} is relative, and the pane shows no web page to take it from"))?;
    let without_fragment = current.split('#').next().unwrap_or(current);
    let base = match input.as_bytes()[0] {
        b'#' => without_fragment,
        b'?' => without_fragment.split('?').next().unwrap_or(without_fragment),
        _ => {
            // The origin: everything before the first `/` after `://`.
            let after_scheme = current.find("://").map_or(0, |i| i + 3);
            let end = current[after_scheme..]
                .find(['/', '?', '#'])
                .map_or(current.len(), |i| after_scheme + i);
            &current[..end]
        }
    };
    Ok(format!("{base}{input}"))
}

/// The host of a scheme-less `host[:port][/path]`.
fn host_of(input: &str) -> &str {
    let authority = input.split(['/', '?', '#']).next().unwrap_or(input);
    if authority.starts_with('[') {
        // An IPv6 literal keeps its colons: `[::1]:3000`.
        return authority.split(']').next().map_or(authority, |h| &authority[..h.len() + 1]);
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_loopback(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1]"
        || host == "0.0.0.0"
        || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn a_url_with_a_scheme_loads_as_it_is() {
        for url in ["http://example.com/a", "https://localhost:3000", "file:///tmp/x.html", "about:blank"] {
            assert_eq!(resolve(url, None).unwrap(), url);
        }
    }

    #[test]
    fn a_loopback_host_is_http() {
        for (input, url) in [
            ("localhost", "http://localhost"),
            ("localhost:3000", "http://localhost:3000"),
            ("localhost:3000/page2?x=1", "http://localhost:3000/page2?x=1"),
            ("LOCALHOST:5173", "http://LOCALHOST:5173"),
            ("app.localhost:8080", "http://app.localhost:8080"),
            ("127.0.0.1:8000/a", "http://127.0.0.1:8000/a"),
            ("127.1.2.3", "http://127.1.2.3"),
            ("[::1]:3000/x", "http://[::1]:3000/x"),
            ("0.0.0.0:4173", "http://0.0.0.0:4173"),
        ] {
            assert_eq!(resolve(input, None).unwrap(), url, "{input}");
        }
    }

    #[test]
    fn any_other_host_is_https() {
        for (input, url) in [
            ("example.com", "https://example.com"),
            ("example.com/path", "https://example.com/path"),
            ("localhost.example.com", "https://localhost.example.com"),
            ("128.0.0.1:80", "https://128.0.0.1:80"),
        ] {
            assert_eq!(resolve(input, None).unwrap(), url, "{input}");
        }
    }

    #[test]
    fn a_path_is_taken_against_the_current_page() {
        let current = Some("http://127.0.0.1:3000/shop/cart?x=1#top");
        assert_eq!(resolve("/page2", current).unwrap(), "http://127.0.0.1:3000/page2");
        assert_eq!(resolve("?q=2", current).unwrap(), "http://127.0.0.1:3000/shop/cart?q=2");
        assert_eq!(resolve("#end", current).unwrap(), "http://127.0.0.1:3000/shop/cart?x=1#end");
        assert_eq!(resolve("/a", Some("https://example.com")).unwrap(), "https://example.com/a");
        assert_eq!(resolve("/a", Some("https://example.com?q")).unwrap(), "https://example.com/a");
    }

    #[test]
    fn a_path_with_no_web_page_to_take_it_from_is_an_error() {
        assert!(resolve("/page2", None).is_err());
        assert!(resolve("/page2", Some("about:blank")).is_err());
    }

    #[test]
    fn nothing_typed_is_an_error() {
        assert!(resolve("", None).is_err());
        assert!(resolve("  ", None).is_err());
    }
}
