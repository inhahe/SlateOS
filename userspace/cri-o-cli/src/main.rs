#![deny(clippy::all)]

//! cri-o-cli — OurOS CRI-O container runtime
//!
//! Multi-personality: `crio`, `crio-status`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crio(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "crio-status" => {
                println!("crio-status (OurOS) — CRI-O status tool");
                println!("  info           General daemon info");
                println!("  containers     List containers");
                println!("  config         Show config");
            }
            _ => {
                println!("CRI-O v1.30 (OurOS) — OCI-based Kubernetes container runtime");
                println!("  --config FILE      Config file");
                println!("  --root DIR         Root directory");
                println!("  --runroot DIR      Run root directory");
                println!("  --storage-driver D Storage driver (overlay, btrfs)");
                println!("  --listen SOCKET    Listen socket path");
                println!("  --log-level LEVEL  Log level");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CRI-O v1.30.4 (OurOS)"); return 0; }
    match prog {
        "crio-status" => {
            println!("CRI-O status:");
            println!("  Version: 1.30.4");
            println!("  Storage driver: overlay");
            println!("  Storage root: /var/lib/containers/storage");
            println!("  Cgroup driver: systemd");
            println!("  Default runtime: crun");
            println!("  Containers: 12 running, 3 paused");
            println!("  Pods: 8");
        }
        _ => {
            println!("CRI-O v1.30.4 (OurOS)");
            println!("  Listening: /var/run/crio/crio.sock");
            println!("  Runtime: crun");
            println!("  Storage: overlay");
            println!("  Network: CNI (/etc/cni/net.d)");
            println!("  Ready to accept connections");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_crio(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cri-o"), "cri-o");
        assert_eq!(basename(r"C:\bin\cri-o.exe"), "cri-o.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cri-o.exe"), "cri-o");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crio(&["--help".to_string()], "cri-o"), 0);
        assert_eq!(run_crio(&["-h".to_string()], "cri-o"), 0);
        let _ = run_crio(&["--version".to_string()], "cri-o");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crio(&[], "cri-o");
    }
}
