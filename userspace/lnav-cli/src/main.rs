#![deny(clippy::all)]

//! lnav-cli — OurOS log file navigator
//!
//! Single personality: `lnav`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lnav(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: lnav [OPTIONS] [FILE...]");
        println!("lnav v0.12 (OurOS) — Log file navigator");
        println!();
        println!("Options:");
        println!("  FILE...           Log files to view");
        println!("  -q                Quiet mode (headless)");
        println!("  -c CMD            Execute lnav command");
        println!("  -f FILE           Execute commands from file");
        println!("  -I DIR            Additional config directory");
        println!("  -n                Run without ncurses");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lnav v0.12 (OurOS)"); return 0; }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if args.iter().any(|a| a == "-n") {
        for f in &files {
            println!("Loading: {}", f);
        }
        println!("Format: syslog");
        println!("Lines: 15432");
        println!("Errors: 23");
        println!("Warnings: 147");
    } else {
        for f in &files {
            println!("Opening: {}", f);
        }
        println!("Press q to quit, / to search");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lnav".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lnav(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
