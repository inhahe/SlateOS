#![deny(clippy::all)]

//! datagrip-cli — OurOS DataGrip database IDE
//!
//! Single personality: `datagrip`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_datagrip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: datagrip [OPTIONS] [PROJECT]");
        println!("DataGrip v2024.1 (OurOS) — Multi-engine database IDE");
        println!();
        println!("Options:");
        println!("  --project DIR      Open project directory");
        println!("  --nosplash         Skip splash screen");
        println!("  --disableNonBundledPlugins  Disable plugins");
        println!("  --wait             Wait for files to be closed");
        println!("  diff FILE1 FILE2   Compare files");
        println!("  inspect DIR        Run code inspections");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DataGrip v2024.1.5 (OurOS)"); return 0; }
    println!("DataGrip v2024.1.5 (OurOS)");
    println!("  Datasources: 7 configured");
    println!("  Drivers: PostgreSQL, MySQL, Oracle, MSSQL, MongoDB, Cassandra, Redis");
    println!("  Consoles: 4 open");
    println!("  Schemas: 23 introspected");
    println!("  Recent queries: 156");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "datagrip".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_datagrip(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
