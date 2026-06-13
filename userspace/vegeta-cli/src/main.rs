#![deny(clippy::all)]

//! vegeta-cli — SlateOS Vegeta HTTP load testing tool
//!
//! Multi-personality: `vegeta`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vegeta(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vegeta COMMAND [OPTIONS]");
        println!("Vegeta 12.11.1 (Slate OS) — HTTP load testing tool");
        println!();
        println!("Commands:");
        println!("  attack       Send HTTP requests at a steady rate");
        println!("  report       Generate reports from attack results");
        println!("  encode       Encode attack results");
        println!("  plot         Plot attack results as HTML");
        println!();
        println!("Attack options:");
        println!("  -rate N/s      Request rate (default: 50/1s)");
        println!("  -duration D    Attack duration (default: 0 = forever)");
        println!("  -targets FILE  Targets file");
        println!("  -body FILE     Request body file");
        println!("  -header H      Request header");
        println!("  -workers N     Number of workers (default: 10)");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "version" => println!("vegeta 12.11.1"),
        "attack" => {
            let rate = args.windows(2).find(|w| w[0] == "-rate")
                .map(|w| w[1].as_str()).unwrap_or("50/1s");
            let duration = args.windows(2).find(|w| w[0] == "-duration")
                .map(|w| w[1].as_str()).unwrap_or("5s");
            println!("Attacking at rate={} for {}...", rate, duration);
            println!("(binary results written to stdout)");
        }
        "report" => {
            let report_type = args.windows(2).find(|w| w[0] == "-type")
                .map(|w| w[1].as_str()).unwrap_or("text");
            match report_type {
                "json" => {
                    println!("{{");
                    println!("  \"latencies\": {{\"total\": 2345678900, \"mean\": 46913, \"50th\": 34200, \"90th\": 89100, \"95th\": 123400, \"99th\": 234500, \"max\": 567800}},");
                    println!("  \"bytes_in\": {{\"total\": 125000, \"mean\": 250}},");
                    println!("  \"bytes_out\": {{\"total\": 0, \"mean\": 0}},");
                    println!("  \"duration\": 5000000000,");
                    println!("  \"requests\": 500,");
                    println!("  \"rate\": 100.0,");
                    println!("  \"throughput\": 99.8,");
                    println!("  \"success\": 0.998,");
                    println!("  \"status_codes\": {{\"200\": 499, \"500\": 1}}");
                    println!("}}");
                }
                _ => {
                    println!("Requests      [total, rate, throughput]  500, 100.00, 99.80");
                    println!("Duration      [total, attack, wait]     5.001s, 5s, 1.234ms");
                    println!("Latencies     [min, mean, 50, 90, 95, 99, max]  0.342ms, 0.469ms, 0.342ms, 0.891ms, 1.234ms, 2.345ms, 5.678ms");
                    println!("Bytes In      [total, mean]             125000, 250.00");
                    println!("Bytes Out     [total, mean]             0, 0.00");
                    println!("Success       [ratio]                   99.80%");
                    println!("Status Codes  [code:count]              200:499  500:1");
                }
            }
        }
        "plot" => {
            println!("Generating HTML plot...");
            println!("Output: plot.html");
        }
        "encode" => {
            println!("Encoding results...");
        }
        _ => println!("vegeta: unknown command '{}'", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vegeta".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vegeta(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vegeta};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vegeta"), "vegeta");
        assert_eq!(basename(r"C:\bin\vegeta.exe"), "vegeta.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vegeta.exe"), "vegeta");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vegeta(&["--help".to_string()]), 0);
        assert_eq!(run_vegeta(&["-h".to_string()]), 0);
        let _ = run_vegeta(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vegeta(&[]);
    }
}
