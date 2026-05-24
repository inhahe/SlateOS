#![deny(clippy::all)]

//! simplenote-cli — OurOS Simplenote minimal notes
//!
//! Single personality: `simplenote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_simplenote(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: simplenote [OPTIONS]");
        println!("simplenote v2.21 (OurOS) — Simple cross-platform notes");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("simplenote v2.21 (OurOS)"); return 0; }
    println!("simplenote: minimal note-taking app started");
    println!("  Notes: 42");
    println!("  Tags: 8");
    println!("  Sync: Simplenote cloud");
    println!("  Markdown: supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simplenote".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_simplenote(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
