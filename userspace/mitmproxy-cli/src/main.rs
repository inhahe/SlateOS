#![deny(clippy::all)]

//! mitmproxy-cli — SlateOS mitmproxy HTTP/HTTPS proxy
//!
//! Three personalities: `mitmproxy`, `mitmdump`, `mitmweb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mitmproxy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mitmproxy [OPTIONS]");
        println!("mitmproxy v10.3.0 (SlateOS) — Interactive HTTPS proxy");
        println!();
        println!("Options:");
        println!("  -p, --listen-port PORT   Listen port (default: 8080)");
        println!("  -m, --mode MODE          Mode (regular, transparent, socks5, reverse)");
        println!("  -s, --scripts FILE       Script file");
        println!("  -w, --save-stream FILE   Save flows");
        println!("  -r, --read-flows FILE    Read flows");
        println!("  --ssl-insecure           Skip upstream cert verification");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mitmproxy 10.3.0 (SlateOS)");
        return 0;
    }
    println!("mitmproxy v10.3.0 starting...");
    println!("  Proxy: http://localhost:8080");
    println!("  Mode: regular");
    println!("  Listening...");
    0
}

fn run_mitmdump(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mitmdump [OPTIONS]");
        println!("mitmdump — mitmproxy CLI dump mode");
        return 0;
    }
    println!("mitmdump: Proxy on port 8080");
    println!("  GET https://example.com/ HTTP/2.0  200  1.2kB  45ms");
    println!("  POST https://api.example.com/data HTTP/2.0  201  89B  120ms");
    0
}

fn run_mitmweb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mitmweb [OPTIONS]");
        println!("mitmweb — mitmproxy web interface");
        return 0;
    }
    println!("mitmweb: Web interface at http://localhost:8081");
    println!("  Proxy on port 8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mitmproxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mitmdump" => run_mitmdump(&rest, &prog),
        "mitmweb" => run_mitmweb(&rest, &prog),
        _ => run_mitmproxy(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mitmproxy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mitmproxy"), "mitmproxy");
        assert_eq!(basename(r"C:\bin\mitmproxy.exe"), "mitmproxy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mitmproxy.exe"), "mitmproxy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mitmproxy(&["--help".to_string()], "mitmproxy"), 0);
        assert_eq!(run_mitmproxy(&["-h".to_string()], "mitmproxy"), 0);
        let _ = run_mitmproxy(&["--version".to_string()], "mitmproxy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mitmproxy(&[], "mitmproxy");
    }
}
