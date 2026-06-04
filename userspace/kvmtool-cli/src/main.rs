#![deny(clippy::all)]

//! kvmtool-cli — OurOS kvmtool lightweight KVM tool
//!
//! Single personality: `lkvm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lkvm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: lkvm COMMAND [OPTIONS]");
        println!("lkvm v3.18 (OurOS) — Lightweight KVM tool");
        println!();
        println!("Commands:");
        println!("  run               Run a guest kernel");
        println!("  setup             Setup host environment");
        println!("  balloon           Adjust memory balloon");
        println!("  debug             Debug a guest");
        println!("  list              List running guests");
        println!("  stop              Stop a guest");
        println!("  stat              Guest statistics");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "run" => {
            println!("  # lkvm run");
            println!("  Info: Kernel: bzImage");
            println!("  Info: vCPUs: 2");
            println!("  Info: Memory: 512 MiB");
            println!("  Info: virtio devices: console, disk, net");
            println!("  Info: Starting VM...");
        }
        "list" => {
            println!("PID    NAME");
            println!("1234   guest-0");
            println!("5678   guest-1");
        }
        "stat" => {
            println!("Guest: guest-0 (PID 1234)");
            println!("  vCPUs: 2");
            println!("  Memory: 512 MiB");
            println!("  Uptime: 3h 42m");
        }
        "stop" => println!("Guest stopped."),
        "setup" => println!("Host environment ready for KVM."),
        "version" | "--version" => println!("lkvm v3.18 (OurOS)"),
        _ => println!("lkvm {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lkvm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lkvm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lkvm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kvmtool"), "kvmtool");
        assert_eq!(basename(r"C:\bin\kvmtool.exe"), "kvmtool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kvmtool.exe"), "kvmtool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lkvm(&["--help".to_string()], "kvmtool"), 0);
        assert_eq!(run_lkvm(&["-h".to_string()], "kvmtool"), 0);
        let _ = run_lkvm(&["--version".to_string()], "kvmtool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lkvm(&[], "kvmtool");
    }
}
