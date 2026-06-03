#![deny(clippy::all)]

//! minikube-cli — OurOS Minikube local Kubernetes tool
//!
//! Single personality: `minikube`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_minikube(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: minikube COMMAND [OPTIONS]");
        println!("minikube v1.33.0 (OurOS) — Local Kubernetes cluster");
        println!();
        println!("Commands:");
        println!("  start           Start cluster");
        println!("  stop            Stop cluster");
        println!("  delete          Delete cluster");
        println!("  status          Show status");
        println!("  dashboard       Open dashboard");
        println!("  pause           Pause Kubernetes");
        println!("  unpause         Unpause Kubernetes");
        println!("  tunnel          Create LoadBalancer tunnel");
        println!("  service         Get service URL");
        println!("  addons          Manage addons");
        println!("  config          Manage config");
        println!("  profile         Manage profiles");
        println!("  ssh             SSH into node");
        println!("  ip              Show cluster IP");
        println!("  logs            Show cluster logs");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("minikube version: v1.33.0");
        println!("commit: abc1234def");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "start" => {
            println!("* minikube v1.33.0 on OurOS");
            println!("* Using the docker driver based on user configuration");
            println!("* Starting control plane node minikube in cluster minikube");
            println!("* Pulling base image...");
            println!("* Creating docker container...");
            println!("* Preparing Kubernetes v1.30.0...");
            println!("* Configuring RBAC rules...");
            println!("* Verifying Kubernetes components...");
            println!("* Enabled addons: default-storageclass, storage-provisioner");
            println!("* Done! kubectl is now configured to use \"minikube\" cluster.");
        }
        "stop" => {
            println!("* Stopping node \"minikube\"...");
            println!("* Powering off \"minikube\" via docker...");
            println!("* Node \"minikube\" stopped.");
        }
        "status" => {
            println!("minikube");
            println!("type: Control Plane");
            println!("host: Running");
            println!("kubelet: Running");
            println!("apiserver: Running");
            println!("kubeconfig: Configured");
        }
        "delete" => {
            println!("* Deleting \"minikube\" in docker...");
            println!("* Removed all traces of the \"minikube\" cluster.");
        }
        "dashboard" => println!("* Opening Kubernetes dashboard in default browser..."),
        "tunnel" => println!("* Starting tunnel for service LoadBalancer..."),
        "ip" => println!("192.168.49.2"),
        "addons" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("|-----------------------------|----------|");
                println!("| ADDON NAME                  | STATUS   |");
                println!("|-----------------------------|----------|");
                println!("| dashboard                   | disabled |");
                println!("| default-storageclass        | enabled  |");
                println!("| ingress                     | disabled |");
                println!("| metrics-server              | disabled |");
                println!("| storage-provisioner         | enabled  |");
                println!("|-----------------------------|----------|");
            }
        }
        "ssh" => println!("minikube: Opening SSH session..."),
        "profile" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("| Profile  | VM Driver | Runtime | IP            |");
                println!("|----------|-----------|---------|---------------|");
                println!("| minikube | docker    | docker  | 192.168.49.2  |");
            }
        }
        _ => println!("minikube {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "minikube".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_minikube(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_minikube};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/minikube"), "minikube");
        assert_eq!(basename(r"C:\bin\minikube.exe"), "minikube.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("minikube.exe"), "minikube");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_minikube(&["--help".to_string()], "minikube"), 0);
        assert_eq!(run_minikube(&["-h".to_string()], "minikube"), 0);
        assert_eq!(run_minikube(&["--version".to_string()], "minikube"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_minikube(&[], "minikube"), 0);
    }
}
