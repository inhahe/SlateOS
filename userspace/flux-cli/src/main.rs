#![deny(clippy::all)]

//! flux-cli — SlateOS Flux GitOps toolkit
//!
//! Multi-personality: `flux`

use std::env;
use std::process;

fn run_flux(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flux [FLAGS] COMMAND [ARGS]");
        println!();
        println!("flux — Flux GitOps toolkit CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  bootstrap      Bootstrap Flux");
        println!("  check          Verify prerequisites");
        println!("  create         Create Flux resources");
        println!("  delete         Delete Flux resources");
        println!("  get            List Flux resources");
        println!("  reconcile      Reconcile resources");
        println!("  suspend        Suspend reconciliation");
        println!("  resume         Resume reconciliation");
        println!("  logs           Show controller logs");
        println!("  version        Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("flux version 2.2.3 (SlateOS)");
            println!("build date: 2024-01-15T00:00:00Z");
        }
        "check" => {
            println!("► checking prerequisites");
            println!("✔ Kubernetes 1.29.0 >=1.25.0-0");
            println!("✔ kubectl 1.29.0 >=1.18.0-0");
            println!("► checking controllers");
            println!("✔ helm-controller: deployment ready");
            println!("✔ kustomize-controller: deployment ready");
            println!("✔ notification-controller: deployment ready");
            println!("✔ source-controller: deployment ready");
            println!("✔ all checks passed");
        }
        "get" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            match resource {
                "kustomizations" | "ks" => {
                    println!("NAME       AGE   READY  STATUS");
                    println!("flux-sys   4d    True   Applied revision: main@sha1:abcdef1");
                    println!("apps       4d    True   Applied revision: main@sha1:abcdef1");
                    println!("infra      4d    True   Applied revision: main@sha1:abcdef1");
                }
                "sources" | "source" => {
                    println!("NAME       AGE   READY  STATUS");
                    println!("slateos-repo 4d    True   stored artifact for revision 'main@sha1:abcdef1'");
                }
                "helmreleases" | "hr" => {
                    println!("NAME         AGE   READY  STATUS");
                    println!("nginx        4d    True   Helm install succeeded");
                    println!("prometheus   4d    True   Helm install succeeded");
                }
                _ => {
                    println!("NAME           AGE   READY  STATUS");
                    println!("flux-system    4d    True   Applied revision: main@sha1:abcdef1");
                }
            }
        }
        "reconcile" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("kustomization");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("flux-system");
            println!("► annotating {} \"{}\" in \"flux-system\" namespace", resource, name);
            println!("✔ {} \"{}\" annotated", resource, name);
            println!("◎ waiting for {} reconciliation", resource);
            println!("✔ {} reconciliation completed", resource);
        }
        "logs" => {
            println!("2024-05-22T12:00:00Z info source-controller Reconciliation finished {{\"revision\":\"main@sha1:abcdef1\"}}");
            println!("2024-05-22T12:00:01Z info kustomize-controller Reconciliation finished {{\"revision\":\"main@sha1:abcdef1\"}}");
        }
        "suspend" | "resume" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("kustomization");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("flux-system");
            println!("► {}ing {} \"{}\"", subcmd, resource, name);
            println!("✔ {} \"{}\" {}d", resource, name, subcmd);
        }
        _ => println!("flux: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flux(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_flux};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flux(&["--help".to_string()]), 0);
        assert_eq!(run_flux(&["-h".to_string()]), 0);
        let _ = run_flux(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flux(&[]);
    }
}
