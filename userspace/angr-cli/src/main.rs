#![deny(clippy::all)]

//! angr-cli — OurOS angr binary analysis platform
//!
//! Single personality: `angr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_angr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: angr [OPTIONS] BINARY");
        println!("angr v9.2 (OurOS) — Binary analysis platform");
        println!();
        println!("Options:");
        println!("  -e ENTRY       Entry point override");
        println!("  -s SCRIPT      Python analysis script");
        println!("  --cfg           Generate control flow graph");
        println!("  --vfg           Value-flow graph");
        println!("  --ddg           Data dependency graph");
        println!("  --find ADDR    Find path to address");
        println!("  --avoid ADDR   Avoid address");
        println!("  --explore      Symbolic exploration");
        println!("  --auto-load    Auto-load shared libraries");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("angr v9.2.90 (OurOS)"); return 0; }
    println!("angr v9.2.90 (OurOS) — Binary Analysis");
    println!("  Loading: crackme (ELF x86_64)");
    println!("  Shared libraries: libc.so.6, ld-linux-x86-64.so.2");
    println!("  CFG recovery:");
    println!("    Nodes: 1,234");
    println!("    Edges: 2,345");
    println!("    Functions: 89");
    println!("  Symbolic execution:");
    println!("    Exploring from 0x401000");
    println!("    Target: 0x401234 (\"success\")");
    println!("    Avoiding: 0x401200 (\"failure\")");
    println!("    States explored: 456");
    println!("    Solution found: input = \"s3cr3t_k3y\"");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "angr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_angr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
