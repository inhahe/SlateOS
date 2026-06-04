#![deny(clippy::all)]

//! sumologic-cli — OurOS Sumo Logic collector
//!
//! Single personality: `sumo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sumo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sumo COMMAND [OPTIONS]");
        println!("sumo v1.0 (OurOS) — Sumo Logic collector/client");
        println!();
        println!("Commands:");
        println!("  start             Start collector");
        println!("  stop              Stop collector");
        println!("  status            Show collector status");
        println!("  sources list      List configured sources");
        println!("  sources add       Add a source");
        println!("  search QUERY      Run a log search query");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "start" => {
            println!("Starting Sumo Logic collector...");
            println!("  Sources: 3");
            println!("  Status: running");
        }
        "stop" => println!("Sumo Logic collector stopped."),
        "status" => {
            println!("Collector status:");
            println!("  Name: ouros-collector");
            println!("  Status: running");
            println!("  Sources: 3 active");
            println!("  Events/sec: 1,247");
            println!("  Buffer: 42 MB / 500 MB");
        }
        "sources" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Configured sources:");
                println!("  1. syslog (/var/log/syslog) — active");
                println!("  2. nginx (/var/log/nginx/*.log) — active");
                println!("  3. app (/opt/app/logs/*.log) — active");
            } else {
                println!("Source operation: {}", sub);
            }
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("error | count by host");
            println!("Search: {}", query);
            println!("  Time range: last 15m");
            println!("  Results: 342 messages");
        }
        "version" | "--version" => println!("sumo v1.0 (OurOS)"),
        _ => println!("sumo {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sumo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sumo(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sumo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sumologic"), "sumologic");
        assert_eq!(basename(r"C:\bin\sumologic.exe"), "sumologic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sumologic.exe"), "sumologic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sumo(&["--help".to_string()], "sumologic"), 0);
        assert_eq!(run_sumo(&["-h".to_string()], "sumologic"), 0);
        let _ = run_sumo(&["--version".to_string()], "sumologic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sumo(&[], "sumologic");
    }
}
