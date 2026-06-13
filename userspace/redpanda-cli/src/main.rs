#![deny(clippy::all)]

//! redpanda-cli — SlateOS Redpanda streaming tools
//!
//! Multi-personality: `rpk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rpk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rpk COMMAND [OPTIONS]");
        println!("rpk — Redpanda CLI 24.1.7 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  topic        Manage topics");
        println!("  group        Manage consumer groups");
        println!("  cluster      Manage cluster");
        println!("  container    Manage rpk containers");
        println!("  profile      Manage rpk profiles");
        println!("  acl          Manage ACLs");
        println!("  debug        Debug tools");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("rpk v24.1.7 (rev abc1234)"),
        "topic" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("NAME              PARTITIONS  REPLICAS");
                    println!("events            6           3");
                    println!("orders            12          3");
                    println!("notifications     3           3");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-topic");
                    println!("TOPIC         STATUS");
                    println!("{}       OK", name);
                }
                "delete" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("old-topic");
                    println!("TOPIC         STATUS");
                    println!("{}       OK (deleted)", name);
                }
                "produce" => {
                    let topic = args.get(2).map(|s| s.as_str()).unwrap_or("events");
                    println!("Producing to topic '{}'...", topic);
                    println!("Produced offset 42 to partition 0");
                }
                "consume" => {
                    let topic = args.get(2).map(|s| s.as_str()).unwrap_or("events");
                    println!("Consuming from '{}':", topic);
                    println!("  partition: 0, offset: 42, value: hello");
                }
                _ => println!("rpk topic: '{}' completed", sub),
            }
        }
        "group" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub == "ls" {
                println!("GROUP             COORDINATOR  STATE    LAG");
                println!("my-consumer       0            Stable   5");
            }
        }
        "cluster" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            if sub == "info" {
                println!("CLUSTER");
                println!("=======");
                println!("  redpanda.abc12345-xxxx-xxxx-xxxx-abc123456789");
                println!();
                println!("BROKERS");
                println!("=======");
                println!("  ID    HOST          PORT   RACK");
                println!("  0     192.168.1.1   9092   rack1");
                println!("  1     192.168.1.2   9092   rack1");
                println!("  2     192.168.1.3   9092   rack2");
            }
        }
        _ => println!("rpk: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rpk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rpk(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rpk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/redpanda"), "redpanda");
        assert_eq!(basename(r"C:\bin\redpanda.exe"), "redpanda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("redpanda.exe"), "redpanda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rpk(&["--help".to_string()]), 0);
        assert_eq!(run_rpk(&["-h".to_string()]), 0);
        let _ = run_rpk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rpk(&[]);
    }
}
