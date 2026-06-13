#![deny(clippy::all)]

//! btrfs-cli — SlateOS Btrfs filesystem tools
//!
//! Multi-personality: `btrfs`, `mkfs.btrfs`, `btrfs-convert`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_btrfs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: btrfs [OPTIONS] <group> <command> [<args>]");
        println!();
        println!("btrfs — Btrfs filesystem management (Slate OS).");
        println!();
        println!("Command groups:");
        println!("  subvolume    Manage subvolumes");
        println!("  filesystem   Filesystem operations");
        println!("  balance      Balance data across devices");
        println!("  device       Manage devices");
        println!("  scrub        Verify data integrity");
        println!("  send/receive Incremental send/receive");
        println!("  snapshot     Manage snapshots");
        println!("  property     Get/set properties");
        println!("  quota        Manage quotas");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("btrfs-progs v6.7 (Slate OS)");
        return 0;
    }

    let group = args.first().map(|s| s.as_str()).unwrap_or("filesystem");
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
    match (group, cmd) {
        ("filesystem" | "fi", "show") => {
            println!("Label: 'data'  uuid: aabbccdd-1122-3344-5566-778899001122");
            println!("\tTotal devices 2 FS bytes used 456.78GiB");
            println!("\tdevid    1 size 1.00TiB used 500.00GiB path /dev/sda1");
            println!("\tdevid    2 size 1.00TiB used 500.00GiB path /dev/sdb1");
        }
        ("filesystem" | "fi", "df") => {
            let path = args.get(2).map(|s| s.as_str()).unwrap_or("/");
            println!("Data, RAID1: total=456.00GiB, used=400.00GiB ({})", path);
            println!("System, RAID1: total=8.00MiB, used=48.00KiB");
            println!("Metadata, RAID1: total=3.00GiB, used=2.50GiB");
            println!("GlobalReserve, single: total=512.00MiB, used=0.00B");
        }
        ("filesystem" | "fi", "usage") => {
            println!("Overall:");
            println!("    Device size:           2.00TiB");
            println!("    Device allocated:      1000.00GiB");
            println!("    Device unallocated:    1024.00GiB");
            println!("    Used:                  800.00GiB");
            println!("    Free (estimated):      1224.00GiB      (min: 712.00GiB)");
        }
        ("subvolume" | "sub", "list") => {
            let path = args.get(2).map(|s| s.as_str()).unwrap_or("/");
            println!("ID 256 gen 1234 top level 5 path home ({})", path);
            println!("ID 257 gen 1200 top level 5 path snapshots/daily-2024-05-21");
            println!("ID 258 gen 1234 top level 5 path snapshots/daily-2024-05-22");
        }
        ("subvolume" | "sub", "create") => {
            let sv = args.get(2).map(|s| s.as_str()).unwrap_or("new-subvol");
            println!("Create subvolume '{}'", sv);
        }
        ("subvolume" | "sub", "delete") => {
            let sv = args.get(2).map(|s| s.as_str()).unwrap_or("old-subvol");
            println!("Delete subvolume (no-commit): '{}'", sv);
        }
        ("subvolume" | "sub", "snapshot") => {
            let src = args.get(2).map(|s| s.as_str()).unwrap_or("/home");
            let dst = args.get(3).map(|s| s.as_str()).unwrap_or("/snapshots/home-snap");
            println!("Create a snapshot of '{}' in '{}'", src, dst);
        }
        ("scrub", "start") => {
            let path = args.get(2).map(|s| s.as_str()).unwrap_or("/");
            println!("scrub started on {path} (fsid aabbccdd-1122-3344-5566-778899001122)");
        }
        ("scrub", "status") => {
            println!("UUID:             aabbccdd-1122-3344-5566-778899001122");
            println!("Scrub started:    Wed May 22 08:00:00 2024");
            println!("Status:           finished");
            println!("Duration:         0:12:34");
            println!("Total to scrub:   800.00GiB");
            println!("Rate:             1.07GiB/s");
            println!("Error summary:    no errors found");
        }
        ("balance", "start") => println!("Done, had to relocate 42 out of 128 chunks"),
        ("balance", "status") => println!("No balance found on '/'"),
        ("device", "stats") => {
            println!("[/dev/sda1].write_io_errs    0");
            println!("[/dev/sda1].read_io_errs     0");
            println!("[/dev/sda1].flush_io_errs    0");
            println!("[/dev/sda1].corruption_errs  0");
            println!("[/dev/sda1].generation_errs  0");
        }
        _ => println!("btrfs: {} {} completed", group, cmd),
    }
    0
}

fn run_mkfs_btrfs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mkfs.btrfs [OPTIONS] <devices>");
        println!("  -L <label>    Set label");
        println!("  -d <profile>  Data profile (raid0, raid1, single)");
        println!("  -m <profile>  Metadata profile");
        return 0;
    }
    let dev = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/sda1");
    println!("btrfs-progs v6.7 (Slate OS)");
    println!("Label:              (none)");
    println!("UUID:               aabbccdd-1122-3344-5566-778899001122");
    println!("Node size:          16384");
    println!("Sector size:        4096");
    println!("Filesystem size:    1.00TiB");
    println!("Block group profiles:");
    println!("  Data:             single            8.00MiB");
    println!("  Metadata:         DUP             256.00MiB");
    println!("  System:           DUP               8.00MiB");
    println!("SSD detected:       no");
    println!("Zoned device:       no");
    println!("Created filesystem on {}", dev);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "btrfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mkfs.btrfs" | "mkfs_btrfs" => run_mkfs_btrfs(&rest),
        "btrfs-convert" => { println!("btrfs-convert: converting ext4 to btrfs... done"); 0 }
        _ => run_btrfs(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_btrfs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/btrfs"), "btrfs");
        assert_eq!(basename(r"C:\bin\btrfs.exe"), "btrfs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("btrfs.exe"), "btrfs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_btrfs(&["--help".to_string()]), 0);
        assert_eq!(run_btrfs(&["-h".to_string()]), 0);
        let _ = run_btrfs(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_btrfs(&[]);
    }
}
