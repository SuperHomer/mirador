//! `cmux` CLI. Only `notify` is implemented until the automation socket
//! lands in milestone M6 — it prints an OSC 777 escape sequence, so it
//! works from any shell inside cmux, including over SSH.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("notify") => notify(&args[1..]),
        Some("--help" | "-h" | "help") | None => {
            eprintln!("usage: cmux notify [--title <title>] <body>...");
            eprintln!();
            eprintln!("Sends a notification to the cmux pane this shell runs in.");
            eprintln!("(Full automation commands arrive with the cmux socket, M6.)");
            std::process::exit(if args.is_empty() { 1 } else { 0 });
        }
        Some(other) => {
            eprintln!("cmux: unknown command `{other}` (only `notify` exists until M6)");
            std::process::exit(1);
        }
    }
}

fn notify(args: &[String]) {
    let mut title = String::new();
    let mut body_parts: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--title" | "-t" if i + 1 < args.len() => {
                title = args[i + 1].clone();
                i += 2;
            }
            arg => {
                body_parts.push(arg);
                i += 1;
            }
        }
    }
    let body = body_parts.join(" ");
    if body.is_empty() && title.is_empty() {
        eprintln!("usage: cmux notify [--title <title>] <body>...");
        std::process::exit(1);
    }
    // Escape sequences can't contain the terminator; strip control chars.
    let clean = |s: &str| {
        s.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
    };
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]777;notify;{};{}\x1b\\", clean(&title), clean(&body));
    let _ = out.flush();
}
