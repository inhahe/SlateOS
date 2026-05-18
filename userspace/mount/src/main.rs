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
// Syscall interface
// ============================================================================

// Mount/unmount syscalls (from kernel syscall table, fs range 600-799).
const SYS_MOUNT: u64 = 620;
const SYS_UMOUNT: u64 = 621;

/// Invoke a syscall. In our OS, userspace calls the kernel via `syscall`.
/// For now, we use inline asm on x86_64.
///
/// Arguments: rdi=arg1, rsi=arg2, rdx=arg3, r10=arg4, r8=arg5.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall5(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid pointers/values for the
    // given syscall number. The kernel validates all inputs.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Mount a filesystem.
///
/// `source`: device path (null-terminated).
/// `target`: mount point (null-terminated).
/// `fstype`: filesystem type (null-terminated).
/// `flags`: mount flags.
/// `data`: filesystem-specific options (null-terminated, or null for none).
fn do_mount(source: &str, target: &str, fstype: &str, flags: u64, data: &str) -> Result<(), String> {
    let src = format!("{source}\0");
    let tgt = format!("{target}\0");
    let fst = format!("{fstype}\0");
    let dat = format!("{data}\0");

    let ret = unsafe {
        syscall5(
            SYS_MOUNT,
            src.as_ptr() as u64,
            tgt.as_ptr() as u64,
            fst.as_ptr() as u64,
            flags,
            dat.as_ptr() as u64,
        )
    };

    if ret < 0 {
        Err(format!("mount failed: error {ret}"))
    } else {
        Ok(())
    }
}

/// Unmount a filesystem.
fn do_umount(target: &str, flags: u64) -> Result<(), String> {
    let tgt = format!("{target}\0");

    let ret = unsafe {
        syscall5(
            SYS_UMOUNT,
            tgt.as_ptr() as u64,
            flags,
            0, 0, 0,
        )
    };

    if ret < 0 {
        Err(format!("umount failed: error {ret}"))
    } else {
        Ok(())
    }
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
