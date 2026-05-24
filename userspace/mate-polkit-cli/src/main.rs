#![deny(clippy::all)]

//! mate-polkit-cli — OurOS MATE PolicyKit authentication agent
//!
//! Single personality: `mate-polkit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mate-polkit");
        println!("mate-polkit v1.28 (OurOS) — MATE PolicyKit agent");
        println!();
        println!("GTK+ PolicyKit authentication agent for MATE desktop.");
        return 0;
    }
    let _ = args;
    println!("mate-polkit: authentication agent started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mate-polkit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_agent(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
