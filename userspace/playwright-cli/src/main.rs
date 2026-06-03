#![deny(clippy::all)]

//! playwright-cli — OurOS Playwright testing CLI
//!
//! Single personality: `playwright`

use std::env;
use std::process;

fn run_playwright(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: playwright <COMMAND> [OPTIONS]");
        println!();
        println!("Playwright end-to-end testing CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  test         Run tests");
        println!("  show-report  Show HTML report");
        println!("  codegen      Generate test code");
        println!("  open         Open browser");
        println!("  install      Install browsers");
        println!("  screenshot   Take screenshot");
        println!("  pdf          Generate PDF");
        println!("  trace        Manage traces");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version 1.41.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "test" => {
            let grep = args.windows(2).find(|w| w[0] == "--grep").map(|w| w[1].as_str());
            println!("Running 12 tests using 4 workers");
            println!();
            println!("  ✓  1 [chromium] › login.spec.ts:5 › should login successfully (2.1s)");
            println!("  ✓  2 [chromium] › login.spec.ts:15 › should show error for wrong password (1.8s)");
            println!("  ✓  3 [firefox] › login.spec.ts:5 › should login successfully (2.5s)");
            println!("  ✓  4 [firefox] › login.spec.ts:15 › should show error for wrong password (2.0s)");
            println!("  ✓  5 [chromium] › dashboard.spec.ts:5 › should load dashboard (3.2s)");
            println!("  ✓  6 [chromium] › dashboard.spec.ts:20 › should filter by date (2.8s)");
            println!("  ✗  7 [webkit] › dashboard.spec.ts:5 › should load dashboard (4.1s)");
            println!();
            println!("  7) [webkit] › dashboard.spec.ts:5 › should load dashboard");
            println!("     Timeout 5000ms exceeded.");
            println!();
            println!("  11 passed, 1 failed");
            println!("  Finished in 15.3s");
            if let Some(g) = grep {
                println!("  (filtered by: {})", g);
            }
            1
        }
        "show-report" => {
            println!("Serving HTML report at http://localhost:9323");
            println!("Press Ctrl+C to stop.");
            0
        }
        "codegen" => {
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("http://localhost:3000");
            println!("Opening browser for codegen at {}", url);
            println!("  Recording actions...");
            println!("  (close browser to save generated test)");
            0
        }
        "install" => {
            println!("Installing browsers...");
            println!("  ✔ chromium 120.0.6099.28 installed");
            println!("  ✔ firefox 121.0 installed");
            println!("  ✔ webkit 17.4 installed");
            0
        }
        "screenshot" => {
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("http://localhost:3000");
            let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output").map(|w| w[1].as_str()).unwrap_or("screenshot.png");
            println!("Capturing screenshot of {}", url);
            println!("  Saved to {}", output);
            0
        }
        "pdf" => {
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("http://localhost:3000");
            let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output").map(|w| w[1].as_str()).unwrap_or("page.pdf");
            println!("Generating PDF of {}", url);
            println!("  Saved to {}", output);
            0
        }
        "trace" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" => {
                    let file = args.get(2).map(|s| s.as_str()).unwrap_or("trace.zip");
                    println!("Opening trace viewer for {}", file);
                    println!("  Serving at http://localhost:9322");
                }
                _ => { println!("Trace operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: playwright <command>. See --help.");
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
    let code = run_playwright(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_playwright};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_playwright(vec!["--help".to_string()]), 0);
        assert_eq!(run_playwright(vec!["-h".to_string()]), 0);
        assert_eq!(run_playwright(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_playwright(vec![]), 0);
    }
}
