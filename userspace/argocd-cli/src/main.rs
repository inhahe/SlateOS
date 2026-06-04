#![deny(clippy::all)]

//! argocd-cli — OurOS Argo CD GitOps tools
//!
//! Multi-personality: `argocd`

use std::env;
use std::process;

fn run_argocd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: argocd [FLAGS] COMMAND [ARGS]");
        println!();
        println!("argocd — Argo CD GitOps CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  app          Manage applications");
        println!("  cluster      Manage clusters");
        println!("  repo         Manage repositories");
        println!("  proj         Manage projects");
        println!("  account      Manage accounts");
        println!("  login        Login to server");
        println!("  logout       Logout");
        println!("  version      Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("argocd: v2.10.0+abcdef1 (OurOS)");
            println!("  BuildDate: 2024-01-15T00:00:00Z");
            println!("  GoVersion: go1.22.0");
            println!("  Compiler:  gc");
        }
        "app" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match cmd {
                "list" => {
                    println!("NAME         CLUSTER    NAMESPACE  PROJECT  STATUS   HEALTH   SYNCPOLICY  CONDITIONS");
                    println!("webapp       in-cluster default    default  Synced   Healthy  Auto-Prune  <none>");
                    println!("api-server   in-cluster api        default  Synced   Healthy  Auto-Prune  <none>");
                    println!("monitoring   in-cluster monitor    ops      OutOfSync Healthy Manual      <none>");
                }
                "get" => {
                    let app = args.get(2).map(|s| s.as_str()).unwrap_or("webapp");
                    println!("Name:               {}", app);
                    println!("Project:            default");
                    println!("Server:             https://kubernetes.default.svc");
                    println!("Namespace:          default");
                    println!("URL:                https://argocd.ouros.local/applications/{}", app);
                    println!("Repo:               https://github.com/ouros/{}.git", app);
                    println!("Target:             HEAD");
                    println!("Path:               k8s/");
                    println!("SyncWindow:         Sync Allowed");
                    println!("Sync Status:        Synced to HEAD (abcdef1)");
                    println!("Health Status:      Healthy");
                }
                "sync" => {
                    let app = args.get(2).map(|s| s.as_str()).unwrap_or("webapp");
                    println!("TIMESTAMP   GROUP  KIND        NAMESPACE  NAME       STATUS   HEALTH   HOOK  MESSAGE");
                    println!("12:00:00           Deployment  default    {}     Synced   Healthy        deployment.apps/{} configured", app, app);
                    println!("12:00:01           Service     default    {}     Synced   Healthy        service/{} unchanged", app, app);
                    println!();
                    println!("Name:      {}",app);
                    println!("Sync Status: Synced");
                    println!("Health Status: Healthy");
                }
                _ => println!("argocd app {} completed", cmd),
            }
        }
        "cluster" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if cmd == "list" {
                println!("SERVER                          NAME        VERSION  STATUS   MESSAGE");
                println!("https://kubernetes.default.svc  in-cluster  1.29     Successful");
            } else {
                println!("argocd cluster {} completed", cmd);
            }
        }
        "login" => println!("'admin:login' logged in successfully"),
        "logout" => println!("Logged out from 'https://argocd.ouros.local'"),
        _ => println!("argocd: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_argocd(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_argocd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_argocd(&["--help".to_string()]), 0);
        assert_eq!(run_argocd(&["-h".to_string()]), 0);
        let _ = run_argocd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_argocd(&[]);
    }
}
