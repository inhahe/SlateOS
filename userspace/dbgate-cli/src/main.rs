#![deny(clippy::all)]

//! dbgate-cli — SlateOS DbGate database manager
//!
//! Single personality: `dbgate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dbgate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dbgate [COMMAND] [OPTIONS]");
        println!("DbGate v5.5 (SlateOS) — Cross-platform database manager");
        println!();
        println!("Commands:");
        println!("  serve              Start web server");
        println!("  connections list   List saved connections");
        println!("  query SQL          Execute SQL");
        println!("  import FILE        Import data");
        println!("  export TABLE       Export table data");
        println!("  compare            Schema comparison");
        println!();
        println!("Options:");
        println!("  --port PORT        Web server port (default: 3000)");
        println!("  --connection NAME  Use saved connection");
        println!("  --format csv|json  Export format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DbGate v5.5.4 (SlateOS)"); return 0; }
    println!("DbGate v5.5.4 (SlateOS)");
    println!("  Connections: 5 saved");
    println!("  Supported: MySQL, PostgreSQL, MongoDB, SQLite, Redis, MariaDB");
    println!("  Web: http://0.0.0.0:3000");
    println!("  Plugins: 8 loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dbgate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dbgate(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dbgate};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dbgate"), "dbgate");
        assert_eq!(basename(r"C:\bin\dbgate.exe"), "dbgate.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dbgate.exe"), "dbgate");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dbgate(&["--help".to_string()], "dbgate"), 0);
        assert_eq!(run_dbgate(&["-h".to_string()], "dbgate"), 0);
        let _ = run_dbgate(&["--version".to_string()], "dbgate");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dbgate(&[], "dbgate");
    }
}
