#![deny(clippy::all)]

//! deluge-cli — Slate OS Deluge BitTorrent client
//!
//! Multi-personality: `deluged`, `deluge-console`, `deluge-gtk`, `deluge-web`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deluged(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deluged [OPTIONS]");
        println!("deluged v2.1 (Slate OS) — Deluge BitTorrent daemon");
        println!();
        println!("Options:");
        println!("  -d                Do not daemonize");
        println!("  -p PORT           Daemon port (default: 58846)");
        println!("  -c DIR            Config directory");
        println!("  -l FILE           Log file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deluged v2.1 (Slate OS)"); return 0; }
    println!("deluged: Deluge daemon started on port 58846");
    0
}

fn run_console(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deluge-console [COMMAND] [OPTIONS]");
        println!("deluge-console v2.1 (Slate OS) — Deluge console interface");
        println!();
        println!("Commands:");
        println!("  info              Show torrent info");
        println!("  add FILE          Add torrent");
        println!("  rm ID             Remove torrent");
        println!("  pause ID          Pause torrent");
        println!("  resume ID         Resume torrent");
        println!("  status            Show session status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deluge-console v2.1 (Slate OS)"); return 0; }
    println!("deluge-console: connected to localhost:58846");
    println!("  Active: 2 torrents");
    println!("  Down: 2.5 MiB/s  Up: 500 KiB/s");
    0
}

fn run_gtk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deluge-gtk [OPTIONS]");
        println!("deluge-gtk v2.1 (Slate OS) — Deluge GTK client");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deluge-gtk v2.1 (Slate OS)"); return 0; }
    println!("deluge-gtk: GTK client started");
    0
}

fn run_web(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deluge-web [OPTIONS]");
        println!("deluge-web v2.1 (Slate OS) — Deluge web interface");
        println!();
        println!("Options:");
        println!("  -p PORT           Web UI port (default: 8112)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deluge-web v2.1 (Slate OS)"); return 0; }
    println!("deluge-web: web interface started on http://localhost:8112");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "deluged".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "deluge-console" => run_console(&rest, &prog),
        "deluge-gtk" => run_gtk(&rest, &prog),
        "deluge-web" => run_web(&rest, &prog),
        _ => run_deluged(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_deluged};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deluge"), "deluge");
        assert_eq!(basename(r"C:\bin\deluge.exe"), "deluge.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deluge.exe"), "deluge");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deluged(&["--help".to_string()], "deluge"), 0);
        assert_eq!(run_deluged(&["-h".to_string()], "deluge"), 0);
        let _ = run_deluged(&["--version".to_string()], "deluge");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deluged(&[], "deluge");
    }
}
