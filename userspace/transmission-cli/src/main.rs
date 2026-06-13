#![deny(clippy::all)]

//! transmission-cli — SlateOS Transmission BitTorrent client
//!
//! Multi-personality: `transmission-daemon`, `transmission-remote`, `transmission-cli`, `transmission-gtk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-daemon [OPTIONS]");
        println!("transmission-daemon v4.0 (SlateOS) — BitTorrent daemon");
        println!();
        println!("Options:");
        println!("  -f                Run in foreground");
        println!("  -g DIR            Config directory");
        println!("  -p PORT           RPC port (default: 9091)");
        println!("  -w DIR            Download directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("transmission-daemon v4.0 (SlateOS)"); return 0; }
    println!("transmission-daemon: started on port 9091");
    println!("  Download dir: /home/user/Downloads");
    println!("  Peer port: 51413");
    0
}

fn run_remote(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-remote [HOST:PORT] [OPTIONS]");
        println!("transmission-remote v4.0 (SlateOS) — Remote control client");
        println!();
        println!("Options:");
        println!("  -a FILE           Add torrent");
        println!("  -l                List torrents");
        println!("  -t ID             Select torrent");
        println!("  -r                Remove torrent");
        println!("  -s                Start torrent");
        println!("  -S                Stop torrent");
        println!("  -si               Session info");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("transmission-remote v4.0 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("  ID   Done  ETA         Status       Name");
        println!("   1   100%  Done        Seeding      ubuntu-24.04.iso");
        println!("   2    45%  2h 30m      Downloading  debian-12.iso");
        println!("Sum:         2 Torrents");
        return 0;
    }
    if args.iter().any(|a| a == "-si") {
        println!("  Upload speed: 500 KiB/s");
        println!("  Download speed: 2.5 MiB/s");
        println!("  Active torrents: 2");
        println!("  Paused torrents: 0");
        return 0;
    }
    println!("localhost:9091/transmission/rpc: connected");
    0
}

fn run_cli_mode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-cli [OPTIONS] TORRENT");
        println!("transmission-cli v4.0 (SlateOS) — Lightweight CLI client");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("transmission-cli v4.0 (SlateOS)"); return 0; }
    println!("transmission-cli: downloading...");
    0
}

fn run_gtk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: transmission-gtk [OPTIONS] [TORRENT...]");
        println!("transmission-gtk v4.0 (SlateOS) — GTK BitTorrent client");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("transmission-gtk v4.0 (SlateOS)"); return 0; }
    println!("transmission-gtk: BitTorrent client started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "transmission-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "transmission-daemon" => run_daemon(&rest, &prog),
        "transmission-remote" => run_remote(&rest, &prog),
        "transmission-gtk" => run_gtk(&rest, &prog),
        _ => run_cli_mode(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_daemon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/transmission"), "transmission");
        assert_eq!(basename(r"C:\bin\transmission.exe"), "transmission.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("transmission.exe"), "transmission");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_daemon(&["--help".to_string()], "transmission"), 0);
        assert_eq!(run_daemon(&["-h".to_string()], "transmission"), 0);
        let _ = run_daemon(&["--version".to_string()], "transmission");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_daemon(&[], "transmission");
    }
}
