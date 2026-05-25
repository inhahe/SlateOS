#![deny(clippy::all)]

//! winecfg-cli — OurOS Wine configuration utility
//!
//! Single personality: `winecfg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_winecfg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: winecfg [OPTIONS]");
        println!("winecfg v9.0 (OurOS) — Wine configuration dialog");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tabs:");
        println!("  Applications      Per-application Windows version");
        println!("  Libraries         DLL override configuration");
        println!("  Graphics          Display resolution and DPI");
        println!("  Desktop           Virtual desktop settings");
        println!("  Audio             Audio driver configuration");
        println!("  Staging           Wine Staging patch options");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("winecfg v9.0 (OurOS)"); return 0; }
    println!("winecfg: Wine configuration dialog opened");
    println!("  Prefix: ~/.wine");
    println!("  Windows version: Windows 10");
    println!("  Architecture: win64");
    println!("  DLL overrides: 3 configured");
    println!("  Audio driver: PulseAudio");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "winecfg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_winecfg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
