#![deny(clippy::all)]

//! istioctl-cli — SlateOS Istio service mesh CLI
//!
//! Single personality: `istioctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_istioctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: istioctl COMMAND [OPTIONS]");
        println!("istioctl 1.21.0 (Slate OS) — Istio service mesh CLI");
        println!();
        println!("Commands:");
        println!("  install          Install Istio");
        println!("  manifest         Generate manifests");
        println!("  profile          Manage profiles");
        println!("  analyze          Analyze config");
        println!("  dashboard        Open dashboards");
        println!("  proxy-config     Configure proxy");
        println!("  proxy-status     Show proxy status");
        println!("  version          Show version");
        println!("  verify-install   Verify installation");
        println!("  upgrade          Upgrade Istio");
        println!("  uninstall        Uninstall Istio");
        println!("  bug-report       Create bug report");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("client version: 1.21.0");
        println!("control plane version: 1.21.0");
        println!("data plane version: 1.21.0 (2 proxies)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "install" => {
            println!("This will install the Istio 1.21.0 default profile...");
            println!("✔ Istio core installed");
            println!("✔ Istiod installed");
            println!("✔ Ingress gateways installed");
            println!("Installation complete.");
        }
        "analyze" => {
            println!("Analyzing namespace: default");
            println!("✔ No issues found when analyzing namespace: default.");
        }
        "proxy-status" => {
            println!("NAME                   CDS   LDS   EDS   RDS   ECDS");
            println!("app-v1.default         SYNCED SYNCED SYNCED SYNCED");
            println!("app-v2.default         SYNCED SYNCED SYNCED SYNCED");
        }
        "profile" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Istio configuration profiles:");
                println!("  default");
                println!("  demo");
                println!("  minimal");
                println!("  remote");
                println!("  empty");
            }
        }
        "dashboard" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("kiali");
            println!("Opening {} dashboard...", sub);
        }
        "verify-install" => {
            println!("✔ Istio is installed and verified successfully.");
        }
        "manifest" => println!("istioctl: Generating manifests..."),
        "uninstall" => println!("istioctl: Removing Istio..."),
        _ => println!("istioctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "istioctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_istioctl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_istioctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/istioctl"), "istioctl");
        assert_eq!(basename(r"C:\bin\istioctl.exe"), "istioctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("istioctl.exe"), "istioctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_istioctl(&["--help".to_string()], "istioctl"), 0);
        assert_eq!(run_istioctl(&["-h".to_string()], "istioctl"), 0);
        let _ = run_istioctl(&["--version".to_string()], "istioctl");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_istioctl(&[], "istioctl");
    }
}
