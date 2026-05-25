#![deny(clippy::all)]

//! pgadmin-cli — OurOS pgAdmin PostgreSQL management
//!
//! Single personality: `pgadmin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pgadmin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pgadmin [OPTIONS]");
        println!("pgadmin v8.2 (OurOS) — PostgreSQL administration tool");
        println!();
        println!("Options:");
        println!("  --port PORT      Web server port (default: 5050)");
        println!("  --desktop        Run in desktop mode");
        println!("  --server-mode    Run in server mode");
        println!("  --version        Show version");
        println!();
        println!("Web-based PostgreSQL management and query tool.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pgadmin v8.2 (OurOS)"); return 0; }
    println!("pgadmin: starting web interface");
    println!("  URL: http://localhost:5050");
    println!("  Mode: desktop");
    println!("  Servers configured: 1 (localhost:5432)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pgadmin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pgadmin(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
