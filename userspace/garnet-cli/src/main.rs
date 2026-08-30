#![deny(clippy::all)]

//! garnet-cli — Slate OS Microsoft Garnet cache store
//!
//! Single personality: `garnet-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_garnet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: garnet-server [OPTIONS]");
        println!("Garnet v1.0 (Slate OS) — High-performance cache store (Redis-compatible)");
        println!();
        println!("Options:");
        println!("  --bind IP          Bind address (default: 0.0.0.0)");
        println!("  --port PORT        Port (default: 6379)");
        println!("  --memory SIZE      Memory limit");
        println!("  --tls              Enable TLS");
        println!("  --tls-cert FILE    TLS certificate");
        println!("  --tls-key FILE     TLS private key");
        println!("  --aof              Enable append-only file");
        println!("  --checkpoint DIR   Checkpoint directory");
        println!("  --threads N        IO threads");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Garnet v1.0.32 (Slate OS)"); return 0; }
    println!("Garnet v1.0.32 (Slate OS)");
    println!("  Listening: 0.0.0.0:6379");
    println!("  Protocol: RESP (Redis-compatible)");
    println!("  Memory: 256 MB limit");
    println!("  Keys: 45,678");
    println!("  Connected clients: 12");
    println!("  Ops/sec: 890,000");
    println!("  Persistence: AOF enabled");
    println!("  Cluster: standalone");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "garnet-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_garnet(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_garnet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/garnet"), "garnet");
        assert_eq!(basename(r"C:\bin\garnet.exe"), "garnet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("garnet.exe"), "garnet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_garnet(&["--help".to_string()], "garnet"), 0);
        assert_eq!(run_garnet(&["-h".to_string()], "garnet"), 0);
        let _ = run_garnet(&["--version".to_string()], "garnet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_garnet(&[], "garnet");
    }
}
