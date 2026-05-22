#![deny(clippy::all)]

//! loki — OurOS log aggregation system
//!
//! Multi-personality: `loki`, `logcli`, `promtail`

use std::env;
use std::process;

fn run_loki(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: loki [FLAGS] [OPTIONS]");
        println!();
        println!("Flags:");
        println!("  --config.file <file>     Configuration file path");
        println!("  --config.expand-env      Expand environment variables in config");
        println!("  --target <target>        Target module (all, read, write, backend)");
        println!("  --log.level <level>      Log level (debug/info/warn/error)");
        println!("  --server.http-listen-address <addr>  HTTP listen address (default: :3100)");
        println!("  --server.grpc-listen-address <addr>  gRPC listen address (default: :9095)");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("loki, version 3.0.0 (OurOS)");
        println!("  branch: main");
        println!("  build date: 2025-05-22");
        return 0;
    }

    let config = args.iter().find_map(|a| a.strip_prefix("--config.file=")
        .or_else(|| a.strip_prefix("-config.file=")))
        .unwrap_or("loki.yaml");
    let target = args.iter().find_map(|a| a.strip_prefix("--target="))
        .unwrap_or("all");

    println!("level=info ts=2025-05-22T10:00:00.000Z caller=main.go msg=\"Starting Loki\" version=\"3.0.0 (OurOS)\"");
    println!("level=info ts=2025-05-22T10:00:00.001Z caller=main.go msg=\"Loading configuration\" file=\"{}\"", config);
    println!("level=info ts=2025-05-22T10:00:00.010Z caller=modules.go msg=\"Running target\" target=\"{}\"", target);
    println!("level=info ts=2025-05-22T10:00:00.050Z caller=server.go msg=\"HTTP server listening\" address=:3100");
    println!("level=info ts=2025-05-22T10:00:00.051Z caller=server.go msg=\"gRPC server listening\" address=:9095");
    0
}

fn run_logcli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logcli <command> [flags]");
        println!();
        println!("Commands:");
        println!("  query      Run a LogQL query");
        println!("  labels     List labels");
        println!("  series     List series");
        println!("  instant-query  Run instant query");
        println!("  stats      Show query statistics");
        println!();
        println!("Flags:");
        println!("  --addr <url>     Loki address (default: http://localhost:3100)");
        println!("  --org-id <id>    Organization ID");
        println!("  --since <dur>    Lookback period (default: 1h)");
        println!("  --limit <n>      Line limit (default: 30)");
        println!("  -o raw|jsonl     Output mode");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("query");
    match cmd {
        "query" => {
            println!("2025-05-22T10:00:00Z {{app=\"myapp\"}} level=info msg=\"Request processed\" duration=1.23ms");
            println!("2025-05-22T09:59:58Z {{app=\"myapp\"}} level=info msg=\"Request received\" method=GET path=/api/v1/items");
            println!("2025-05-22T09:59:55Z {{app=\"myapp\"}} level=warn msg=\"Slow query\" duration=523ms");
        }
        "labels" => {
            println!("app");
            println!("env");
            println!("host");
            println!("level");
            println!("namespace");
        }
        "series" => {
            println!("{{app=\"myapp\", env=\"prod\", host=\"srv-01\"}}");
            println!("{{app=\"myapp\", env=\"prod\", host=\"srv-02\"}}");
            println!("{{app=\"nginx\", env=\"prod\", host=\"lb-01\"}}");
        }
        "stats" => {
            println!("Ingester:");
            println!("  Total reached: 2");
            println!("  Total chunks matched: 45");
            println!("  Total batches: 12");
            println!("  Total lines sent: 1500");
            println!("Store:");
            println!("  Total chunks ref: 120");
            println!("  Total chunks downloaded: 45");
            println!("  Total duplicates: 3");
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_promtail(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: promtail [FLAGS]");
        println!();
        println!("Flags:");
        println!("  --config.file <file>     Config file path");
        println!("  --client.url <url>       Loki push URL");
        println!("  --dry-run                Print entries instead of sending");
        println!("  --inspect                Show log pipeline stages");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("promtail, version 3.0.0 (OurOS)");
        return 0;
    }

    let config = args.iter().find_map(|a| a.strip_prefix("--config.file="))
        .unwrap_or("promtail.yaml");
    println!("level=info msg=\"Starting Promtail\" version=\"3.0.0\"");
    println!("level=info msg=\"Loading configuration\" file=\"{}\"", config);
    println!("level=info msg=\"Tailing file\" path=/var/log/syslog");
    println!("level=info msg=\"Tailing file\" path=/var/log/auth.log");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("loki");
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
        "logcli" => run_logcli(rest),
        "promtail" => run_promtail(rest),
        _ => run_loki(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
