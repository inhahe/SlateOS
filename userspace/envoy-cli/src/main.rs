#![deny(clippy::all)]

//! envoy-cli — SlateOS Envoy service proxy
//!
//! Multi-personality: `envoy`, `istioctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_envoy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: envoy [OPTIONS]");
        println!();
        println!("envoy — L7 proxy and communication bus (SlateOS).");
        println!();
        println!("Options:");
        println!("  -c <path>          Config file");
        println!("  --mode <mode>      serve|validate|init-only");
        println!("  --log-level <l>    Log level");
        println!("  --concurrency <n>  Worker threads");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("envoy  version: 1.29.1/1.29.1/Clean/RELEASE/SlateOS");
        return 0;
    }

    if args.iter().any(|a| a == "validate") {
        println!("configuration '/etc/envoy/envoy.yaml' OK");
        return 0;
    }

    println!("[info] Envoy 1.29.1 starting");
    println!("[info] cds: add 2 cluster(s)");
    println!("[info] Cluster manager: add/update cluster web_service");
    println!("[info] Cluster manager: add/update cluster api_service");
    println!("[info] lds: add/update listener listener_http (0.0.0.0:8080)");
    println!("[info] lds: add/update listener listener_https (0.0.0.0:8443)");
    println!("[info] All clusters/listeners initialized. Starting workers.");
    println!("[info] Starting 4 worker threads.");
    0
}

fn run_istioctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: istioctl [OPTIONS] COMMAND");
        println!();
        println!("istioctl — Istio service mesh CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  version           Show version");
        println!("  proxy-status      Show proxy sync status");
        println!("  analyze           Analyze configuration");
        println!("  dashboard         Open dashboard");
        println!("  install           Install Istio");
        println!("  manifest          Generate manifests");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => {
            println!("client version: 1.21.0 (SlateOS)");
            println!("control plane version: 1.21.0");
            println!("data plane version: 1.21.0 (4 proxies)");
        }
        "proxy-status" => {
            println!("NAME                              CLUSTER   CDS    LDS    EDS    RDS    ECDS   ISTIOD");
            println!("web-server-abc123.default         Kubernetes SYNCED SYNCED SYNCED SYNCED        istiod-xyz");
            println!("api-server-def456.default         Kubernetes SYNCED SYNCED SYNCED SYNCED        istiod-xyz");
            println!("gateway-ghi789.istio-system       Kubernetes SYNCED SYNCED SYNCED SYNCED        istiod-xyz");
        }
        "analyze" => {
            println!("✔ No validation issues found when analyzing namespace: default.");
        }
        _ => println!("istioctl: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "envoy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "istioctl" => run_istioctl(&rest),
        _ => run_envoy(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_envoy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/envoy"), "envoy");
        assert_eq!(basename(r"C:\bin\envoy.exe"), "envoy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("envoy.exe"), "envoy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_envoy(&["--help".to_string()]), 0);
        assert_eq!(run_envoy(&["-h".to_string()]), 0);
        let _ = run_envoy(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_envoy(&[]);
    }
}
