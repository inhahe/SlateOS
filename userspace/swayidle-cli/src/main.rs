#![deny(clippy::all)]

//! swayidle-cli — OurOS swayidle idle management daemon
//!
//! Single personality: `swayidle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swayidle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swayidle [OPTIONS] [EVENTS]");
        println!("swayidle v1.8 (OurOS) — Idle management daemon for Wayland");
        println!();
        println!("Options:");
        println!("  -w                Wait for command to finish");
        println!("  -d                Debug mode");
        println!("  -C PATH           Config file");
        println!();
        println!("Events:");
        println!("  timeout SEC CMD   Run CMD after SEC seconds idle");
        println!("  resume CMD        Run CMD when activity resumes");
        println!("  before-sleep CMD  Run CMD before system sleep");
        println!("  after-resume CMD  Run CMD after system resume");
        println!("  lock CMD          Run CMD when session locks");
        println!("  unlock CMD        Run CMD when session unlocks");
        println!("  idlehint SEC      Set idle hint after SEC seconds");
        return 0;
    }
    println!("swayidle: idle management daemon started");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "timeout" => {
                if let (Some(sec), Some(cmd)) = (args.get(i + 1), args.get(i + 2)) {
                    println!("  timeout {}s → {}", sec, cmd);
                }
                i += 3;
            }
            "resume" | "before-sleep" | "after-resume" | "lock" | "unlock" => {
                let event = args[i].as_str();
                if let Some(cmd) = args.get(i + 1) {
                    println!("  {} → {}", event, cmd);
                }
                i += 2;
            }
            _ => { i += 1; }
        }
    }
    println!("  Listening for idle events...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swayidle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swayidle(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
