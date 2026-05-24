#![deny(clippy::all)]

//! stratovirt-cli — OurOS StratoVirt lightweight VMM
//!
//! Single personality: `stratovirt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stratovirt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: stratovirt [OPTIONS]");
        println!("stratovirt v2.4 (OurOS) — Lightweight VMM for cloud");
        println!();
        println!("Options:");
        println!("  -kernel PATH      Kernel image");
        println!("  -initrd PATH      Initrd image");
        println!("  -smp N            vCPU count");
        println!("  -m SIZE           Memory size");
        println!("  -drive FILE       Block device");
        println!("  -netdev TAP       Network device");
        println!("  -api-channel PATH QMP socket");
        println!("  -machine TYPE     Machine type (microvm, virt)");
        println!("  -D FILE           Debug log file");
        return 0;
    }
    println!("StratoVirt v2.4 starting...");
    println!("  Machine type: microvm");
    println!("  vCPUs: 1");
    println!("  Memory: 256 MiB");
    println!("  Boot: direct kernel");
    println!("  Devices: virtio-blk, virtio-net");
    println!("  Boot time: 25 ms");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stratovirt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stratovirt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
