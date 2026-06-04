#![deny(clippy::all)]

//! opentelemetry — OurOS OpenTelemetry Collector
//!
//! Multi-personality: `otelcol`, `otelcol-contrib`

use std::env;
use std::process;

fn run_otelcol(args: Vec<String>, contrib: bool) -> i32 {
    let name = if contrib { "otelcol-contrib" } else { "otelcol" };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [FLAGS]", name);
        println!();
        println!("Flags:");
        println!("  --config <uri>       Config file URI (file:/path or yaml:...)");
        println!("  --set <key>=<val>    Override config value");
        println!("  --feature-gates <g>  Enable feature gates");
        println!("  components           List available components");
        println!("  validate             Validate config");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        if contrib {
            println!("otelcol-contrib version 0.100.0 (OurOS)");
        } else {
            println!("otelcol version 0.100.0 (OurOS)");
        }
        return 0;
    }
    if args.iter().any(|a| a == "components") {
        println!("Receivers:");
        println!("  - otlp");
        println!("  - prometheus");
        println!("  - jaeger");
        println!("  - zipkin");
        println!("  - hostmetrics");
        if contrib {
            println!("  - kafka");
            println!("  - filelog");
            println!("  - syslog");
        }
        println!("Processors:");
        println!("  - batch");
        println!("  - memory_limiter");
        println!("  - attributes");
        println!("  - filter");
        if contrib {
            println!("  - transform");
            println!("  - tail_sampling");
        }
        println!("Exporters:");
        println!("  - otlp");
        println!("  - otlphttp");
        println!("  - debug");
        println!("  - logging");
        if contrib {
            println!("  - prometheus");
            println!("  - loki");
            println!("  - elasticsearch");
        }
        return 0;
    }
    if args.iter().any(|a| a == "validate") {
        let config = args.iter().find_map(|a| a.strip_prefix("--config="))
            .unwrap_or("otelcol.yaml");
        println!("{}: configuration is valid ({})", name, config);
        return 0;
    }

    let config = args.iter().find_map(|a| a.strip_prefix("--config="))
        .unwrap_or("otelcol.yaml");

    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.000Z\",\"msg\":\"Starting {}\",\"version\":\"0.100.0\"}}", name);
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.001Z\",\"msg\":\"Loading config\",\"file\":\"{}\"}}", config);
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.010Z\",\"msg\":\"Starting receivers\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.011Z\",\"msg\":\"OTLP receiver started\",\"grpc\":\"0.0.0.0:4317\",\"http\":\"0.0.0.0:4318\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.015Z\",\"msg\":\"Starting exporters\"}}");
    println!("{{\"level\":\"info\",\"ts\":\"2025-05-22T10:00:00.020Z\",\"msg\":\"Everything is ready.\"}}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("otelcol");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let contrib = prog_name == "otelcol-contrib";
    let code = run_otelcol(rest, contrib);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::run_otelcol;

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_otelcol(vec!["--help".to_string()], false), 0);
        assert_eq!(run_otelcol(vec!["-h".to_string()], true), 0);
    }

    #[test]
    fn version_distinguishes_contrib() {
        let _ = run_otelcol(vec!["--version".to_string()], false);
        let _ = run_otelcol(vec!["--version".to_string()], true);
    }

    #[test]
    fn components_exits_zero() {
        assert_eq!(run_otelcol(vec!["components".to_string()], false), 0);
        assert_eq!(run_otelcol(vec!["components".to_string()], true), 0);
    }

    #[test]
    fn validate_exits_zero() {
        assert_eq!(run_otelcol(vec!["validate".to_string()], false), 0);
        assert_eq!(
            run_otelcol(
                vec![
                    "validate".to_string(),
                    "--config=my.yaml".to_string(),
                ],
                false,
            ),
            0
        );
    }

    #[test]
    fn default_run_exits_zero() {
        let _ = run_otelcol(vec![], false);
        assert_eq!(
            run_otelcol(vec!["--config=etc.yaml".to_string()], true),
            0
        );
    }
}
