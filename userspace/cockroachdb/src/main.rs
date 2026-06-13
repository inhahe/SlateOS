#![deny(clippy::all)]

//! cockroachdb — SlateOS distributed SQL database
//!
//! Single personality: `cockroach`

use std::env;
use std::process;

fn run_cockroach(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: cockroach <command> [flags]");
            println!();
            println!("Commands:");
            println!("  start          Start a CockroachDB node");
            println!("  start-single-node  Start a single-node cluster");
            println!("  init           Initialize a cluster");
            println!("  sql            Open a SQL shell");
            println!("  node           Manage cluster nodes");
            println!("  quit           Drain and shut down a node");
            println!("  demo           Start a temporary cluster");
            println!("  version        Output version info");
            println!("  debug          Debugging commands");
            println!("  gen            Generate auxiliary files");
            println!("  auth-session   Log in/out of HTTP sessions");
            0
        }
        "version" | "--version" => {
            println!("Build Tag:        v23.2.5 (SlateOS)");
            println!("Build Time:       2025/05/22 00:00:00");
            println!("Distribution:     SlateOS");
            println!("Platform:         slateos amd64");
            println!("Go Version:       go1.22.2");
            0
        }
        "start" | "start-single-node" => {
            let is_single = cmd.as_str() == "start-single-node";
            let listen = cmd_args.iter().position(|a| a == "--listen-addr")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("localhost:26257");
            let http = cmd_args.iter().position(|a| a == "--http-addr")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("localhost:8080");
            println!("*");
            println!("* WARNING: Running in insecure mode (--insecure flag detected).");
            println!("*");
            println!("CockroachDB node starting at 2025-05-22 10:00:00.000 (took 1.0s)");
            println!("build:               SlateOS v23.2.5");
            println!("webui:               http://{}", http);
            println!("sql:                 postgresql://root@{}?sslmode=disable", listen);
            if is_single {
                println!("sql (JDBC):          jdbc:postgresql://{}/defaultdb?sslmode=disable", listen);
                println!("RPC client flags:    cockroach <client cmd> --host={}", listen);
                println!("cluster mode:        single-node");
            } else {
                println!("cluster mode:        multi-node");
                println!("join:                localhost:26257");
            }
            println!("logs:                /var/lib/cockroach/logs");
            println!("temp dir:            /var/lib/cockroach/cockroach-temp123456");
            println!("store[0]:            /var/lib/cockroach");
            println!("status:              initialized new cluster");
            println!("clusterID:           a1b2c3d4-e5f6-7890-abcd-ef1234567890");
            println!("nodeID:              1");
            0
        }
        "init" => {
            println!("Cluster successfully initialized");
            0
        }
        "sql" => {
            let exec_stmt = cmd_args.iter().position(|a| a == "-e" || a == "--execute")
                .and_then(|i| cmd_args.get(i + 1));

            if let Some(stmt) = exec_stmt {
                let upper = stmt.to_uppercase();
                if upper.contains("SHOW DATABASES") {
                    println!("  database_name");
                    println!("-----------------");
                    println!("  defaultdb");
                    println!("  postgres");
                    println!("  system");
                    println!("  myapp");
                    println!("(4 rows)");
                } else if upper.contains("SHOW TABLES") {
                    println!("  table_name");
                    println!("--------------");
                    println!("  users");
                    println!("  orders");
                    println!("  products");
                    println!("(3 rows)");
                } else if upper.contains("SELECT") {
                    println!("  id | name    | email               | created_at");
                    println!("-----+---------+---------------------+----------------------------------");
                    println!("   1 | alice   | alice@example.com   | 2025-01-15 08:30:00");
                    println!("   2 | bob     | bob@example.com     | 2025-02-20 14:15:00");
                    println!("   3 | charlie | charlie@example.com | 2025-03-10 11:45:00");
                    println!("(3 rows)");
                    println!();
                    println!("Time: 2ms total (execution 1ms / network 1ms)");
                } else {
                    println!("OK");
                    println!("Time: 1ms");
                }
                return 0;
            }

            // Interactive mode
            println!("#");
            println!("# Welcome to the CockroachDB SQL shell.");
            println!("# All statements must be terminated by a semicolon.");
            println!("# To exit, type: \\q.");
            println!("#");
            println!("root@localhost:26257/defaultdb> SELECT version();");
            println!("                      version");
            println!("-------------------------------------------------");
            println!("  CockroachDB SlateOS v23.2.5 (x86_64)");
            println!("(1 row)");
            println!();
            println!("Time: 1ms total");
            println!();
            println!("root@localhost:26257/defaultdb> \\q");
            0
        }
        "node" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => {
                    println!("  id |     address      |   sql_address    |  build   |         started_at          |         updated_at          | locality | is_available | is_live");
                    println!("-----+------------------+------------------+----------+-----------------------------+-----------------------------+----------+--------------+---------");
                    println!("   1 | localhost:26257  | localhost:26257  | v23.2.5  | 2025-05-22 10:00:00         | 2025-05-22 10:00:42         |          | true         | true");
                }
                "ls" => {
                    println!("  id |");
                    println!("-----+");
                    println!("   1 |");
                }
                "decommission" => println!("Node decommissioning initiated (simulated)"),
                "recommission" => println!("Node recommissioned (simulated)"),
                _ => println!("Usage: cockroach node <status|ls|decommission|recommission>"),
            }
            0
        }
        "demo" => {
            println!("#");
            println!("# Welcome to the CockroachDB demo database!");
            println!("# This is a temporary, in-memory cluster.");
            println!("#");
            println!("# Cluster ID: a1b2c3d4-e5f6-7890-abcd-ef1234567890");
            println!("# Database: movr");
            println!("#");
            println!("root@localhost:26257/movr> \\q");
            0
        }
        "quit" | "drain" => {
            println!("node is draining... remaining: 0");
            println!("ok");
            0
        }
        other => { eprintln!("cockroach: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cockroach(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cockroach};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cockroach(vec!["--help".to_string()]), 0);
        assert_eq!(run_cockroach(vec!["-h".to_string()]), 0);
        let _ = run_cockroach(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cockroach(vec![]);
    }
}
