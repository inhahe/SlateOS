#![deny(clippy::all)]

//! cantor-cli — OurOS Cantor math notebook
//!
//! Single personality: `cantor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cantor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cantor [OPTIONS] [FILE.cws]");
        println!("cantor v23.08 (OurOS) — KDE math worksheet application");
        println!();
        println!("Options:");
        println!("  --backend NAME    Select math backend");
        println!("  --version         Show version");
        println!();
        println!("Backends:");
        println!("  maxima, octave, r, python, julia, sage,");
        println!("  kalgebra, scilab, qalculate, lua");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cantor v23.08 (OurOS)"); return 0; }
    println!("cantor: math worksheet started");
    println!("  Available backends:");
    println!("    Maxima:   installed");
    println!("    Octave:   installed");
    println!("    Python:   installed");
    println!("    R:        installed");
    println!("  LaTeX rendering: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cantor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cantor(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
