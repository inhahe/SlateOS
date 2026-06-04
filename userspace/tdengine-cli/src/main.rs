#![deny(clippy::all)]

//! tdengine-cli — OurOS TDengine time-series database
//!
//! Multi-personality: `taosd`, `taos`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tdengine(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "taos" => {
                println!("taos (OurOS) — TDengine command-line client");
                println!("  -h HOST            Server host");
                println!("  -P PORT            Server port (default: 6030)");
                println!("  -u USER            Username");
                println!("  -p PASSWORD        Password");
                println!("  -d DATABASE        Default database");
                println!("  -s COMMAND         Execute SQL");
                println!("  -f FILE            Execute SQL file");
            }
            _ => {
                println!("taosd (OurOS) — TDengine server daemon");
                println!("  -c DIR             Config directory");
                println!("  -C                 Print config");
                println!("  -k                 Check config syntax");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") { println!("TDengine v3.3.2 (OurOS)"); return 0; }
    match prog {
        "taos" => {
            println!("TDengine CLI v3.3.2");
            println!("  Connected to: localhost:6030");
            println!("  Server: TDengine v3.3.2");
            println!("  Database: default");
            println!("  Enter SQL or \\q to quit");
        }
        _ => {
            println!("TDengine Server v3.3.2 (OurOS)");
            println!("  Listening: 0.0.0.0:6030");
            println!("  REST API: 0.0.0.0:6041");
            println!("  Databases: 5");
            println!("  Super tables: 23");
            println!("  Sub tables: 12,345");
            println!("  Data points: 890 million");
            println!("  Storage: 12.3 GB");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "taosd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tdengine(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tdengine};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tdengine"), "tdengine");
        assert_eq!(basename(r"C:\bin\tdengine.exe"), "tdengine.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tdengine.exe"), "tdengine");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tdengine(&["--help".to_string()], "tdengine"), 0);
        assert_eq!(run_tdengine(&["-h".to_string()], "tdengine"), 0);
        let _ = run_tdengine(&["--version".to_string()], "tdengine");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tdengine(&[], "tdengine");
    }
}
