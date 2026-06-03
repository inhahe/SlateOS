//! OurOS Mount/Umount Utility
//!
//! Manages filesystem mount points. Lists current mounts, mounts new
//! filesystems, and unmounts existing ones.
//!
//! # Usage
//!
//! ```text
//! mount                         List all mount points
//! mount <device> <mountpoint>   Mount a device
//! mount -t <type> <dev> <dir>   Mount with explicit filesystem type
//! mount -o <opts> <dev> <dir>   Mount with options (ro, rw, noexec, nosuid, etc.)
//! mount -a                      Mount all entries from /etc/fstab
//! mount --bind <src> <dst>      Bind mount
//! umount <mountpoint>           Unmount a filesystem
//! umount -f <mountpoint>        Force unmount
//! umount -l <mountpoint>        Lazy unmount (detach now, cleanup later)
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// DESIGN GAP -- mount/umount have no kernel ABI yet
// ============================================================================
//
// The OurOS kernel does **not** expose mount(2) or umount(2) syscalls. There
// is no SYS_MOUNT or SYS_UMOUNT in kernel/src/syscall/number.rs.
//
// An earlier version of this file hardcoded SYS_MOUNT=620 and SYS_UMOUNT=621
// and fired them via a raw `syscall` instruction. Both numbers map to
// **DESTRUCTIVE** unrelated filesystem syscalls on OurOS:
//
//   * 620 = SYS_FS_TRASH_RESTORE — "mount /dev/sda1 /mnt" would call
//     fs::trash::restore(low_bits_of_src_ptr_as_filename_ptr, ...) and
//     attempt to restore a file from the trash bin using arbitrary
//     filename bytes.
//   * 621 = SYS_FS_TRASH_EMPTY — "umount /mnt" would call
//     fs::trash::empty(), **permanently deleting every file currently
//     in the user's recycle bin**. This is catastrophic data loss
//     gated only by a File-WRITE capability, which every admin tool
//     already holds. A sysadmin running `umount /mnt` to clear a stuck
//     mount would silently nuke the trash without warning.
//
// The safe and correct interim behavior is to fail with a clear "not
// implemented" error. The tool stays in the tree so it's ready when the
// kernel ABI lands. See todo.txt for the tracking entry.

/// Stub return for every mount/umount operation in this tool.
#[inline]
fn enosys(op: &str) -> Result<(), String> {
    Err(format!(
        "{op}: not implemented in this kernel \
         (no SYS_MOUNT / SYS_UMOUNT ABI yet)"
    ))
}

/// Mount a filesystem.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block
/// above for why the previous implementation was removed.
fn do_mount(_source: &str, _target: &str, _fstype: &str, _flags: u64, _data: &str) -> Result<(), String> {
    enosys("mount")
}

/// Unmount a filesystem.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block
/// above for why the previous implementation was removed.
fn do_umount(_target: &str, _flags: u64) -> Result<(), String> {
    enosys("umount")
}

// ============================================================================
// Mount flags
// ============================================================================

const MS_RDONLY: u64 = 1;
const MS_NOSUID: u64 = 2;
const MS_NODEV: u64 = 4;
const MS_NOEXEC: u64 = 8;
const MS_REMOUNT: u64 = 32;
const MS_BIND: u64 = 4096;

const MNT_FORCE: u64 = 1;
const MNT_DETACH: u64 = 2;

fn parse_mount_options(opts_str: &str) -> (u64, String) {
    let mut flags = 0u64;
    let mut data_parts = Vec::new();

    for opt in opts_str.split(',') {
        let opt = opt.trim();
        match opt {
            "ro" | "readonly" => flags |= MS_RDONLY,
            "rw" | "readwrite" => { /* default, clear rdonly */ }
            "nosuid" => flags |= MS_NOSUID,
            "nodev" => flags |= MS_NODEV,
            "noexec" => flags |= MS_NOEXEC,
            "remount" => flags |= MS_REMOUNT,
            "bind" => flags |= MS_BIND,
            "" => {}
            other => data_parts.push(other.to_string()),
        }
    }

    (flags, data_parts.join(","))
}

// ============================================================================
// /proc/mounts reader
// ============================================================================

