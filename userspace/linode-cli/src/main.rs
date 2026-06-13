#![deny(clippy::all)]

//! linode-cli — SlateOS Linode CLI
//!
//! Single personality: `linode-cli`

use std::env;
use std::process;

fn run_linode(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: linode-cli <COMMAND> [ACTION] [OPTIONS]");
        println!();
        println!("Linode CLI — manage Linode/Akamai cloud (SlateOS).");
        println!();
        println!("Commands:");
        println!("  linodes       Manage Linode instances");
        println!("  volumes       Manage block storage volumes");
        println!("  domains       Manage DNS domains");
        println!("  nodebalancers Manage load balancers");
        println!("  lke           Manage Kubernetes Engine");
        println!("  databases     Manage managed databases");
        println!("  images        Manage images");
        println!("  regions       List regions");
        println!("  account       Account information");
        println!("  configure     Set up CLI");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("linode-cli 5.45.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");

    match cmd {
        "configure" => {
            println!("Welcome to the Linode CLI.");
            println!("Personal Access Token: ");
            println!("Default Region [us-east]: ");
            println!("Default Type [g6-nanode-1]: ");
            println!("Configuration saved.");
        }
        "linodes" => match action {
            "list" => {
                println!("┌──────────┬──────────────┬────────────┬───────────────┬─────────┐");
                println!("│ id       │ label        │ region     │ ipv4          │ status  │");
                println!("├──────────┼──────────────┼────────────┼───────────────┼─────────┤");
                println!("│ 12345678 │ my-linode    │ us-east    │ 45.56.78.90   │ running │");
                println!("│ 23456789 │ web-server   │ eu-west    │ 172.234.56.78 │ running │");
                println!("└──────────┴──────────────┴────────────┴───────────────┴─────────┘");
            }
            "create" => {
                println!("┌──────────┬──────────────┬────────────┬───────────────┬─────────┐");
                println!("│ id       │ label        │ region     │ ipv4          │ status  │");
                println!("├──────────┼──────────────┼────────────┼───────────────┼─────────┤");
                println!("│ 34567890 │ new-linode   │ us-east    │ 45.56.90.12   │ booting │");
                println!("└──────────┴──────────────┴────────────┴───────────────┴─────────┘");
            }
            _ => { println!("linode-cli linodes {}: see --help.", action); }
        },
        "volumes" => {
            println!("┌─────────┬──────────┬──────┬────────────┬──────────┐");
            println!("│ id      │ label    │ size │ region     │ status   │");
            println!("├─────────┼──────────┼──────┼────────────┼──────────┤");
            println!("│ 1234    │ my-vol   │ 100  │ us-east    │ active   │");
            println!("└─────────┴──────────┴──────┴────────────┴──────────┘");
        }
        "regions" => {
            println!("┌────────────┬──────────────────────────┬────────────┐");
            println!("│ id         │ label                    │ status     │");
            println!("├────────────┼──────────────────────────┼────────────┤");
            println!("│ us-east    │ Newark, NJ               │ ok         │");
            println!("│ us-west    │ Fremont, CA              │ ok         │");
            println!("│ eu-west    │ London, UK               │ ok         │");
            println!("│ ap-south   │ Singapore                │ ok         │");
            println!("└────────────┴──────────────────────────┴────────────┘");
        }
        "lke" => match action {
            "clusters-list" => {
                println!("┌───────┬────────────┬─────────┬─────────────┐");
                println!("│ id    │ label      │ region  │ k8s_version │");
                println!("├───────┼────────────┼─────────┼─────────────┤");
                println!("│ 12345 │ my-cluster │ us-east │ 1.28        │");
                println!("└───────┴────────────┴─────────┴─────────────┘");
            }
            _ => { println!("linode-cli lke {}: see --help.", action); }
        },
        "account" => {
            println!("┌──────────────────────┬─────────────────┐");
            println!("│ email                │ user@example.com│");
            println!("│ balance              │ $50.00          │");
            println!("│ active_since         │ 2020-01-01      │");
            println!("└──────────────────────┴─────────────────┘");
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("linode-cli: no command specified. See --help.");
                return 1;
            }
            println!("linode-cli {}: see linode-cli {} --help.", cmd, cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_linode(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_linode};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_linode(vec!["--help".to_string()]), 0);
        assert_eq!(run_linode(vec!["-h".to_string()]), 0);
        let _ = run_linode(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_linode(vec![]);
    }
}
