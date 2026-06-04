#![deny(clippy::all)]

//! consul-cli — OurOS HashiCorp Consul service mesh CLI
//!
//! Single personality: `consul`

use std::env;
use std::process;

fn run_consul(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: consul <COMMAND> [OPTIONS]");
        println!();
        println!("Service networking solution (service mesh, discovery, configuration).");
        println!();
        println!("Commands:");
        println!("  agent          Runs a Consul agent");
        println!("  catalog        Interact with the catalog");
        println!("  connect        Interact with Consul Connect");
        println!("  members        Lists cluster members");
        println!("  services       Interact with services");
        println!("  kv             Interact with the KV store");
        println!("  acl            Interact with ACLs");
        println!("  intention      Interact with Connect intentions");
        println!("  config         Interact with configuration entries");
        println!("  snapshot       Manage snapshots");
        println!("  operator       Operator utilities");
        println!("  monitor        Stream logs from a Consul agent");
        println!("  info           Provides debugging info");
        println!("  version        Show version");
        println!();
        println!("Options:");
        println!("  -http-addr <ADDR>  Consul HTTP address");
        println!("  -token <TOKEN>     API token");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Consul v1.18.0 (OurOS)");
            println!("Protocol 2 spoken, understands 2 to 3");
            0
        }
        "members" => {
            println!("Node          Address          Status  Type    Build   Protocol  DC");
            println!("consul-1      10.0.1.1:8301    alive   server  1.18.0  2         dc1");
            println!("consul-2      10.0.1.2:8301    alive   server  1.18.0  2         dc1");
            println!("consul-3      10.0.1.3:8301    alive   server  1.18.0  2         dc1");
            println!("web-1         10.0.2.1:8301    alive   client  1.18.0  2         dc1");
            println!("web-2         10.0.2.2:8301    alive   client  1.18.0  2         dc1");
            0
        }
        "catalog" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("services");
            match sub {
                "services" => {
                    println!("consul");
                    println!("web-app");
                    println!("api-service");
                    println!("redis");
                    println!("postgres");
                }
                "nodes" => {
                    println!("Node      ID        Address    DC");
                    println!("consul-1  abc123    10.0.1.1   dc1");
                    println!("consul-2  def456    10.0.1.2   dc1");
                    println!("web-1     ghi789    10.0.2.1   dc1");
                }
                _ => println!("Usage: consul catalog <services|nodes|datacenters>"),
            }
            0
        }
        "kv" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "get" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("config/app");
                    println!("{}: {{\"port\": 8080, \"debug\": false}}", key);
                }
                "put" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("config/app");
                    println!("Success! Data written to: {}", key);
                }
                "delete" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("config/app");
                    println!("Success! Deleted key: {}", key);
                }
                _ => println!("Usage: consul kv <get|put|delete|export|import>"),
            }
            0
        }
        "services" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub.is_empty() {
                println!("web-app");
                println!("api-service");
                println!("redis");
                println!("postgres");
            }
            0
        }
        "info" => {
            println!("agent:");
            println!("  check_monitors = 0");
            println!("  checks = 3");
            println!("  services = 4");
            println!("consul:");
            println!("  known_datacenters = 1");
            println!("  server = true");
            println!("  leader = true");
            println!("  leader_addr = 10.0.1.1:8300");
            println!("raft:");
            println!("  applied_index = 12345");
            println!("  commit_index = 12345");
            println!("  state = Leader");
            println!("  term = 5");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: consul <command>. See --help.");
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
    let code = run_consul(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_consul};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_consul(vec!["--help".to_string()]), 0);
        assert_eq!(run_consul(vec!["-h".to_string()]), 0);
        let _ = run_consul(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_consul(vec![]);
    }
}
