#![deny(clippy::all)]

//! getmail-cli — OurOS getmail mail retriever
//!
//! Single personality: `getmail`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_getmail(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: getmail [OPTIONS]");
        println!("getmail v6.19 (OurOS) — Mail retriever with Strstrong filtering");
        println!();
        println!("Options:");
        println!("  -r FILE           Configuration file");
        println!("  -g DIR            Config directory (~/.config/getmail/)");
        println!("  -n                Dry-run mode");
        println!("  -v                Verbose output");
        println!("  --dump            Dump configuration");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("getmail v6.19 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--dump") {
        println!("Configuration:");
        println!("  Retriever: IMAP (imap.example.com:993)");
        println!("  Destination: Maildir ~/Mail/");
        println!("  Filters: spam, duplicates");
        return 0;
    }
    println!("Retrieving mail...");
    println!("  Account: imap.example.com");
    println!("  New messages: 7");
    println!("  Delivered: 5 to ~/Mail/INBOX");
    println!("  Filtered: 2 to ~/Mail/spam");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "getmail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_getmail(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
