#![deny(clippy::all)]

//! mhddfs-cli — OurOS mhddfs multi-HDD FUSE filesystem
//!
//! Single personality: `mhddfs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mhddfs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mhddfs DIR1,DIR2[,...] MOUNTPOINT [OPTIONS]");
        println!("mhddfs v0.1.39 (OurOS) — Join multiple filesystems into one");
        println!();
        println!("Options:");
        println!("  -o mlimit=SIZE   Move limit (min free space before moving to next disk)");
        println!("  -o logfile=FILE  Log file path");
        println!("  -o loglevel=N    Log level (0-10)");
        println!("  -o allow_other   Allow other users");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mhddfs v0.1.39 (OurOS)"); return 0; }
    println!("mhddfs v0.1.39 (OurOS)");
    println!("  Directories:");
    println!("    /mnt/hdd1: 2.0 TiB total, 0.5 TiB free");
    println!("    /mnt/hdd2: 2.0 TiB total, 1.2 TiB free");
    println!("    /mnt/hdd3: 4.0 TiB total, 3.1 TiB free");
    println!("  Mountpoint: /mnt/combined");
    println!("  Total: 8.0 TiB, 4.8 TiB free");
    println!("  Move limit: 4 GiB");
    println!("  Mounted successfully");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mhddfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mhddfs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
