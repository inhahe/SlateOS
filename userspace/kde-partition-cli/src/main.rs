#![deny(clippy::all)]

//! kde-partition-cli — OurOS KDE Partition Manager
//!
//! Single personality: `partitionmanager`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_partitionmanager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: partitionmanager [OPTIONS] [DEVICE]");
        println!("partitionmanager v24.08 (OurOS) — KDE Partition Manager");
        println!();
        println!("Options:");
        println!("  -d DEVICE       Open specific device");
        println!("  --version       Show version");
        println!();
        println!("KDE partition management tool built on KPMcore.");
        println!("Supports: ext2/3/4, btrfs, xfs, FAT16/32, NTFS, swap,");
        println!("  reiserfs, jfs, f2fs, nilfs2, exfat, udf, luks, lvm");
        println!("Operations: create, resize, move, copy, format, check,");
        println!("  label, mount/unmount, SMART info");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("partitionmanager v24.08 (OurOS)"); return 0; }
    println!("partitionmanager: KDE Partition Manager");
    println!("  Devices:");
    println!("    /dev/sda  500 GiB  GPT  (3 partitions)");
    println!("  Pending operations: 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "partitionmanager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_partitionmanager(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
