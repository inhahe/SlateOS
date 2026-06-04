#![deny(clippy::all)]

//! mate-notification-cli — OurOS MATE notification daemon
//!
//! Multi-personality: `mate-notification-daemon`, `mate-notification-properties`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mate_notifyd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mate-notification-daemon [OPTIONS]");
        println!("mate-notification-daemon v1.28 (OurOS) — MATE notifications");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mate-notification-daemon v1.28 (OurOS)"); return 0; }
    println!("mate-notification-daemon: running");
    println!("  Theme: standard");
    println!("  Position: top-right");
    0
}

fn run_mate_notifyd_props(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mate-notification-properties");
        println!("Configure MATE notification daemon settings.");
        return 0;
    }
    let _ = args;
    println!("mate-notification-properties: settings dialog");
    println!("  Theme: standard, slider, nodoka, coco");
    println!("  Position: configurable");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mate-notification-daemon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mate-notification-properties" => run_mate_notifyd_props(&rest, &prog),
        _ => run_mate_notifyd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mate_notifyd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mate-notification"), "mate-notification");
        assert_eq!(basename(r"C:\bin\mate-notification.exe"), "mate-notification.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mate-notification.exe"), "mate-notification");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mate_notifyd(&["--help".to_string()], "mate-notification"), 0);
        assert_eq!(run_mate_notifyd(&["-h".to_string()], "mate-notification"), 0);
        let _ = run_mate_notifyd(&["--version".to_string()], "mate-notification");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mate_notifyd(&[], "mate-notification");
    }
}