struct MountEntry {
    device: String,
    mount_point: String,
    fs_type: String,
    options: String,
}

fn read_mounts() -> Vec<MountEntry> {
    let mut entries = Vec::new();

    let content = match fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            entries.push(MountEntry {
                device: parts[0].to_string(),
                mount_point: parts[1].to_string(),
                fs_type: parts[2].to_string(),
                options: parts[3].to_string(),
            });
        }
    }

    entries
}

fn list_mounts() {
    let mounts = read_mounts();

    if mounts.is_empty() {
        println!("(no mounts found — /proc/mounts not available)");
        return;
    }

    for m in &mounts {
        println!("{} on {} type {} ({})", m.device, m.mount_point, m.fs_type, m.options);
    }
}

// ============================================================================
// /etc/fstab reader
// ============================================================================

struct FstabEntry {
    device: String,
    mount_point: String,
    fs_type: String,
    options: String,
    #[allow(dead_code)] // dump and pass are read from fstab but only used for mount -a
    dump: u32,
    #[allow(dead_code)]
    pass: u32,
}

fn read_fstab() -> Vec<FstabEntry> {
    let mut entries = Vec::new();

    let content = match fs::read_to_string("/etc/fstab") {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            entries.push(FstabEntry {
                device: parts[0].to_string(),
                mount_point: parts[1].to_string(),
                fs_type: parts[2].to_string(),
                options: parts[3].to_string(),
                dump: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                pass: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }

    entries
}

/// Mount all filesystems from /etc/fstab that aren't already mounted.
fn mount_all() {
    let fstab = read_fstab();
    let current_mounts = read_mounts();

    if fstab.is_empty() {
        println!("No entries in /etc/fstab");
        return;
    }

    let mut success = 0u32;
    let mut fail = 0u32;

    for entry in &fstab {
        // Skip swap entries and "none" mount points.
        if entry.fs_type == "swap" || entry.mount_point == "none" {
            continue;
        }

        // Skip if already mounted.
        if current_mounts.iter().any(|m| m.mount_point == entry.mount_point) {
            println!("  skip:  {} (already mounted)", entry.mount_point);
            continue;
        }

        let (flags, data) = parse_mount_options(&entry.options);
        print!("  mount: {} on {} ({})... ", entry.device, entry.mount_point, entry.fs_type);

        match do_mount(&entry.device, &entry.mount_point, &entry.fs_type, flags, &data) {
            Ok(()) => {
                println!("ok");
                success += 1;
            }
            Err(e) => {
                println!("FAILED: {e}");
                fail += 1;
            }
        }
    }

    println!("\nMounted: {success}, Failed: {fail}");
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("OurOS Mount Utility v0.1.0");
    println!();
    println!("List, mount, and unmount filesystems.");
    println!();
    println!("USAGE:");
    println!("  mount                           List all mount points");
    println!("  mount <device> <mountpoint>     Mount a device");
    println!("  mount -t <type> <dev> <dir>     Specify filesystem type");
    println!("  mount -o <opts> <dev> <dir>     Mount with options");
    println!("  mount -a                        Mount all from /etc/fstab");
    println!("  mount --bind <src> <dst>        Bind mount");
    println!("  mount --fstab                   Show /etc/fstab entries");
    println!("  umount <mountpoint>             Unmount");
    println!("  umount -f <mountpoint>          Force unmount");
    println!("  umount -l <mountpoint>          Lazy unmount");
    println!();
    println!("OPTIONS:");
    println!("  -t <type>   Filesystem type (ext4, fat32, tmpfs, procfs, sysfs, ...)");
    println!("  -o <opts>   Comma-separated: ro, rw, nosuid, nodev, noexec, remount");
    println!("  -a          Mount all /etc/fstab entries");
    println!("  --bind      Bind mount (mirror a directory tree)");
    println!("  -f          Force unmount (even if busy)");
    println!("  -l          Lazy unmount (detach now, clean up when idle)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let progname = args.first()
        .map(|s| s.as_str())
        .unwrap_or("mount");

    // Detect if invoked as "umount".
    let is_umount = progname.ends_with("umount");

    if is_umount {
        // umount mode.
        if args.len() < 2 {
            eprintln!("usage: umount [-f|-l] <mountpoint>");
            process::exit(1);
        }

        let mut force = false;
        let mut lazy = false;
        let mut target = None;

        for arg in args.iter().skip(1) {
            match arg.as_str() {
                "-f" | "--force" => force = true,
                "-l" | "--lazy" => lazy = true,
                "--help" | "-h" => {
                    print_usage();
                    process::exit(0);
                }
                s => target = Some(s.to_string()),
            }
        }

        let target = match target {
            Some(t) => t,
            None => {
                eprintln!("error: no mount point specified");
                process::exit(1);
            }
        };

        let flags = if force {
            MNT_FORCE
        } else if lazy {
            MNT_DETACH
        } else {
            0
        };

        match do_umount(&target, flags) {
            Ok(()) => println!("Unmounted {target}"),
            Err(e) => {
                eprintln!("{e}");
                process::exit(1);
            }
        }
        return;
    }

    // mount mode.
    if args.len() < 2 {
        list_mounts();
        return;
    }

    let mut fstype = String::new();
    let mut options = String::new();
    let mut bind = false;
    let mut mount_all_flag = false;
    let mut show_fstab = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -t requires a filesystem type");
                    process::exit(1);
                }
                fstype = args[i + 1].clone();
                i += 2;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -o requires options");
                    process::exit(1);
                }
                options = args[i + 1].clone();
                i += 2;
            }
            "-a" | "--all" => {
                mount_all_flag = true;
                i += 1;
            }
            "--bind" => {
                bind = true;
                i += 1;
            }
            "--fstab" => {
                show_fstab = true;
                i += 1;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }

    if show_fstab {
        let entries = read_fstab();
        if entries.is_empty() {
            println!("No entries in /etc/fstab (or file not found)");
        } else {
            println!("{:<20} {:<20} {:<10} {}", "Device", "Mount", "Type", "Options");
            println!("{:<20} {:<20} {:<10} {}", "------", "-----", "----", "-------");
            for e in &entries {
                println!("{:<20} {:<20} {:<10} {}", e.device, e.mount_point, e.fs_type, e.options);
            }
        }
        return;
    }

    if mount_all_flag {
        mount_all();
        return;
    }

    if positional.len() < 2 {
        if positional.is_empty() {
            list_mounts();
        } else {
            eprintln!("error: both <device> and <mountpoint> required");
            eprintln!("Run 'mount --help' for usage.");
            process::exit(1);
        }
        return;
    }

    let device = &positional[0];
    let mountpoint = &positional[1];

    // Parse options into flags + data.
    let (mut flags, data) = parse_mount_options(&options);
    if bind {
        flags |= MS_BIND;
    }

    // Auto-detect filesystem type if not specified.
    if fstype.is_empty() {
        fstype = "auto".to_string();
    }

    match do_mount(device, mountpoint, &fstype, flags, &data) {
        Ok(()) => println!("Mounted {device} on {mountpoint} (type {fstype})"),
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Confirm mount/umount fail safely instead of firing the destructive
    // SYS_FS_TRASH_RESTORE / SYS_FS_TRASH_EMPTY syscalls that the old code
    // collided with. See the DESIGN GAP block near the top of this file.

    #[test]
    fn test_do_mount_returns_enosys() {
        let err = do_mount("/dev/sda1", "/mnt", "ext4", 0, "").unwrap_err();
        assert!(err.contains("mount"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_do_umount_returns_enosys() {
        let err = do_umount("/mnt", 0).unwrap_err();
        assert!(err.contains("umount"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_parse_mount_options_ro() {
        let (flags, data) = parse_mount_options("ro,noexec");
        assert_eq!(flags & MS_RDONLY, MS_RDONLY);
        assert_eq!(flags & MS_NOEXEC, MS_NOEXEC);
        assert_eq!(data, "");
    }

    #[test]
    fn test_parse_mount_options_bind() {
        let (flags, _) = parse_mount_options("bind");
        assert_eq!(flags & MS_BIND, MS_BIND);
    }

    #[test]
    fn test_parse_mount_options_pass_through_unknown() {
        let (_, data) = parse_mount_options("uid=1000,gid=1000");
        assert!(data.contains("uid=1000"));
        assert!(data.contains("gid=1000"));
    }
}
