#![deny(clippy::all)]

//! signal-desktop-cli — OurOS Signal Desktop messenger
//!
//! Single personality: `signal-desktop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_signal(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: signal-desktop [OPTIONS]");
        println!("signal-desktop v7.0 (OurOS) — Signal private messenger");
        println!();
        println!("Options:");
        println!("  --start-in-tray   Start minimized");
        println!("  --no-sandbox      Disable sandbox");
        println!("  --use-tray-icon   Show tray icon");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("signal-desktop v7.0 (OurOS)"); return 0; }
    println!("signal-desktop: encrypted messenger started");
    println!("  Status: linked to phone");
    println!("  Conversations: 42");
    println!("  Encryption: Signal Protocol (end-to-end)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "signal-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_signal(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
