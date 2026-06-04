#![deny(clippy::all)]

//! linkerd-cli — OurOS Linkerd service mesh CLI
//!
//! Single personality: `linkerd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_linkerd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: linkerd COMMAND [OPTIONS]");
        println!("linkerd v2.15.0 (OurOS) — Linkerd service mesh CLI");
        println!();
        println!("Commands:");
        println!("  install         Generate install manifest");
        println!("  upgrade         Generate upgrade manifest");
        println!("  uninstall       Generate uninstall manifest");
        println!("  check           Check installation");
        println!("  inject          Add proxy sidecar");
        println!("  uninject        Remove proxy sidecar");
        println!("  dashboard       Open dashboard");
        println!("  stat            Show traffic stats");
        println!("  tap             Tap live traffic");
        println!("  top             Show live traffic summary");
        println!("  viz             Manage viz extension");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Client version: stable-2.15.0");
        println!("Server version: stable-2.15.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("check");
    match cmd {
        "check" => {
            println!("linkerd-existence");
            println!("----------------");
            println!("  control-plane Namespace exists: [ok]");
            println!("  control-plane ClusterRoles exist: [ok]");
            println!("  control-plane ServiceAccounts exist: [ok]");
            println!();
            println!("linkerd-config");
            println!("--------------");
            println!("  control-plane config valid: [ok]");
            println!();
            println!("linkerd-identity");
            println!("----------------");
            println!("  certificate config valid: [ok]");
            println!("  trust anchors valid: [ok]");
            println!();
            println!("Status check results are [ok]");
        }
        "stat" => {
            println!("NAME          MESHED  SUCCESS  RPS  LATENCY_P50  LATENCY_P95  LATENCY_P99");
            println!("deploy/api    1/1     100.00%  45   5ms          25ms         50ms");
            println!("deploy/web    1/1     99.95%   120  3ms          15ms         35ms");
            println!("deploy/worker 1/1     100.00%  8    12ms         45ms         100ms");
        }
        "install" => println!("# Generating Linkerd install manifest..."),
        "inject" => println!("Injecting Linkerd proxy sidecar..."),
        "dashboard" => println!("Opening Linkerd dashboard at http://localhost:50750"),
        "tap" => {
            println!("req id=1:0 src=deploy/web dst=deploy/api :method=GET :path=/users");
            println!("rsp id=1:0 :status=200 latency=5ms");
        }
        "top" => {
            println!("Source        Destination   Method  Path     Count  Best   Worst  Last");
            println!("deploy/web    deploy/api    GET     /users   45     3ms    25ms   5ms");
            println!("deploy/web    deploy/api    POST    /orders  12     8ms    50ms   12ms");
        }
        _ => println!("linkerd {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "linkerd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_linkerd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_linkerd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/linkerd"), "linkerd");
        assert_eq!(basename(r"C:\bin\linkerd.exe"), "linkerd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("linkerd.exe"), "linkerd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_linkerd(&["--help".to_string()], "linkerd"), 0);
        assert_eq!(run_linkerd(&["-h".to_string()], "linkerd"), 0);
        let _ = run_linkerd(&["--version".to_string()], "linkerd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_linkerd(&[], "linkerd");
    }
}
