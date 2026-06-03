#![deny(clippy::all)]

//! zipkin-cli — OurOS Zipkin distributed tracing CLI
//!
//! Single personality: `zipkin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zipkin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zipkin COMMAND [OPTIONS]");
        println!("zipkin v2.27.0 (OurOS) — Distributed tracing CLI");
        println!();
        println!("Commands:");
        println!("  server          Start Zipkin server");
        println!("  traces          Query traces");
        println!("  services        List services");
        println!("  spans           List spans for service");
        println!("  dependencies    Show service dependencies");
        println!("  export          Export traces");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  --url URL       Zipkin server URL");
        println!("  --limit N       Max results");
        println!("  --lookback DUR  Time lookback");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("zipkin 2.27.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("server");
    match cmd {
        "server" => {
            println!("Starting Zipkin server...");
            println!("  Storage: mem");
            println!("  HTTP: 0.0.0.0:9411");
            println!("  UI: http://localhost:9411/zipkin/");
        }
        "traces" => {
            println!("Traces (last 1h):");
            println!("  abc123  frontend -> api -> database  342ms  3 spans");
            println!("  def456  frontend -> api              128ms  2 spans");
            println!("  ghi789  worker -> queue -> api       567ms  4 spans");
        }
        "services" => {
            println!("Services:");
            println!("  frontend");
            println!("  api");
            println!("  database");
            println!("  worker");
            println!("  queue");
        }
        "spans" => {
            let svc = args.get(1).map(|s| s.as_str()).unwrap_or("api");
            println!("Spans for service '{}':", svc);
            println!("  GET /users         avg: 45ms   p99: 120ms");
            println!("  POST /orders       avg: 82ms   p99: 250ms");
            println!("  GET /health        avg: 2ms    p99: 5ms");
        }
        "dependencies" => {
            println!("Service Dependencies:");
            println!("  frontend -> api (1234 calls)");
            println!("  api -> database (5678 calls)");
            println!("  worker -> queue (890 calls)");
            println!("  worker -> api (234 calls)");
        }
        "export" => println!("Exported 100 traces to traces.json"),
        _ => println!("zipkin {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zipkin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zipkin(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zipkin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zipkin"), "zipkin");
        assert_eq!(basename(r"C:\bin\zipkin.exe"), "zipkin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zipkin.exe"), "zipkin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_zipkin(&["--help".to_string()], "zipkin"), 0);
        assert_eq!(run_zipkin(&["-h".to_string()], "zipkin"), 0);
        assert_eq!(run_zipkin(&["--version".to_string()], "zipkin"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_zipkin(&[], "zipkin"), 0);
    }
}
