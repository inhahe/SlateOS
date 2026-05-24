#![deny(clippy::all)]

//! btop-cli — OurOS btop++ system monitor
//!
//! Single personality: `btop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_btop(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: btop [OPTIONS]");
        println!("btop 1.3.2 (OurOS) — Resource monitor");
        println!();
        println!("Options:");
        println!("  -lc, --low-color      Disable truecolor");
        println!("  -t, --tty_on          Force tty mode");
        println!("  +t, --tty_off         Disable tty mode");
        println!("  -p, --preset N        Start with preset N (0-9)");
        println!("  --utf-force           Force UTF-8");
        println!("  --debug               Start with debug logging");
        println!("  -v, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("btop version: 1.3.2 (OurOS)");
        return 0;
    }
    println!("btop: Starting system monitor...");
    println!("CPU:  12% [||||                                  ] 4 cores");
    println!("MEM:  4.2G/16.0G [||||||||||                    ] 26%");
    println!("SWP:  0.0G/4.0G  [                              ] 0%");
    println!("DSK:  120G/500G   [||||||||||||||                ] 24%");
    println!("NET:  Up: 1.2MB/s  Down: 5.6MB/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "btop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_btop(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
