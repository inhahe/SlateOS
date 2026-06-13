#![deny(clippy::all)]

//! sonic-cli — SlateOS Sonic search backend
//!
//! Single personality: `sonic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sonic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sonic [OPTIONS]");
        println!("Sonic v1.4 (SlateOS) — Fast, lightweight search backend");
        println!();
        println!("Options:");
        println!("  -c FILE            Config file (default: config.cfg)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sonic v1.4.8 (SlateOS)"); return 0; }
    println!("Sonic v1.4.8 (SlateOS)");
    println!("  Channel: inet (0.0.0.0:1491)");
    println!("  Mode: search + ingest + control");
    println!("  Collections: 5");
    println!("  Buckets: 23");
    println!("  Objects: 456,789");
    println!("  KV store: RocksDB");
    println!("  Memory: 45 MB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sonic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sonic(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sonic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sonic"), "sonic");
        assert_eq!(basename(r"C:\bin\sonic.exe"), "sonic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sonic.exe"), "sonic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sonic(&["--help".to_string()], "sonic"), 0);
        assert_eq!(run_sonic(&["-h".to_string()], "sonic"), 0);
        let _ = run_sonic(&["--version".to_string()], "sonic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sonic(&[], "sonic");
    }
}
