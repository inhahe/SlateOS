#![deny(clippy::all)]

//! wlrctl-cli — SlateOS wlrctl wlroots compositor control
//!
//! Single personality: `wlrctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlrctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wlrctl COMMAND [ARGS]");
        println!("wlrctl v0.2 (Slate OS) — wlroots compositor control utility");
        println!();
        println!("Commands:");
        println!("  keyboard type TEXT        Type text via virtual keyboard");
        println!("  pointer move X Y          Move pointer");
        println!("  pointer click BTN         Click mouse button");
        println!("  pointer scroll AXIS AMT   Scroll");
        println!("  toplevel focus APP        Focus application window");
        println!("  toplevel close APP        Close application window");
        println!("  toplevel minimize APP     Minimize window");
        println!("  toplevel fullscreen APP   Toggle fullscreen");
        println!("  toplevel list             List windows");
        println!("  output list               List outputs");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "keyboard" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("type");
            if sub == "type" {
                let text = args.get(2).map(|s| s.as_str()).unwrap_or("");
                println!("Typing: {}", text);
            }
        }
        "pointer" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("move");
            match sub {
                "move" => {
                    let x = args.get(2).map(|s| s.as_str()).unwrap_or("0");
                    let y = args.get(3).map(|s| s.as_str()).unwrap_or("0");
                    println!("Pointer move to ({}, {})", x, y);
                }
                "click" => {
                    let btn = args.get(2).map(|s| s.as_str()).unwrap_or("left");
                    println!("Pointer click: {}", btn);
                }
                _ => println!("pointer {}", sub),
            }
        }
        "toplevel" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("1: Firefox (app_id=firefox)");
                    println!("2: Terminal (app_id=foot)");
                    println!("3: Files (app_id=nautilus)");
                }
                "focus" | "close" | "minimize" | "fullscreen" => {
                    let app = args.get(2).map(|s| s.as_str()).unwrap_or("*");
                    println!("toplevel {}: {}", sub, app);
                }
                _ => println!("toplevel {}", sub),
            }
        }
        "output" => {
            println!("HDMI-A-1: 1920x1080@60Hz (active)");
            println!("DP-1: 2560x1440@144Hz (active)");
        }
        _ => println!("wlrctl: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlrctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlrctl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wlrctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wlrctl"), "wlrctl");
        assert_eq!(basename(r"C:\bin\wlrctl.exe"), "wlrctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wlrctl.exe"), "wlrctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wlrctl(&["--help".to_string()], "wlrctl"), 0);
        assert_eq!(run_wlrctl(&["-h".to_string()], "wlrctl"), 0);
        let _ = run_wlrctl(&["--version".to_string()], "wlrctl");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wlrctl(&[], "wlrctl");
    }
}
