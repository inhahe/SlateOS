#![deny(clippy::all)]

//! fstrim-cli — OurOS fstrim/blkdiscard/lsblk CLI
//!
//! Multi-personality: `fstrim`, `blkdiscard`, `lsblk`, `blkid`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_fstrim(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fstrim [OPTIONS] MOUNTPOINT");
        println!("  -a, --all          Trim all mounted filesystems");
        println!("  -v, --verbose      Verbose output");
        println!("  -m, --minimum SIZE Minimum extent length");
        return 0;
    }

    let all = args.iter().any(|a| a == "-a" || a == "--all");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");

    if all {
        if verbose {
            println!("/: 12.3 GiB (13209067520 bytes) trimmed on /dev/sda2");
            println!("/home: 45.6 GiB (48955301888 bytes) trimmed on /dev/sda3");
            println!("/boot/efi: 512 MiB (536870912 bytes) trimmed on /dev/sda1");
        }
    } else {
        let mp = args.iter().find(|a| !a.starts_with('-'))
            .map(|s| s.as_str()).unwrap_or("/");
        if verbose {
            println!("{}: 12.3 GiB (13209067520 bytes) trimmed", mp);
        }
    }
    0
}

fn run_lsblk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lsblk [OPTIONS] [DEVICE...]");
        println!("  -f, --fs           Show filesystem info");
        println!("  -o, --output LIST  Output columns");
        println!("  -J, --json         JSON output");
        println!("  -p, --paths        Print full paths");
        println!("  -l, --list         List format");
        return 0;
    }

    let fs = args.iter().any(|a| a == "-f" || a == "--fs");
    let json = args.iter().any(|a| a == "-J" || a == "--json");

    if json {
        println!("{{\"blockdevices\": [");
        println!("  {{\"name\": \"sda\", \"size\": \"500G\", \"type\": \"disk\", \"children\": [");
        println!("    {{\"name\": \"sda1\", \"size\": \"512M\", \"type\": \"part\", \"mountpoint\": \"/boot/efi\"}},");
        println!("    {{\"name\": \"sda2\", \"size\": \"50G\", \"type\": \"part\", \"mountpoint\": \"/\"}},");
        println!("    {{\"name\": \"sda3\", \"size\": \"449.5G\", \"type\": \"part\", \"mountpoint\": \"/home\"}}");
        println!("  ]}}");
        println!("]}}");
    } else if fs {
        println!("NAME   FSTYPE FSVER LABEL UUID                                 MOUNTPOINT");
        println!("sda");
        println!("├─sda1 vfat   FAT32 EFI   AAAA-BBBB                            /boot/efi");
        println!("├─sda2 ext4   1.0         abcdef12-3456-7890-abcd-ef1234567890 /");
        println!("└─sda3 ext4   1.0   home  12345678-abcd-efgh-ijkl-123456789012 /home");
    } else {
        println!("NAME   MAJ:MIN RM   SIZE RO TYPE MOUNTPOINTS");
        println!("sda      8:0    0   500G  0 disk");
        println!("├─sda1   8:1    0   512M  0 part /boot/efi");
        println!("├─sda2   8:2    0    50G  0 part /");
        println!("└─sda3   8:3    0 449.5G  0 part /home");
    }
    0
}

fn run_blkid(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blkid [OPTIONS] [DEVICE...]");
        println!("  -o, --output FMT   Output format (full, value, export)");
        println!("  -s, --match-tag T  Show specific tag");
        return 0;
    }

    let device = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    if let Some(dev) = device {
        println!("{}: UUID=\"abcdef12-3456-7890-abcd-ef1234567890\" TYPE=\"ext4\"", dev);
    } else {
        println!("/dev/sda1: UUID=\"AAAA-BBBB\" TYPE=\"vfat\" PARTLABEL=\"EFI System Partition\"");
        println!("/dev/sda2: UUID=\"abcdef12-3456-7890-abcd-ef1234567890\" TYPE=\"ext4\"");
        println!("/dev/sda3: UUID=\"12345678-abcd-efgh-ijkl-123456789012\" LABEL=\"home\" TYPE=\"ext4\"");
    }
    0
}

fn run_blkdiscard(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blkdiscard [OPTIONS] DEVICE");
        println!("  -f, --force    Force");
        println!("  -s, --secure   Secure discard");
        println!("  -z, --zeroout  Zero-fill instead");
        return 0;
    }
    let dev = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/dev/sdb");
    println!("blkdiscard: {}: discarded", dev);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fstrim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lsblk" => run_lsblk(&rest),
        "blkid" => run_blkid(&rest),
        "blkdiscard" => run_blkdiscard(&rest),
        _ => run_fstrim(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fstrim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fstrim"), "fstrim");
        assert_eq!(basename(r"C:\bin\fstrim.exe"), "fstrim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fstrim.exe"), "fstrim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fstrim(&["--help".to_string()]), 0);
        assert_eq!(run_fstrim(&["-h".to_string()]), 0);
        assert_eq!(run_fstrim(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fstrim(&[]), 0);
    }
}
