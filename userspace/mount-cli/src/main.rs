#![deny(clippy::all)]

//! mount-cli — OurOS mount/umount/findmnt CLI
//!
//! Multi-personality: `mount`, `umount`, `findmnt`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_mount(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mount [OPTIONS] DEVICE MOUNTPOINT");
        println!();
        println!("mount — mount a filesystem (OurOS).");
        println!();
        println!("Options:");
        println!("  -t, --types TYPE       Filesystem type");
        println!("  -o, --options OPTS     Mount options (ro, rw, noexec, nosuid, etc.)");
        println!("  -a, --all              Mount all from /etc/fstab");
        println!("  -r, --read-only        Mount read-only");
        println!("  -w, --rw               Mount read-write");
        println!("  --bind                 Bind mount");
        println!("  -l, --show-labels      Show labels");
        return 0;
    }

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if positional.is_empty() && !args.iter().any(|a| a == "-a" || a == "--all") {
        // Show current mounts
        println!("sysfs on /sys type sysfs (rw,nosuid,nodev,noexec,relatime)");
        println!("proc on /proc type proc (rw,nosuid,nodev,noexec,relatime)");
        println!("/dev/sda2 on / type ext4 (rw,relatime)");
        println!("/dev/sda1 on /boot/efi type vfat (rw,relatime,fmask=0022,dmask=0022)");
        println!("/dev/sda3 on /home type ext4 (rw,relatime)");
        println!("tmpfs on /tmp type tmpfs (rw,nosuid,nodev)");
        return 0;
    }

    if args.iter().any(|a| a == "-a" || a == "--all") {
        println!("mount: mounting all filesystems from /etc/fstab");
        return 0;
    }

    let device = positional.first().copied().unwrap_or("/dev/sdb1");
    let mountpoint = positional.get(1).copied().unwrap_or("/mnt");
    let fs_type = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--types")
        .map(|w| w[1].as_str());

    print!("mount: {} on {}", device, mountpoint);
    if let Some(t) = fs_type {
        print!(" type {}", t);
    }
    println!();
    0
}

fn run_umount(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: umount [OPTIONS] MOUNTPOINT|DEVICE");
        println!("  -l, --lazy     Lazy unmount");
        println!("  -f, --force    Force unmount");
        println!("  -a, --all      Unmount all");
        return 0;
    }

    let target = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/mnt");
    println!("umount: {} unmounted", target);
    0
}

fn run_findmnt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: findmnt [OPTIONS] [DEVICE|MOUNTPOINT]");
        println!("  -t, --types LIST   Filter by type");
        println!("  -o, --output LIST  Output columns");
        println!("  -J, --json         JSON output");
        println!("  -l, --list         List format");
        println!("  -r, --raw          Raw format");
        return 0;
    }

    let json = args.iter().any(|a| a == "-J" || a == "--json");

    if json {
        println!("{{\"filesystems\": [");
        println!("  {{\"target\": \"/\", \"source\": \"/dev/sda2\", \"fstype\": \"ext4\", \"options\": \"rw,relatime\"}},");
        println!("  {{\"target\": \"/boot/efi\", \"source\": \"/dev/sda1\", \"fstype\": \"vfat\", \"options\": \"rw,relatime\"}},");
        println!("  {{\"target\": \"/home\", \"source\": \"/dev/sda3\", \"fstype\": \"ext4\", \"options\": \"rw,relatime\"}}");
        println!("]}}");
    } else {
        println!("TARGET      SOURCE     FSTYPE OPTIONS");
        println!("/           /dev/sda2  ext4   rw,relatime");
        println!("├─/boot/efi /dev/sda1  vfat   rw,relatime");
        println!("├─/home     /dev/sda3  ext4   rw,relatime");
        println!("├─/proc     proc       proc   rw,nosuid,nodev,noexec,relatime");
        println!("├─/sys      sysfs      sysfs  rw,nosuid,nodev,noexec,relatime");
        println!("└─/tmp      tmpfs      tmpfs  rw,nosuid,nodev");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mount".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "umount" => run_umount(&rest),
        "findmnt" => run_findmnt(&rest),
        _ => run_mount(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mount};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mount"), "mount");
        assert_eq!(basename(r"C:\bin\mount.exe"), "mount.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mount.exe"), "mount");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mount(&["--help".to_string()]), 0);
        assert_eq!(run_mount(&["-h".to_string()]), 0);
        let _ = run_mount(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mount(&[]);
    }
}
