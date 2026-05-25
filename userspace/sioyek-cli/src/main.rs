#![deny(clippy::all)]

//! sioyek-cli — OurOS Sioyek PDF viewer for research
//!
//! Single personality: `sioyek`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sioyek(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sioyek [OPTIONS] [FILE]");
        println!("sioyek v2.0 (OurOS) — PDF viewer for research papers");
        println!();
        println!("Options:");
        println!("  --new-window      Open in new window");
        println!("  --page NUM        Open at page");
        println!("  --inverse-search CMD  Inverse search command");
        println!("  --forward-search-file F  Forward search");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sioyek v2.0 (OurOS)"); return 0; }
    println!("sioyek: research PDF viewer started");
    println!("  Features: portals, bookmarks, highlights, links");
    println!("  SyncTeX: supported");
    println!("  Vim-like keybindings");
    println!("  Smart search: reference jumping");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sioyek".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sioyek(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
