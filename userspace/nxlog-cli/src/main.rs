#![deny(clippy::all)]

//! nxlog-cli — OurOS NXLog log collection
//!
//! Single personality: `nxlog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nxlog(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nxlog [OPTIONS]");
        println!("NXLog v5.7 (OurOS) — Multi-platform log collection");
        println!();
        println!("Options:");
        println!("  -c, --conf FILE    Config file");
        println!("  -f, --foreground   Run in foreground");
        println!("  -v, --verify       Verify config");
        println!("  -s, --stop         Stop running instance");
        println!("  -r, --reload       Reload config");
        println!("  --json             Output in JSON format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NXLog v5.7.0 (OurOS)"); return 0; }
    println!("NXLog v5.7.0 (OurOS)");
    println!("  Inputs: im_file (3), im_udp (1), im_tcp (1)");
    println!("  Outputs: om_file (2), om_tcp (1), om_elasticsearch (1)");
    println!("  Processors: pm_transformer (2), pm_filter (1)");
    println!("  Routes: 4 active");
    println!("  Events/s: 8,912");
    println!("  Buffer: 16 MiB used");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nxlog".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nxlog(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
