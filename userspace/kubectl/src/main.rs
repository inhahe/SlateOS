#![deny(clippy::all)]

//! kubectl — OurOS Kubernetes command-line tool
//!
//! Single personality: `kubectl`

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_kubectl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("kubectl controls the Kubernetes cluster manager.");
            println!();
            println!("Usage: kubectl [flags] [command] [TYPE[.GROUP]/NAME] [flags]");
            println!();
            println!("Basic Commands:");
            println!("  create      Create a resource from a file or stdin");
            println!("  get         Display one or many resources");
            println!("  describe    Show details of a resource");
            println!("  delete      Delete resources");
            println!("  edit        Edit a resource");
            println!("  apply       Apply a configuration to a resource");
            println!("  patch       Update fields of a resource");
            println!();
            println!("Deploy Commands:");
            println!("  rollout     Manage the rollout of a resource");
            println!("  scale       Set a new size for a resource");
            println!("  expose      Expose a resource as a new Kubernetes Service");
            println!();
            println!("Cluster Management:");
            println!("  cluster-info  Display cluster info");
            println!("  top           Display resource usage");
            println!("  cordon        Mark node as unschedulable");
            println!("  drain         Drain node for maintenance");
            println!();
            println!("Troubleshooting:");
            println!("  logs        Print container logs");
            println!("  exec        Execute a command in a container");
            println!("  port-forward  Forward ports");
            println!("  cp           Copy files to/from containers");
            println!();
            println!("Other:");
            println!("  config      Modify kubeconfig files");
            println!("  version     Print the client and server version");
            0
        }
        "version" | "--version" => {
            let short = cmd_args.iter().any(|a| a == "--short");
            if short {
                println!("Client Version: v1.29.0 (OurOS)");
            } else {
                println!("Client Version: v1.29.0 (OurOS)");
                println!("Server Version: v1.29.0 (simulated)");
            }
            0
        }
        "get" => {
            let resource = cmd_args.first().map(|s| s.as_str()).unwrap_or("pods");
            let wide = cmd_args.iter().any(|a| a == "-o" && cmd_args.iter().any(|b| b == "wide")) ||
                cmd_args.iter().any(|a| a == "-o=wide");
            let namespace = cmd_args.iter().position(|a| a == "-n" || a == "--namespace")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("default");

            match resource {
                "pods" | "pod" | "po" => {
                    println!("NAME                      READY   STATUS    RESTARTS   AGE{}", if wide { "   IP            NODE" } else { "" });
                    println!("nginx-7f456bf56d-x2k9l    1/1     Running   0          2d{}", if wide { "    10.244.1.5    node-01" } else { "" });
                    println!("postgres-5b8f4c9d6-m3n7p  1/1     Running   0          5d{}", if wide { "    10.244.2.8    node-02" } else { "" });
                    println!("redis-8c7d6e5f4-k9j8h     1/1     Running   1          3d{}", if wide { "    10.244.1.12   node-01" } else { "" });
                }
                "services" | "service" | "svc" => {
                    println!("NAME         TYPE        CLUSTER-IP     EXTERNAL-IP   PORT(S)          AGE");
                    println!("kubernetes   ClusterIP   10.96.0.1      <none>        443/TCP          30d");
                    println!("nginx        NodePort    10.96.45.123   <none>        80:30080/TCP     2d");
                    println!("postgres     ClusterIP   10.96.78.56    <none>        5432/TCP         5d");
                }
                "nodes" | "node" | "no" => {
                    println!("NAME      STATUS   ROLES           AGE   VERSION");
                    println!("node-01   Ready    control-plane   30d   v1.29.0");
                    println!("node-02   Ready    <none>          30d   v1.29.0");
                    println!("node-03   Ready    <none>          30d   v1.29.0");
                }
                "deployments" | "deployment" | "deploy" => {
                    println!("NAME       READY   UP-TO-DATE   AVAILABLE   AGE");
                    println!("nginx      1/1     1            1           2d");
                    println!("postgres   1/1     1            1           5d");
                    println!("redis      1/1     1            1           3d");
                }
                "namespaces" | "namespace" | "ns" => {
                    println!("NAME              STATUS   AGE");
                    println!("default           Active   30d");
                    println!("kube-system       Active   30d");
                    println!("kube-public       Active   30d");
                }
                _ => println!("No resources found in {} namespace.", namespace),
            }
            0
        }
        "describe" => {
            let resource = cmd_args.first().map(|s| s.as_str()).unwrap_or("pod");
            let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("nginx");
            println!("Name:         {}", name);
            println!("Namespace:    default");
            println!("Node:         node-01/10.0.0.1");
            println!("Status:       Running");
            println!("IP:           10.244.1.5");
            println!("Containers:");
            println!("  {}:", name);
            println!("    Image:        {}:latest", name);
            println!("    Port:         80/TCP");
            println!("    State:        Running");
            println!("    Ready:        True");
            println!("Events:");
            println!("  Type    Reason   Age  Message");
            println!("  Normal  Pulled   2d   Container image pulled");
            println!("  Normal  Created  2d   Created container {}", name);
            println!("  Normal  Started  2d   Started container {}", name);
            println!("(resource type: {})", resource);
            0
        }
        "logs" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("nginx");
            let follow = cmd_args.iter().any(|a| a == "-f" || a == "--follow");
            println!("[{}] 2025-05-22T10:00:00Z Starting...", name);
            println!("[{}] 2025-05-22T10:00:01Z Listening on :80", name);
            println!("[{}] 2025-05-22T10:00:05Z GET / 200 0.5ms", name);
            if follow { println!("(following logs — simulated)"); }
            0
        }
        "apply" => {
            let file = cmd_args.iter().position(|a| a == "-f")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("manifest.yaml");
            println!("deployment.apps/nginx configured");
            println!("service/nginx configured");
            println!("(from: {})", file);
            0
        }
        "create" => {
            println!("resource created (simulated)");
            0
        }
        "delete" => {
            let resource = cmd_args.first().map(|s| s.as_str()).unwrap_or("pod");
            let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("resource");
            println!("{} \"{}\" deleted", resource, name);
            0
        }
        "exec" => {
            println!("(executing in container — simulated)");
            0
        }
        "scale" => {
            let replicas = cmd_args.iter().find(|a| a.starts_with("--replicas="))
                .map(|a| a.split('=').nth(1).unwrap_or("1"))
                .unwrap_or("1");
            println!("deployment.apps/nginx scaled to {} replicas", replicas);
            0
        }
        "rollout" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "status" => println!("deployment \"nginx\" successfully rolled out"),
                "history" => {
                    println!("REVISION  CHANGE-CAUSE");
                    println!("1         Initial deploy");
                    println!("2         Update image to nginx:1.25");
                }
                "restart" => println!("deployment.apps/nginx restarted"),
                "undo" => println!("deployment.apps/nginx rolled back"),
                _ => println!("rollout {}: (simulated)", sub),
            }
            0
        }
        "cluster-info" => {
            println!("Kubernetes control plane is running at https://10.0.0.1:6443");
            println!("CoreDNS is running at https://10.0.0.1:6443/api/v1/namespaces/kube-system/services/kube-dns:dns/proxy");
            0
        }
        "top" => {
            let resource = cmd_args.first().map(|s| s.as_str()).unwrap_or("nodes");
            match resource {
                "nodes" | "node" => {
                    println!("NAME      CPU(cores)   CPU%   MEMORY(bytes)   MEMORY%");
                    println!("node-01   250m         12%    1024Mi          25%");
                    println!("node-02   180m         9%     768Mi           18%");
                }
                "pods" | "pod" => {
                    println!("NAME                      CPU(cores)   MEMORY(bytes)");
                    println!("nginx-7f456bf56d-x2k9l    10m          64Mi");
                    println!("postgres-5b8f4c9d6-m3n7p  50m          256Mi");
                }
                _ => println!("top {}: (simulated)", resource),
            }
            0
        }
        "config" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("view");
            match sub {
                "view" => {
                    println!("apiVersion: v1");
                    println!("clusters:");
                    println!("- cluster:");
                    println!("    server: https://10.0.0.1:6443");
                    println!("  name: ouros-cluster");
                    println!("contexts:");
                    println!("- context:");
                    println!("    cluster: ouros-cluster");
                    println!("    user: admin");
                    println!("  name: ouros-context");
                    println!("current-context: ouros-context");
                }
                "get-contexts" => {
                    println!("CURRENT   NAME             CLUSTER          USER");
                    println!("*         ouros-context    ouros-cluster    admin");
                }
                "use-context" => {
                    let ctx = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("context");
                    println!("Switched to context \"{}\".", ctx);
                }
                _ => println!("config {}: (simulated)", sub),
            }
            0
        }
        "port-forward" => { println!("Forwarding from 127.0.0.1:8080 -> 80 (simulated)"); 0 }
        "expose" => { println!("service/nginx exposed (simulated)"); 0 }
        "cordon" => { println!("node/node-02 cordoned"); 0 }
        "drain" => { println!("node/node-02 drained (simulated)"); 0 }
        "edit" => { println!("(editing resource — simulated)"); 0 }
        "patch" => { println!("resource patched (simulated)"); 0 }
        other => { eprintln!("kubectl: unknown command \"{}\"", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kubectl(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

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
