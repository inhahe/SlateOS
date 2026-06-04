#![deny(clippy::all)]

//! xfce4-notifyd-cli — OurOS XFCE notification daemon
//!
//! Multi-personality: `xfce4-notifyd`, `xfce4-notifyd-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_notifyd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xfce4-notifyd [OPTIONS]");
        println!("xfce4-notifyd v0.9 (OurOS) — XFCE notification daemon");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xfce4-notifyd v0.9 (OurOS)"); return 0; }
    println!("xfce4-notifyd: notification daemon running");
    println!("  Theme: Default");
    println!("  Position: top-right");
    println!("  Opacity: 0.85");
    0
}

fn run_notifyd_config(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xfce4-notifyd-config [OPTIONS]");
        println!("xfce4-notifyd-config v0.9 (OurOS) — Notification settings");
        return 0;
    }
    let _ = args;
    println!("xfce4-notifyd-config: notification settings dialog");
    println!("  Position: top-right, bottom-right, top-left, bottom-left");
    println!("  Disappear after: 10 seconds");
    println!("  Do not disturb: off");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xfce4-notifyd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "xfce4-notifyd-config" => run_notifyd_config(&rest, &prog),
        _ => run_notifyd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_notifyd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xfce4-notifyd"), "xfce4-notifyd");
        assert_eq!(basename(r"C:\bin\xfce4-notifyd.exe"), "xfce4-notifyd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xfce4-notifyd.exe"), "xfce4-notifyd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_notifyd(&["--help".to_string()], "xfce4-notifyd"), 0);
        assert_eq!(run_notifyd(&["-h".to_string()], "xfce4-notifyd"), 0);
        let _ = run_notifyd(&["--version".to_string()], "xfce4-notifyd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_notifyd(&[], "xfce4-notifyd");
    }
}
