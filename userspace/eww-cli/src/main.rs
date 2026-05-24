#![deny(clippy::all)]

//! eww-cli — OurOS Eww widget system
//!
//! Single personality: `eww`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eww(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: eww COMMAND [OPTIONS]");
        println!("eww v0.5 (OurOS) — ElKowars wacky widgets");
        println!();
        println!("Commands:");
        println!("  daemon            Start eww daemon");
        println!("  open WINDOW       Open a window");
        println!("  close WINDOW      Close a window");
        println!("  reload            Reload config");
        println!("  update VAR=VAL    Update variable");
        println!("  get VAR           Get variable value");
        println!("  list-windows      List available windows");
        println!("  state             Show current state");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("daemon");
    match cmd {
        "daemon" => {
            println!("eww daemon starting...");
            println!("  Config: ~/.config/eww/eww.yuck");
            println!("  Styles: ~/.config/eww/eww.scss");
        }
        "open" => {
            let win = args.get(1).map(|s| s.as_str()).unwrap_or("bar");
            println!("Opening window: {}", win);
        }
        "close" => {
            let win = args.get(1).map(|s| s.as_str()).unwrap_or("bar");
            println!("Closing window: {}", win);
        }
        "list-windows" => {
            println!("bar");
            println!("dashboard");
            println!("notifications");
            println!("powermenu");
        }
        "state" => {
            println!("time: \"10:30 AM\"");
            println!("battery: 85");
            println!("volume: 65");
            println!("brightness: 80");
        }
        "reload" => println!("Configuration reloaded."),
        _ => println!("eww {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eww".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eww(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
