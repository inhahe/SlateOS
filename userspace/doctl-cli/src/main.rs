#![deny(clippy::all)]

//! doctl-cli — SlateOS DigitalOcean CLI
//!
//! Single personality: `doctl`

use std::env;
use std::process;

fn run_doctl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: doctl [GROUP] [COMMAND] [OPTIONS]");
        println!();
        println!("doctl — DigitalOcean CLI (Slate OS).");
        println!();
        println!("Groups:");
        println!("  compute         Manage Droplets, volumes, images, etc.");
        println!("  databases       Manage databases");
        println!("  kubernetes      Manage Kubernetes clusters");
        println!("  apps            Manage App Platform apps");
        println!("  serverless      Manage serverless functions");
        println!("  monitoring      Manage monitoring");
        println!("  registry        Manage container registry");
        println!("  vpcs            Manage VPCs");
        println!("  balance         Show account balance");
        println!("  account         Show account info");
        println!("  auth            Manage authentication");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("doctl version 1.100.0-release (Slate OS)");
        return 0;
    }

    let group = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let action = args.get(2).map(|s| s.as_str()).unwrap_or("");

    match group {
        "auth" => match sub {
            "init" => {
                println!("Please authenticate doctl for use with your DigitalOcean account.");
                println!("Enter your access token: ");
                println!("Validating token... OK");
            }
            _ => { println!("doctl auth: see doctl auth -h."); }
        },
        "account" => {
            println!("Email: user@example.com");
            println!("Team: My Team");
            println!("Status: active");
            println!("UUID: 12345678-abcd-1234-abcd-123456789012");
        }
        "balance" => {
            println!("Month-To-Date Balance:   $12.34");
            println!("Account Balance:         $100.00");
            println!("Month-To-Date Usage:     $12.34");
            println!("Generated At:            2024-01-15T12:00:00Z");
        }
        "compute" => match sub {
            "droplet" => match action {
                "list" => {
                    println!("ID          Name           Public IPv4      Region    Size        Status");
                    println!("12345678    my-droplet     143.198.12.34    nyc1      s-1vcpu-1gb active");
                    println!("23456789    web-server     143.198.56.78    sfo3      s-2vcpu-4gb active");
                }
                "create" => {
                    let name = args.iter().position(|a| a == "--name").and_then(|i| args.get(i + 1))
                        .map(|s| s.as_str()).unwrap_or("new-droplet");
                    println!("ID          Name           Public IPv4      Region    Status");
                    println!("34567890    {}    pending          nyc1      new", name);
                }
                _ => { println!("doctl compute droplet {}: see -h.", action); }
            },
            "volume" => {
                println!("ID                                      Name         Size      Region    Droplet IDs");
                println!("abcdef12-3456-7890-abcd-ef1234567890    my-vol       100 GiB   nyc1      12345678");
            }
            _ => { println!("doctl compute {}: see doctl compute -h.", sub); }
        },
        "kubernetes" => match sub {
            "cluster" => match action {
                "list" => {
                    println!("ID                                      Name           Region    Version    Status");
                    println!("abcdef12-3456-7890-abcd-ef1234567890    my-k8s         nyc1      1.28.2     running");
                }
                "kubeconfig" => {
                    println!("Notice: adding cluster credentials to kubeconfig file.");
                }
                _ => { println!("doctl kubernetes cluster {}: see -h.", action); }
            },
            _ => { println!("doctl kubernetes {}: see -h.", sub); }
        },
        "apps" => match sub {
            "list" => {
                println!("ID                                      Spec Name      Default Ingress     Active Deployment");
                println!("abcdef12-3456-7890-abcd-ef1234567890    my-app         my-app.ondigitalocean.app    OK");
            }
            _ => { println!("doctl apps {}: see -h.", sub); }
        },
        "registry" => match sub {
            "login" => { println!("Logging Docker in to registry.digitalocean.com"); }
            _ => { println!("doctl registry {}: see -h.", sub); }
        },
        _ => {
            if group.is_empty() {
                eprintln!("doctl: no command specified. See doctl --help.");
                return 1;
            }
            println!("doctl {}: see doctl {} -h.", group, group);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_doctl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_doctl};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_doctl(vec!["--help".to_string()]), 0);
        assert_eq!(run_doctl(vec!["-h".to_string()]), 0);
        let _ = run_doctl(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_doctl(vec![]);
    }
}
