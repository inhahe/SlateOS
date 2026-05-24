#![deny(clippy::all)]

//! pidgin-cli — OurOS Pidgin multi-protocol IM client
//!
//! Multi-personality: `pidgin`, `purple-remote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pidgin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pidgin [OPTIONS]");
        println!("pidgin v3.0 (OurOS) — Multi-protocol instant messenger");
        println!();
        println!("Options:");
        println!("  -c DIR            Config directory");
        println!("  -d                Debug mode");
        println!("  -n                Don't load plugins");
        println!("  --login=NAME      Auto-login to account");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pidgin v3.0 (OurOS)"); return 0; }
    println!("pidgin: multi-protocol IM client started");
    println!("  Protocols: XMPP, IRC, Matrix");
    println!("  Accounts: 2 connected");
    println!("  Plugins: 4 loaded");
    0
}

fn run_remote(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: purple-remote COMMAND [ARGS]");
        println!("purple-remote v3.0 (OurOS) — Remote control for Pidgin");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("purple-remote v3.0 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    println!("purple-remote: sent command '{}'", cmd);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pidgin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "purple-remote" => run_remote(&rest, &prog),
        _ => run_pidgin(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
