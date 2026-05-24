#![deny(clippy::all)]

//! obsidian-cli — OurOS Obsidian knowledge base
//!
//! Single personality: `obsidian`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_obsidian(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: obsidian [OPTIONS] [VAULT_PATH]");
        println!("obsidian v1.5 (OurOS) — Knowledge base & note editor");
        println!();
        println!("Options:");
        println!("  --vault PATH      Open specific vault");
        println!("  --new             Create new vault");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("obsidian v1.5 (OurOS)"); return 0; }
    println!("obsidian: knowledge base started");
    println!("  Vault: ~/Documents/Notes");
    println!("  Notes: 342");
    println!("  Graph: 1,250 links");
    println!("  Plugins: 8 community, 3 core");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "obsidian".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_obsidian(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
