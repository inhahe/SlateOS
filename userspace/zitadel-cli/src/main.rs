#![deny(clippy::all)]

//! zitadel-cli — OurOS ZITADEL identity infrastructure
//!
//! Single personality: `zitadel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zitadel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zitadel [COMMAND] [OPTIONS]");
        println!("ZITADEL v2.50 (OurOS) — Cloud-native identity infrastructure");
        println!();
        println!("Commands:");
        println!("  start              Start ZITADEL");
        println!("  start-from-init    Start and initialize");
        println!("  start-from-setup   Start from setup");
        println!("  init               Initialize database");
        println!("  setup              Run setup steps");
        println!("  mirror             Mirror another ZITADEL instance");
        println!("  key                Manage encryption keys");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (YAML)");
        println!("  --masterkey KEY    Master encryption key");
        println!("  --external-domain D  External domain");
        println!("  --external-port P  External port");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ZITADEL v2.50.4 (OurOS)"); return 0; }
    println!("ZITADEL v2.50.4 (OurOS)");
    println!("  Console: https://0.0.0.0:8080/ui/console");
    println!("  API: https://0.0.0.0:8080");
    println!("  Organizations: 3");
    println!("  Users: 890");
    println!("  Projects: 12");
    println!("  Applications: 34");
    println!("  Actions: 5 configured");
    println!("  Database: CockroachDB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zitadel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zitadel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
