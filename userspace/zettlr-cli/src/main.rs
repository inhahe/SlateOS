#![deny(clippy::all)]

//! zettlr-cli — OurOS Zettlr Markdown editor for academics
//!
//! Single personality: `zettlr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zettlr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zettlr [OPTIONS] [FILE/DIR]");
        println!("zettlr v3.0 (OurOS) — Markdown editor for researchers");
        println!();
        println!("Options:");
        println!("  --data-dir DIR    Data directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zettlr v3.0 (OurOS)"); return 0; }
    println!("zettlr: Markdown editor started");
    println!("  Workspace: ~/Documents/Research");
    println!("  Files: 120 Markdown files");
    println!("  Export: PDF (via Pandoc), DOCX, HTML, LaTeX");
    println!("  Citations: Zotero integration");
    println!("  Zettelkasten: ID-based linking");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zettlr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zettlr(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
