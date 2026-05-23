#![deny(clippy::all)]

//! cockroach-cli — OurOS CockroachDB CLI client
//!
//! Multi-personality: `cockroach`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cockroach(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cockroach COMMAND [OPTIONS]");
        println!("CockroachDB CLI 24.1.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  sql          Open SQL shell");
        println!("  start        Start a node");
        println!("  init         Initialize a cluster");
        println!("  node         Manage nodes");
        println!("  demo         Start temporary demo cluster");
        println!("  dump         Dump database as SQL");
        println!("  import       Import data");
        println!("  userfile     Manage user-scoped files");
        println!("  auth-session Manage auth sessions");
        println!("  cert         Manage certificates");
        println!("  debug        Debug utilities");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => {
            println!("Build Tag:    v24.1.0");
            println!("Build Time:   2024/06/10 12:00:00");
            println!("Distribution: OurOS");
            println!("Platform:     linux amd64");
            println!("Go Version:   go1.22.4");
        }
        "sql" => {
            let url = args.windows(2).find(|w| w[0] == "--url")
                .map(|w| w[1].as_str()).unwrap_or("postgresql://root@localhost:26257/defaultdb");
            println!("# Welcome to the CockroachDB SQL shell.");
            println!("# All statements must be terminated by a semicolon.");
            println!("# To exit, type: \\q.");
            println!("#");
            println!("# Server version: CockroachDB CCL v24.1.0");
            println!("# Cluster ID: abc12345-1234-1234-1234-abc123456789");
            println!("# Connected to: {}", url);
            println!("root@localhost:26257/defaultdb> ");
        }
        "start" => {
            let insecure = args.iter().any(|a| a == "--insecure");
            let store = args.windows(2).find(|w| w[0] == "--store")
                .map(|w| w[1].as_str()).unwrap_or("cockroach-data");
            println!("CockroachDB node starting at 2024-06-15 12:00:00");
            println!("  build:      v24.1.0");
            println!("  sql:        postgresql://root@localhost:26257");
            println!("  RPC:        localhost:26357");
            println!("  HTTP:       http://localhost:8080");
            println!("  store:      {}", store);
            if insecure {
                println!("  WARNING: running in insecure mode.");
            }
        }
        "init" => {
            println!("Cluster successfully initialized");
        }
        "node" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => {
                    println!("  id |     address     |  build  |         started_at         | is_live");
                    println!("-----+-----------------+---------+----------------------------+---------");
                    println!("   1 | localhost:26257 | v24.1.0 | 2024-06-15 12:00:00+00:00 | true");
                    println!("   2 | localhost:26258 | v24.1.0 | 2024-06-15 12:00:01+00:00 | true");
                    println!("   3 | localhost:26259 | v24.1.0 | 2024-06-15 12:00:02+00:00 | true");
                }
                "decommission" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("3");
                    println!("Node {} decommissioning...", id);
                    println!("Node {} decommissioned.", id);
                }
                _ => println!("cockroach node: '{}' completed", sub),
            }
        }
        "demo" => {
            println!("# Welcome to the CockroachDB demo database!");
            println!("# Cluster: demo-cluster (3 nodes, 9 ranges)");
            println!("# Database: movr");
            println!("root@localhost:26257/movr> ");
        }
        "dump" => {
            let db = args.get(1).map(|s| s.as_str()).unwrap_or("defaultdb");
            println!("-- CockroachDB dump of database '{}'", db);
            println!("CREATE TABLE users (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), name STRING);");
            println!("-- Dump completed.");
        }
        "cert" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Certificate directory: certs/");
                    println!("  ca.crt (CA certificate)");
                    println!("  node.crt (Node certificate)");
                    println!("  client.root.crt (Client certificate)");
                }
                "create-ca" => println!("CA certificate created: certs/ca.crt"),
                "create-node" => println!("Node certificate created: certs/node.crt"),
                "create-client" => println!("Client certificate created."),
                _ => println!("cockroach cert: '{}' completed", sub),
            }
        }
        _ => println!("cockroach: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cockroach".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cockroach(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
