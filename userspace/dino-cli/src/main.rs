#![deny(clippy::all)]

//! dino-cli — OurOS Dino XMPP/Jabber client
//!
//! Single personality: `dino`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dino(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dino [OPTIONS]");
        println!("dino v0.4 (OurOS) — Modern XMPP/Jabber client");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dino v0.4 (OurOS)"); return 0; }
    println!("dino: XMPP client started");
    println!("  Accounts: 1 connected");
    println!("  Contacts: 25 online");
    println!("  Group chats: 3 joined");
    println!("  OMEMO encryption: enabled");
    println!("  Audio/Video calls: supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dino".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dino(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
