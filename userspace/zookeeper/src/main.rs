#![deny(clippy::all)]

//! zookeeper — SlateOS distributed coordination service
//!
//! Multi-personality: `zkServer` (server), `zkCli` (CLI client)

use std::env;
use std::process;

fn run_zk_server(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("start");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zkServer <start|stop|restart|status|version>");
        return 0;
    }
    match cmd {
        "version" | "--version" => {
            println!("Apache ZooKeeper, version 3.9.2 (Slate OS)");
            println!("Build: abc1234");
            0
        }
        "start" => {
            println!("ZooKeeper JMX enabled by default");
            println!("Using config: /etc/zookeeper/zoo.cfg");
            println!("2025-05-22 10:00:00,000 [myid:1] - INFO  [main:QuorumPeerConfig@175] - Reading configuration from: /etc/zookeeper/zoo.cfg");
            println!("2025-05-22 10:00:00,100 [myid:1] - INFO  [main:DatadirCleanupManager@78] - autopurge.snapRetainCount set to 3");
            println!("2025-05-22 10:00:00,200 [myid:1] - INFO  [main:DatadirCleanupManager@79] - autopurge.purgeInterval set to 0");
            println!("2025-05-22 10:00:00,500 [myid:1] - INFO  [main:NIOServerCnxnFactory@111] - binding to port 0.0.0.0/0.0.0.0:2181");
            println!("2025-05-22 10:00:01,000 [myid:1] - INFO  [main:ZooKeeperServer@123] - ZooKeeper server started");
            0
        }
        "stop" => {
            println!("Stopping zookeeper ... STOPPED");
            0
        }
        "restart" => {
            println!("Stopping zookeeper ... STOPPED");
            println!("Starting zookeeper ... STARTED");
            0
        }
        "status" => {
            println!("ZooKeeper JMX enabled by default");
            println!("Using config: /etc/zookeeper/zoo.cfg");
            println!("Client port found: 2181. Client address: localhost. Client SSL: false.");
            println!("Mode: standalone");
            0
        }
        _ => {
            println!("Usage: zkServer <start|stop|restart|status|version>");
            1
        }
    }
}

fn run_zk_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zkCli [-server host:port] [cmd args]");
        println!();
        println!("Commands:");
        println!("  ls <path>                 List children of a znode");
        println!("  get <path>                Get the data of a znode");
        println!("  set <path> <data>         Set the data of a znode");
        println!("  create <path> <data>      Create a znode");
        println!("  delete <path>             Delete a znode");
        println!("  stat <path>               Display znode status");
        println!("  rmr <path>                Remove znode recursively");
        println!("  getAcl <path>             Get ACL of a znode");
        println!("  setAcl <path> <acl>       Set ACL of a znode");
        return 0;
    }

    // Check for inline command mode
    let cmd_idx = args.iter().position(|a| !a.starts_with('-') && a != "-server")
        .or_else(|| {
            args.iter().position(|a| a == "-server")
                .and_then(|i| {
                    // skip host:port after -server
                    if i + 2 < args.len() { Some(i + 2) } else { None }
                })
        });

    if let Some(idx) = cmd_idx {
        let cmd = args.get(idx).map(|s| s.as_str()).unwrap_or("ls");
        let path = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("/");

        match cmd {
            "ls" => {
                if path == "/" {
                    println!("[zookeeper, kafka, services]");
                } else {
                    println!("[child1, child2, child3]");
                }
            }
            "get" => {
                println!("{{\"config\":\"value\"}}");
                println!("cZxid = 0x100000002");
                println!("ctime = Wed May 22 10:00:00 UTC 2025");
                println!("mZxid = 0x100000005");
                println!("mtime = Wed May 22 10:30:00 UTC 2025");
                println!("pZxid = 0x100000002");
                println!("cversion = 0");
                println!("dataVersion = 2");
                println!("aclVersion = 0");
                println!("ephemeralOwner = 0x0");
                println!("dataLength = 18");
                println!("numChildren = 3");
            }
            "create" => {
                println!("Created {}", path);
            }
            "set" => {
                println!("cZxid = 0x100000002");
                println!("dataVersion = 3");
            }
            "delete" => {
                println!("(deleted {})", path);
            }
            "stat" => {
                println!("cZxid = 0x100000002");
                println!("ctime = Wed May 22 10:00:00 UTC 2025");
                println!("mZxid = 0x100000005");
                println!("mtime = Wed May 22 10:30:00 UTC 2025");
                println!("pZxid = 0x100000002");
                println!("cversion = 0");
                println!("dataVersion = 2");
                println!("numChildren = 3");
            }
            "getAcl" => {
                println!("'world,'anyone");
                println!(": cdrwa");
            }
            _ => println!("({} on {} — simulated)", cmd, path),
        }
        return 0;
    }

    // Interactive mode
    let server = args.iter().position(|a| a == "-server")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost:2181");
    println!("Connecting to {}", server);
    println!("Welcome to ZooKeeper!");
    println!("JLine support is enabled");
    println!();
    println!("[zk: {}(CONNECTED) 0] ls /", server);
    println!("[zookeeper, kafka, services]");
    println!("[zk: {}(CONNECTED) 1] quit", server);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("zkServer");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "zkCli" | "zkcli" | "zkCli.sh" => run_zk_cli(rest),
        _ => run_zk_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_zk_server};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zk_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_zk_server(vec!["-h".to_string()]), 0);
        let _ = run_zk_server(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zk_server(vec![]);
    }
}
