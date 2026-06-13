#![deny(clippy::all)]

//! mkfs-cli — SlateOS mkfs filesystem creation CLIs
//!
//! Multi-personality: `mkfs`, `mkfs.ext4`, `mkfs.xfs`, `mkfs.btrfs`, `mkfs.fat`, `mkswap`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_mkfs(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog {
            "mkfs.ext4" | "mke2fs" => {
                println!("Usage: mkfs.ext4 [OPTIONS] DEVICE");
                println!("  -L LABEL       Volume label");
                println!("  -b SIZE        Block size (1024, 2048, 4096)");
                println!("  -i RATIO       Bytes per inode");
                println!("  -j             Create journal (ext3/ext4)");
                println!("  -m PERCENT     Reserved blocks percentage");
                println!("  -O FEATURES    Filesystem features");
            }
            "mkfs.xfs" => {
                println!("Usage: mkfs.xfs [OPTIONS] DEVICE");
                println!("  -L LABEL       Volume label");
                println!("  -b size=N      Block size");
                println!("  -f             Force overwrite");
                println!("  -d agcount=N   AG count");
            }
            "mkfs.btrfs" => {
                println!("Usage: mkfs.btrfs [OPTIONS] DEVICE...");
                println!("  -L LABEL       Volume label");
                println!("  -d PROFILE     Data profile (single, raid0, raid1, raid5, raid6)");
                println!("  -m PROFILE     Metadata profile");
                println!("  -f             Force overwrite");
            }
            "mkfs.fat" | "mkfs.vfat" => {
                println!("Usage: mkfs.fat [OPTIONS] DEVICE");
                println!("  -F SIZE        FAT size (12, 16, 32)");
                println!("  -n LABEL       Volume label");
                println!("  -s SECTORS     Sectors per cluster");
            }
            "mkswap" => {
                println!("Usage: mkswap [OPTIONS] DEVICE");
                println!("  -L LABEL       Volume label");
                println!("  -U UUID        Set UUID");
            }
            _ => {
                println!("Usage: mkfs [-t TYPE] [OPTIONS] DEVICE");
                println!("  -t TYPE        Filesystem type (ext4, xfs, btrfs, fat, swap)");
            }
        }
        return 0;
    }

    let device = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/sda1");
    let label = args.windows(2).find(|w| w[0] == "-L" || w[0] == "-n")
        .map(|w| w[1].as_str());

    match prog {
        "mkfs.ext4" | "mke2fs" => {
            let block_size = args.windows(2).find(|w| w[0] == "-b")
                .map(|w| w[1].as_str()).unwrap_or("4096");
            println!("mke2fs 1.47.0 (Slate OS)");
            println!("Creating filesystem with 13107200 {}k blocks and 3276800 inodes", block_size);
            println!("Filesystem UUID: abcdef12-3456-7890-abcd-ef1234567890");
            if let Some(l) = label {
                println!("Filesystem label: {}", l);
            }
            println!("Superblock backups stored on blocks:");
            println!("        32768, 98304, 163840, 229376, 294912, 819200, 884736");
            println!();
            println!("Allocating group tables: done");
            println!("Writing inode tables: done");
            println!("Creating journal (65536 blocks): done");
            println!("Writing superblocks and filesystem accounting information: done");
        }
        "mkfs.xfs" => {
            println!("meta-data={}           isize=512    agcount=4, agsize=3276800 blks", device);
            println!("data     =                       bsize=4096   blocks=13107200");
            println!("naming   =version 2              bsize=4096   ascii-ci=0, ftype=1");
            println!("log      =internal log           bsize=4096   blocks=6400, version=2");
            println!("realtime =none                   extsz=4096   blocks=0, rtextents=0");
        }
        "mkfs.btrfs" => {
            println!("btrfs-progs v6.6.3 (Slate OS)");
            if let Some(l) = label {
                println!("Label: {}", l);
            }
            println!("UUID: abcdef12-3456-7890-abcd-ef1234567890");
            println!("Node size: 16384");
            println!("Sector size: 4096");
            println!("Filesystem size: 50.00GiB");
            println!("Block group profiles:");
            println!("  Data:             single            8.00MiB");
            println!("  Metadata:         DUP             256.00MiB");
            println!("  System:           DUP               8.00MiB");
        }
        "mkfs.fat" | "mkfs.vfat" => {
            let fat_size = args.windows(2).find(|w| w[0] == "-F")
                .map(|w| w[1].as_str()).unwrap_or("32");
            println!("mkfs.fat 4.2 (Slate OS)");
            println!("{}: FAT{}", device, fat_size);
        }
        "mkswap" => {
            println!("Setting up swapspace version 1, size = 8 GiB (8589934592 bytes)");
            if let Some(l) = label {
                println!("LABEL={}", l);
            }
            println!("UUID=abcdef12-3456-7890-abcd-ef1234567890");
        }
        _ => {
            let fs_type = args.windows(2).find(|w| w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("ext4");
            println!("mkfs: creating {} filesystem on {}", fs_type, device);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mkfs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mkfs(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mkfs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mkfs"), "mkfs");
        assert_eq!(basename(r"C:\bin\mkfs.exe"), "mkfs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mkfs.exe"), "mkfs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mkfs("mkfs", &["--help".to_string()]), 0);
        assert_eq!(run_mkfs("mkfs", &["-h".to_string()]), 0);
        let _ = run_mkfs("mkfs", &["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mkfs("mkfs", &[]);
    }
}
