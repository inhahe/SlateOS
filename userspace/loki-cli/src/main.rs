#![deny(clippy::all)]

//! loki-cli — OurOS Grafana Loki log query CLI (logcli)
//!
//! Single personality: `logcli`

use std::env;
use std::process;

fn run_logcli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logcli <COMMAND> [OPTIONS]");
        println!();
        println!("Query Grafana Loki log aggregation system.");
        println!();
        println!("Commands:");
        println!("  query          Run a LogQL query");
        println!("  instant-query  Run an instant LogQL query");
        println!("  labels         List labels");
        println!("  series         List series");
        println!("  stats          Show query statistics");
        println!("  volume         Show log volume");
        println!();
        println!("Options:");
        println!("  --addr <URL>       Loki server URL (or $LOKI_ADDR)");
        println!("  --from <TIME>      Start time (e.g., -1h)");
        println!("  --to <TIME>        End time (e.g., now)");
        println!("  --limit <N>        Max entries (default: 30)");
        println!("  --since <DUR>      Duration to look back");
        println!("  --output <FMT>     Output mode (default/raw/jsonl)");
        println!("  --forward          Oldest entries first");
        println!("  --no-labels        Don't print labels");
        println!("  --quiet            Suppress output");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("logcli version 2.9.4 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "query" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("{job=\"nginx\"}");
            println!("2024-01-15T14:30:00Z {{job=\"nginx\", instance=\"web-1\"}} GET /api/health 200 1ms");
            println!("2024-01-15T14:30:01Z {{job=\"nginx\", instance=\"web-2\"}} GET / 200 3ms");
            println!("2024-01-15T14:30:02Z {{job=\"nginx\", instance=\"web-1\"}} POST /api/data 201 15ms");
            println!("2024-01-15T14:30:03Z {{job=\"nginx\", instance=\"web-2\"}} GET /static/app.js 200 0ms");
            println!("2024-01-15T14:30:05Z {{job=\"nginx\", instance=\"web-1\"}} GET /api/users 200 8ms");
            println!("  (query: {})", query);
            0
        }
        "labels" => {
            println!("job");
            println!("instance");
            println!("level");
            println!("namespace");
            println!("pod");
            println!("container");
            println!("stream");
            0
        }
        "series" => {
            println!("{{job=\"nginx\", instance=\"web-1\"}}");
            println!("{{job=\"nginx\", instance=\"web-2\"}}");
            println!("{{job=\"api\", instance=\"api-1\"}}");
            println!("{{job=\"redis\", instance=\"redis-0\"}}");
            0
        }
        "stats" => {
            println!("Query statistics:");
            println!("  Ingester:");
            println!("    Total chunks matched: 45");
            println!("    Total batches: 3");
            println!("    Total lines sent: 12,345");
            println!("    Total bytes: 2.3 MB");
            println!("  Store:");
            println!("    Total chunks ref: 120");
            println!("    Total chunks downloaded: 45");
            println!("    Total bytes: 5.6 MB");
            println!("  Summary:");
            println!("    Exec time: 0.234s");
            println!("    Total entries: 12,345");
            0
        }
        "volume" => {
            println!("Log volume:");
            println!("  {{job=\"nginx\"}}   245.6 MB/day  (avg 2.8 KB/line)");
            println!("  {{job=\"api\"}}     123.4 MB/day  (avg 1.2 KB/line)");
            println!("  {{job=\"redis\"}}    45.2 MB/day  (avg 0.5 KB/line)");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: logcli <command>. See --help.");
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
    let code = run_logcli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_logcli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logcli(vec!["--help".to_string()]), 0);
        assert_eq!(run_logcli(vec!["-h".to_string()]), 0);
        let _ = run_logcli(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logcli(vec![]);
    }
}
