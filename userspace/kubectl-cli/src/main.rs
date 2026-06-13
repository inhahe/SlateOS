#![deny(clippy::all)]

//! kubectl-cli — SlateOS Kubernetes command-line tool
//!
//! Single personality: `kubectl`

use std::env;
use std::process;

fn run_kubectl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kubectl [OPTIONS] <COMMAND>");
        println!();
        println!("Kubernetes command-line tool.");
        println!();
        println!("Basic commands:");
        println!("  get            Display resources");
        println!("  create         Create a resource");
        println!("  delete         Delete resources");
        println!("  apply          Apply a configuration");
        println!("  edit           Edit a resource");
        println!("  expose         Expose a resource as a service");
        println!("  run            Run a particular image");
        println!("  scale          Set new size for a deployment/replicaset");
        println!();
        println!("Deploy commands:");
        println!("  rollout        Manage rollout of a resource");
        println!("  autoscale      Auto-scale a deployment");
        println!();
        println!("Cluster management:");
        println!("  cluster-info   Display cluster info");
        println!("  top            Display resource usage");
        println!("  cordon         Mark node as unschedulable");
        println!("  uncordon       Mark node as schedulable");
        println!("  drain          Drain node for maintenance");
        println!();
        println!("Troubleshooting:");
        println!("  describe       Show details of a resource");
        println!("  logs           Print container logs");
        println!("  exec           Execute command in container");
        println!("  port-forward   Forward ports to a pod");
        println!("  cp             Copy files to/from containers");
        println!();
        println!("Settings:");
        println!("  config         Modify kubeconfig");
        println!("  version        Show version");
        println!();
        println!("Options:");
        println!("  -n, --namespace <NS>   Namespace");
        println!("  --context <CTX>        Context name");
        println!("  -o, --output <FMT>     Output format (json/yaml/wide/name)");
        println!("  -l, --selector <SEL>   Label selector");
        println!("  --all-namespaces       All namespaces");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Client Version: v1.29.1 (Slate OS)");
            println!("Server Version: v1.29.0");
            0
        }
        "get" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("pods");
            let wide = args.iter().any(|a| a == "-o" || a == "--output");
            match resource {
                "pods" | "pod" | "po" => {
                    println!("NAME                     READY   STATUS    RESTARTS   AGE");
                    println!("web-app-7d4f9b8c6-x2k4j  1/1    Running   0          3d");
                    println!("web-app-7d4f9b8c6-m9n2p  1/1    Running   0          3d");
                    println!("api-srv-5c6d7e8f9-a1b2c  1/1    Running   1          5d");
                    println!("redis-0                   1/1    Running   0          7d");
                    println!("postgres-0                1/1    Running   0          7d");
                }
                "svc" | "services" | "service" => {
                    println!("NAME         TYPE           CLUSTER-IP      EXTERNAL-IP    PORT(S)        AGE");
                    println!("kubernetes   ClusterIP      10.96.0.1       <none>         443/TCP        30d");
                    println!("web-app      LoadBalancer   10.96.45.123    54.123.45.67   80:31234/TCP   3d");
                    println!("api-srv      ClusterIP      10.96.78.234    <none>         8080/TCP       5d");
                    println!("redis        ClusterIP      10.96.12.345    <none>         6379/TCP       7d");
                }
                "nodes" | "node" | "no" => {
                    println!("NAME           STATUS   ROLES           AGE   VERSION");
                    println!("node-1         Ready    control-plane   30d   v1.29.0");
                    println!("node-2         Ready    <none>          30d   v1.29.0");
                    println!("node-3         Ready    <none>          30d   v1.29.0");
                }
                "deploy" | "deployments" | "deployment" => {
                    println!("NAME      READY   UP-TO-DATE   AVAILABLE   AGE");
                    println!("web-app   2/2     2            2           3d");
                    println!("api-srv   1/1     1            1           5d");
                }
                "ns" | "namespaces" | "namespace" => {
                    println!("NAME              STATUS   AGE");
                    println!("default           Active   30d");
                    println!("kube-system       Active   30d");
                    println!("kube-public       Active   30d");
                    println!("production        Active   20d");
                    println!("staging           Active   20d");
                }
                _ => println!("No resources found in default namespace."),
            }
            let _ = wide;
            0
        }
        "describe" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("pod");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("web-app-7d4f9b8c6-x2k4j");
            println!("Name:         {}", name);
            println!("Namespace:    default");
            println!("Node:         node-2/10.0.1.2");
            println!("Status:       Running");
            println!("IP:           10.244.1.23");
            println!("Containers:");
            println!("  {}:", resource);
            println!("    Image:    nginx:1.25");
            println!("    Port:     80/TCP");
            println!("    State:    Running");
            println!("    Ready:    True");
            println!("Events:       <none>");
            0
        }
        "logs" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("web-app-7d4f9b8c6-x2k4j");
            println!("[{}] 2024/01/15 14:30:00 Starting server on :80", name);
            println!("[{}] 2024/01/15 14:30:01 Server ready", name);
            println!("[{}] 2024/01/15 14:31:23 GET / 200 3ms", name);
            println!("[{}] 2024/01/15 14:31:45 GET /api/health 200 1ms", name);
            0
        }
        "apply" => {
            let file = args.windows(2)
                .find(|w| w[0] == "-f")
                .map(|w| w[1].as_str())
                .unwrap_or("manifest.yaml");
            println!("deployment.apps/web-app configured");
            println!("service/web-app unchanged");
            println!("  (applied from {})", file);
            0
        }
        "cluster-info" => {
            println!("Kubernetes control plane is running at https://10.0.0.1:6443");
            println!("CoreDNS is running at https://10.0.0.1:6443/api/v1/namespaces/kube-system/services/kube-dns:dns/proxy");
            0
        }
        "top" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("nodes");
            match resource {
                "nodes" | "node" => {
                    println!("NAME     CPU(cores)   CPU%   MEMORY(bytes)   MEMORY%");
                    println!("node-1   250m         12%    1024Mi          26%");
                    println!("node-2   180m         9%     890Mi           23%");
                    println!("node-3   320m         16%    1200Mi          31%");
                }
                "pods" | "pod" => {
                    println!("NAME                      CPU(cores)   MEMORY(bytes)");
                    println!("web-app-7d4f9b8c6-x2k4j   15m          64Mi");
                    println!("web-app-7d4f9b8c6-m9n2p   12m          58Mi");
                    println!("api-srv-5c6d7e8f9-a1b2c   45m          128Mi");
                }
                _ => println!("error: unknown resource type \"{}\"", resource),
            }
            0
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "get-contexts" => {
                    println!("CURRENT   NAME         CLUSTER      AUTHINFO     NAMESPACE");
                    println!("*         dev          dev-cluster  dev-user     default");
                    println!("          staging      stg-cluster  stg-user     staging");
                    println!("          production   prd-cluster  prd-user     production");
                }
                "current-context" => println!("dev"),
                "use-context" => {
                    let ctx = args.get(2).map(|s| s.as_str()).unwrap_or("dev");
                    println!("Switched to context \"{}\".", ctx);
                }
                _ => println!("Usage: kubectl config <get-contexts|current-context|use-context>"),
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: kubectl <command>. See --help.");
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
    let code = run_kubectl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_kubectl};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kubectl(vec!["--help".to_string()]), 0);
        assert_eq!(run_kubectl(vec!["-h".to_string()]), 0);
        let _ = run_kubectl(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kubectl(vec![]);
    }
}
