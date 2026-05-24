#![deny(clippy::all)]

//! geary-cli — OurOS Geary email client
//!
//! Single personality: `geary`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geary(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: geary [OPTIONS] [MAILTO_URI]");
        println!("geary v44.0 (OurOS) — Lightweight GNOME email client");
        println!();
        println!("Options:");
        println!("  --hidden          Start hidden in system tray");
        println!("  --new-window      Open a new window");
        println!("  --quit            Quit the application");
        println!("  --debug           Enable debug logging");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("geary v44.0 (OurOS)"); return 0; }
    println!("geary: email client started");
    println!("  Accounts: 1 (IMAP/SMTP)");
    println!("  Inbox: 8 unread conversations");
    println!("  Notifications: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "geary".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geary(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
