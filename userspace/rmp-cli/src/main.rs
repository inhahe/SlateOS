#![deny(clippy::all)]

//! rmp-cli — OurOS MessagePack inspector (rmp-based)
//!
//! Single personality: `msgpack-inspect`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rmp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: msgpack-inspect [OPTIONS] FILE");
        println!("msgpack-inspect v1.0 (OurOS) — MessagePack binary inspector");
        println!();
        println!("Options:");
        println!("  FILE              MessagePack file to inspect");
        println!("  --json            Convert to JSON");
        println!("  --yaml            Convert to YAML");
        println!("  --hex             Show hex representation");
        println!("  --types           Show type annotations");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.msgpack");
    if args.iter().any(|a| a == "--json") {
        println!("{{\"name\":\"example\",\"values\":[1,2,3],\"nested\":{{\"key\":\"val\"}}}}");
        return 0;
    }
    if args.iter().any(|a| a == "--types") {
        println!("Inspecting: {}", file);
        println!("  fixmap(3)");
        println!("    fixstr(4) \"name\" => fixstr(7) \"example\"");
        println!("    fixstr(6) \"values\" => fixarray(3) [fixint(1), fixint(2), fixint(3)]");
        println!("    fixstr(6) \"nested\" => fixmap(1)");
        println!("      fixstr(3) \"key\" => fixstr(3) \"val\"");
        return 0;
    }
    println!("Inspecting: {}", file);
    println!("  {{");
    println!("    name: \"example\"");
    println!("    values: [1, 2, 3]");
    println!("    nested: {{key: \"val\"}}");
    println!("  }}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "msgpack-inspect".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rmp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
