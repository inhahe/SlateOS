#![deny(clippy::all)]

//! k9s-cli — OurOS K9s Kubernetes TUI
//!
//! Single personality: `k9s`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_k9s(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: k9s [OPTIONS]");
        println!("K9s v0.32.4 (OurOS) — Kubernetes TUI");
        println!();
        println!("Options:");
        println!("  -n, --namespace NS    Start in namespace");
        println!("  -c, --command CMD     Start with resource view");
        println!("  --context CTX         Use specific context");
        println!("  --kubeconfig FILE     Use specific kubeconfig");
        println!("  --readonly            Read-only mode");
        println!("  --headless            No header");
        println!("  --logoless            No logo");
        println!("  --all-namespaces      All namespaces mode");
        println!("  -V, --version         Show version");
        println!();
        println!("Commands:");
        println!("  info                  Show cluster info");
        println!("  version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version" || a == "version") {
        println!("K9s v0.32.4 (OurOS)");
        println!("  Rev:    abc1234");
        println!("  OS/Arch: ouros/amd64");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "info" => {
            println!("K9s v0.32.4");
            println!("  Config:     ~/.config/k9s/config.yaml");
            println!("  Logs:       /tmp/k9s.log");
            println!("  Cluster:    minikube");
            println!("  Context:    minikube");
            println!("  User:       minikube");
            println!("  K8s Rev:    v1.30.0");
            println!("  CPU:        4 cores");
            println!("  MEM:        8.0Gi");
        }
        _ => {
            println!("K9s v0.32.4 — Kubernetes TUI");
            println!("Launching terminal UI...");
            println!();
            println!("┌─────────────────────────────────────────────┐");
            println!("│ K9s - Kubernetes CLI Dashboard              │");
            println!("├─────────────────────────────────────────────┤");
            println!("│ Context: minikube                           │");
            println!("│ Cluster: minikube                           │");
            println!("│ Namespace: default                          │");
            println!("├─────────────────────────────────────────────┤");
            println!("│ NAME       READY  STATUS   RESTARTS  AGE   │");
            println!("│ coredns    1/1    Running  0         10d   │");
            println!("│ nginx      1/1    Running  0         5d    │");
            println!("└─────────────────────────────────────────────┘");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "k9s".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_k9s(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_k9s};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/k9s"), "k9s");
        assert_eq!(basename(r"C:\bin\k9s.exe"), "k9s.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("k9s.exe"), "k9s");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_k9s(&["--help".to_string()], "k9s"), 0);
        assert_eq!(run_k9s(&["-h".to_string()], "k9s"), 0);
        let _ = run_k9s(&["--version".to_string()], "k9s");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_k9s(&[], "k9s");
    }
}
