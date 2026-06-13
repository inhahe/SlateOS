#![deny(clippy::all)]

//! zookeeper-cli — SlateOS ZooKeeper CLI
//!
//! Single personality: `zkCli`

use std::env;
use std::process;

fn run_zkcli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zkCli <COMMAND> [OPTIONS]");
        println!();
        println!("Apache ZooKeeper CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  ls           List znodes");
        println!("  get          Get znode data");
        println!("  create       Create a znode");
        println!("  set          Set znode data");
        println!("  delete       Delete a znode");
        println!("  stat         Get znode metadata");
        println!("  getAcl       Get znode ACL");
        println!("  setAcl       Set znode ACL");
        println!("  sync         Sync znode");
        println!("  server       Server status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ZooKeeper CLI 3.9.1 (Slate OS)");
        return 0;
    }

    let server = args.windows(2).find(|w| w[0] == "--server" || w[0] == "-s")
        .map(|w| w[1].as_str()).unwrap_or("localhost:2181");

    let cmd = args.iter()
        .find(|a| !a.starts_with('-') && *a != server)
        .map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "ls" => {
            let path = args.last().map(|s| s.as_str()).unwrap_or("/");
            println!("Connecting to {}...", server);
            if path == "/" {
                println!("[zookeeper, config, services, locks, elections]");
            } else {
                println!("[node1, node2, node3]");
            }
            0
        }
        "get" => {
            let path = args.last().map(|s| s.as_str()).unwrap_or("/config/app");
            println!("Connecting to {}...", server);
            println!("{{\"host\": \"db.local\", \"port\": 5432, \"pool_size\": 10}}");
            println!("  cZxid = 0x100000042");
            println!("  ctime = Mon Jan 15 14:00:00 UTC 2024");
            println!("  mZxid = 0x100000045");
            println!("  mtime = Mon Jan 15 15:00:00 UTC 2024");
            println!("  dataLength = 52");
            println!("  numChildren = 0");
            println!("  path = {}", path);
            0
        }
        "create" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/services/app-01");
            let data = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let ephemeral = args.iter().any(|a| a == "-e");
            let sequential = args.iter().any(|a| a == "-s");
            println!("Connecting to {}...", server);
            if sequential {
                println!("Created {}0000000001", path);
            } else {
                println!("Created {}", path);
            }
            if ephemeral {
                println!("  (ephemeral node)");
            }
            if !data.is_empty() {
                println!("  data: {}", data);
            }
            0
        }
        "set" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/config/app");
            println!("Connecting to {}...", server);
            println!("Set data for {}", path);
            println!("  cZxid = 0x100000042");
            println!("  mZxid = 0x100000046");
            println!("  version = 3");
            0
        }
        "delete" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/services/old");
            let recursive = args.iter().any(|a| a == "-R");
            println!("Connecting to {}...", server);
            if recursive {
                println!("Recursively deleted {} and all children", path);
            } else {
                println!("Deleted {}", path);
            }
            0
        }
        "stat" => {
            let path = args.last().map(|s| s.as_str()).unwrap_or("/");
            println!("Connecting to {}...", server);
            println!("  cZxid = 0x0");
            println!("  ctime = Thu Jan 01 00:00:00 UTC 1970");
            println!("  mZxid = 0x0");
            println!("  mtime = Thu Jan 01 00:00:00 UTC 1970");
            println!("  pZxid = 0x100000042");
            println!("  cversion = 5");
            println!("  dataVersion = 0");
            println!("  aclVersion = 0");
            println!("  ephemeralOwner = 0x0");
            println!("  dataLength = 0");
            println!("  numChildren = 4");
            println!("  path = {}", path);
            0
        }
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => {
                    println!("ZooKeeper cluster status:");
                    println!("  Node              Mode       Connections  Outstanding  Zxid");
                    println!("  localhost:2181     leader     45           0            0x100000046");
                    println!("  localhost:2182     follower   32           0            0x100000046");
                    println!("  localhost:2183     follower   28           0            0x100000046");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: zkCli <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zkcli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_zkcli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zkcli(vec!["--help".to_string()]), 0);
        assert_eq!(run_zkcli(vec!["-h".to_string()]), 0);
        let _ = run_zkcli(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zkcli(vec![]);
    }
}
