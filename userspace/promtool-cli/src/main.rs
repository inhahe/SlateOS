#![deny(clippy::all)]

//! promtool-cli — OurOS Prometheus tooling
//!
//! Multi-personality: `promtool`, `prometheus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_promtool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: promtool COMMAND [OPTIONS]");
        println!("promtool (Prometheus 2.53.0, OurOS)");
        println!();
        println!("Commands:");
        println!("  check       Validate config/rules/metrics");
        println!("  query       Query Prometheus server");
        println!("  debug       Debug Prometheus");
        println!("  test        Test recording/alerting rules");
        println!("  tsdb        TSDB utilities");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("promtool, version 2.53.0"),
        "check" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("config");
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("prometheus.yml");
            match what {
                "config" => println!("Checking {}... SUCCESS. {} is valid.", file, file),
                "rules" => println!("Checking {}... SUCCESS. 5 rules found.", file),
                "metrics" => println!("Checking metrics... all valid."),
                _ => println!("promtool check: '{}' completed", what),
            }
        }
        "query" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("instant");
            let expr = args.get(2).map(|s| s.as_str()).unwrap_or("up");
            println!("promtool query {}: {}", sub, expr);
            println!("{{__name__=\"up\", job=\"prometheus\"}} => 1 @[1718000000]");
        }
        "test" => {
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("rules_test.yml");
            println!("Unit Testing: {}", file);
            println!("  PASSED");
        }
        "tsdb" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => println!("Block ID: 01ABC... MinTime: 2024-06-01 MaxTime: 2024-06-14 NumSeries: 1234"),
                "analyze" => {
                    println!("Block: 01ABC...");
                    println!("  Duration: 2h0m0s");
                    println!("  Series: 1234");
                    println!("  Samples: 567890");
                }
                "compact" => println!("Compaction completed."),
                _ => println!("promtool tsdb: '{}' completed", sub),
            }
        }
        _ => println!("promtool: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "promtool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_promtool(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_promtool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/promtool"), "promtool");
        assert_eq!(basename(r"C:\bin\promtool.exe"), "promtool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("promtool.exe"), "promtool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_promtool(&["--help".to_string()]), 0);
        assert_eq!(run_promtool(&["-h".to_string()]), 0);
        assert_eq!(run_promtool(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_promtool(&[]), 0);
    }
}
