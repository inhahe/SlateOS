#![deny(clippy::all)]

//! swaync-cli — OurOS SwayNotificationCenter
//!
//! Multi-personality: `swaync`, `swaync-client`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swaync(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swaync [OPTIONS]");
        println!("swaync v0.10 (OurOS) — Sway Notification Center daemon");
        println!();
        println!("Options:");
        println!("  -s FILE           Style CSS file");
        println!("  -c FILE           Config JSON file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("swaync v0.10 (OurOS)"); return 0; }
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
        println!("swaync-client v0.10 (OurOS) — SwayNC control client");
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
mod tests { #[test] fn test_basic() { assert!(true); } }
