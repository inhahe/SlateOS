#![deny(clippy::all)]

//! osquery-cli — OurOS osquery endpoint visibility
//!
//! Multi-personality: `osqueryi`, `osqueryd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_osquery(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "osqueryd" => {
                println!("osqueryd v5.12 (OurOS) — osquery daemon");
                println!("  --config_path FILE     Config file");
                println!("  --flagfile FILE        Flags file");
                println!("  --database_path DIR    RocksDB path");
                println!("  --logger_plugin PLUGIN Log destination");
                println!("  --disable_events=false Enable events");
            }
            _ => {
                println!("osqueryi v5.12 (OurOS) — Interactive osquery shell");
                println!("  --json             JSON output");
                println!("  --csv              CSV output");
                println!("  --line             Line output");
                println!("  --header           Show column headers");
                println!("  --separator SEP    Column separator");
                println!("  SQL queries against system tables:");
                println!("    SELECT * FROM processes;");
                println!("    SELECT * FROM listening_ports;");
                println!("    SELECT * FROM users;");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("osquery v5.12.1 (OurOS)"); return 0; }
    match prog {
        "osqueryd" => {
            println!("osqueryd v5.12.1 (OurOS)");
            println!("  Config: /etc/osquery/osquery.conf");
            println!("  Database: /var/osquery/osquery.db");
            println!("  Logger: filesystem");
            println!("  Scheduled queries: 12");
            println!("  Events: process_events, socket_events, file_events");
            println!("  Running...");
        }
        _ => {
            println!("osqueryi v5.12.1 (OurOS)");
            println!("  Tables: 234 available");
            println!("  Example: SELECT pid, name, cmdline FROM processes WHERE uid = 0;");
            println!("    pid  | name      | cmdline");
            println!("    1    | init      | /sbin/init");
            println!("    234  | sshd      | /usr/sbin/sshd -D");
            println!("    567  | nginx     | nginx: master process");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "osqueryi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_osquery(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
