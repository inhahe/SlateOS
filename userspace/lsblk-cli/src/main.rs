#![deny(clippy::all)]

//! lsblk-cli — Slate OS block device lister
//!
//! Multi-personality: `lsblk`, `blkid`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lsblk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lsblk [OPTIONS] [DEVICE...]");
        println!();
        println!("lsblk — list block devices (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --all         Show all devices");
        println!("  -f, --fs          Show filesystem info");
        println!("  -l, --list        List format");
        println!("  -o, --output LIST Output columns");
        println!("  -p, --paths       Show full device paths");
        println!("  -t, --topology    Show topology info");
        println!("  -J, --json        JSON output");
        println!("  -b, --bytes       Size in bytes");
        println!("  -d, --nodeps      Don't show partitions");
        println!("  -n, --noheadings  No header line");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("lsblk from util-linux 2.39 (Slate OS)");
        return 0;
    }

    let fs_mode = args.iter().any(|a| a == "-f" || a == "--fs");
    let json = args.iter().any(|a| a == "-J" || a == "--json");

    if json {
        println!("{{");
        println!("   \"blockdevices\": [");
        println!("      {{\"name\":\"sda\", \"maj:min\":\"8:0\", \"rm\":false, \"size\":\"931.5G\", \"ro\":false, \"type\":\"disk\"}},");
        println!("      {{\"name\":\"sda1\", \"maj:min\":\"8:1\", \"rm\":false, \"size\":\"512M\", \"ro\":false, \"type\":\"part\", \"mountpoint\":\"/boot/efi\"}},");
        println!("      {{\"name\":\"sda2\", \"maj:min\":\"8:2\", \"rm\":false, \"size\":\"931G\", \"ro\":false, \"type\":\"part\", \"mountpoint\":\"/\"}},");
        println!("      {{\"name\":\"nvme0n1\", \"maj:min\":\"259:0\", \"rm\":false, \"size\":\"1.9T\", \"ro\":false, \"type\":\"disk\"}}");
        println!("   ]");
        println!("}}");
        return 0;
    }

    if fs_mode {
        println!("NAME      FSTYPE FSVER LABEL  UUID                                 FSAVAIL FSUSE% MOUNTPOINTS");
        println!("sda");
        println!("├─sda1    vfat   FAT32 EFI    ABCD-1234                              450M     12% /boot/efi");
        println!("└─sda2    ext4   1.0          12345678-1234-1234-1234-123456789abc  800.2G    14% /");
        println!("nvme0n1");
        println!("├─nvme0n1p1 ext4 1.0   data   87654321-4321-4321-4321-cba987654321    1.5T     21% /data");
        println!("└─nvme0n1p2 swap 1            aabbccdd-eeff-0011-2233-445566778899                  [SWAP]");
    } else {
        println!("NAME        MAJ:MIN RM   SIZE RO TYPE MOUNTPOINTS");
        println!("sda           8:0    0 931.5G  0 disk");
        println!("├─sda1        8:1    0   512M  0 part /boot/efi");
        println!("└─sda2        8:2    0   931G  0 part /");
        println!("nvme0n1     259:0    0   1.9T  0 disk");
        println!("├─nvme0n1p1 259:1    0   1.8T  0 part /data");
        println!("└─nvme0n1p2 259:2    0    16G  0 part [SWAP]");
        println!("sr0          11:0    1  1024M  0 rom");
    }
    0
}

fn run_blkid(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blkid [OPTIONS] [DEVICE...]");
        println!();
        println!("blkid — locate/print block device attributes (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c FILE      Read from cache FILE");
        println!("  -g           Garbage collect cache");
        println!("  -o FORMAT    Output format (value, device, export, full)");
        println!("  -p           Low-level probing mode");
        println!("  -s TAG       Show only TAG");
        println!("  -t TAG       Find by TAG");
        println!("  -l           Look up one device with TAG");
        return 0;
    }

    let device = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());

    if let Some(dev) = device {
        println!("{}: UUID=\"12345678-1234-1234-1234-123456789abc\" BLOCK_SIZE=\"4096\" TYPE=\"ext4\"", dev);
    } else {
        println!("/dev/sda1: UUID=\"ABCD-1234\" BLOCK_SIZE=\"512\" TYPE=\"vfat\" PARTLABEL=\"EFI\" PARTUUID=\"11111111-2222-3333-4444-555555555555\"");
        println!("/dev/sda2: UUID=\"12345678-1234-1234-1234-123456789abc\" BLOCK_SIZE=\"4096\" TYPE=\"ext4\" PARTUUID=\"66666666-7777-8888-9999-aaaaaaaaaaaa\"");
        println!("/dev/nvme0n1p1: UUID=\"87654321-4321-4321-4321-cba987654321\" BLOCK_SIZE=\"4096\" TYPE=\"ext4\" PARTLABEL=\"data\" PARTUUID=\"bbbbbbbb-cccc-dddd-eeee-ffffffffffff\"");
        println!("/dev/nvme0n1p2: UUID=\"aabbccdd-eeff-0011-2233-445566778899\" TYPE=\"swap\" PARTUUID=\"00112233-4455-6677-8899-aabbccddeeff\"");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lsblk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "blkid" => run_blkid(&rest),
        _ => run_lsblk(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lsblk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lsblk"), "lsblk");
        assert_eq!(basename(r"C:\bin\lsblk.exe"), "lsblk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lsblk.exe"), "lsblk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lsblk(&["--help".to_string()]), 0);
        assert_eq!(run_lsblk(&["-h".to_string()]), 0);
        let _ = run_lsblk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lsblk(&[]);
    }
}
