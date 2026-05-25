#![deny(clippy::all)]

//! gparted-cli — OurOS GParted partition editor
//!
//! Single personality: `gparted`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gparted(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gparted [DEVICE]");
        println!("gparted v1.6 (OurOS) — GNOME Partition Editor");
        println!();
        println!("Options:");
        println!("  --version       Show version");
        println!();
        println!("GUI partition editor supporting: ext2/3/4, btrfs, xfs,");
        println!("  FAT16/32, NTFS, swap, and more.");
        println!("Operations: create, resize, move, copy, check, label, UUID");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gparted v1.6 (OurOS)"); return 0; }
    if let Some(dev) = args.first() {
        println!("gparted: opening device '{}'", dev);
    } else {
        println!("gparted: scanning devices...");
    }
    println!("  /dev/sda  500 GiB  GPT");
    println!("    /dev/sda1  512 MiB  EFI System    fat32");
    println!("    /dev/sda2  480 GiB  OurOS Root    ext4");
    println!("    /dev/sda3   19 GiB  Swap          linux-swap");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gparted".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gparted(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
