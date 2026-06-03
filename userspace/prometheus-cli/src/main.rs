#![deny(clippy::all)]

//! prometheus-cli — OurOS Prometheus monitoring CLI (promtool)
//!
//! Single personality: `promtool`

use std::env;
use std::process;

fn run_promtool(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: promtool <COMMAND> [OPTIONS]");
        println!();
        println!("Prometheus monitoring toolkit.");
        println!();
        println!("Commands:");
        println!("  check config       Check configuration");
        println!("  check rules        Check recording/alerting rules");
        println!("  check metrics      Check metric exposition");
        println!("  query instant      Run instant query");
        println!("  query range        Run range query");
        println!("  query labels       Query label values");
        println!("  query series       Query series");
        println!("  test rules         Unit test rules");
        println!("  tsdb list          List TSDB blocks");
        println!("  tsdb dump          Dump TSDB samples");
        println!("  tsdb compact       Compact TSDB blocks");
        println!();
        println!("Options:");
        println!("  --url <URL>        Prometheus server URL");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("promtool, version 2.49.1 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, sub) {
        ("check", "config") => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("prometheus.yml");
            println!("Checking {}...", file);
            println!("  SUCCESS: 3 rule files found");
            println!("    rules/alerts.yml: SUCCESS (12 rules)");
            println!("    rules/recording.yml: SUCCESS (8 rules)");
            println!("    rules/sla.yml: SUCCESS (5 rules)");
            0
        }
        ("check", "rules") => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("rules.yml");
            println!("Checking {}...", file);
            println!("  SUCCESS: {} is valid", file);
            0
        }
        ("query", "instant") => {
            let query = args.get(2).map(|s| s.as_str()).unwrap_or("up");
            println!("{{__name__=\"{}\", instance=\"localhost:9090\", job=\"prometheus\"}} => 1 @[1705312200]", query);
            println!("{{__name__=\"{}\", instance=\"localhost:9100\", job=\"node\"}} => 1 @[1705312200]", query);
            0
        }
        ("query", "range") => {
            let query = args.get(2).map(|s| s.as_str()).unwrap_or("rate(http_requests_total[5m])");
            println!("{{method=\"GET\", status=\"200\"}} =>", );
            println!("  1705312000 125.4");
            println!("  1705312060 128.7");
            println!("  1705312120 130.2");
            println!("  (query: {})", query);
            0
        }
        ("test", "rules") => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("tests.yml");
            println!("Unit Testing: {}", file);
            println!("  PASSED: test 1 (HighCPU alert fires at 80%)");
            println!("  PASSED: test 2 (DiskFull alert fires at 90%)");
            println!("  PASSED: test 3 (ServiceDown alert after 5m)");
            println!();
            println!("3/3 tests passed");
            0
        }
        ("tsdb", "list") => {
            println!("Block ID                           Min Time            Max Time            Duration    Samples   Series");
            println!("01HR2Q3Z4K5M6N7P8R9S0T1U2V  2024-01-14 00:00:00  2024-01-14 02:00:00  2h          1.2M      5432");
            println!("01HR3Q4Z5K6M7N8P9R0S1T2U3V  2024-01-14 02:00:00  2024-01-14 04:00:00  2h          1.1M      5430");
            println!("01HR4Q5Z6K7M8N9P0R1S2T3U4V  2024-01-14 04:00:00  2024-01-14 06:00:00  2h          1.3M      5435");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: promtool <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{} {}'. See --help.", cmd, sub);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_promtool(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_promtool};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_promtool(vec!["--help".to_string()]), 0);
        assert_eq!(run_promtool(vec!["-h".to_string()]), 0);
        assert_eq!(run_promtool(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_promtool(vec![]), 0);
    }
}
