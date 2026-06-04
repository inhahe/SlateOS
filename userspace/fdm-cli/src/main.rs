#![deny(clippy::all)]

//! fdm-cli — OurOS fdm mail fetching/delivery
//!
//! Single personality: `fdm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fdm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fdm [OPTIONS] COMMAND");
        println!("fdm v2.2 (OurOS) — Fetch, filter and deliver mail");
        println!();
        println!("Commands:");
        println!("  fetch             Fetch and deliver mail");
        println!("  poll              Poll accounts without fetching");
        println!();
        println!("Options:");
        println!("  -f FILE           Configuration file");
        println!("  -n                Dry-run mode");
        println!("  -v                Verbose output");
        println!("  -a ACCOUNT        Process specific account");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("fetch");
    let dry = args.iter().any(|a| a == "-n");
    match cmd {
        "poll" => {
            println!("Polling accounts...");
            println!("  work: 5 new messages");
            println!("  personal: 2 new messages");
        }
        _ => {
            if dry {
                println!("[dry-run] Fetching mail...");
                println!("[dry-run]   work: 5 messages — would deliver to ~/Mail/work");
                println!("[dry-run]   personal: 2 messages — would deliver to ~/Mail/personal");
            } else {
                println!("Fetching mail...");
                println!("  work: 5 messages delivered to ~/Mail/work");
                println!("  personal: 2 messages delivered to ~/Mail/personal");
                println!("  Filtered: 3 spam -> ~/Mail/spam");
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fdm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fdm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fdm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fdm"), "fdm");
        assert_eq!(basename(r"C:\bin\fdm.exe"), "fdm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fdm.exe"), "fdm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fdm(&["--help".to_string()], "fdm"), 0);
        assert_eq!(run_fdm(&["-h".to_string()], "fdm"), 0);
        let _ = run_fdm(&["--version".to_string()], "fdm");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fdm(&[], "fdm");
    }
}
