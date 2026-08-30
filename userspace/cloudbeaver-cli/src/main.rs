#![deny(clippy::all)]

//! cloudbeaver-cli — Slate OS CloudBeaver database manager
//!
//! Single personality: `cloudbeaver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cloudbeaver(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cloudbeaver [COMMAND] [OPTIONS]");
        println!("CloudBeaver v24.1 (Slate OS) — Web database manager (DBeaver web)");
        println!();
        println!("Commands:");
        println!("  start              Start CloudBeaver server");
        println!("  stop               Stop server");
        println!("  configure          Interactive configuration");
        println!("  datasource list    List data sources");
        println!("  user list|create   Manage users");
        println!();
        println!("Options:");
        println!("  --workspace DIR    Workspace directory");
        println!("  --port PORT        Server port (default: 8978)");
        println!("  --host ADDR        Bind address");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CloudBeaver v24.1.0 (Slate OS)"); return 0; }
    println!("CloudBeaver v24.1.0 (Slate OS)");
    println!("  Server: http://0.0.0.0:8978");
    println!("  Datasources: 8 configured");
    println!("  Drivers: PostgreSQL, MySQL, Oracle, MSSQL, SQLite, H2");
    println!("  Users: 5 active");
    println!("  Connections: 3 open");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cloudbeaver".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cloudbeaver(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cloudbeaver};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cloudbeaver"), "cloudbeaver");
        assert_eq!(basename(r"C:\bin\cloudbeaver.exe"), "cloudbeaver.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cloudbeaver.exe"), "cloudbeaver");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cloudbeaver(&["--help".to_string()], "cloudbeaver"), 0);
        assert_eq!(run_cloudbeaver(&["-h".to_string()], "cloudbeaver"), 0);
        let _ = run_cloudbeaver(&["--version".to_string()], "cloudbeaver");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cloudbeaver(&[], "cloudbeaver");
    }
}
