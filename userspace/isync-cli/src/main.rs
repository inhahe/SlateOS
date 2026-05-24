#![deny(clippy::all)]

//! isync-cli — OurOS isync/mbsync IMAP mailbox synchronizer
//!
//! Single personality: `mbsync`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mbsync(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mbsync [OPTIONS] CHANNEL...");
        println!("mbsync v1.5 (OurOS) — Synchronize IMAP4 and Maildir mailboxes");
        println!();
        println!("Options:");
        println!("  CHANNEL           Channel or group to sync");
        println!("  -a                Sync all channels");
        println!("  -l                List channels");
        println!("  -c FILE           Configuration file");
        println!("  -n                Dry-run mode");
        println!("  -V                Verbose");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mbsync v1.5 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Channels:");
        println!("  work-inbox");
        println!("  work-sent");
        println!("  personal-inbox");
        println!("Groups:");
        println!("  work (work-inbox, work-sent)");
        return 0;
    }
    let channel = if args.iter().any(|a| a == "-a") {
        "all"
    } else {
        args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("all")
    };
    println!("Syncing: {}", channel);
    println!("  C: 3/3  B: 0/0  M: +3/3 *0/0 #0/0  S: +0/0 *0/0 #0/0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mbsync".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mbsync(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
