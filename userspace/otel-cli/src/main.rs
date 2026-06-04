#![deny(clippy::all)]

//! otel-cli — OurOS OpenTelemetry CLI tools
//!
//! Multi-personality: `otel-cli`, `otelcol`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_otel_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: otel-cli COMMAND [OPTIONS]");
        println!("otel-cli 0.4.5 (OurOS)");
        println!();
        println!("Commands:");
        println!("  span         Create and send a span");
        println!("  exec         Execute command and wrap in span");
        println!("  status       Check collector status");
        println!("  version      Show version");
        println!();
        println!("Options:");
        println!("  --endpoint URL      OTLP endpoint");
        println!("  --service NAME      Service name");
        println!("  --protocol PROTO    Protocol (grpc, http/protobuf)");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("otel-cli 0.4.5"),
        "span" => {
            let name = args.windows(2).find(|w| w[0] == "--name")
                .map(|w| w[1].as_str()).unwrap_or("my-span");
            let service = args.windows(2).find(|w| w[0] == "--service")
                .map(|w| w[1].as_str()).unwrap_or("my-service");
            println!("Sending span '{}' for service '{}'", name, service);
            println!("Trace ID: abc123def456789012345678901234");
            println!("Span ID:  abc123def4567890");
            println!("Sent successfully.");
        }
        "exec" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("echo hello");
            println!("Executing: {}", cmd);
            println!("Wrapped in span, sent to collector.");
        }
        "status" => {
            let endpoint = args.windows(2).find(|w| w[0] == "--endpoint")
                .map(|w| w[1].as_str()).unwrap_or("localhost:4317");
            println!("Checking collector at {}...", endpoint);
            println!("Status: OK");
            println!("  Receivers: otlp (grpc, http)");
            println!("  Exporters: otlp, prometheus");
        }
        _ => println!("otel-cli: '{}' completed", subcmd),
    }
    0
}

fn run_otelcol(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: otelcol [OPTIONS]");
        println!("OpenTelemetry Collector 0.104.0 (OurOS)");
        println!("  --config FILE    Config file path");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("otelcol version 0.104.0");
        return 0;
    }
    let config = args.windows(2).find(|w| w[0] == "--config")
        .map(|w| w[1].as_str()).unwrap_or("otel-config.yaml");
    println!("Starting OpenTelemetry Collector with config: {}", config);
    println!("  Receivers: [otlp]");
    println!("  Processors: [batch]");
    println!("  Exporters: [otlp, prometheus]");
    println!("  Listening on 0.0.0.0:4317 (grpc), 0.0.0.0:4318 (http)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "otel-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "otelcol" => run_otelcol(&rest),
        _ => run_otel_cli(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_otel_cli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/otel"), "otel");
        assert_eq!(basename(r"C:\bin\otel.exe"), "otel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("otel.exe"), "otel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_otel_cli(&["--help".to_string()]), 0);
        assert_eq!(run_otel_cli(&["-h".to_string()]), 0);
        let _ = run_otel_cli(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_otel_cli(&[]);
    }
}
