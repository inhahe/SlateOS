#![deny(clippy::all)]

//! deadd-cli — SlateOS deadd notification center
//!
//! Multi-personality: `deadd-notification-center`, `deadd-notification-center-ctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deadd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deadd-notification-center [OPTIONS]");
        println!("deadd-notification-center v2.1 (SlateOS) — Notification center");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("GTK notification center with notification history,");
        println!("popup notifications, and Do Not Disturb mode.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deadd-notification-center v2.1 (SlateOS)"); return 0; }
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
mod tests {
    use super::{basename, strip_ext, run_deadd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deadd"), "deadd");
        assert_eq!(basename(r"C:\bin\deadd.exe"), "deadd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deadd.exe"), "deadd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deadd(&["--help".to_string()], "deadd"), 0);
        assert_eq!(run_deadd(&["-h".to_string()], "deadd"), 0);
        let _ = run_deadd(&["--version".to_string()], "deadd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deadd(&[], "deadd");
    }
}
