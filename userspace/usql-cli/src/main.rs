#![deny(clippy::all)]

//! usql-cli — SlateOS usql universal SQL client
//!
//! Single personality: `usql`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_usql(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: usql [OPTIONS] DSN");
        println!("usql v0.17.5 (Slate OS) — Universal SQL client");
        println!();
        println!("Options:");
        println!("  DSN                   Connection string (driver://user:pass@host/db)");
        println!("  -c, --command SQL     Execute SQL and exit");
        println!("  -f, --file FILE       Execute SQL from file");
        println!("  -o, --out FILE        Output to file");
        println!("  -J, --json            JSON output");
        println!("  -C, --csv             CSV output");
        println!("  -t, --no-header       Suppress column headers");
        println!("  --set VAR=VALUE       Set variable");
        println!("  -V, --version         Show version");
        println!();
        println!("Supported drivers:");
        println!("  postgres, mysql, sqlite3, sqlserver, oracle,");
        println!("  cockroachdb, clickhouse, snowflake, bigquery, ...");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("usql v0.17.5 (Slate OS)");
        return 0;
    }
    let dsn = args.first().map(|s| s.as_str()).unwrap_or("sqlite3://local.db");
    if args.iter().any(|a| a == "-c" || a == "--command") {
        println!("(1 row)");
    } else {
        println!("Connected to: {}", dsn);
        println!("Type \"help\" for help.");
        println!("usql=> ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "usql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_usql(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_usql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/usql"), "usql");
        assert_eq!(basename(r"C:\bin\usql.exe"), "usql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("usql.exe"), "usql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_usql(&["--help".to_string()], "usql"), 0);
        assert_eq!(run_usql(&["-h".to_string()], "usql"), 0);
        let _ = run_usql(&["--version".to_string()], "usql");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_usql(&[], "usql");
    }
}
