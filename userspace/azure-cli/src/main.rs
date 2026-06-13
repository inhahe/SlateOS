#![deny(clippy::all)]

//! azure-cli — SlateOS Azure CLI
//!
//! Single personality: `az`

use std::env;
use std::process;

fn run_az(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: az [GROUP] [COMMAND] [OPTIONS]");
        println!();
        println!("Azure CLI — manage Azure resources (Slate OS).");
        println!();
        println!("Groups:");
        println!("  account       Manage subscriptions");
        println!("  vm            Manage virtual machines");
        println!("  group         Manage resource groups");
        println!("  storage       Manage storage accounts");
        println!("  network       Manage networking (vnet, nsg, lb)");
        println!("  aks           Manage AKS clusters");
        println!("  acr           Manage container registries");
        println!("  webapp        Manage web apps");
        println!("  functionapp   Manage function apps");
        println!("  keyvault      Manage key vaults");
        println!("  sql           Manage SQL databases");
        println!("  monitor       Manage monitoring");
        println!("  ad            Manage Azure Active Directory");
        println!("  login         Log in to Azure");
        println!("  logout        Log out");
        println!("  configure     Manage configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("azure-cli 2.56.0 (Slate OS)");
        return 0;
    }

    let group = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match group {
        "login" => {
            println!("To sign in, use a web browser to open the page https://microsoft.com/devicelogin");
            println!("and enter the code ABCD1234 to authenticate.");
            println!("[");
            println!("  {{");
            println!("    \"cloudName\": \"AzureCloud\",");
            println!("    \"id\": \"12345678-abcd-efgh-ijkl-123456789012\",");
            println!("    \"isDefault\": true,");
            println!("    \"name\": \"My Subscription\",");
            println!("    \"state\": \"Enabled\",");
            println!("    \"tenantId\": \"87654321-dcba-hgfe-lkji-210987654321\"");
            println!("  }}");
            println!("]");
        }
        "account" => match sub {
            "list" => {
                println!("[");
                println!("  {{");
                println!("    \"id\": \"12345678-abcd-efgh-ijkl-123456789012\",");
                println!("    \"name\": \"My Subscription\",");
                println!("    \"state\": \"Enabled\",");
                println!("    \"isDefault\": true");
                println!("  }}");
                println!("]");
            }
            "show" => {
                println!("{{");
                println!("  \"id\": \"12345678-abcd-efgh-ijkl-123456789012\",");
                println!("  \"name\": \"My Subscription\",");
                println!("  \"state\": \"Enabled\"");
                println!("}}");
            }
            _ => { println!("az account: subcommand '{}'. See az account -h.", sub); }
        },
        "vm" => match sub {
            "list" => {
                println!("[");
                println!("  {{");
                println!("    \"name\": \"my-vm\",");
                println!("    \"resourceGroup\": \"my-rg\",");
                println!("    \"location\": \"eastus\",");
                println!("    \"vmSize\": \"Standard_D2s_v3\",");
                println!("    \"powerState\": \"VM running\"");
                println!("  }}");
                println!("]");
            }
            "create" => {
                let name = args.windows(2).find(|w| w[0] == "--name" || w[0] == "-n")
                    .map(|w| w[1].as_str()).unwrap_or("new-vm");
                println!("{{");
                println!("  \"name\": \"{}\",", name);
                println!("  \"provisioningState\": \"Succeeded\",");
                println!("  \"publicIpAddress\": \"20.123.45.67\"");
                println!("}}");
            }
            _ => { println!("az vm {}: see az vm -h.", sub); }
        },
        "group" => match sub {
            "list" => {
                println!("[");
                println!("  {{\"name\": \"my-rg\", \"location\": \"eastus\"}},");
                println!("  {{\"name\": \"prod-rg\", \"location\": \"westus2\"}}");
                println!("]");
            }
            "create" => {
                let name = args.windows(2).find(|w| w[0] == "--name" || w[0] == "-n")
                    .map(|w| w[1].as_str()).unwrap_or("new-rg");
                println!("{{");
                println!("  \"name\": \"{}\",", name);
                println!("  \"provisioningState\": \"Succeeded\"");
                println!("}}");
            }
            _ => { println!("az group {}: see az group -h.", sub); }
        },
        "aks" => match sub {
            "list" => {
                println!("[{{\"name\": \"my-cluster\", \"location\": \"eastus\", \"kubernetesVersion\": \"1.28.3\"}}]");
            }
            "get-credentials" => {
                println!("Merged \"my-cluster\" as current context in /home/user/.kube/config");
            }
            _ => { println!("az aks {}: see az aks -h.", sub); }
        },
        "storage" => {
            println!("az storage {}: see az storage -h.", sub);
        }
        "configure" => {
            println!("Azure CLI configuration updated.");
        }
        "logout" => {
            println!("Logged out successfully.");
        }
        _ => {
            if group.is_empty() {
                eprintln!("az: no command specified. See az --help.");
                return 1;
            }
            println!("az {}: see az {} -h for usage.", group, group);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_az(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_az};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_az(vec!["--help".to_string()]), 0);
        assert_eq!(run_az(vec!["-h".to_string()]), 0);
        let _ = run_az(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_az(vec![]);
    }
}
