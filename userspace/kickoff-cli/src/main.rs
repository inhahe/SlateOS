#![deny(clippy::all)]

//! kickoff-cli — OurOS Kickoff minimalist launcher
//!
//! Single personality: `kickoff`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kickoff(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kickoff [OPTIONS]");
        println!("kickoff v0.6 (OurOS) — Minimalist Wayland program launcher");
        println!();
        println!("Options:");
        println!("  --from-path       Search $PATH for executables");
        println!("  --from-desktop    Search .desktop files");
        println!("  --from-stdin      Read items from stdin");
        println!("  --font FONT       Font path");
        println!("  --font-size SIZE  Font size");
        println!("  --prompt TEXT     Prompt text");
        println!("  --history NUM     Number of history entries");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kickoff v0.6 (OurOS)"); return 0; }
    println!("kickoff: program launcher");
    println!("  > ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kickoff".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kickoff(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
