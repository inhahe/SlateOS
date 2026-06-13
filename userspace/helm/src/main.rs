#![deny(clippy::all)]

//! helm — Slate OS Kubernetes package manager
//!
//! Single personality: `helm`

use std::env;
use std::process;

fn run_helm(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("The Kubernetes package manager");
            println!();
            println!("Usage: helm [command]");
            println!();
            println!("Commands:");
            println!("  install     Install a chart");
            println!("  uninstall   Uninstall a release");
            println!("  upgrade     Upgrade a release");
            println!("  rollback    Roll back a release");
            println!("  list        List releases");
            println!("  status      Display status of a release");
            println!("  history     Fetch release history");
            println!("  repo        Manage chart repositories");
            println!("  search      Search for charts");
            println!("  create      Create a new chart");
            println!("  package     Package a chart");
            println!("  template    Locally render templates");
            println!("  lint        Examine a chart for issues");
            println!("  show        Show chart info");
            println!("  --version   Show version");
            0
        }
        "version" | "--version" => { println!("v3.14.0+g (Slate OS)"); 0 }
        "install" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("myrelease");
            let chart = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("stable/nginx");
            println!("NAME: {}", name);
            println!("LAST DEPLOYED: Thu May 22 10:00:00 2025");
            println!("NAMESPACE: default");
            println!("STATUS: deployed");
            println!("REVISION: 1");
            println!("NOTES:");
            println!("  Chart {} installed successfully.", chart);
            0
        }
        "uninstall" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("myrelease");
            println!("release \"{}\" uninstalled", name);
            0
        }
        "upgrade" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("myrelease");
            println!("Release \"{}\" has been upgraded. Happy Helming!", name);
            0
        }
        "list" | "ls" => {
            println!("NAME          NAMESPACE  REVISION  STATUS    CHART            APP VERSION");
            println!("nginx         default    1         deployed  nginx-15.0.0     1.25.0");
            println!("postgres      default    3         deployed  postgresql-13.0  16.0");
            println!("prometheus    monitoring 2         deployed  prometheus-25.0  2.50.0");
            0
        }
        "status" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("nginx");
            println!("NAME: {}", name);
            println!("STATUS: deployed");
            println!("REVISION: 1");
            0
        }
        "history" => {
            println!("REVISION  STATUS      CHART           DESCRIPTION");
            println!("1         superseded  nginx-14.0.0    Install complete");
            println!("2         superseded  nginx-14.1.0    Upgrade complete");
            println!("3         deployed    nginx-15.0.0    Upgrade complete");
            0
        }
        "repo" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "add" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("repo");
                    println!("\"{}\" has been added to your repositories", name);
                }
                "list" => {
                    println!("NAME      URL");
                    println!("stable    https://charts.helm.sh/stable");
                    println!("bitnami   https://charts.bitnami.com/bitnami");
                }
                "update" => println!("...Successfully got an update from the \"stable\" chart repository"),
                "remove" => println!("\"repo\" has been removed from your repositories"),
                _ => println!("repo {}: (simulated)", sub),
            }
            0
        }
        "search" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("hub");
            let query = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("nginx");
            println!("NAME                  CHART VERSION   APP VERSION   DESCRIPTION");
            println!("bitnami/nginx         15.0.0          1.25.0        NGINX web server");
            println!("bitnami/nginx-ingress 10.0.0          1.10.0        NGINX Ingress Controller");
            println!("({} search for '{}')", sub, query);
            0
        }
        "create" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("mychart");
            println!("Creating {}", name);
            0
        }
        "template" => { println!("---\n# Source: mychart/templates/deployment.yaml\napiVersion: apps/v1\nkind: Deployment\n(simulated)"); 0 }
        "lint" => { println!("==> Linting mychart\n[INFO] Chart.yaml: icon is recommended\n\n1 chart(s) linted, 0 chart(s) failed"); 0 }
        "package" => { println!("Successfully packaged chart and saved it to: mychart-0.1.0.tgz"); 0 }
        "show" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("chart");
            println!("({} info — simulated)", sub);
            0
        }
        "rollback" => { println!("Rollback was a success! (simulated)"); 0 }
        other => { eprintln!("Error: unknown command \"{}\"", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_helm(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_helm};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_helm(vec!["--help".to_string()]), 0);
        assert_eq!(run_helm(vec!["-h".to_string()]), 0);
        let _ = run_helm(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_helm(vec![]);
    }
}
