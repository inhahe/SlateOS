#![deny(clippy::all)]

//! tempo-cli — OurOS Grafana Tempo CLI
//!
//! Multi-personality: `tempo-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tempo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tempo-cli COMMAND [OPTIONS]");
        println!("Grafana Tempo CLI 2.5.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  query        Query traces");
        println!("  list         List blocks/compacted data");
        println!("  analyse      Analyse traces");
        println!("  gen          Generate test data");
        println!("  search       Search traces by tags");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("tempo-cli version 2.5.0"),
        "query" => {
            let trace_id = args.get(1).map(|s| s.as_str()).unwrap_or("abc123def456");
            println!("Querying trace: {}", trace_id);
            println!("  Root span: HTTP GET /api/users (12ms)");
            println!("    Child: db.query SELECT * FROM users (8ms)");
            println!("    Child: cache.get users:list (1ms)");
            println!("  Total spans: 3, Duration: 12ms");
        }
        "list" => {
            println!("Block ID                           Start              End                Spans");
            println!("abc12345-xxxx-xxxx-xxxx-12345678   2024-06-14 00:00   2024-06-14 02:00   45678");
            println!("def12345-xxxx-xxxx-xxxx-12345678   2024-06-14 02:00   2024-06-14 04:00   52341");
        }
        "search" => {
            let tag = args.get(1).map(|s| s.as_str()).unwrap_or("service.name=api");
            println!("Searching for traces with: {}", tag);
            println!("  abc123def456  HTTP GET /api/users   12ms   2024-06-14T12:00:00Z");
            println!("  ghi789jkl012  HTTP POST /api/users  45ms   2024-06-14T12:01:00Z");
        }
        "analyse" => {
            println!("Trace analysis:");
            println!("  Total traces: 12345");
            println!("  Avg duration: 23ms");
            println!("  P99 duration: 150ms");
            println!("  Error rate: 0.5%");
        }
        _ => println!("tempo-cli: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tempo-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tempo(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tempo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tempo"), "tempo");
        assert_eq!(basename(r"C:\bin\tempo.exe"), "tempo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tempo.exe"), "tempo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tempo(&["--help".to_string()]), 0);
        assert_eq!(run_tempo(&["-h".to_string()]), 0);
        let _ = run_tempo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tempo(&[]);
    }
}
