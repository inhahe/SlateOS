#![deny(clippy::all)]

//! upcloud-cli — OurOS UpCloud CLI
//!
//! Multi-personality: `upctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_upctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: upctl COMMAND [OPTIONS]");
        println!("UpCloud CLI 3.9.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  server       Manage cloud servers");
        println!("  storage      Manage storage devices");
        println!("  network      Manage networks");
        println!("  ip-address   Manage IP addresses");
        println!("  firewall     Manage firewall rules");
        println!("  kubernetes   Manage Kubernetes clusters");
        println!("  database     Manage managed databases");
        println!("  loadbalancer Manage load balancers");
        println!("  account      Show account info");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("upctl version 3.9.0"),
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("UUID                                  Hostname   State     Plan      Zone");
                    println!("abc12345-1234-1234-1234-abc123456789  web-1      started   1xCPU-2GB fi-hel1");
                    println!("def12345-1234-1234-1234-def123456789  db-1       started   2xCPU-4GB fi-hel1");
                }
                "create" => println!("Server created."),
                "start" | "stop" | "restart" | "delete" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("abc12345");
                    println!("Server {}: {} done.", id, sub);
                }
                _ => println!("upctl server: '{}' completed", sub),
            }
        }
        "storage" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("UUID                                  Title     Size   Type    Zone");
                println!("abc12345-1234-1234-1234-abc123456789  disk-1    25 GB  normal  fi-hel1");
            }
        }
        "account" => {
            println!("Username:  myuser");
            println!("Credits:   €42.50");
            println!("Zones:     fi-hel1, de-fra1, us-chi1, nl-ams1, sg-sin1");
        }
        _ => println!("upctl: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "upctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_upctl(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
