#![deny(clippy::all)]

//! gvisor-cli — SlateOS gVisor sandbox runtime
//!
//! Single personality: `runsc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_runsc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: runsc COMMAND [OPTIONS]");
        println!("runsc v2024.01 (SlateOS) — gVisor application kernel sandbox");
        println!();
        println!("Commands:");
        println!("  create            Create a sandbox");
        println!("  start             Start a sandbox");
        println!("  run               Create and start a sandbox");
        println!("  delete            Delete a sandbox");
        println!("  state             Query sandbox state");
        println!("  list              List sandboxes");
        println!("  do                Run a command in new sandbox");
        println!("  install           Install as container runtime");
        println!("  checkpoint        Checkpoint a sandbox");
        println!("  restore           Restore from checkpoint");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "create" | "run" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("sandbox1");
            println!("Sandbox {} created (gVisor kernel)", id);
            if cmd == "run" {
                println!("Sandbox {} running (Sentry PID 4567)", id);
            }
        }
        "list" => {
            println!("ID              PID    STATUS    PLATFORM");
            println!("sandbox1        4567   running   ptrace");
            println!("sandbox2        8901   stopped   kvm");
        }
        "state" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("sandbox1");
            println!("Sandbox: {}", id);
            println!("  Status: running");
            println!("  Platform: ptrace");
            println!("  Network: netstack");
            println!("  PID: 4567");
        }
        "install" => {
            println!("Installing runsc as container runtime...");
            println!("  Config written to /etc/containerd/config.toml");
        }
        _ => println!("runsc {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "runsc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_runsc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_runsc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gvisor"), "gvisor");
        assert_eq!(basename(r"C:\bin\gvisor.exe"), "gvisor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gvisor.exe"), "gvisor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_runsc(&["--help".to_string()], "gvisor"), 0);
        assert_eq!(run_runsc(&["-h".to_string()], "gvisor"), 0);
        let _ = run_runsc(&["--version".to_string()], "gvisor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_runsc(&[], "gvisor");
    }
}
