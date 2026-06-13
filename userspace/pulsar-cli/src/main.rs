#![deny(clippy::all)]

//! pulsar-cli — SlateOS Apache Pulsar CLI
//!
//! Single personality: `pulsar-admin`

use std::env;
use std::process;

fn run_pulsar_admin(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pulsar-admin <COMMAND> [OPTIONS]");
        println!();
        println!("Apache Pulsar admin CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  topics       Manage topics");
        println!("  tenants      Manage tenants");
        println!("  namespaces   Manage namespaces");
        println!("  subscriptions  Manage subscriptions");
        println!("  clusters     Manage clusters");
        println!("  brokers      Manage brokers");
        println!("  functions    Manage Pulsar functions");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pulsar-admin 3.2.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "topics" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let ns = args.get(2).map(|s| s.as_str()).unwrap_or("public/default");
                    println!("Topics in {}:", ns);
                    println!("  persistent://public/default/orders");
                    println!("  persistent://public/default/events");
                    println!("  persistent://public/default/notifications");
                }
                "stats" => {
                    let topic = args.get(2).map(|s| s.as_str()).unwrap_or("persistent://public/default/orders");
                    println!("Topic: {}", topic);
                    println!("  Messages In:     12,345/s");
                    println!("  Messages Out:    12,340/s");
                    println!("  Storage Size:    456.7 MB");
                    println!("  Subscriptions:   3");
                    println!("  Producers:       2");
                    println!("  Consumers:       6");
                    println!("  Backlog:         5 messages");
                }
                "create" => {
                    let topic = args.get(2).map(|s| s.as_str()).unwrap_or("persistent://public/default/new-topic");
                    println!("Topic {} created", topic);
                }
                _ => { println!("Topic operation: {}", sub); }
            }
            0
        }
        "tenants" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Tenants:");
                    println!("  public");
                    println!("  myorg");
                    println!("  analytics");
                }
                _ => { println!("Tenant operation: {}", sub); }
            }
            0
        }
        "namespaces" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let tenant = args.get(2).map(|s| s.as_str()).unwrap_or("public");
                    println!("Namespaces in {}:", tenant);
                    println!("  {}/default", tenant);
                    println!("  {}/production", tenant);
                    println!("  {}/staging", tenant);
                }
                _ => { println!("Namespace operation: {}", sub); }
            }
            0
        }
        "clusters" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Clusters:");
                    println!("  standalone");
                }
                "get" => {
                    println!("Cluster: standalone");
                    println!("  Service URL:   http://localhost:8080");
                    println!("  Broker URL:    pulsar://localhost:6650");
                    println!("  Brokers:       3");
                }
                _ => { println!("Cluster operation: {}", sub); }
            }
            0
        }
        "brokers" => {
            println!("Active Brokers:");
            println!("  localhost:8080  (leader)");
            println!("  node2:8080");
            println!("  node3:8080");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: pulsar-admin <command>. See --help.");
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
    let code = run_pulsar_admin(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pulsar_admin};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pulsar_admin(vec!["--help".to_string()]), 0);
        assert_eq!(run_pulsar_admin(vec!["-h".to_string()]), 0);
        let _ = run_pulsar_admin(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pulsar_admin(vec![]);
    }
}
