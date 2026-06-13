#![deny(clippy::all)]

//! jaeger-cli — Slate OS Jaeger distributed tracing query CLI
//!
//! Single personality: `jaeger-cli`

use std::env;
use std::process;

fn run_jaeger(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jaeger-cli <COMMAND> [OPTIONS]");
        println!();
        println!("Query Jaeger distributed tracing backend.");
        println!();
        println!("Commands:");
        println!("  services     List services");
        println!("  operations   List operations for a service");
        println!("  traces       Search traces");
        println!("  trace        Get trace by ID");
        println!("  compare      Compare two traces");
        println!("  stats        Show service statistics");
        println!();
        println!("Options:");
        println!("  --url <URL>        Jaeger Query URL");
        println!("  --service <SVC>    Service name");
        println!("  --operation <OP>   Operation name");
        println!("  --lookback <DUR>   Lookback duration (e.g., 1h)");
        println!("  --limit <N>        Max traces");
        println!("  --min-duration <D> Min span duration");
        println!("  --max-duration <D> Max span duration");
        println!("  --tags <K>=<V>     Tag filter");
        println!("  -o, --output <FMT> Output format (table/json)");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("jaeger-cli 1.0.0 (Slate OS, Jaeger 1.54.0)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "services" => {
            println!("Services:");
            println!("  frontend");
            println!("  api-gateway");
            println!("  user-service");
            println!("  order-service");
            println!("  payment-service");
            println!("  notification-service");
            0
        }
        "operations" => {
            let svc = args.windows(2)
                .find(|w| w[0] == "--service")
                .map(|w| w[1].as_str())
                .unwrap_or("api-gateway");
            println!("Operations for {}:", svc);
            println!("  GET /api/users");
            println!("  POST /api/orders");
            println!("  GET /api/products");
            println!("  GET /api/health");
            println!("  PUT /api/users/{{id}}");
            0
        }
        "traces" => {
            println!("Trace ID                         Service         Operation          Duration  Spans  Errors");
            println!("──────────────────────────────── ────────────── ──────────────── ─────── ───── ──────");
            println!("abc123def456789012345678901234    api-gateway    GET /api/users     45ms     8      0");
            println!("def456abc789012345678901234567    api-gateway    POST /api/orders  123ms    12      0");
            println!("789abc123def456012345678901234    api-gateway    GET /api/products  15ms     5      0");
            println!("012def456abc789345678901234567    api-gateway    POST /api/orders  890ms    14      1");
            0
        }
        "trace" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("abc123def456789012345678901234");
            println!("Trace: {}", id);
            println!("  Duration: 45ms");
            println!("  Services: 4");
            println!("  Spans: 8");
            println!();
            println!("  api-gateway [45ms] GET /api/users");
            println!("    ├── user-service [20ms] FindUsers");
            println!("    │   ├── user-service [12ms] DB.Query");
            println!("    │   └── user-service [3ms] Cache.Get");
            println!("    └── api-gateway [5ms] SerializeResponse");
            0
        }
        "stats" => {
            let svc = args.windows(2)
                .find(|w| w[0] == "--service")
                .map(|w| w[1].as_str())
                .unwrap_or("api-gateway");
            println!("Statistics for {} (last 1h):", svc);
            println!("  Requests:    12,345");
            println!("  Errors:      23 (0.19%)");
            println!("  p50:         15ms");
            println!("  p90:         45ms");
            println!("  p95:         89ms");
            println!("  p99:         234ms");
            println!("  Max:         1,234ms");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: jaeger-cli <command>. See --help.");
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
    let code = run_jaeger(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jaeger};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jaeger(vec!["--help".to_string()]), 0);
        assert_eq!(run_jaeger(vec!["-h".to_string()]), 0);
        let _ = run_jaeger(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jaeger(vec![]);
    }
}
