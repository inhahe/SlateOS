#![deny(clippy::all)]

//! rtorrent-cli — Slate OS rTorrent terminal BitTorrent client
//!
//! Single personality: `rtorrent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rtorrent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rtorrent [OPTIONS] [URL|FILE...]");
        println!("rtorrent v0.9.8 (Slate OS) — Terminal BitTorrent client");
        println!();
        println!("Options:");
        println!("  -d DIR            Download directory");
        println!("  -i ADDR           Bind to address");
        println!("  -p PORT-PORT      Port range");
        println!("  -s DIR            Session directory");
        println!("  -n                Don't load session");
        println!("  -o KEY=VAL        Set option");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rtorrent v0.9.8 (Slate OS)"); return 0; }
    println!("rtorrent: ncurses BitTorrent client started");
    println!("  Session: ~/.local/share/rtorrent/session");
    println!("  Download: ~/Downloads");
    println!("  Port range: 6881-6999");
    println!("  DHT: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rtorrent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rtorrent(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rtorrent};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rtorrent"), "rtorrent");
        assert_eq!(basename(r"C:\bin\rtorrent.exe"), "rtorrent.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rtorrent.exe"), "rtorrent");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rtorrent(&["--help".to_string()], "rtorrent"), 0);
        assert_eq!(run_rtorrent(&["-h".to_string()], "rtorrent"), 0);
        let _ = run_rtorrent(&["--version".to_string()], "rtorrent");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rtorrent(&[], "rtorrent");
    }
}
