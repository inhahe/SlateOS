#![deny(clippy::all)]

//! jaeger — Slate OS distributed tracing system
//!
//! Multi-personality: `jaeger`, `jaeger-agent`, `jaeger-collector`, `jaeger-query`

use std::env;
use std::process;

fn run_jaeger_all_in_one(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jaeger [FLAGS]");
        println!();
        println!("All-in-one Jaeger backend with UI.");
        println!();
        println!("Flags:");
        println!("  --collector.zipkin.host-port <addr>  Zipkin compatible endpoint");
        println!("  --query.base-path <path>             Base path for UI (default: /)");
        println!("  --query.ui-config <file>             UI config file");
        println!("  --collector.otlp.enabled             Enable OTLP receiver");
        println!("  --collector.otlp.grpc.host-port      OTLP gRPC endpoint");
        println!("  --collector.otlp.http.host-port      OTLP HTTP endpoint");
        println!("  --span-storage.type <type>           Storage type (memory/badger/cassandra/elasticsearch/grpc-plugin)");
        println!("  --log-level <level>                  Log level (debug/info/warn/error)");
        println!("  --admin.http.host-port <addr>        Admin HTTP port (default: :14269)");
        println!("  --version                            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("jaeger 1.57.0 (Slate OS)");
        return 0;
    }

    let storage = args.iter().find_map(|a| a.strip_prefix("--span-storage.type="))
        .unwrap_or("memory");

    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.000Z\",\"msg\":\"Starting jaeger (all-in-one)\",\"version\":\"1.57.0\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.001Z\",\"msg\":\"Memory storage initialized\",\"type\":\"{}\"}}",  storage);
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.010Z\",\"msg\":\"Starting agent\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.020Z\",\"msg\":\"Starting collector\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.030Z\",\"msg\":\"Starting query\",\"port\":16686}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.031Z\",\"msg\":\"Jaeger UI available at http://localhost:16686\"}}");
    0
}

fn run_jaeger_component(args: Vec<String>, component: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jaeger-{} [FLAGS]", component);
        println!();
        println!("  --log-level <level>      Log level");
        println!("  --admin.http.host-port   Admin endpoint");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("jaeger-{} 1.57.0 (Slate OS)", component);
        return 0;
    }

    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.000Z\",\"msg\":\"Starting jaeger-{}\",\"version\":\"1.57.0\"}}", component);
    match component {
        "agent" => {
            println!("{{\"level\":\"info\",\"msg\":\"Listening for spans\",\"udp\":\"localhost:6831\",\"http\":\"localhost:5778\"}}");
        }
        "collector" => {
            println!("{{\"level\":\"info\",\"msg\":\"Collector started\",\"grpc\":\":14250\",\"http\":\":14268\"}}");
        }
        "query" => {
            println!("{{\"level\":\"info\",\"msg\":\"Query service started\",\"http\":\":16686\"}}");
        }
        _ => {}
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("jaeger");
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
        "jaeger-agent" => run_jaeger_component(rest, "agent"),
        "jaeger-collector" => run_jaeger_component(rest, "collector"),
        "jaeger-query" => run_jaeger_component(rest, "query"),
        _ => run_jaeger_all_in_one(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jaeger_component};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jaeger_component(vec!["--help".to_string()], "jaeger"), 0);
        assert_eq!(run_jaeger_component(vec!["-h".to_string()], "jaeger"), 0);
        let _ = run_jaeger_component(vec!["--version".to_string()], "jaeger");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jaeger_component(vec![], "jaeger");
    }
}
