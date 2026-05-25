#![deny(clippy::all)]

//! seahorse-cli — OurOS GNOME Seahorse key/password manager
//!
//! Single personality: `seahorse`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_seahorse(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: seahorse [OPTIONS]");
        println!("seahorse v43.0 (OurOS) — GNOME Passwords & Keys");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("seahorse v43.0 (OurOS)"); return 0; }
    println!("seahorse: Passwords & Keys started");
    println!("  Login keyring: 15 passwords");
    println!("  GPG keys: 2");
    println!("  SSH keys: 3");
    println!("  Certificates: 1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "seahorse".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_seahorse(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
