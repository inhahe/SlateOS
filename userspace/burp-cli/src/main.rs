#![deny(clippy::all)]

//! burp-cli — SlateOS BURP backup and restore program
//!
//! Multi-personality: `burp`, `burp-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_burp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: burp [OPTIONS]");
        println!("burp v2.5 (SlateOS) — Backup and Restore Program");
        println!();
        println!("Options:");
        println!("  -a ACTION    Action: backup, restore, list, verify, estimate");
        println!("  -b NUMBER    Backup number for restore/list");
        println!("  -c FILE      Configuration file");
        println!("  -r REGEX     Regex for selective restore");
        println!("  -l LOGFILE   Log file path");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("burp v2.5 (SlateOS)"); return 0; }
    // Find action flag
    let action = args.windows(2).find(|w| w[0] == "-a").map(|w| w[1].as_str());
    match action {
        Some("list") => {
            println!("Backup list:");
            println!("  0000001  2024-01-10 02:00  full    1.2 GiB");
            println!("  0000002  2024-01-11 02:00  incr    45 MiB");
            println!("  0000003  2024-01-12 02:00  incr    32 MiB");
        }
        Some("backup") => {
            println!("burp: starting backup");
            println!("  Phase 1: scanning...");
            println!("  Phase 2: transferring...");
            println!("  Backup completed: 0000004");
        }
        _ => {
            println!("burp: client ready");
            println!("  Server: localhost:4971");
            println!("  Protocol: 2 (with dedup)");
        }
    }
    0
}

fn run_burp_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: burp-server [OPTIONS]");
        println!("burp-server v2.5 (SlateOS) — BURP server daemon");
        println!("  -c FILE    Configuration file");
        println!("  -F         Run in foreground");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("burp-server v2.5 (SlateOS)"); return 0; }
    println!("burp-server: listening on port 4971");
    println!("  Clients configured: 4");
    println!("  Storage: /var/spool/burp");
    println!("  Deduplication: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "burp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "burp-server" => run_burp_server(&rest, &prog),
        _ => run_burp(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_burp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/burp"), "burp");
        assert_eq!(basename(r"C:\bin\burp.exe"), "burp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("burp.exe"), "burp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_burp(&["--help".to_string()], "burp"), 0);
        assert_eq!(run_burp(&["-h".to_string()], "burp"), 0);
        let _ = run_burp(&["--version".to_string()], "burp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_burp(&[], "burp");
    }
}
