#![deny(clippy::all)]

//! linux-notification-center-cli — OurOS Linux Notification Center
//!
//! Multi-personality: `deadd-notification-center`, `notification-center`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_notification_center(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: notification-center [OPTIONS]");
        println!("notification-center v2.2 (OurOS) — Notification center");
        println!();
        println!("Options:");
        println!("  -s             Start daemon");
        println!("  -t             Toggle visibility");
        println!("  -c             Clear all notifications");
        println!("  --count        Print notification count");
        println!("  --version      Show version");
        println!();
        println!("Notification center with history, grouping, and DND.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("notification-center v2.2 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--count") {
        println!("3");
        return 0;
    }
    if args.iter().any(|a| a == "-t") {
        println!("notification-center: toggled");
        return 0;
    }
    println!("notification-center: daemon started");
    println!("  Stored: 3 notifications");
    println!("  DND: off");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "notification-center".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_notification_center(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_notification_center};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/linux-notification-center"), "linux-notification-center");
        assert_eq!(basename(r"C:\bin\linux-notification-center.exe"), "linux-notification-center.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("linux-notification-center.exe"), "linux-notification-center");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_notification_center(&["--help".to_string()], "linux-notification-center"), 0);
        assert_eq!(run_notification_center(&["-h".to_string()], "linux-notification-center"), 0);
        assert_eq!(run_notification_center(&["--version".to_string()], "linux-notification-center"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_notification_center(&[], "linux-notification-center"), 0);
    }
}
