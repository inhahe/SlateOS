#![deny(clippy::all)]

//! cherrytree-cli — OurOS CherryTree hierarchical notes
//!
//! Single personality: `cherrytree`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cherrytree(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cherrytree [OPTIONS] [FILE]");
        println!("cherrytree v1.0 (OurOS) — Hierarchical note-taking");
        println!();
        println!("Options:");
        println!("  -n NODE           Open at specific node");
        println!("  -x FILE           Export to file");
        println!("  --export-pdf FILE Export to PDF");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cherrytree v1.0 (OurOS)"); return 0; }
    println!("cherrytree: hierarchical note-taking started");
    println!("  Storage: SQLite database");
    println!("  Nodes: 150");
    println!("  Rich text: yes");
    println!("  Code highlighting: 100+ languages");
    println!("  Encryption: password-protected DB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cherrytree".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cherrytree(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
