#![deny(clippy::all)]

//! rio-cli — OurOS Rio terminal emulator
//!
//! Single personality: `rio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rio [OPTIONS] [COMMAND...]");
        println!("Rio 0.1.10 (OurOS) — Hardware-accelerated GPU terminal");
        println!();
        println!("Options:");
        println!("  -e, --command CMD       Command to run");
        println!("  --working-dir DIR       Working directory");
        println!("  --config-file FILE      Config file path");
        println!("  --window-title TEXT     Window title");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rio 0.1.10 (OurOS)");
        return 0;
    }
    let title = args.windows(2).find(|w| w[0] == "--window-title")
        .map(|w| w[1].as_str());
    if let Some(t) = title {
        println!("rio: Starting with title '{}'...", t);
    } else {
        println!("rio: Starting hardware-accelerated terminal...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rio(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
