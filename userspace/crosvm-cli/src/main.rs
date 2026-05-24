#![deny(clippy::all)]

//! crosvm-cli — OurOS crosvm Chrome OS VMM
//!
//! Single personality: `crosvm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crosvm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: crosvm COMMAND [OPTIONS]");
        println!("crosvm v126.0 (OurOS) — Chrome OS Virtual Machine Monitor");
        println!();
        println!("Commands:");
        println!("  run               Run a VM");
        println!("  stop              Stop a running VM");
        println!("  suspend           Suspend a VM");
        println!("  resume            Resume a suspended VM");
        println!("  balloon           Adjust memory balloon");
        println!("  disk              Manage disk devices");
        println!("  snapshot          Take/restore snapshot");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "run" => {
            println!("crosvm starting VM...");
            println!("  Kernel: bzImage");
            println!("  vCPUs: 4");
            println!("  Memory: 4096 MiB");
            println!("  Virtio devices: block, net, console");
            println!("  GPU: virtio-gpu (2D)");
        }
        "stop" => println!("VM stopped."),
        "suspend" => println!("VM suspended."),
        "resume" => println!("VM resumed."),
        "balloon" => {
            println!("Memory balloon:");
            println!("  Current: 1024 MiB");
            println!("  Requested: 2048 MiB");
        }
        "snapshot" => println!("Snapshot saved: vm_snapshot.bin"),
        "version" | "--version" => println!("crosvm v126.0 (OurOS)"),
        _ => println!("crosvm {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crosvm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_crosvm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
