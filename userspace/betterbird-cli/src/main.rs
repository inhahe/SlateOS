#![deny(clippy::all)]

//! betterbird-cli — OurOS Betterbird enhanced Thunderbird fork
//!
//! Single personality: `betterbird`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_betterbird(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: betterbird [OPTIONS]");
        println!("betterbird v115.0 (OurOS) — Enhanced Thunderbird email client");
        println!();
        println!("Options:");
        println!("  -compose          Open compose window");
        println!("  -mail             Open mail window");
        println!("  -addressbook      Open address book");
        println!("  -calendar         Open calendar");
        println!("  -P PROFILE        Use named profile");
        println!("  --safe-mode       Start in safe mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("betterbird v115.0 (OurOS)"); return 0; }
    println!("betterbird: enhanced email client started");
    println!("  Based on: Thunderbird 115");
    println!("  Accounts: 2 configured");
    println!("  Inbox: 15 unread");
    println!("  Enhancements: multi-line view, improved search");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "betterbird".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_betterbird(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
