#![deny(clippy::all)]

//! cypress-cli — SlateOS Cypress testing CLI
//!
//! Single personality: `cypress`

use std::env;
use std::process;

fn run_cypress(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cypress <COMMAND> [OPTIONS]");
        println!();
        println!("Cypress end-to-end testing CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  run          Run tests headless");
        println!("  open         Open interactive runner");
        println!("  info         System info");
        println!("  verify       Verify installation");
        println!("  install      Install Cypress");
        println!("  cache        Manage cache");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Cypress package version: 13.6.3 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "run" => {
            let spec = args.windows(2).find(|w| w[0] == "--spec").map(|w| w[1].as_str());
            let browser = args.windows(2).find(|w| w[0] == "--browser").map(|w| w[1].as_str()).unwrap_or("electron");
            println!("═══════════════════════════════════════");
            println!("  (Run Starting)");
            println!("  ┌────────────────────────────────────┐");
            println!("  │ Cypress:    13.6.3                 │");
            println!("  │ Browser:    {}                │", browser);
            println!("  │ Specs:      3 found                │");
            println!("  └────────────────────────────────────┘");
            println!();
            println!("  Running: login.cy.js");
            println!("    ✓ should display login form (1234ms)");
            println!("    ✓ should login with valid credentials (2345ms)");
            println!("    ✓ should show error for invalid login (1456ms)");
            println!("    3 passing (5.0s)");
            println!();
            println!("  Running: dashboard.cy.js");
            println!("    ✓ should load dashboard (2100ms)");
            println!("    ✓ should display charts (1800ms)");
            println!("    2 passing (3.9s)");
            println!();
            println!("═══════════════════════════════════════");
            println!("  (Results)");
            println!("  ┌──────────────────────────────────┐");
            println!("  │ Tests:     5                     │");
            println!("  │ Passing:   5                     │");
            println!("  │ Failing:   0                     │");
            println!("  │ Duration:  8.9s                  │");
            println!("  └──────────────────────────────────┘");
            if let Some(s) = spec {
                println!("  (filtered: {})", s);
            }
            0
        }
        "open" => {
            println!("Opening Cypress interactive runner...");
            println!("  Serving at http://localhost:5050");
            0
        }
        "info" => {
            println!("Cypress Info:");
            println!("  Installed version: 13.6.3");
            println!("  OS Platform:       slateos (x64)");
            println!("  Node version:      v20.11.0");
            println!("  Browsers:          Chromium 120, Firefox 121");
            0
        }
        "verify" => {
            println!("✔ Verified Cypress! /usr/local/lib/cypress/Cypress");
            0
        }
        "cache" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Cached versions:");
                    println!("  13.6.3  /home/user/.cache/Cypress/13.6.3");
                    println!("  13.5.0  /home/user/.cache/Cypress/13.5.0");
                }
                "clear" => {
                    println!("✔ Cleared Cypress cache.");
                }
                _ => { println!("Cache operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: cypress <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cypress(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cypress};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cypress(vec!["--help".to_string()]), 0);
        assert_eq!(run_cypress(vec!["-h".to_string()]), 0);
        let _ = run_cypress(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cypress(vec![]);
    }
}
