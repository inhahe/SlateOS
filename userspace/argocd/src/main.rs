#![deny(clippy::all)]

//! argocd — OurOS Argo CD GitOps controller
//!
//! Single personality: `argocd`

use std::env;
use std::process;

fn run_argocd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: argocd <command> [flags]");
        println!();
        println!("Commands:");
        println!("  app          Manage applications");
        println!("  cluster      Manage clusters");
        println!("  context      Manage contexts");
        println!("  login        Log in to server");
        println!("  logout       Log out from server");
        println!("  proj         Manage projects");
        println!("  repo         Manage repositories");
        println!("  repocreds    Manage repository credentials");
        println!("  account      Manage accounts");
        println!("  admin        Admin commands");
        println!("  cert         Manage repository certificates");
        println!("  gpg          Manage GPG keys");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("argocd: v2.11.3 (OurOS)");
            println!("  BuildDate: 2025-05-22T10:00:00Z");
            println!("  GitCommit: (simulated)");
            println!("  GoVersion: go1.22");
        }
        "app" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("NAME       CLUSTER                 NAMESPACE  PROJECT  STATUS  HEALTH   SYNCPOLICY");
                    println!("myapp      https://kubernetes.local default    default  Synced  Healthy  Auto-Prune");
                    println!("nginx      https://kubernetes.local ingress    default  Synced  Healthy  Manual");
                }
                "get" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myapp");
                    println!("Name:               {}", name);
                    println!("Project:            default");
                    println!("Server:             https://kubernetes.local");
                    println!("Namespace:          default");
                    println!("Repo:               https://github.com/org/repo.git");
                    println!("Path:               deploy/");
                    println!("Target Revision:    HEAD");
                    println!("Sync Status:        Synced");
                    println!("Health Status:      Healthy");
                }
                "sync" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myapp");
                    println!("TIMESTAMP                  GROUP  KIND         NAMESPACE  NAME   STATUS  HEALTH   HOOK  MESSAGE");
                    println!("2025-05-22T10:00:00+00:00         Deployment   default    {}   Synced  Healthy        deployment configured", name);
                }
                "diff" => println!("(no diff — application is in sync)"),
                "history" => {
                    println!("ID  DATE                           REVISION");
                    println!("1   2025-05-22 10:00:00 +0000 UTC  abc123");
                    println!("2   2025-05-22 09:00:00 +0000 UTC  def456");
                }
                _ => println!("Subcommands: list, get, sync, diff, history, create, delete, set, wait, rollback"),
            }
        }
        "cluster" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("SERVER                          NAME        VERSION  STATUS");
                    println!("https://kubernetes.default.svc  in-cluster  1.29     Successful");
                }
                _ => println!("Subcommands: list, add, remove, get"),
            }
        }
        "login" => println!("'admin:login' logged in successfully"),
        "logout" => println!("Logged out from server"),
        "repo" | "proj" | "account" | "admin" | "cert" | "gpg" | "context" | "repocreds" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_argocd(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_argocd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_argocd(vec!["--help".to_string()]), 0);
        assert_eq!(run_argocd(vec!["-h".to_string()]), 0);
        let _ = run_argocd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_argocd(vec![]);
    }
}
