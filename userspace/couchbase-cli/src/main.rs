#![deny(clippy::all)]

//! couchbase-cli — OurOS Couchbase Server distributed NoSQL
//!
//! Single personality: `couchbase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: couchbase [OPTIONS] [SUBCMD]");
        println!("Couchbase Server 7.6 Enterprise (OurOS) — Distributed JSON document DB");
        println!();
        println!("Options:");
        println!("  -c HOST:PORT           Cluster endpoint");
        println!("  -u USER -p PASS        Authentication");
        println!("  cluster-init           Initialize cluster");
        println!("  bucket-create          Create bucket");
        println!("  --cbq                  cbq N1QL shell");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Couchbase Server 7.6.2 Enterprise Edition (OurOS)"); return 0; }
    println!("Couchbase Server 7.6.2 Enterprise Edition (OurOS)");
    println!("  Data model: JSON documents with N1QL (SQL for JSON) query language");
    println!("  Services: Data, Query, Index, Search (FTS), Analytics, Eventing, Backup");
    println!("  Memory-first: integrated managed cache (Memcached-derived)");
    println!("  Mobile sync: Couchbase Lite (embedded), Sync Gateway");
    println!("  XDCR: cross datacenter replication, multi-region active-active");
    println!("  Capella: fully-managed DBaaS on AWS/GCP/Azure");
    println!("  License: Free Community Edition; Enterprise subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "couchbase".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/couchbase"), "couchbase");
        assert_eq!(basename(r"C:\bin\couchbase.exe"), "couchbase.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("couchbase.exe"), "couchbase");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cb(&["--help".to_string()], "couchbase"), 0);
        assert_eq!(run_cb(&["-h".to_string()], "couchbase"), 0);
        assert_eq!(run_cb(&["--version".to_string()], "couchbase"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cb(&[], "couchbase"), 0);
    }
}
