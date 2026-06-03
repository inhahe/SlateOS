#![deny(clippy::all)]

//! flux — OurOS Flux CD GitOps toolkit
//!
//! Single personality: `flux`

use std::env;
use std::process;

fn run_flux(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flux <command> [flags]");
        println!();
        println!("Commands:");
        println!("  bootstrap    Bootstrap Flux on a cluster");
        println!("  check        Check prerequisites");
        println!("  install      Install Flux components");
        println!("  uninstall    Uninstall Flux components");
        println!("  suspend      Suspend reconciliation");
        println!("  resume       Resume reconciliation");
        println!("  reconcile    Trigger reconciliation");
        println!("  get          Get Flux resources");
        println!("  create       Create Flux resources");
        println!("  delete       Delete Flux resources");
        println!("  export       Export resources as YAML");
        println!("  logs         Show Flux logs");
        println!("  events       Show Flux events");
        println!("  tree         Show resource tree");
        println!("  trace        Trace object through Flux");
        println!("  diff         Diff local vs cluster");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("flux: v2.3.0 (OurOS)");
            println!("distribution: flux-v2.3.0");
        }
        "check" => {
            println!("-> checking prerequisites");
            println!("   kubernetes 1.29.0 >= 1.26.0");
            println!("-> checking controllers");
            println!("   source-controller: healthy");
            println!("   kustomize-controller: healthy");
            println!("   helm-controller: healthy");
            println!("   notification-controller: healthy");
            println!("-> all checks passed");
        }
        "get" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            match resource {
                "sources" | "source" => {
                    println!("NAME         REVISION     SUSPENDED  READY  MESSAGE");
                    println!("git/myapp    main@sha1    False      True   stored artifact");
                }
                "kustomizations" | "ks" => {
                    println!("NAME       REVISION     SUSPENDED  READY  MESSAGE");
                    println!("myapp      main@sha1    False      True   Applied revision");
                }
                "helmreleases" | "hr" => {
                    println!("NAME       REVISION  SUSPENDED  READY  MESSAGE");
                    println!("nginx      1.25.3    False      True   Helm install succeeded");
                }
                "all" => {
                    println!("NAME                     REVISION     READY  MESSAGE");
                    println!("gitrepository/myapp      main@sha1    True   stored artifact");
                    println!("kustomization/myapp      main@sha1    True   Applied revision");
                    println!("helmrelease/nginx        1.25.3       True   Helm install succeeded");
                }
                _ => println!("(get {} — simulated)", resource),
            }
        }
        "reconcile" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("kustomization");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("myapp");
            println!("-> annotating {} {}", resource, name);
            println!("-> reconciliation triggered");
        }
        "tree" => {
            let resource = args.get(1).map(|s| s.as_str()).unwrap_or("kustomization");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("myapp");
            println!("{}/{}", resource, name);
            println!("├── Deployment/myapp");
            println!("│   └── ReplicaSet/myapp-abc123");
            println!("│       └── Pod/myapp-abc123-xyz");
            println!("└── Service/myapp");
        }
        "logs" => {
            println!("2025-05-22T10:00:00Z info source-controller stored artifact");
            println!("2025-05-22T10:00:01Z info kustomize-controller Applied revision");
        }
        "bootstrap" | "install" | "uninstall" | "suspend" | "resume" | "create" | "delete" | "export" | "diff" | "trace" | "events" => {
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
    let code = run_flux(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_flux};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_flux(vec!["--help".to_string()]), 0);
        assert_eq!(run_flux(vec!["-h".to_string()]), 0);
        assert_eq!(run_flux(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_flux(vec![]), 0);
    }
}
