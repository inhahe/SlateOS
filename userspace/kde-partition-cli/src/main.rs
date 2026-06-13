#![deny(clippy::all)]

//! kde-partition-cli — Slate OS KDE Partition Manager
//!
//! Single personality: `partitionmanager`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_partitionmanager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: partitionmanager [OPTIONS] [DEVICE]");
        println!("partitionmanager v24.08 (Slate OS) — KDE Partition Manager");
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
    if args.iter().any(|a| a == "--version") { println!("partitionmanager v24.08 (Slate OS)"); return 0; }
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
mod tests {
    use super::{basename, strip_ext, run_partitionmanager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kde-partition"), "kde-partition");
        assert_eq!(basename(r"C:\bin\kde-partition.exe"), "kde-partition.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kde-partition.exe"), "kde-partition");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_partitionmanager(&["--help".to_string()], "kde-partition"), 0);
        assert_eq!(run_partitionmanager(&["-h".to_string()], "kde-partition"), 0);
        let _ = run_partitionmanager(&["--version".to_string()], "kde-partition");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_partitionmanager(&[], "kde-partition");
    }
}
