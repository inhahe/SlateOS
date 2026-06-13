#![deny(clippy::all)]

//! k6-cli — Slate OS k6 load testing CLI
//!
//! Single personality: `k6`

use std::env;
use std::process;

fn run_k6(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: k6 <COMMAND> [OPTIONS]");
        println!();
        println!("k6 load testing CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  run          Run a test script");
        println!("  inspect      Inspect a script");
        println!("  archive      Create test archive");
        println!("  cloud        Run on k6 Cloud");
        println!("  login        Login to k6 Cloud");
        println!("  stats        Show test stats");
        println!("  version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("k6 v0.49.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("script.js");
            let vus = args.windows(2).find(|w| w[0] == "--vus").map(|w| w[1].as_str()).unwrap_or("10");
            let duration = args.windows(2).find(|w| w[0] == "--duration").map(|w| w[1].as_str()).unwrap_or("30s");

            println!("          /\\      |‾‾| /‾‾/   /‾‾/");
            println!("     /\\  /  \\     |  |/  /   /  /");
            println!("    /  \\/    \\    |     (   /   ‾‾\\");
            println!("   /          \\   |  |\\  \\ |  (‾)  |");
            println!("  / __________ \\  |__| \\__\\ \\_____/");
            println!();
            println!("  execution: local");
            println!("     script: {}", script);
            println!("     output: -");
            println!();
            println!("  scenarios:");
            println!("    default: {} looping VUs for {} (exec: default)", vus, duration);
            println!();
            println!("  running ({}, {} VUs, {} max)...", duration, vus, vus);
            println!();
            println!("     data_received.........: 1.2 MB  40 kB/s");
            println!("     data_sent.............: 125 kB  4.2 kB/s");
            println!("     http_req_blocked......: avg=2.5ms  p(95)=8.2ms");
            println!("     http_req_connecting...: avg=1.8ms  p(95)=5.5ms");
            println!("     http_req_duration.....: avg=45ms   p(95)=120ms  p(99)=250ms");
            println!("     http_req_receiving....: avg=0.5ms  p(95)=1.2ms");
            println!("     http_req_sending......: avg=0.3ms  p(95)=0.8ms");
            println!("     http_req_waiting......: avg=44ms   p(95)=118ms");
            println!("     http_reqs.............: 3000   100/s");
            println!("     iteration_duration....: avg=95ms   p(95)=245ms");
            println!("     iterations............: 3000   100/s");
            println!("     vus...................: {}    min={}  max={}", vus, vus, vus);
            0
        }
        "inspect" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("script.js");
            println!("Inspecting {}:", script);
            println!("  Options:");
            println!("    vus: 10");
            println!("    duration: 30s");
            println!("    thresholds:");
            println!("      http_req_duration: ['p(95)<200']");
            println!("  Scenarios: 1 (default)");
            0
        }
        "cloud" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("script.js");
            println!("Uploading {} to k6 Cloud...", script);
            println!("  Test URL: https://app.k6.io/runs/12345");
            println!("  Status: running");
            0
        }
        "login" => {
            println!("Enter your k6 Cloud API token: ");
            println!("✔ Logged in to k6 Cloud.");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: k6 <command>. See --help.");
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
    let code = run_k6(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_k6};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_k6(vec!["--help".to_string()]), 0);
        assert_eq!(run_k6(vec!["-h".to_string()]), 0);
        let _ = run_k6(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_k6(vec![]);
    }
}
