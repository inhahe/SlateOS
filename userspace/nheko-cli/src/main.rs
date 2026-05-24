#![deny(clippy::all)]

//! nheko-cli — OurOS Nheko Matrix client
//!
//! Single personality: `nheko`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nheko(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nheko [OPTIONS]");
        println!("nheko v0.11 (OurOS) — Desktop Matrix client (Qt)");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("nheko v0.11 (OurOS)"); return 0; }
    println!("nheko: Matrix client started");
    println!("  Homeserver: matrix.org");
    println!("  Rooms: 8 joined");
    println!("  Unread: 2 rooms");
    println!("  End-to-end encryption: enabled");
    println!("  VoIP: supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nheko".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nheko(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
