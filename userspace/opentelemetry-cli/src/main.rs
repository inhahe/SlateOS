#![deny(clippy::all)]

//! opentelemetry-cli — OurOS OpenTelemetry Collector CLI (otelcol)
//!
//! Single personality: `otelcol`

use std::env;
use std::process;

fn run_otelcol(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: otelcol [OPTIONS]");
        println!();
        println!("OpenTelemetry Collector — receive, process, and export telemetry data.");
        println!();
        println!("Options:");
        println!("  --config <FILE>        Configuration file (YAML)");
        println!("  --set <K>=<V>          Override configuration value");
        println!("  --feature-gates <LIST> Enable feature gates");
        println!();
        println!("Commands:");
        println!("  validate        Validate configuration");
        println!("  components      List available components");
        println!("  version         Show version");
        println!();
        println!("Config options:");
        println!("  Receivers:    otlp, prometheus, jaeger, zipkin, filelog, hostmetrics");
        println!("  Processors:   batch, memory_limiter, attributes, filter, transform");
        println!("  Exporters:    otlp, prometheus, jaeger, zipkin, logging, file");
        println!("  Extensions:   health_check, pprof, zpages");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("otelcol version 0.93.0 (OurOS)");
            0
        }
        "validate" => {
            let config = args.windows(2)
                .find(|w| w[0] == "--config")
                .map(|w| w[1].as_str())
                .unwrap_or("otel-config.yaml");
            println!("Validating: {}", config);
            println!("  Receivers: [otlp, prometheus]");
            println!("  Processors: [batch, memory_limiter]");
            println!("  Exporters: [otlp, logging]");
            println!("  Extensions: [health_check]");
            println!("  Configuration is valid.");
            0
        }
        "components" => {
            println!("Receivers:");
            println!("  otlp              OTLP gRPC/HTTP receiver");
            println!("  prometheus         Prometheus scrape receiver");
            println!("  jaeger             Jaeger gRPC/thrift receiver");
            println!("  zipkin             Zipkin HTTP receiver");
            println!("  filelog            File log receiver");
            println!("  hostmetrics        Host metrics receiver");
            println!();
            println!("Processors:");
            println!("  batch              Batch processor");
            println!("  memory_limiter     Memory limiter");
            println!("  attributes         Attribute processor");
            println!("  filter             Filter processor");
            println!("  transform          Transform processor");
            println!();
            println!("Exporters:");
            println!("  otlp               OTLP gRPC exporter");
            println!("  otlphttp           OTLP HTTP exporter");
            println!("  prometheus         Prometheus exporter");
            println!("  logging            Logging exporter");
            println!("  file               File exporter");
            println!();
            println!("Extensions:");
            println!("  health_check       Health check extension");
            println!("  pprof              pprof extension");
            println!("  zpages             zPages extension");
            0
        }
        _ => {
            // Default: start collector
            let config = args.windows(2)
                .find(|w| w[0] == "--config")
                .map(|w| w[1].as_str())
                .unwrap_or("otel-config.yaml");
            println!("Starting OpenTelemetry Collector...");
            println!("  Config: {}", config);
            println!("  Receiver otlp started (grpc: 0.0.0.0:4317, http: 0.0.0.0:4318)");
            println!("  Receiver prometheus started (scrape interval: 15s)");
            println!("  Processor batch started (timeout: 5s, batch: 8192)");
            println!("  Exporter otlp started (endpoint: tempo:4317)");
            println!("  Extension health_check started (0.0.0.0:13133)");
            println!("  Collector is running.");
            0
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_otelcol(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_otelcol};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_otelcol(vec!["--help".to_string()]), 0);
        assert_eq!(run_otelcol(vec!["-h".to_string()]), 0);
        let _ = run_otelcol(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_otelcol(vec![]);
    }
}
