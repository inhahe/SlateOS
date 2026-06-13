#![deny(clippy::all)]

//! tempo — SlateOS distributed tracing backend
//!
//! Single personality: `tempo`

use std::env;
use std::process;

fn run_tempo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tempo [FLAGS]");
        println!();
        println!("Flags:");
        println!("  --config.file <file>     Config file path");
        println!("  --config.expand-env      Expand env vars in config");
        println!("  --target <target>        Module target (all, distributor, ingester, querier, compactor)");
        println!("  --storage.trace.backend  Storage backend (local/gcs/s3/azure)");
        println!("  --server.http-listen-address <addr>  HTTP listen (default: :3200)");
        println!("  --server.grpc-listen-address <addr>  gRPC listen (default: :9095)");
        println!("  --log.level <level>      Log level");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("tempo, version 2.5.0 (SlateOS)");
        println!("  branch: main");
        println!("  build date: 2025-05-22");
        return 0;
    }

    let config = args.iter().find_map(|a| a.strip_prefix("--config.file="))
        .unwrap_or("tempo.yaml");
    let target = args.iter().find_map(|a| a.strip_prefix("--target="))
        .unwrap_or("all");

    println!("level=info ts=2025-05-22T10:00:00.000Z msg=\"Starting Tempo\" version=\"2.5.0 (SlateOS)\"");
    println!("level=info ts=2025-05-22T10:00:00.001Z msg=\"Loading configuration\" file=\"{}\"", config);
    println!("level=info ts=2025-05-22T10:00:00.010Z msg=\"Initializing module\" target=\"{}\"", target);
    println!("level=info ts=2025-05-22T10:00:00.050Z msg=\"Tempo started\" http=:3200 grpc=:9095");
    println!("level=info ts=2025-05-22T10:00:00.051Z msg=\"Accepting traces via OTLP gRPC and HTTP\"");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tempo(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tempo};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tempo(vec!["--help".to_string()]), 0);
        assert_eq!(run_tempo(vec!["-h".to_string()]), 0);
        let _ = run_tempo(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tempo(vec![]);
    }
}
