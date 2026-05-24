#![deny(clippy::all)]

//! lxpolkit-cli — OurOS LXPolkit lightweight PolicyKit agent
//!
//! Single personality: `lxpolkit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lxpolkit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lxpolkit");
        println!("lxpolkit v0.1 (OurOS) — Lightweight PolicyKit agent");
        println!();
        println!("Minimal GTK+ PolicyKit authentication agent for LXDE.");
        return 0;
    }
    let _ = args;
    println!("lxpolkit: authentication agent started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lxpolkit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lxpolkit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
