#![deny(clippy::all)]

//! gcloud-cli — Slate OS Google Cloud SDK CLI
//!
//! Single personality: `gcloud`

use std::env;
use std::process;

fn run_gcloud(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gcloud [OPTIONS] <GROUP> <COMMAND> [ARGS]");
        println!();
        println!("The Google Cloud CLI (Slate OS).");
        println!();
        println!("Groups:");
        println!("  compute      Compute Engine");
        println!("  container    GKE / container operations");
        println!("  iam          Identity and Access Management");
        println!("  projects     Project management");
        println!("  services     Service management");
        println!("  storage      Cloud Storage (gsutil)");
        println!("  functions    Cloud Functions");
        println!("  run          Cloud Run");
        println!("  sql          Cloud SQL");
        println!("  pubsub       Pub/Sub");
        println!("  logging      Cloud Logging");
        println!("  config       CLI configuration");
        println!("  auth         Authentication");
        println!("  info         CLI info");
        println!();
        println!("Options:");
        println!("  --project <PROJECT>  GCP project");
        println!("  --region <REGION>    GCP region");
        println!("  --zone <ZONE>        GCP zone");
        println!("  --format <FMT>       Output format (json/yaml/table/csv)");
        println!("  --quiet              Suppress prompts");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("Google Cloud SDK 460.0.0 (Slate OS)");
        println!("bq 2.0.100");
        println!("core 2024.01.15");
        println!("gsutil 5.27");
        return 0;
    }

    let group = args.first().map(|s| s.as_str()).unwrap_or("");
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match group {
        "info" => {
            println!("Google Cloud SDK [460.0.0]");
            println!("Account: [user@example.com]");
            println!("Project: [my-project-123]");
            println!("Current Properties:");
            println!("  [core]");
            println!("    account: [user@example.com]");
            println!("    project: [my-project-123]");
            println!("  [compute]");
            println!("    region: [us-central1]");
            println!("    zone: [us-central1-a]");
            0
        }
        "auth" => {
            match command {
                "list" => {
                    println!("             Credentialed Accounts");
                    println!("ACTIVE  ACCOUNT");
                    println!("*       user@example.com");
                    println!("        deploy@example.com");
                }
                "login" => {
                    println!("Your browser has been opened to visit:");
                    println!("  https://accounts.google.com/o/oauth2/auth?...");
                    println!();
                    println!("You are now logged in as [user@example.com].");
                }
                "print-access-token" => {
                    println!("ya29.A0ARrdaM...(truncated)");
                }
                _ => {
                    eprintln!("Usage: gcloud auth <list|login|print-access-token|revoke>. See --help.");
                    return 1;
                }
            }
            0
        }
        "config" => {
            match command {
                "list" => {
                    println!("[core]");
                    println!("account = user@example.com");
                    println!("project = my-project-123");
                    println!("[compute]");
                    println!("region = us-central1");
                    println!("zone = us-central1-a");
                }
                "set" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("core/project");
                    let val = args.get(3).map(|s| s.as_str()).unwrap_or("my-project");
                    println!("Updated property [{}] to [{}].", key, val);
                }
                _ => {
                    eprintln!("Usage: gcloud config <list|set|get-value|configurations>. See --help.");
                    return 1;
                }
            }
            0
        }
        "compute" => {
            match command {
                "instances" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("NAME          ZONE            MACHINE_TYPE   STATUS");
                            println!("web-server-1  us-central1-a   e2-medium      RUNNING");
                            println!("web-server-2  us-central1-b   e2-medium      RUNNING");
                            println!("db-server     us-central1-a   n2-standard-4  RUNNING");
                        }
                        "create" => {
                            let name = args.get(3).map(|s| s.as_str()).unwrap_or("new-instance");
                            println!("Created [https://compute.googleapis.com/.../instances/{}].", name);
                            println!("NAME          ZONE           MACHINE_TYPE  STATUS");
                            println!("{}  us-central1-a  e2-medium     RUNNING", name);
                        }
                        _ => { println!("Instance operation: {}", sub); }
                    }
                }
                "ssh" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("web-server-1");
                    println!("Updating project ssh metadata...");
                    println!("Waiting for SSH key to propagate.");
                    println!("Connected to {}.", name);
                }
                _ => {
                    eprintln!("Usage: gcloud compute <instances|ssh|disks|images|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "container" => {
            match command {
                "clusters" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("NAME          LOCATION       MASTER_VERSION  NUM_NODES  STATUS");
                            println!("prod-cluster  us-central1    1.28.3-gke.100  6          RUNNING");
                            println!("dev-cluster   us-east1       1.28.3-gke.100  3          RUNNING");
                        }
                        "get-credentials" => {
                            let name = args.get(3).map(|s| s.as_str()).unwrap_or("prod-cluster");
                            println!("Fetching cluster endpoint and auth data.");
                            println!("kubeconfig entry generated for {}.", name);
                        }
                        _ => { println!("Cluster operation: {}", sub); }
                    }
                }
                _ => {
                    eprintln!("Usage: gcloud container <clusters|node-pools|images>. See --help.");
                    return 1;
                }
            }
            0
        }
        "projects" => {
            match command {
                "list" => {
                    println!("PROJECT_ID         NAME              PROJECT_NUMBER");
                    println!("my-project-123     My Project        123456789012");
                    println!("staging-456        Staging           456789012345");
                }
                "describe" => {
                    let proj = args.get(2).map(|s| s.as_str()).unwrap_or("my-project-123");
                    println!("createTime: '2024-01-01T00:00:00.000Z'");
                    println!("name: My Project");
                    println!("projectId: {}", proj);
                    println!("projectNumber: '123456789012'");
                    println!("lifecycleState: ACTIVE");
                }
                _ => {
                    eprintln!("Usage: gcloud projects <list|describe|create|delete>. See --help.");
                    return 1;
                }
            }
            0
        }
        "run" => {
            match command {
                "services" => {
                    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    match sub {
                        "list" => {
                            println!("SERVICE       REGION         URL                                    LAST DEPLOYED");
                            println!("my-api        us-central1    https://my-api-abc123-uc.a.run.app     2024-01-15");
                            println!("web-frontend  us-east1       https://web-frontend-def456-ue.a.run.app 2024-01-14");
                        }
                        _ => { println!("Cloud Run services: {}", sub); }
                    }
                }
                "deploy" => {
                    let svc = args.get(2).map(|s| s.as_str()).unwrap_or("my-api");
                    println!("Deploying container to Cloud Run service [{}]...", svc);
                    println!("  Building...");
                    println!("  Deploying...");
                    println!("  Setting IAM policy...");
                    println!("Service [{}] revision [{}--00001-abc] has been deployed.", svc, svc);
                    println!("Service URL: https://{}-abc123-uc.a.run.app", svc);
                }
                _ => {
                    eprintln!("Usage: gcloud run <services|deploy|jobs>. See --help.");
                    return 1;
                }
            }
            0
        }
        _ => {
            if group.is_empty() {
                eprintln!("Usage: gcloud <group> <command>. See --help.");
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
    let code = run_gcloud(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gcloud};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gcloud(vec!["--help".to_string()]), 0);
        assert_eq!(run_gcloud(vec!["-h".to_string()]), 0);
        let _ = run_gcloud(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gcloud(vec![]);
    }
}
