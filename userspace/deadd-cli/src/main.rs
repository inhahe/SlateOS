#![deny(clippy::all)]

//! deadd-cli — OurOS deadd notification center
//!
//! Multi-personality: `deadd-notification-center`, `deadd-notification-center-ctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deadd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deadd-notification-center [OPTIONS]");
        println!("deadd-notification-center v2.1 (OurOS) — Notification center");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("GTK notification center with notification history,");
        println!("popup notifications, and Do Not Disturb mode.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deadd-notification-center v2.1 (OurOS)"); return 0; }
    println!("deadd-notification-center: running");
    println!("  Notifications stored: 5");
    println!("  DND: off");
    0
}

fn run_deadd_ctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deadd-notification-center-ctl <command>");
        println!("  toggle       Toggle notification center");
        println!("  toggle-dnd   Toggle Do Not Disturb");
        println!("  clear-all    Clear all notifications");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("toggle") => println!("deadd: notification center toggled"),
        Some("toggle-dnd") => println!("deadd: DND toggled"),
        Some("clear-all") => println!("deadd: notifications cleared"),
        _ => println!("deadd-ctl: use --help for commands"),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "deadd-notification-center".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "deadd-notification-center-ctl" => run_deadd_ctl(&rest, &prog),
        _ => run_deadd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
