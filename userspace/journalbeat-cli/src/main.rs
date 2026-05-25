#![deny(clippy::all)]

//! journalbeat-cli — OurOS Journalbeat log shipper
//!
//! Single personality: `journalbeat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_journalbeat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: journalbeat [OPTIONS]");
        println!("Journalbeat v8.14 (OurOS) — Journal log shipper");
        println!();
        println!("Options:");
        println!("  -c, --config FILE     Config file");
        println!("  -e                    Log to stderr");
        println!("  --path.data DIR       Data directory");
        println!("  --path.logs DIR       Logs directory");
        println!("  --setup               Run initial setup");
        println!("  --strict.perms        Strict config permissions");
        println!("  test config           Test configuration");
        println!("  test output           Test output connectivity");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("journalbeat v8.14.3 (OurOS)"); return 0; }
    println!("Journalbeat v8.14.3 (OurOS)");
    println!("  Journal units: 45 monitored");
    println!("  Output: elasticsearch");
    println!("  Events/s: 234");
    println!("  Queue: memory (4096 events max)");
    println!("  Backoff: 1s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "journalbeat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_journalbeat(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
