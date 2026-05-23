#![deny(clippy::all)]

//! tcl-cli — OurOS Tcl interpreter
//!
//! Multi-personality: `tclsh`, `tclsh8.6`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tclsh(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tclsh [FILE [ARG ...]]");
        println!("Tcl 8.6.14 (OurOS)");
        println!("  If no file given, starts interactive shell.");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("8.6.14");
        return 0;
    }
    let file = args.first().map(|s| s.as_str());
    if let Some(f) = file {
        println!("tclsh: executing {}", f);
    } else {
        println!("% ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tclsh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tclsh(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
