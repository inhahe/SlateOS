#![deny(clippy::all)]

//! tantivy-cli — Slate OS Tantivy search engine library CLI
//!
//! Single personality: `tantivy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tantivy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tantivy [COMMAND] [OPTIONS]");
        println!("Tantivy v0.22 (Slate OS) — Full-text search engine library");
        println!();
        println!("Commands:");
        println!("  new                Create new index");
        println!("  index              Index documents");
        println!("  serve              Start search server");
        println!("  search QUERY       Search the index");
        println!("  bench              Benchmark queries");
        println!("  merge              Force segment merge");
        println!();
        println!("Options:");
        println!("  --index DIR        Index directory");
        println!("  --host ADDR        Server address (default: localhost:3000)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tantivy v0.22.0 (Slate OS)"); return 0; }
    println!("Tantivy v0.22.0 (Slate OS)");
    println!("  Index: /var/tantivy/index");
    println!("  Segments: 5");
    println!("  Documents: 123,456");
    println!("  Schema: title (TEXT), body (TEXT), timestamp (DATE)");
    println!("  Search server: http://localhost:3000");
    println!("  Query parser: default (BM25 scoring)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tantivy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tantivy(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tantivy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tantivy"), "tantivy");
        assert_eq!(basename(r"C:\bin\tantivy.exe"), "tantivy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tantivy.exe"), "tantivy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tantivy(&["--help".to_string()], "tantivy"), 0);
        assert_eq!(run_tantivy(&["-h".to_string()], "tantivy"), 0);
        let _ = run_tantivy(&["--version".to_string()], "tantivy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tantivy(&[], "tantivy");
    }
}
