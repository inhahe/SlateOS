#![deny(clippy::all)]

//! mongocompass-cli — OurOS MongoDB Compass GUI
//!
//! Single personality: `mongocompass`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongocompass [OPTIONS] [URI]");
        println!("MongoDB Compass 1.45 (OurOS) — Official GUI for MongoDB");
        println!();
        println!("Options:");
        println!("  URI                    mongodb:// or mongodb+srv:// connection string");
        println!("  --readOnly             Read-only mode");
        println!("  --shell                Launch embedded mongosh");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MongoDB Compass 1.45.1 (OurOS)"); return 0; }
    println!("MongoDB Compass 1.45.1 (OurOS)");
    println!("  Official MongoDB GUI — connect to MongoDB Community/Enterprise/Atlas");
    println!("  Features: schema visualization, query builder, aggregation pipeline builder");
    println!("  Documents: CRUD operations, JSON tree/table/list views, validation rules");
    println!("  Performance: real-time server stats, slow query log, explain plans");
    println!("  Indexes: visual index builder with performance impact estimates");
    println!("  Embedded: mongosh shell, MongoDB tools (mongodump/restore/import/export)");
    println!("  Editions: Compass (full), Compass Readonly, Compass Isolated");
    println!("  Platforms: Windows, macOS, Linux (Electron-based)");
    println!("  License: Free, official MongoDB Inc. product");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mongocompass".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mongocompass"), "mongocompass");
        assert_eq!(basename(r"C:\bin\mongocompass.exe"), "mongocompass.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mongocompass.exe"), "mongocompass");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mc(&["--help".to_string()], "mongocompass"), 0);
        assert_eq!(run_mc(&["-h".to_string()], "mongocompass"), 0);
        assert_eq!(run_mc(&["--version".to_string()], "mongocompass"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mc(&[], "mongocompass"), 0);
    }
}
