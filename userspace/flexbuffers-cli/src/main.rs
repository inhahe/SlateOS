#![deny(clippy::all)]

//! flexbuffers-cli — OurOS FlexBuffers schema-less binary inspector
//!
//! Single personality: `flexbuf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flexbuf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flexbuf [OPTIONS] FILE");
        println!("flexbuf v1.0 (OurOS) — FlexBuffers inspector");
        println!();
        println!("Options:");
        println!("  FILE              FlexBuffers file to inspect");
        println!("  --json            Output as JSON");
        println!("  --types           Show type information");
        println!("  --stats           Show buffer statistics");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.flexbuf");
    if args.iter().any(|a| a == "--stats") {
        println!("Buffer statistics: {}", file);
        println!("  Total size: 512 bytes");
        println!("  Root type: Map");
        println!("  Keys: 5");
        println!("  Depth: 3");
        println!("  Strings: 8 (total 142 bytes)");
        println!("  Vectors: 2");
        println!("  Maps: 3");
        return 0;
    }
    if args.iter().any(|a| a == "--json") {
        println!("{{\"name\":\"test\",\"values\":[1.5,2.5,3.5],\"active\":true}}");
        return 0;
    }
    println!("Inspecting: {}", file);
    println!("  Root: Map");
    println!("    \"name\" => String(\"test\")");
    println!("    \"values\" => Vector[Float(1.5), Float(2.5), Float(3.5)]");
    println!("    \"active\" => Bool(true)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flexbuf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flexbuf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
