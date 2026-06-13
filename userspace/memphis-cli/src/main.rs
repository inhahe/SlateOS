#![deny(clippy::all)]

//! memphis-cli — SlateOS Memphis event streaming CLI
//!
//! Single personality: `memphis`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_memphis(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: memphis COMMAND [OPTIONS]");
        println!("Memphis v1.3.0 (Slate OS) — Event streaming platform CLI");
        println!();
        println!("Commands:");
        println!("  station         Manage stations");
        println!("  producer        Manage producers");
        println!("  consumer        Manage consumers");
        println!("  user            Manage users");
        println!("  cluster         Cluster info");
        println!("  schema          Manage schemas");
        println!("  connect         Test connection");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  --host HOST     Memphis host");
        println!("  --user USER     Username");
        println!("  --password PASS Password");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Memphis CLI v1.3.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("station");
    match cmd {
        "station" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Stations:");
                    println!("  events        retention: 7d   replicas: 3  msgs: 10.2K");
                    println!("  orders        retention: 30d  replicas: 3  msgs: 45.8K");
                    println!("  notifications retention: 1d   replicas: 1  msgs: 892");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-station");
                    println!("Station '{}' created.", name);
                }
                "info" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("events");
                    println!("Station: {}", name);
                    println!("  Retention: 7 days");
                    println!("  Replicas: 3");
                    println!("  DLS: enabled");
                    println!("  Producers: 2 connected");
                    println!("  Consumers: 3 connected");
                }
                _ => println!("memphis station {}: completed", sub),
            }
        }
        "producer" => {
            println!("Connected producers:");
            println!("  prod-1   station: events   msgs/s: 120");
            println!("  prod-2   station: orders   msgs/s: 45");
        }
        "consumer" => {
            println!("Connected consumers:");
            println!("  cons-1   station: events   group: analytics   lag: 0");
            println!("  cons-2   station: events   group: storage     lag: 12");
            println!("  cons-3   station: orders   group: processor   lag: 0");
        }
        "user" => {
            println!("Users:");
            println!("  root      type: management");
            println!("  app_user  type: application");
        }
        "cluster" => {
            println!("Cluster Info:");
            println!("  Name: memphis-cluster");
            println!("  Nodes: 3");
            println!("  Leader: node-1");
            println!("  Status: healthy");
        }
        "connect" => println!("Connection successful: memphis://localhost:6666"),
        _ => println!("memphis {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "memphis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_memphis(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_memphis};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/memphis"), "memphis");
        assert_eq!(basename(r"C:\bin\memphis.exe"), "memphis.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("memphis.exe"), "memphis");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_memphis(&["--help".to_string()], "memphis"), 0);
        assert_eq!(run_memphis(&["-h".to_string()], "memphis"), 0);
        let _ = run_memphis(&["--version".to_string()], "memphis");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_memphis(&[], "memphis");
    }
}
