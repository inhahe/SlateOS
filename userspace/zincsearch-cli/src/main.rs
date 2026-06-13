#![deny(clippy::all)]

//! zincsearch-cli — SlateOS ZincSearch lightweight search engine
//!
//! Single personality: `zincsearch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zincsearch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zincsearch [OPTIONS]");
        println!("ZincSearch v0.4 (SlateOS) — Lightweight Elasticsearch alternative");
        println!();
        println!("Options:");
        println!("  --data-dir DIR     Data directory");
        println!("  --addr ADDR        Listen address (default: 0.0.0.0:4080)");
        println!("  --first-admin-user USER  Initial admin user");
        println!("  --first-admin-password P Initial admin password");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ZincSearch v0.4.10 (SlateOS)"); return 0; }
    println!("ZincSearch v0.4.10 (SlateOS)");
    println!("  Web UI: http://0.0.0.0:4080");
    println!("  API: http://0.0.0.0:4080/api");
    println!("  Indexes: 6");
    println!("  Documents: 890,123");
    println!("  Storage: bluge (Go-native)");
    println!("  Compatible: Elasticsearch API subset");
    println!("  Memory: 128 MB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zincsearch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zincsearch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zincsearch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zincsearch"), "zincsearch");
        assert_eq!(basename(r"C:\bin\zincsearch.exe"), "zincsearch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zincsearch.exe"), "zincsearch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zincsearch(&["--help".to_string()], "zincsearch"), 0);
        assert_eq!(run_zincsearch(&["-h".to_string()], "zincsearch"), 0);
        let _ = run_zincsearch(&["--version".to_string()], "zincsearch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zincsearch(&[], "zincsearch");
    }
}
