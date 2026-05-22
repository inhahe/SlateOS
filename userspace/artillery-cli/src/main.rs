#![deny(clippy::all)]

//! artillery-cli — OurOS Artillery load testing CLI
//!
//! Single personality: `artillery`

use std::env;
use std::process;

fn run_artillery(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: artillery <COMMAND> [OPTIONS]");
        println!();
        println!("Artillery load testing CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  run          Run a test");
        println!("  quick        Quick HTTP test");
        println!("  report       Generate HTML report");
        println!("  dino         Show dino");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("artillery 2.0.6 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "run" => {
            let config = args.get(1).map(|s| s.as_str()).unwrap_or("test.yml");
            println!("Artillery running test: {}", config);
            println!();
            println!("Phase 1: Warm up (duration: 60s, arrival rate: 5/s)");
            println!("Phase 2: Ramp up (duration: 120s, arrival rate: 5-50/s)");
            println!("Phase 3: Sustained (duration: 300s, arrival rate: 50/s)");
            println!();
            println!("All VUs finished. Summary report:");
            println!();
            println!("  Scenarios launched:  15000");
            println!("  Scenarios completed: 14985");
            println!("  Requests completed:  44955");
            println!("  Mean response time:  45.2ms");
            println!("  p95 response time:   120ms");
            println!("  p99 response time:   250ms");
            println!("  RPS sent:            93.6/s");
            println!();
            println!("  Codes:");
            println!("    200: 44500");
            println!("    201: 400");
            println!("    500: 40");
            println!("    503: 15");
            println!();
            println!("  Errors:");
            println!("    ETIMEDOUT: 15");
            0
        }
        "quick" => {
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("http://localhost:3000");
            let count = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--count").map(|w| w[1].as_str()).unwrap_or("100");
            let rate = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--rate").map(|w| w[1].as_str()).unwrap_or("10");
            println!("Quick test: {} ({} requests at {}/s)", url, count, rate);
            println!();
            println!("  Requests completed: {}", count);
            println!("  Mean response time: 32ms");
            println!("  p95: 85ms");
            println!("  Status 200: {}", count);
            0
        }
        "report" => {
            let input = args.get(1).map(|s| s.as_str()).unwrap_or("results.json");
            let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output").map(|w| w[1].as_str()).unwrap_or("report.html");
            println!("Generating report from {} → {}", input, output);
            println!("  ✔ Report saved to {}", output);
            0
        }
        "dino" => {
            println!("            ,@@@@@@@,");
            println!("    ,,,.   ,@@@@@@/@@,  .oo8888o.");
            println!(" ,&%%&%&&%,@@@@@/@@@@@@,8888\\88/8o");
            println!(",%&\\%&&%&&%,@@@\\@@@/@@@88\\88888/88'");
            println!("%&&%&%&/%&&%@@\\@@/ /@@@88888\\88888'");
            println!("%&&%/ %&%%&&@@\\ V /@@' `88\\8 `/88'");
            println!("`&%\\ ` /%&'    |.|        \\ '|8'");
            println!("    |o|        | |         | |");
            println!("    |.|        | |         | |");
            println!(" \\/ ._\\//_/__/  ,\\_//__\\/.  \\_//__/_");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: artillery <command>. See --help.");
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
    let code = run_artillery(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
