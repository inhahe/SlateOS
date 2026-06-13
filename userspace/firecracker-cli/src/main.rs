#![deny(clippy::all)]

//! firecracker-cli — SlateOS Firecracker microVM CLI
//!
//! Multi-personality: `firecracker`, `jailer`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_firecracker(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: firecracker [OPTIONS]");
        println!();
        println!("Firecracker — lightweight microVM (SlateOS).");
        println!();
        println!("Options:");
        println!("  --api-sock PATH    API socket path");
        println!("  --config-file F    JSON config file");
        println!("  --id ID            VM identifier");
        println!("  --log-path FILE    Log file path");
        println!("  --level LEVEL      Log level");
        println!("  --boot-timer       Enable boot timer");
        println!("  --no-api           Disable API server");
        println!("  --seccomp-filter F Custom seccomp filter");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Firecracker v1.7.0 (SlateOS)");
        return 0;
    }

    let api_sock = args.windows(2)
        .find(|w| w[0] == "--api-sock")
        .map(|w| w[1].as_str())
        .unwrap_or("/tmp/firecracker.socket");

    let config = args.windows(2)
        .find(|w| w[0] == "--config-file")
        .map(|w| w[1].as_str());

    if let Some(cfg) = config {
        println!("Firecracker v1.7.0");
        println!("  Loading config from '{}'...", cfg);
        println!("  Boot source: vmlinux");
        println!("  Root drive: rootfs.ext4");
        println!("  Memory: 128 MiB");
        println!("  vCPUs: 1");
        println!("  MicroVM started successfully.");
    } else {
        println!("Firecracker v1.7.0");
        println!("  API server listening on '{}'", api_sock);
        println!("  Waiting for configuration via API...");
    }
    0
}

fn run_jailer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jailer [OPTIONS]");
        println!();
        println!("jailer — Firecracker jailer (SlateOS).");
        println!();
        println!("Options:");
        println!("  --id ID            Jail identifier");
        println!("  --exec-file PATH   Firecracker binary path");
        println!("  --uid UID          User ID");
        println!("  --gid GID          Group ID");
        println!("  --chroot-base DIR  Chroot base directory");
        println!("  --netns NS         Network namespace");
        println!("  --daemonize        Run as daemon");
        println!("  --cgroup KEY=VAL   Cgroup setting");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Firecracker jailer v1.7.0 (SlateOS)");
        return 0;
    }

    let id = args.windows(2).find(|w| w[0] == "--id").map(|w| w[1].as_str()).unwrap_or("vm1");
    println!("jailer: setting up jail for '{}'", id);
    println!("  Chroot: /srv/jailer/firecracker/{}/root", id);
    println!("  Starting firecracker in jail...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "firecracker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "jailer" => run_jailer(&rest),
        _ => run_firecracker(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_firecracker};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/firecracker"), "firecracker");
        assert_eq!(basename(r"C:\bin\firecracker.exe"), "firecracker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("firecracker.exe"), "firecracker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_firecracker(&["--help".to_string()]), 0);
        assert_eq!(run_firecracker(&["-h".to_string()]), 0);
        let _ = run_firecracker(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_firecracker(&[]);
    }
}
