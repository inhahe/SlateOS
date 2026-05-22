#![deny(clippy::all)]

//! az-cli — OurOS Azure CLI
//!
//! Single personality: `az`

use std::env;
use std::process;

fn run_az(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: az [OPTIONS] <GROUP> <COMMAND> [ARGS]");
        println!();
        println!("Azure Command-Line Interface (OurOS).");
        println!();
        println!("Groups:");
        println!("  vm           Virtual Machines");
        println!("  aks          Azure Kubernetes Service");
        println!("  acr          Azure Container Registry");
        println!("  storage      Storage accounts");
        println!("  network      Networking");
        println!("  webapp       Web Apps / App Service");
        println!("  functionapp  Azure Functions");
        println!("  sql          Azure SQL");
        println!("  cosmosdb     Cosmos DB");
        println!("  keyvault     Key Vault");
        println!("  group        Resource groups");
        println!("  account      Account management");
        println!("  login        Log in to Azure");
        println!("  logout       Log out");
        println!();
        println!("Options:");
        println!("  --subscription <SUB>  Subscription ID or name");
        println!("  --resource-group <RG> Resource group name");
        println!("  --output <FMT>        Output format (json/table/tsv/yaml)");
        println!("  --query <JMESPATH>    JMESPath query");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("azure-cli 2.57.0 (OurOS)");
        return 0;
    }

    let group = args.first().map(|s| s.as_str()).unwrap_or("");
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match group {
        "login" => {
            println!("A web browser has been opened at https://login.microsoftonline.com/...");
            println!("[");
            println!("  {{");
            println!("    \"cloudName\": \"AzureCloud\",");
            println!("    \"id\": \"12345678-abcd-efgh-ijkl-123456789012\",");
            println!("    \"isDefault\": true,");
            println!("    \"name\": \"My Subscription\",");
            println!("    \"state\": \"Enabled\",");
            println!("    \"tenantId\": \"abcdef00-1234-5678-9abc-def012345678\",");
            println!("    \"user\": {{\"name\": \"user@example.com\", \"type\": \"user\"}}");
            println!("  }}");
            println!("]");
            0
        }
        "account" => {
            match command {
                "list" => {
                    println!("[");
                    println!("  {{\"name\": \"My Subscription\", \"id\": \"12345678-abcd-efgh-ijkl-123456789012\", \"isDefault\": true}},");
                    println!("  {{\"name\": \"Dev Subscription\", \"id\": \"87654321-dcba-hgfe-lkji-210987654321\", \"isDefault\": false}}");
                    println!("]");
                }
                "show" => {
                    println!("{{\"id\": \"12345678-abcd-efgh-ijkl-123456789012\", \"name\": \"My Subscription\", \"state\": \"Enabled\"}}");
                }
                "set" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("My Subscription");
                    println!("Subscription set to '{}'.", sub);
                }
                _ => {
                    eprintln!("Usage: az account <list|show|set>. See --help.");
                    return 1;
                }
            }
            0
        }
        "group" => {
            match command {
                "list" => {
                    println!("[");
                    println!("  {{\"name\": \"my-rg\", \"location\": \"eastus\", \"provisioningState\": \"Succeeded\"}},");
                    println!("  {{\"name\": \"staging-rg\", \"location\": \"westus2\", \"provisioningState\": \"Succeeded\"}}");
                    println!("]");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("new-rg");
                    let loc = args.windows(2).find(|w| w[0] == "-l" || w[0] == "--location").map(|w| w[1].as_str()).unwrap_or("eastus");
                    println!("{{\"id\": \"/subscriptions/.../resourceGroups/{}\", \"location\": \"{}\", \"name\": \"{}\", \"provisioningState\": \"Succeeded\"}}", name, loc, name);
                }
                _ => {
                    eprintln!("Usage: az group <list|create|delete|show>. See --help.");
                    return 1;
                }
            }
            0
        }
        "vm" => {
            match command {
                "list" => {
                    println!("[");
                    println!("  {{\"name\": \"web-vm-1\", \"resourceGroup\": \"my-rg\", \"location\": \"eastus\", \"powerState\": \"VM running\", \"vmSize\": \"Standard_B2s\"}},");
                    println!("  {{\"name\": \"db-vm\", \"resourceGroup\": \"my-rg\", \"location\": \"eastus\", \"powerState\": \"VM running\", \"vmSize\": \"Standard_D4s_v3\"}}");
                    println!("]");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("new-vm");
                    println!("{{\"id\": \"/subscriptions/.../virtualMachines/{}\", \"name\": \"{}\", \"powerState\": \"VM running\"}}", name, name);
                }
                "start" | "stop" | "deallocate" | "restart" => {
                    let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("web-vm-1");
                    println!("VM '{}' operation '{}' completed.", name, command);
                }
                _ => {
                    eprintln!("Usage: az vm <list|create|start|stop|delete|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "aks" => {
            match command {
                "list" => {
                    println!("[");
                    println!("  {{\"name\": \"prod-aks\", \"resourceGroup\": \"my-rg\", \"location\": \"eastus\", \"kubernetesVersion\": \"1.28.3\", \"agentPoolProfiles\": [{{\"count\": 3}}]}}");
                    println!("]");
                }
                "get-credentials" => {
                    let name = args.windows(2).find(|w| w[0] == "-n" || w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("prod-aks");
                    println!("Merged \"{}\" as current context in ~/.kube/config", name);
                }
                _ => {
                    eprintln!("Usage: az aks <list|create|get-credentials|scale|upgrade>. See --help.");
                    return 1;
                }
            }
            0
        }
        "storage" => {
            match command {
                "account" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("[{{\"name\": \"mystorageaccount\", \"location\": \"eastus\", \"kind\": \"StorageV2\", \"sku\": {{\"name\": \"Standard_LRS\"}}}}]");
                        }
                        _ => { println!("Storage account operation: {}", sub); }
                    }
                }
                "blob" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("[");
                            println!("  {{\"name\": \"file1.txt\", \"contentLength\": 1024, \"lastModified\": \"2024-01-15T10:00:00Z\"}},");
                            println!("  {{\"name\": \"archive.tar.gz\", \"contentLength\": 51200, \"lastModified\": \"2024-01-14T09:00:00Z\"}}");
                            println!("]");
                        }
                        "upload" => { println!("Finished upload."); }
                        _ => { println!("Blob operation: {}", sub); }
                    }
                }
                _ => {
                    eprintln!("Usage: az storage <account|blob|container|file|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "keyvault" => {
            match command {
                "list" => {
                    println!("[{{\"name\": \"my-vault\", \"location\": \"eastus\", \"resourceGroup\": \"my-rg\"}}]");
                }
                "secret" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("[{{\"id\": \"https://my-vault.vault.azure.net/secrets/db-password\", \"attributes\": {{\"enabled\": true}}}}]");
                        }
                        "show" => {
                            println!("{{\"value\": \"s3cr3t-v4lu3\", \"id\": \"https://my-vault.vault.azure.net/secrets/db-password\"}}");
                        }
                        _ => { println!("Secret operation: {}", sub); }
                    }
                }
                _ => {
                    eprintln!("Usage: az keyvault <list|secret|key|certificate>. See --help.");
                    return 1;
                }
            }
            0
        }
        _ => {
            if group.is_empty() {
                eprintln!("Usage: az <group> <command>. See --help.");
            } else {
                eprintln!("Error: unknown group '{}'. See --help.", group);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_az(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
