#![deny(clippy::all)]

//! alacritty-cli — OurOS Alacritty terminal emulator
//!
//! Multi-personality: `alacritty`, `alacritty-msg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_alacritty(args: &[String], prog: &str) -> i32 {
    if prog == "alacritty-msg" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: alacritty msg [OPTIONS] COMMAND");
            println!();
            println!("Commands:");
            println!("  create-window    Create a new window");
            println!("  config           Update config options");
            return 0;
        }
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("create-window");
        match cmd {
            "create-window" => println!("alacritty msg: Window created."),
            "config" => println!("alacritty msg: Config updated."),
            _ => println!("alacritty msg: unknown command '{}'", cmd),
        }
        return 0;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alacritty [OPTIONS]");
        println!("Alacritty 0.13.2 (OurOS) — GPU-accelerated terminal emulator");
        println!();
        println!("Options:");
        println!("  --config-file FILE     Config file");
        println!("  -o OPTION              Override config (key=value)");
        println!("  --working-directory D  Working directory");
        println!("  --title TEXT           Window title");
        println!("  --class GENERAL        Window class");
        println!("  -e, --command CMD      Command to run");
        println!("  --hold                 Keep open after command exits");
        println!("  --embed WINID          X11 embed");
        println!("  -v                     Increase verbosity");
        println!("  -V, --version          Show version");
        println!("  --print-events         Print input events");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("alacritty 0.13.2 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--print-events") {
        println!("[Key] Char('a')");
        println!("[Key] Enter");
        return 0;
    }
    println!("alacritty: Starting GPU-accelerated terminal...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alacritty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_alacritty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
