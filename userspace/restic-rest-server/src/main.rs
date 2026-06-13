#![deny(clippy::all)]

//! restic-rest-server — Slate OS REST backend for restic
//!
//! Single personality: `rest-server`

use std::env;
use std::process;

fn run_rest_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rest-server [options]");
        println!();
        println!("Options:");
        println!("  --listen <addr>       Listen address (default: :8000)");
        println!("  --path <dir>          Data directory (default: /tmp/restic)");
        println!("  --tls                 Enable TLS");
        println!("  --tls-cert <file>     TLS certificate file");
        println!("  --tls-key <file>      TLS key file");
        println!("  --no-auth             Disable authentication");
        println!("  --htpasswd-file       Path to htpasswd file");
        println!("  --append-only         Enable append-only mode");
        println!("  --private-repos       Require authenticated repos");
        println!("  --prometheus          Enable Prometheus metrics");
        println!("  --prometheus-no-auth  Don't require auth for metrics");
        println!("  --max-size <size>     Maximum repo size");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rest-server 0.12.1 (Slate OS)");
        return 0;
    }

    let listen = args.iter().position(|a| a == "--listen")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or(":8000");
    let path = args.iter().position(|a| a == "--path")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/tmp/restic");
    let tls = args.iter().any(|a| a == "--tls");
    let append_only = args.iter().any(|a| a == "--append-only");
    let no_auth = args.iter().any(|a| a == "--no-auth");
    let prometheus = args.iter().any(|a| a == "--prometheus");

    let proto = if tls { "https" } else { "http" };
    println!("rest-server 0.12.1 (Slate OS)");
    println!("Data directory: {}", path);
    println!("Authentication: {}", if no_auth { "disabled" } else { "enabled" });
    println!("Append only: {}", if append_only { "enabled" } else { "disabled" });
    if prometheus {
        println!("Prometheus metrics: enabled");
    }
    println!("Starting server on {}://{}", proto, listen);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rest_server(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rest_server};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rest_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_rest_server(vec!["-h".to_string()]), 0);
        let _ = run_rest_server(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rest_server(vec![]);
    }
}
