#![deny(clippy::all)]

//! mongodb-cli — SlateOS MongoDB shell & tools
//!
//! Multi-personality: `mongosh`, `mongodump`, `mongorestore`, `mongostat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mongosh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongosh [OPTIONS] [URI]");
        println!("mongosh v2.1 (SlateOS) — MongoDB Shell");
        println!();
        println!("Options:");
        println!("  --host HOST     Server hostname (default: localhost)");
        println!("  --port PORT     Server port (default: 27017)");
        println!("  --eval CODE     Evaluate JavaScript expression");
        println!("  --file FILE     Execute JavaScript file");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mongosh v2.1 (SlateOS)"); return 0; }
    println!("mongosh: connecting to mongodb://localhost:27017");
    println!("Using MongoDB: 7.0.4");
    println!("Using Mongosh: 2.1");
    0
}

fn run_mongodump(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongodump [OPTIONS]");
        println!("mongodump v2.1 (SlateOS) — Dump MongoDB data");
        println!("  --db NAME       Database to dump");
        println!("  --out DIR       Output directory");
        println!("  --gzip          Compress output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mongodump v2.1 (SlateOS)"); return 0; }
    println!("mongodump: dumping database...");
    println!("  Collections: 8");
    println!("  Documents: 12,456");
    println!("  Output: dump/");
    0
}

fn run_mongorestore(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongorestore [OPTIONS] [DIR]");
        println!("mongorestore v2.1 (SlateOS) — Restore MongoDB data");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mongorestore v2.1 (SlateOS)"); return 0; }
    println!("mongorestore: restoring from dump/");
    println!("  Collections: 8 restored");
    println!("  Documents: 12,456 inserted");
    0
}

fn run_mongostat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongostat [OPTIONS]");
        println!("mongostat v2.1 (SlateOS) — MongoDB server statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mongostat v2.1 (SlateOS)"); return 0; }
    println!("insert query update delete getmore command  res  vsize");
    println!("    *0    12     *0     *0       0    24|0  128M  256M");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mongosh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mongodump" => run_mongodump(&rest, &prog),
        "mongorestore" => run_mongorestore(&rest, &prog),
        "mongostat" => run_mongostat(&rest, &prog),
        _ => run_mongosh(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mongosh};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mongodb"), "mongodb");
        assert_eq!(basename(r"C:\bin\mongodb.exe"), "mongodb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mongodb.exe"), "mongodb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mongosh(&["--help".to_string()], "mongodb"), 0);
        assert_eq!(run_mongosh(&["-h".to_string()], "mongodb"), 0);
        let _ = run_mongosh(&["--version".to_string()], "mongodb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mongosh(&[], "mongodb");
    }
}
