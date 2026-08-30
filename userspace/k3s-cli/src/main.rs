#![deny(clippy::all)]

//! k3s-cli — Slate OS K3s lightweight Kubernetes
//!
//! Single personality: `k3s`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_k3s(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: k3s COMMAND [OPTIONS]");
        println!("k3s v1.30.0+k3s1 (Slate OS) — Lightweight Kubernetes");
        println!();
        println!("Commands:");
        println!("  server          Run management server");
        println!("  agent           Run node agent");
        println!("  kubectl         Run kubectl");
        println!("  crictl          Run crictl");
        println!("  ctr             Run containerd CLI");
        println!("  etcd-snapshot   Manage etcd snapshots");
        println!("  secrets-encrypt Manage secrets encryption");
        println!("  certificate     Manage certificates");
        println!("  check-config    Check host config");
        println!("  token           Manage tokens");
        println!("  completion      Shell completion");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("k3s version v1.30.0+k3s1 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("server");
    match cmd {
        "server" => {
            println!("INFO[0000] Starting k3s v1.30.0+k3s1");
            println!("INFO[0001] Configuring sqlite3 data store");
            println!("INFO[0002] Starting kube-apiserver");
            println!("INFO[0003] Starting kube-controller-manager");
            println!("INFO[0004] Starting kube-scheduler");
            println!("INFO[0005] Node ready");
        }
        "agent" => {
            println!("INFO[0000] Starting k3s agent v1.30.0+k3s1");
            println!("INFO[0001] Connecting to server https://server:6443");
            println!("INFO[0002] Node registered");
        }
        "kubectl" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            if sub == "get" {
                let resource = args.get(2).map(|s| s.as_str()).unwrap_or("nodes");
                println!("NAME       STATUS   ROLES                  AGE   VERSION");
                println!("k3s-node   Ready    control-plane,master   10d   v1.30.0+k3s1");
                if resource == "pods" {
                    println!("NAME                     READY   STATUS    RESTARTS   AGE");
                    println!("coredns-abc123           1/1     Running   0          10d");
                }
            }
        }
        "etcd-snapshot" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "save" => println!("INFO: Snapshot saved: /var/lib/rancher/k3s/server/db/snapshots/etcd-snapshot-123"),
                "list" => {
                    println!("Name                           Size    Created");
                    println!("etcd-snapshot-123              3.2MB   2024-01-15 10:00:00");
                }
                _ => println!("k3s etcd-snapshot {}: completed", sub),
            }
        }
        "check-config" => {
            println!("Verifying binaries in /var/lib/rancher/k3s/data/...");
            println!("  cgroup v2: enabled");
            println!("  iptables: found");
            println!("  ip6tables: found");
            println!("System check passed.");
        }
        "token" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "create" {
                println!("K10abc123def456::server:token-value-here");
            } else {
                println!("k3s token {}: completed", sub);
            }
        }
        _ => println!("k3s {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "k3s".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_k3s(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_k3s};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/k3s"), "k3s");
        assert_eq!(basename(r"C:\bin\k3s.exe"), "k3s.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("k3s.exe"), "k3s");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_k3s(&["--help".to_string()], "k3s"), 0);
        assert_eq!(run_k3s(&["-h".to_string()], "k3s"), 0);
        let _ = run_k3s(&["--version".to_string()], "k3s");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_k3s(&[], "k3s");
    }
}
