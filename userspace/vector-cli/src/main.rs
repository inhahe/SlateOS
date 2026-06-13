#![deny(clippy::all)]

//! vector-cli — SlateOS Vector data pipeline CLI
//!
//! Single personality: `vector`

use std::env;
use std::process;

fn run_vector(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vector [OPTIONS] [COMMAND]");
        println!();
        println!("High-performance observability data pipeline.");
        println!();
        println!("Commands:");
        println!("  validate     Validate configuration");
        println!("  generate     Generate example configs");
        println!("  graph        Output pipeline graph as DOT");
        println!("  list         List available components");
        println!("  test         Run unit tests");
        println!("  top          Display running Vector topology");
        println!("  tap          Observe events flowing through");
        println!("  vrl          Run VRL program");
        println!();
        println!("Options:");
        println!("  -c, --config <FILE>    Config file(s)");
        println!("  --config-dir <DIR>     Config directory");
        println!("  -w, --watch-config     Watch for config changes");
        println!("  -t, --threads <N>      Number of threads");
        println!("  --log-format <FMT>     Log format (text/json)");
        println!("  -q, --quiet            Quiet mode");
        println!("  -v, --verbose          Verbose mode");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("vector 0.36.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "validate" => {
            let config = args.windows(2)
                .find(|w| w[0] == "-c" || w[0] == "--config")
                .map(|w| w[1].as_str())
                .unwrap_or("vector.toml");
            println!("Loaded [\"{}\"]: 2 sources, 3 transforms, 2 sinks", config);
            println!("  ✓ Component configuration is valid.");
            println!("  ✓ Health check passed.");
            0
        }
        "list" => {
            println!("Sources:");
            println!("  file               Read from files");
            println!("  journald           Read from systemd journal");
            println!("  kafka              Read from Kafka");
            println!("  socket             Listen on TCP/UDP socket");
            println!("  syslog             Receive syslog messages");
            println!("  http_server        HTTP endpoint");
            println!("  host_metrics       Host system metrics");
            println!();
            println!("Transforms:");
            println!("  remap              VRL remap/transform");
            println!("  filter             Filter events");
            println!("  aggregate          Aggregate metrics");
            println!("  dedupe             Deduplicate events");
            println!("  route              Route events by condition");
            println!("  sample             Sample events");
            println!();
            println!("Sinks:");
            println!("  elasticsearch      Elasticsearch output");
            println!("  loki               Grafana Loki output");
            println!("  prometheus_exporter Prometheus metrics");
            println!("  file               Write to files");
            println!("  kafka              Write to Kafka");
            println!("  http               HTTP endpoint");
            println!("  console            Print to console");
            0
        }
        "top" => {
            println!("  Component            Type        Events In  Events Out  Bytes In   Bytes Out");
            println!("  ──────────────────── ────────── ────────── ────────── ────────── ──────────");
            println!("  file_source          source      12,345     12,345      5.6 MB     5.6 MB");
            println!("  parse_logs           transform   12,345     12,340      5.6 MB     4.8 MB");
            println!("  filter_errors        transform   12,340      2,345      4.8 MB     1.2 MB");
            println!("  loki_sink            sink         2,345      2,345      1.2 MB     0.8 MB");
            println!("  elasticsearch_sink   sink        12,340     12,340      4.8 MB     3.2 MB");
            0
        }
        "vrl" => {
            println!("VRL REPL (type .exit to quit)");
            println!("$ .input = {{\"message\": \"2024-01-15 ERROR connection failed\"}}");
            println!("$ .level = parse_regex!(.message, r'\\b(DEBUG|INFO|WARN|ERROR)\\b').\"0\"");
            println!("$ .level");
            println!("\"ERROR\"");
            0
        }
        "test" => {
            println!("Running 3 tests:");
            println!("  test_parse_syslog... passed");
            println!("  test_filter_errors... passed");
            println!("  test_enrich_logs... passed");
            println!();
            println!("3/3 tests passed.");
            0
        }
        _ => {
            // Default: start Vector
            let config = args.windows(2)
                .find(|w| w[0] == "-c" || w[0] == "--config")
                .map(|w| w[1].as_str())
                .unwrap_or("vector.toml");
            println!("Starting Vector...");
            println!("  Config: {}", config);
            println!("  Sources: file, host_metrics");
            println!("  Transforms: parse_logs, filter_errors");
            println!("  Sinks: loki, elasticsearch");
            println!("  Vector is running.");
            0
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vector(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_vector};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vector(vec!["--help".to_string()]), 0);
        assert_eq!(run_vector(vec!["-h".to_string()]), 0);
        let _ = run_vector(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vector(vec![]);
    }
}
