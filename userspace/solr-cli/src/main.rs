#![deny(clippy::all)]

//! solr-cli — SlateOS Apache Solr search platform
//!
//! Single personality: `solr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_solr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: solr [COMMAND] [OPTIONS]");
        println!("Apache Solr v9.5 (Slate OS) — Enterprise search platform");
        println!();
        println!("Commands:");
        println!("  start              Start Solr server");
        println!("  stop               Stop Solr server");
        println!("  restart            Restart Solr");
        println!("  status             Show status");
        println!("  create -c NAME     Create collection/core");
        println!("  delete -c NAME     Delete collection/core");
        println!("  healthcheck -c NAME  Health check");
        println!("  post -c NAME FILES Index documents");
        println!("  zk                 ZooKeeper operations");
        println!();
        println!("Options:");
        println!("  -p PORT            Port (default: 8983)");
        println!("  -m MEMORY          Java heap size");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache Solr v9.5.0 (Slate OS)"); return 0; }
    println!("Apache Solr v9.5.0 (Slate OS)");
    println!("  Admin UI: http://0.0.0.0:8983/solr");
    println!("  Mode: standalone");
    println!("  Collections: 4");
    println!("  Documents: 1,234,567");
    println!("  Memory: 2 GB heap");
    println!("  Handlers: /select, /update, /spell, /suggest");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "solr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_solr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_solr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/solr"), "solr");
        assert_eq!(basename(r"C:\bin\solr.exe"), "solr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("solr.exe"), "solr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_solr(&["--help".to_string()], "solr"), 0);
        assert_eq!(run_solr(&["-h".to_string()], "solr"), 0);
        let _ = run_solr(&["--version".to_string()], "solr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_solr(&[], "solr");
    }
}
