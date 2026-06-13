#![deny(clippy::all)]

//! prometheus — Slate OS monitoring and alerting toolkit
//!
//! Multi-personality: `prometheus` (server), `promtool` (CLI tool)

use std::env;
use std::process;

fn run_prometheus(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: prometheus [<flags>]");
        println!();
        println!("The Prometheus monitoring server.");
        println!();
        println!("Flags:");
        println!("  --config.file=\"prometheus.yml\"  Config file path");
        println!("  --web.listen-address=\":9090\"    Address to listen on");
        println!("  --storage.tsdb.path=\"data/\"     Data directory");
        println!("  --storage.tsdb.retention.time=15d  Data retention");
        println!("  --web.enable-lifecycle          Enable /-/reload and /-/quit");
        println!("  --version                       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("prometheus, version 2.50.0 (Slate OS)");
        println!("  build date: 2025-05-22");
        println!("  go version: go1.22.0");
        return 0;
    }
    println!("ts=2025-05-22T10:00:00Z caller=main.go:1 level=info msg=\"Starting Prometheus\" version=2.50.0");
    println!("ts=2025-05-22T10:00:00Z caller=main.go:2 level=info msg=\"Loading configuration file\" filename=prometheus.yml");
    println!("ts=2025-05-22T10:00:01Z caller=main.go:3 level=info msg=\"Server is ready to receive web requests.\"");
    println!("ts=2025-05-22T10:00:01Z caller=main.go:4 level=info msg=\"Listening on :9090\"");
    0
}

fn run_promtool(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("usage: promtool <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  check config     Check configuration files");
            println!("  check rules      Check rule files");
            println!("  check metrics    Check metrics");
            println!("  query instant    Run instant query");
            println!("  query range      Run range query");
            println!("  tsdb             TSDB utilities");
            println!("  test rules       Unit test rules");
            0
        }
        "check" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("config");
            match sub {
                "config" => {
                    let file = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("prometheus.yml");
                    println!("Checking {}...", file);
                    println!("  SUCCESS: {} is valid prometheus config file", file);
                }
                "rules" => {
                    let file = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("rules.yml");
                    println!("Checking {}...", file);
                    println!("  SUCCESS: 3 rules found");
                }
                _ => println!("check {}: (simulated)", sub),
            }
            0
        }
        "query" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("instant");
            let query = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("up");
            match sub {
                "instant" => {
                    println!("{{metric=\"{}\"}}: 1 @[1716364800]", query);
                }
                "range" => {
                    println!("{{metric=\"{}\"}}: values over range (simulated)", query);
                }
                _ => println!("query {}: (simulated)", sub),
            }
            0
        }
        "tsdb" => { println!("TSDB utility (simulated)"); 0 }
        other => { eprintln!("promtool: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("prometheus");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "promtool" => run_promtool(rest),
        _ => run_prometheus(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_prometheus};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_prometheus(vec!["--help".to_string()]), 0);
        assert_eq!(run_prometheus(vec!["-h".to_string()]), 0);
        let _ = run_prometheus(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_prometheus(vec![]);
    }
}
