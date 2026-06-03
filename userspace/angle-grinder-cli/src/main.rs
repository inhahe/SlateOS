#![deny(clippy::all)]

//! angle-grinder-cli — OurOS log query tool
//!
//! Single personality: `agrind`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_agrind(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: agrind [OPTIONS] QUERY");
        println!("agrind v0.19 (OurOS) — Slice and dice logs on the command line");
        println!();
        println!("Options:");
        println!("  QUERY             agrind query expression");
        println!("  -f FILE           Input file (stdin if omitted)");
        println!("  --format FMT      Input format (json, logfmt, clf)");
        println!("  --output FMT      Output format (text, json, csv)");
        println!("  --version         Show version");
        println!();
        println!("Query operators:");
        println!("  * | json          Parse as JSON");
        println!("  * | where COND    Filter rows");
        println!("  * | count by X    Count grouped by field");
        println!("  * | avg(X)        Average of field");
        println!("  * | sort X        Sort by field");
        println!("  * | limit N       Limit output rows");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("agrind v0.19 (OurOS)"); return 0; }
    let query = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("* | count");
    println!("Query: {}", query);
    println!();
    println!("_count   level    host");
    println!("  847    error    web-01");
    println!("  423    error    web-02");
    println!("  215    warn     db-01");
    println!("  142    error    api-01");
    println!("   89    warn     cache-01");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "agrind".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_agrind(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_agrind};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/angle-grinder"), "angle-grinder");
        assert_eq!(basename(r"C:\bin\angle-grinder.exe"), "angle-grinder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("angle-grinder.exe"), "angle-grinder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_agrind(&["--help".to_string()], "angle-grinder"), 0);
        assert_eq!(run_agrind(&["-h".to_string()], "angle-grinder"), 0);
        assert_eq!(run_agrind(&["--version".to_string()], "angle-grinder"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_agrind(&[], "angle-grinder"), 0);
    }
}
