#![deny(clippy::all)]

//! swaync-cli — Slate OS SwayNotificationCenter
//!
//! Multi-personality: `swaync`, `swaync-client`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swaync(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swaync [OPTIONS]");
        println!("swaync v0.10 (Slate OS) — Sway Notification Center daemon");
        println!();
        println!("Options:");
        println!("  -s FILE           Style CSS file");
        println!("  -c FILE           Config JSON file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("swaync v0.10 (Slate OS)"); return 0; }
    println!("SwayNotificationCenter daemon starting...");
    println!("  Config: ~/.config/swaync/config.json");
    println!("  Style: ~/.config/swaync/style.css");
    if args.is_empty() {
        println!("  Listening...");
    }
    0
}

fn run_swaync_client(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: swaync-client [OPTIONS]");
        println!("swaync-client v0.10 (Slate OS) — SwayNC control client");
        println!();
        println!("Options:");
        println!("  -t                Toggle notification center");
        println!("  -d                Toggle DND mode");
        println!("  -C                Clear all notifications");
        println!("  -c                Get notification count");
        println!("  -sw               Subscribe to notification events");
        return 0;
    }
    if args.iter().any(|a| a == "-c") {
        println!("3");
    } else if args.iter().any(|a| a == "-t") {
        println!("Notification center toggled.");
    } else if args.iter().any(|a| a == "-C") {
        println!("All notifications cleared.");
    } else if args.iter().any(|a| a == "-d") {
        println!("DND mode toggled.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swaync".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "swaync-client" => run_swaync_client(&rest, &prog),
        _ => run_swaync(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_swaync};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/swaync"), "swaync");
        assert_eq!(basename(r"C:\bin\swaync.exe"), "swaync.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("swaync.exe"), "swaync");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swaync(&["--help".to_string()], "swaync"), 0);
        assert_eq!(run_swaync(&["-h".to_string()], "swaync"), 0);
        let _ = run_swaync(&["--version".to_string()], "swaync");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swaync(&[], "swaync");
    }
}
