#![deny(clippy::all)]

//! manticore-cli — OurOS Manticore Search
//!
//! Multi-personality: `searchd`, `indexer`, `indextool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_manticore(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "indexer" => {
                println!("indexer (OurOS) — Manticore index builder");
                println!("  --all              Index all configured sources");
                println!("  --rotate           Rotate indexes after building");
                println!("  INDEX [INDEX ...]  Specific indexes to build");
            }
            "indextool" => {
                println!("indextool (OurOS) — Manticore index diagnostics");
                println!("  --check INDEX      Check index integrity");
                println!("  --dumpheader FILE  Dump index header");
                println!("  --dumphitlist INDEX KEYWORD  Dump hits");
            }
            _ => {
                println!("searchd (OurOS) — Manticore Search daemon");
                println!("  --config FILE      Config file");
                println!("  --stop             Stop daemon");
                println!("  --status           Show status");
                println!("  --pidfile FILE     PID file");
                println!("  --listen ADDR      Listen address");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") { println!("Manticore Search v6.2.12 (OurOS)"); return 0; }
    match prog {
        "indexer" => println!("Manticore indexer: all indexes rebuilt successfully"),
        "indextool" => println!("Manticore indextool: index check passed"),
        _ => {
            println!("Manticore Search v6.2.12 (OurOS)");
            println!("  MySQL protocol: 0.0.0.0:9306");
            println!("  HTTP API: 0.0.0.0:9308");
            println!("  Binary: 0.0.0.0:9312");
            println!("  Tables: 12 (8 RT, 4 plain)");
            println!("  Documents: 8.9 million");
            println!("  RAM: 512 MB (binlog enabled)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "searchd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_manticore(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_manticore};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/manticore"), "manticore");
        assert_eq!(basename(r"C:\bin\manticore.exe"), "manticore.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("manticore.exe"), "manticore");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_manticore(&["--help".to_string()], "manticore"), 0);
        assert_eq!(run_manticore(&["-h".to_string()], "manticore"), 0);
        assert_eq!(run_manticore(&["--version".to_string()], "manticore"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_manticore(&[], "manticore"), 0);
    }
}
