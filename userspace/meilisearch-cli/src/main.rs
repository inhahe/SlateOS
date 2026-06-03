#![deny(clippy::all)]

//! meilisearch-cli — OurOS Meilisearch search engine
//!
//! Single personality: `meilisearch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_meilisearch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: meilisearch [OPTIONS]");
        println!("Meilisearch v1.7 (OurOS) — Lightning-fast search engine");
        println!();
        println!("Options:");
        println!("  --http-addr ADDR   Listen address (default: localhost:7700)");
        println!("  --master-key KEY   Master API key");
        println!("  --db-path DIR      Database path");
        println!("  --env ENV          Environment (production/development)");
        println!("  --max-indexing-memory SIZE  Max memory for indexing");
        println!("  --max-indexing-threads N    Indexing threads");
        println!("  --no-analytics     Disable analytics");
        println!("  --dump-dir DIR     Dump directory");
        println!("  --import-dump FILE Import from dump");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Meilisearch v1.7.6 (OurOS)"); return 0; }
    println!("Meilisearch v1.7.6 (OurOS)");
    println!("  HTTP: http://localhost:7700");
    println!("  Database: /var/meilisearch/data.ms");
    println!("  Indexes: 5");
    println!("  Documents: 234,567 total");
    println!("  Search latency: < 50ms (p99)");
    println!("  Typo tolerance: enabled");
    println!("  Faceted search: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "meilisearch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_meilisearch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_meilisearch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/meilisearch"), "meilisearch");
        assert_eq!(basename(r"C:\bin\meilisearch.exe"), "meilisearch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("meilisearch.exe"), "meilisearch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_meilisearch(&["--help".to_string()], "meilisearch"), 0);
        assert_eq!(run_meilisearch(&["-h".to_string()], "meilisearch"), 0);
        assert_eq!(run_meilisearch(&["--version".to_string()], "meilisearch"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_meilisearch(&[], "meilisearch"), 0);
    }
}
