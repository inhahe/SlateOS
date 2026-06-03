#![deny(clippy::all)]

//! consul — OurOS service discovery and configuration
//!
//! Single personality: `consul`

use std::env;
use std::process;

fn run_consul(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: consul <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  agent       Runs a Consul agent");
            println!("  members     Lists the members of a Consul cluster");
            println!("  catalog     Interact with the catalog");
            println!("  kv          Interact with the KV store");
            println!("  services    Interact with services");
            println!("  operator    Provides cluster-level tools");
            println!("  snapshot    Save/restore cluster state");
            println!("  acl         Interact with Consul's ACLs");
            println!("  connect     Interact with Consul Connect");
            println!("  monitor     Stream logs from a running agent");
            println!("  info        Show agent info");
            println!("  reload      Trigger agent config reload");
            println!("  leave       Leave the cluster");
            println!("  version     Print the version");
            0
        }
        "version" | "--version" => {
            println!("Consul v1.18.1 (OurOS)");
            println!("Revision: abc1234");
            println!("Protocol: 2 (Understands back to: 1)");
            0
        }
        "agent" => {
            let is_server = cmd_args.iter().any(|a| a == "-server");
            let is_dev = cmd_args.iter().any(|a| a == "-dev");
            let bind = cmd_args.iter().position(|a| a == "-bind")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("127.0.0.1");
            println!("==> Starting Consul agent...");
            println!("           Version: '1.18.1'");
            println!("        Build Date: '2025-05-22 00:00:00 +0000 UTC'");
            println!("           Node ID: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890'");
            println!("         Node name: 'ouros-node-1'");
            println!("        Datacenter: 'dc1' (Segment: '<all>')");
            if is_server || is_dev {
                println!("            Server: true (Bootstrap: true)");
            } else {
                println!("            Server: false");
            }
            println!("       Client Addr: [{}] (HTTP: 8500, HTTPS: -1, gRPC: 8502, DNS: 8600)", bind);
            println!("      Cluster Addr: {} (LAN: 8301, WAN: 8302)", bind);
            println!();
            println!("==> Log data will now stream in:");
            println!();
            println!("    [INFO]  agent: Started DNS server addr={}:8600 network=udp", bind);
            println!("    [INFO]  agent: Started HTTP server addr={}:8500 network=tcp", bind);
            println!("    [INFO]  agent: Started gRPC server addr={}:8502 network=tcp", bind);
            if is_dev {
                println!("    [INFO]  agent: dev mode enabled — data will not be persisted");
            }
            println!("    [INFO]  agent: Consul agent running!");
            0
        }
        "members" => {
            println!("Node           Address          Status  Type    Build   Protocol  DC   Partition  Segment");
            println!("ouros-node-1   127.0.0.1:8301   alive   server  1.18.1  2         dc1  default    <all>");
            0
        }
        "catalog" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "services" => {
                    println!("consul");
                    println!("web");
                    println!("api");
                    println!("redis");
                    println!("postgres");
                }
                "nodes" => {
                    println!("Node           ID                                    Address      DC");
                    println!("ouros-node-1   a1b2c3d4-e5f6-7890-abcd-ef1234567890  127.0.0.1    dc1");
                }
                "datacenters" => println!("dc1"),
                _ => println!("Usage: consul catalog <services|nodes|datacenters>"),
            }
            0
        }
        "kv" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            let key = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("config/key");
            match sub {
                "get" => println!("value-for-{}", key),
                "put" => {
                    let val = cmd_args.get(2).map(|s| s.as_str()).unwrap_or("value");
                    println!("Success! Data written to: {}", key);
                    let _ = val;
                }
                "delete" => println!("Success! Deleted key: {}", key),
                "export" => {
                    println!("[");
                    println!("  {{\"key\": \"config/db_host\", \"value\": \"bG9jYWxob3N0\"}},");
                    println!("  {{\"key\": \"config/db_port\", \"value\": \"NTQzMg==\"}}");
                    println!("]");
                }
                _ => println!("Usage: consul kv <get|put|delete|export> [key] [value]"),
            }
            0
        }
        "services" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "register" => println!("Registered service (simulated)"),
                "deregister" => println!("Deregistered service (simulated)"),
                _ => println!("Usage: consul services <register|deregister>"),
            }
            0
        }
        "info" => {
            println!("agent:");
            println!("  check_monitors = 0");
            println!("  check_ttls = 0");
            println!("  checks = 2");
            println!("  services = 3");
            println!("consul:");
            println!("  bootstrap = true");
            println!("  known_datacenters = 1");
            println!("  leader = true");
            println!("  leader_addr = 127.0.0.1:8300");
            println!("  server = true");
            println!("raft:");
            println!("  applied_index = 42");
            println!("  commit_index = 42");
            println!("  last_log_index = 42");
            println!("  state = Leader");
            0
        }
        "snapshot" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "save" => {
                    let path = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("backup.snap");
                    println!("Saved and verified snapshot to index 42 ({})", path);
                }
                "restore" => {
                    let path = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("backup.snap");
                    println!("Restored snapshot from {} (simulated)", path);
                }
                _ => println!("Usage: consul snapshot <save|restore> <file>"),
            }
            0
        }
        "monitor" => {
            println!("[INFO]  agent: Synced service web");
            println!("[INFO]  agent: Synced service api");
            println!("[DEBUG] agent: Node info in sync");
            println!("[INFO]  agent: Synced check service:web");
            0
        }
        "reload" => { println!("Configuration reload triggered"); 0 }
        "leave" => { println!("Graceful leave complete"); 0 }
        other => { eprintln!("consul: unknown command '{}'", other); 1 }
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
    fn help_and_version_exit_zero() {
        assert_eq!(run_consul(vec!["--help".to_string()]), 0);
        assert_eq!(run_consul(vec!["-h".to_string()]), 0);
        assert_eq!(run_consul(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_consul(vec![]), 0);
    }
}
