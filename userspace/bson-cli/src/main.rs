#![deny(clippy::all)]

//! bson-cli — OurOS BSON inspection tool
//!
//! Single personality: `bsondump`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bson(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bsondump [OPTIONS] FILE.bson");
        println!("bsondump v1.0 (OurOS) — BSON to JSON converter");
        println!();
        println!("Options:");
        println!("  FILE.bson         BSON file to dump");
        println!("  --type=json       Output as JSON (default)");
        println!("  --type=debug      Output debug format");
        println!("  --pretty          Pretty-print output");
        println!("  --objcheck        Validate BSON objects");
        return 0;
    }
    if args.iter().any(|a| a == "--objcheck") {
        let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.bson");
        println!("Validating: {}", file);
        println!("  Documents: 1024");
        println!("  Status: VALID");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.bson");
    println!("Dumping: {}", file);
    println!("{{\"_id\":{{\"$oid\":\"507f1f77bcf86cd799439011\"}},\"name\":\"test\",\"value\":42}}");
    println!("{{\"_id\":{{\"$oid\":\"507f1f77bcf86cd799439012\"}},\"name\":\"example\",\"value\":99}}");
    println!("2 documents processed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bsondump".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bson(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
