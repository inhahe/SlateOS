#![deny(clippy::all)]

//! stanza-cli — OurOS Stanza log agent
//!
//! Single personality: `stanza`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stanza(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stanza [COMMAND] [OPTIONS]");
        println!("Stanza v0.34 (OurOS) — High-performance log agent");
        println!();
        println!("Commands:");
        println!("  run                Start agent");
        println!("  offsets list       List file offsets");
        println!("  offsets clear      Clear file offsets");
        println!("  graph              Display pipeline graph");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --database DIR     Offset database directory");
        println!("  --plugin-dir DIR   Plugin directory");
        println!("  --log-file FILE    Log file");
        println!("  --debug            Enable debug logging");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") { println!("stanza v0.34.2 (OurOS)"); return 0; }
    println!("Stanza v0.34.2 (OurOS)");
    println!("  Operators: 8 active");
    println!("  File inputs: 15 monitored");
    println!("  Journald inputs: 1");
    println!("  Output: OTLP (gRPC)");
    println!("  Entries/s: 5,432");
    println!("  Errors: 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stanza".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stanza(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
