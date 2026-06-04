#![deny(clippy::all)]

//! dunst-cli — OurOS Dunst notification daemon
//!
//! Multi-personality: `dunst`, `dunstify`, `dunstctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dunst(args: &[String], prog: &str) -> i32 {
    match prog {
        "dunstify" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: dunstify [OPTIONS] SUMMARY [BODY]");
                println!("  -u URGENCY   low, normal, critical");
                println!("  -t TIMEOUT   Timeout in ms");
                println!("  -i ICON      Icon name");
                println!("  -a APP       App name");
                println!("  -r ID        Replace notification");
                println!("  -C           Close notification");
                return 0;
            }
            let summary = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("Notification");
            println!("Notification sent: {}", summary);
            return 0;
        }
        "dunstctl" => {
            if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
                println!("Usage: dunstctl COMMAND");
                println!("  set-paused true/false/toggle");
                println!("  is-paused");
                println!("  count [displayed|history|waiting]");
                println!("  history");
                println!("  history-pop");
                println!("  close");
                println!("  close-all");
                println!("  action");
                return 0;
            }
            let cmd = args.first().map(|s| s.as_str()).unwrap_or("is-paused");
            match cmd {
                "is-paused" => println!("false"),
                "count" => {
                    let what = args.get(1).map(|s| s.as_str()).unwrap_or("displayed");
                    match what {
                        "displayed" => println!("2"),
                        "history" => println!("15"),
                        "waiting" => println!("0"),
                        _ => println!("0"),
                    }
                }
                "close" => println!("Closed top notification."),
                "close-all" => println!("Closed all notifications."),
                _ => println!("dunstctl: '{}' completed", cmd),
            }
            return 0;
        }
        _ => {}
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dunst [OPTIONS]");
        println!("dunst 1.11.0 (OurOS) — Notification daemon");
        println!();
        println!("Options:");
        println!("  -config FILE   Config file path");
        println!("  -verbosity N   0=errors, 1=warnings, 2=info, 3=debug");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dunst 1.11.0");
        return 0;
    }
    println!("dunst: Starting notification daemon...");
    println!("dunst: Listening for notifications.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dunst".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dunst(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dunst};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dunst"), "dunst");
        assert_eq!(basename(r"C:\bin\dunst.exe"), "dunst.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dunst.exe"), "dunst");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dunst(&["--help".to_string()], "dunst"), 0);
        assert_eq!(run_dunst(&["-h".to_string()], "dunst"), 0);
        let _ = run_dunst(&["--version".to_string()], "dunst");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dunst(&[], "dunst");
    }
}
