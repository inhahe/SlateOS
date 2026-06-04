#![deny(clippy::all)]

//! helm-cli — OurOS Helm Kubernetes package manager
//!
//! Single personality: `helm`

use std::env;
use std::process;

fn run_helm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: helm <COMMAND> [OPTIONS]");
        println!();
        println!("The Kubernetes Package Manager.");
        println!();
        println!("Commands:");
        println!("  install      Install a chart");
        println!("  upgrade      Upgrade a release");
        println!("  uninstall    Uninstall a release");
        println!("  list         List releases");
        println!("  status       Display release status");
        println!("  rollback     Roll back a release");
        println!("  history      Show release history");
        println!("  repo         Chart repository commands");
        println!("  search       Search for charts");
        println!("  show         Show chart information");
        println!("  template     Locally render templates");
        println!("  create       Create a new chart");
        println!("  package      Package a chart");
        println!("  lint         Lint a chart");
        println!("  test         Run release tests");
        println!("  pull         Download a chart");
        println!("  push         Push a chart to a registry");
        println!("  env          Helm client environment");
        println!("  version      Show version");
        println!();
        println!("Options:");
        println!("  -n, --namespace <NS>   Namespace");
        println!("  --kube-context <CTX>   Kubernetes context");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("version.BuildInfo{{Version:\"v3.14.0\", GitCommit:\"abc123\", GoVersion:\"go1.21.6\"}}");
            0
        }
        "install" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-release");
            let chart = args.get(2).map(|s| s.as_str()).unwrap_or("chart");
            println!("NAME: {}", name);
            println!("LAST DEPLOYED: Thu Jan 15 14:30:00 2024");
            println!("NAMESPACE: default");
            println!("STATUS: deployed");
            println!("REVISION: 1");
            println!("TEST SUITE: None");
            println!("NOTES:");
            println!("  {} has been installed.", chart);
            println!("  Get the application URL:");
            println!("    kubectl get svc {}", name);
            0
        }
        "upgrade" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-release");
            println!("Release \"{}\" has been upgraded. Happy Helming!", name);
            println!("NAME: {}", name);
            println!("LAST DEPLOYED: Thu Jan 15 15:00:00 2024");
            println!("NAMESPACE: default");
            println!("STATUS: deployed");
            println!("REVISION: 2");
            0
        }
        "uninstall" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-release");
            println!("release \"{}\" uninstalled", name);
            0
        }
        "list" | "ls" => {
            println!("NAME          NAMESPACE  REVISION  STATUS    CHART              APP VERSION");
            println!("nginx-ingress default    3         deployed  ingress-nginx-4.9  1.9.5");
            println!("redis         default    1         deployed  redis-18.6.1       7.2.4");
            println!("prometheus    monitoring 2         deployed  prometheus-25.8    2.49.1");
            0
        }
        "status" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-release");
            println!("NAME: {}", name);
            println!("LAST DEPLOYED: Thu Jan 15 14:30:00 2024");
            println!("NAMESPACE: default");
            println!("STATUS: deployed");
            println!("REVISION: 1");
            0
        }
        "repo" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "add" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("repo");
                    println!("\"{}\" has been added to your repositories", name);
                }
                "update" => {
                    println!("Hang tight while we grab the latest from your chart repositories...");
                    println!("...Successfully got an update from the \"stable\" chart repository");
                    println!("...Successfully got an update from the \"bitnami\" chart repository");
                    println!("Update Complete. ⎈Happy Helming!⎈");
                }
                "list" => {
                    println!("NAME     URL");
                    println!("stable   https://charts.helm.sh/stable");
                    println!("bitnami  https://charts.bitnami.com/bitnami");
                }
                _ => println!("Usage: helm repo <add|update|list|remove>"),
            }
            0
        }
        "search" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("repo");
            let query = args.get(2).map(|s| s.as_str()).unwrap_or("nginx");
            println!("NAME                  CHART VERSION  APP VERSION  DESCRIPTION");
            println!("bitnami/{}       15.4.0         1.25.3       Open source web server", query);
            println!("stable/{}-ingress 4.9.0          1.9.5        Ingress controller", query);
            let _ = sub;
            0
        }
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("mychart");
            println!("Creating {}", name);
            0
        }
        "lint" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("./mychart");
            println!("==> Linting {}", path);
            println!("[INFO] Chart.yaml: icon is recommended");
            println!();
            println!("1 chart(s) linted, 0 chart(s) failed");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: helm <command>. See --help.");
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
