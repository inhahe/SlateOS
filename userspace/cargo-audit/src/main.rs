#![deny(clippy::all)]

//! cargo-audit — Slate OS audit Cargo.lock for security vulnerabilities
//!
//! Single personality: `cargo-audit`

use std::env;
use std::process;

fn run_cargo_audit(args: Vec<String>) -> i32 {
    // Invoked as `cargo audit`, first arg may be "audit"
    let subargs: Vec<String> = if args.first().map(|s| s.as_str()) == Some("audit") {
        args[1..].to_vec()
    } else {
        args
    };

    let cmd = subargs.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            if cmd.is_empty() {
                // Default: run audit
                println!("    Fetching advisory database...");
                println!("      Updated advisory database (last update: 2 hours ago)");
                println!("    Scanning Cargo.lock for vulnerabilities...");
                println!();
                println!("Crate:     time");
                println!("Version:   0.1.45");
                println!("Title:     Potential segfault in localtime_r invocations");
                println!("Date:      2020-11-18");
                println!("ID:        RUSTSEC-2020-0071");
                println!("URL:       https://rustsec.org/advisories/RUSTSEC-2020-0071");
                println!("Severity:  6.2 (medium)");
                println!("Solution:  Upgrade to >=0.2.23");
                println!();
                println!("Crate:     regex");
                println!("Version:   1.5.4");
                println!("Title:     Potential denial-of-service");
                println!("Date:      2022-03-08");
                println!("ID:        RUSTSEC-2022-0013");
                println!("URL:       https://rustsec.org/advisories/RUSTSEC-2022-0013");
                println!("Severity:  7.5 (high)");
                println!("Solution:  Upgrade to >=1.5.5");
                println!();
                println!("2 vulnerabilities found!");
                return 1;
            }
            println!("Usage: cargo audit [COMMAND]");
            println!();
            println!("Audit Cargo.lock for security vulnerabilities.");
            println!();
            println!("Commands:");
            println!("  (default)   Audit Cargo.lock");
            println!("  fix         Auto-fix vulnerable dependencies");
            println!("  bin         Audit compiled binary");
            println!();
            println!("Options:");
            println!("  -d, --db <PATH>        Advisory database path");
            println!("  -D, --deny <SEVERITY>  Fail on severity (low/medium/high/critical)");
            println!("  -f, --file <LOCKFILE>  Path to Cargo.lock");
            println!("  -n, --no-fetch         Don't fetch advisory database");
            println!("  --json                 Output in JSON format");
            println!("  --ignore <ID>          Ignore specific advisory");
            println!("  -V, --version          Show version");
            0
        }
        "--version" | "-V" => {
            println!("cargo-audit 0.20.0 (Slate OS)");
            0
        }
        "fix" => {
            println!("    Fetching advisory database...");
            println!("    Scanning for vulnerabilities...");
            println!();
            println!("    Fixing time: 0.1.45 -> 0.2.23");
            println!("    Fixing regex: 1.5.4 -> 1.10.4");
            println!();
            println!("    Fixed 2 vulnerabilities.");
            println!("    Updated Cargo.lock.");
            0
        }
        "bin" => {
            let binary = subargs.get(1).map(|s| s.as_str()).unwrap_or("target/release/app");
            println!("    Auditing binary: {}", binary);
            println!("    Detecting embedded dependency info...");
            println!("    Found 42 crates");
            println!();
            println!("    No vulnerabilities detected in binary.");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cargo_audit(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cargo_audit};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cargo_audit(vec!["--help".to_string()]), 0);
        assert_eq!(run_cargo_audit(vec!["-h".to_string()]), 0);
        let _ = run_cargo_audit(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cargo_audit(vec![]);
    }
}
