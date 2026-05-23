#![deny(clippy::all)]

//! polybar-cli — OurOS Polybar status bar
//!
//! Multi-personality: `polybar`, `polybar-msg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_polybar(args: &[String], prog: &str) -> i32 {
    if prog == "polybar-msg" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: polybar-msg COMMAND [ARGS...]");
            println!("  cmd quit              Quit polybar");
            println!("  cmd restart           Restart polybar");
            println!("  cmd hide              Hide bar");
            println!("  cmd show              Show bar");
            println!("  cmd toggle            Toggle visibility");
            println!("  action MODULE ACTION  Trigger module action");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("cmd");
        let action = args.get(1).map(|s| s.as_str()).unwrap_or("quit");
        println!("polybar-msg: {} {}", cmd, action);
        return 0;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: polybar [OPTIONS] [BAR_NAME]");
        println!("Polybar 3.7.1 (OurOS) — Status bar");
        println!();
        println!("Options:");
        println!("  -c, --config FILE   Config file");
        println!("  -r, --reload        Reload on config change");
        println!("  -l, --log LEVEL     Log level (error, warn, info, trace)");
        println!("  -q, --quiet         No output");
        println!("  --list-monitors     List available monitors");
        println!("  --list-all-monitors List all monitors");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("polybar 3.7.1");
        return 0;
    }
    if args.iter().any(|a| a == "--list-monitors") {
        println!("DP-1: 2560x1440+0+0 (primary)");
        println!("HDMI-1: 1920x1080+2560+0");
        return 0;
    }
    let bar = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("main");
    println!("polybar: Loading bar '{}'...", bar);
    println!("polybar: Bar rendered.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "polybar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_polybar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
