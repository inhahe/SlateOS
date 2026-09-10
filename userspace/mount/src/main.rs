//! Slate OS Mount/Umount Utility
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
// Native mount/umount ABI
// ============================================================================
//
// The Slate OS kernel exposes two native syscalls for filesystem namespace
// management (see kernel/src/syscall/number.rs):
//
//   * SYS_FS_MOUNT  = 652  — mount a filesystem at a target path.
//       arg0/arg1 = source/device string ptr+len (may be empty for pseudo-fs)
//       arg2/arg3 = target mount-point path ptr+len
//       arg4/arg5 = filesystem-type string ptr+len
//     All six argument slots are consumed by the three string pairs, so mount
//     flags/options are not yet part of the ABI (deferred to a versioned
//     extension). Root-only.
//
//   * SYS_FS_UMOUNT = 653  — unmount the filesystem at a target path.
//       arg0/arg1 = target mount-point path ptr+len
//     Refuses to unmount "/" and refuses if sub-mounts exist (DeviceBusy).
//     Root-only.
//
// An earlier version of this file collided with the trash syscalls (620/621);
// those numbers are NOT used here. The real syscalls above are issued via the
// native SYSCALL convention (rax=nr, rdi/rsi/rdx/r10/r8/r9 = arg0..arg5).

/// `SYS_FS_MOUNT` — mount a filesystem at a target path.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const SYS_FS_MOUNT: u64 = 652;

/// `SYS_FS_UMOUNT` — unmount the filesystem at a target path.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const SYS_FS_UMOUNT: u64 = 653;

/// Translate a negative kernel error code into a human-readable message.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
fn kernel_errstr(code: i64) -> &'static str {
    match code {
        -2 => "operation not supported (unknown or unsupported filesystem type)",
        -3 => "invalid argument",
        -400 => "permission denied (mount/umount require root)",
        -500 => "no such file or directory",
        -501 => "already mounted / target exists",
        -601 => "no such device",
        -602 => "device busy (in use or has sub-mounts)",
        -509 => "read-only filesystem",
        _ => "mount operation failed",
    }
}

/// Invoke a 6-argument native syscall via inline x86_64 assembly.
///
/// Uses the SlateOS native SYSCALL convention: number in `rax`, arguments in
/// `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`. `rcx`/`r11` are clobbered by the
/// `syscall` instruction itself.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    // SAFETY: the caller guarantees that any pointer arguments reference
    // live, correctly-sized buffers for the duration of the call. The kernel
    // validates all user pointers before dereferencing them.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Map a kernel fstype hint to the canonical string the kernel mount
/// dispatcher recognises. Returns `None` for "auto"/unknown — the kernel
/// has no auto-detection, so the caller must specify a type.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
fn canonical_fstype(fstype: &str) -> Option<&'static str> {
    match fstype {
        "ext4" => Some("ext4"),
        "tmpfs" | "ramfs" | "memfs" => Some("tmpfs"),
        "iso9660" | "iso" | "cd9660" | "udf" => Some("iso9660"),
        "devfs" | "dev" => Some("devfs"),
        "proc" | "procfs" => Some("proc"),
        "sysfs" | "sys" => Some("sysfs"),
        "vfat" | "fat" | "fat32" | "fat16" | "msdos" => Some("vfat"),
        _ => None,
    }
}

/// Mount a filesystem via `SYS_FS_MOUNT`.
///
/// `flags`/`data` are accepted for command-line compatibility but the kernel
/// ABI does not yet carry mount options, so unsupported flags (bind, remount)
/// are rejected up front rather than silently ignored.
#[cfg(target_vendor = "slateos")]
fn do_mount(
    source: &str,
    target: &str,
    fstype: &str,
    flags: u64,
    _data: &str,
) -> Result<(), String> {
    if flags & MS_BIND != 0 {
        return Err("mount: bind mounts are not supported by the kernel ABI".to_string());
    }
    if flags & MS_REMOUNT != 0 {
        return Err("mount: remount is not supported by the kernel ABI".to_string());
    }
    if flags & (MS_RDONLY | MS_NOSUID | MS_NODEV | MS_NOEXEC) != 0 {
        eprintln!(
            "mount: warning: mount options are not yet honoured by the kernel \
             (mounting read-write)"
        );
    }

    let kfstype = match canonical_fstype(fstype) {
        Some(t) => t,
        None => {
            return Err(format!(
                "mount: cannot determine filesystem type '{fstype}' \
                 (the kernel has no auto-detection; specify -t <type>: \
                 ext4, vfat, iso9660, tmpfs, proc, sysfs, devfs)"
            ));
        }
    };

    // Pseudo-filesystems ignore the source; pass it through regardless so a
    // device-backed fs (ext4/vfat/iso9660) receives its block-device name.
    let src = source.as_bytes();
    let tgt = target.as_bytes();
    let fst = kfstype.as_bytes();

    // SAFETY: all three slices stay live across the call; the kernel validates
    // each pointer+length before reading user memory.
    let ret = unsafe {
        syscall6(
            SYS_FS_MOUNT,
            src.as_ptr() as u64,
            src.len() as u64,
            tgt.as_ptr() as u64,
            tgt.len() as u64,
            fst.as_ptr() as u64,
            fst.len() as u64,
        )
    };

    if ret < 0 {
        return Err(format!("mount: {} (code {ret})", kernel_errstr(ret)));
    }
    Ok(())
}

/// Unmount a filesystem via `SYS_FS_UMOUNT`.
///
/// `flags` (force/lazy) are accepted for command-line compatibility but the
/// kernel performs a plain unmount; force/lazy semantics are not yet wired.
#[cfg(target_vendor = "slateos")]
fn do_umount(target: &str, flags: u64) -> Result<(), String> {
    if flags & (MNT_FORCE | MNT_DETACH) != 0 {
        eprintln!("umount: warning: force/lazy unmount not supported; performing a normal unmount");
    }

    let tgt = target.as_bytes();
    // SAFETY: `tgt` stays live across the call; the kernel validates the
    // pointer+length before reading user memory.
    let ret = unsafe {
        syscall6(
            SYS_FS_UMOUNT,
            tgt.as_ptr() as u64,
            tgt.len() as u64,
            0,
            0,
            0,
            0,
        )
    };

    if ret < 0 {
        return Err(format!("umount: {} (code {ret})", kernel_errstr(ret)));
    }
    Ok(())
}

/// Host build fallback: the native mount syscall cannot run off-target.
#[cfg(not(target_vendor = "slateos"))]
fn do_mount(
    _source: &str,
    _target: &str,
    _fstype: &str,
    _flags: u64,
    _data: &str,
) -> Result<(), String> {
    Err("mount: native syscall unavailable on this host architecture".to_string())
}

/// Host build fallback: the native umount syscall cannot run off-target.
#[cfg(not(target_vendor = "slateos"))]
fn do_umount(_target: &str, _flags: u64) -> Result<(), String> {
    Err("umount: native syscall unavailable on this host architecture".to_string())
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

/// `/proc/mounts`, through [`procinfo`].
///
/// # Two things the hand-parser got wrong
///
/// **The kernel escapes whitespace.** Space, tab, newline and backslash in the
/// device and mount-point fields are written `\040`, `\011`, `\012` and
/// `\134`, so `/mnt/my backup` appears as `/mnt/my\040backup`. Splitting on
/// whitespace and taking the fields verbatim therefore produced a mount point
/// that matched nothing a user could type. `procinfo::Mount` undoes it; the
/// crate's module doc names the escaping as one of the three reasons it
/// exists.
///
/// **`read_to_string` fails for the whole file** when any single line holds a
/// byte that is not UTF-8 -- which this filesystem allows in every path
/// component. One oddly-named mount therefore hid *every* mount, not merely
/// its own.
///
/// The fields are escaped for display on the way out rather than carried as
/// bytes: a space is printable and survives untouched, so paths a user can
/// type compare exactly as before, while a byte they cannot type shows as
/// `\xNN` instead of taking the rest of the table with it.
fn read_mounts() -> Vec<MountEntry> {
    read_mounts_in(&procinfo::ProcFs::new())
}

/// [`read_mounts`] against a given `/proc`, so the parsing can be reached from
/// a test.
fn read_mounts_in(proc: &procinfo::ProcFs) -> Vec<MountEntry> {
    proc.mounts()
        .ok()
        .flatten()
        .unwrap_or_default()
        .iter()
        .map(|m| MountEntry {
            device: quoting::escape_unprintable(&m.device),
            mount_point: quoting::escape_unprintable(&m.mount_point),
            fs_type: quoting::escape_unprintable(&m.fstype),
            options: quoting::escape_unprintable(&m.options),
        })
        .collect()
}

fn list_mounts() {
    let mounts = read_mounts();

    if mounts.is_empty() {
        println!("(no mounts found — /proc/mounts not available)");
        return;
    }

    for m in &mounts {
        println!(
            "{} on {} type {} ({})",
            m.device, m.mount_point, m.fs_type, m.options
        );
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
        if current_mounts
            .iter()
            .any(|m| m.mount_point == entry.mount_point)
        {
            println!("  skip:  {} (already mounted)", entry.mount_point);
            continue;
        }

        let (flags, data) = parse_mount_options(&entry.options);
        print!(
            "  mount: {} on {} ({})... ",
            entry.device, entry.mount_point, entry.fs_type
        );

        match do_mount(
            &entry.device,
            &entry.mount_point,
            &entry.fs_type,
            flags,
            &data,
        ) {
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
    println!("Slate OS Mount Utility v0.1.0");
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
    let progname = args.first().map(|s| s.as_str()).unwrap_or("mount");

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
            println!("{:<20} {:<20} {:<10} Options", "Device", "Mount", "Type");
            println!("{:<20} {:<20} {:<10} -------", "------", "-----", "----");
            for e in &entries {
                println!(
                    "{:<20} {:<20} {:<10} {}",
                    e.device, e.mount_point, e.fs_type, e.options
                );
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

    // ---- reading /proc/mounts ----

    use scratchdir::ScratchDir;

    fn proc_with_mounts(scratch: &ScratchDir, content: &[u8]) -> procinfo::ProcFs {
        let root = scratch.path("proc");
        std::fs::create_dir_all(&root).expect("create fixture /proc");
        std::fs::write(root.join("mounts"), content).expect("write fixture mounts");
        procinfo::ProcFs::at(root)
    }

    /// **A mount point containing a space arrives as a space.**
    ///
    /// The kernel writes it as `\040`. The hand-parser this replaces split on
    /// whitespace and kept the field verbatim, so `mount` showed a mount point
    /// spelled `/mnt/my\040backup` -- which matches nothing a user can type,
    /// and is not what is mounted.
    #[test]
    fn an_escaped_space_in_a_mount_point_is_unescaped() {
        let scratch = ScratchDir::new("mount-escape");
        let proc = proc_with_mounts(
            &scratch,
            b"/dev/sda1 /mnt/my\\040backup ext4 rw,relatime 0 0\n",
        );
        let mounts = read_mounts_in(&proc);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mount_point, "/mnt/my backup");
    }

    /// **One mount with a byte that is not UTF-8 used to hide every mount.**
    ///
    /// `read_to_string` fails for the whole file, so the entire table came
    /// back empty -- `mount` reported no filesystems at all rather than one
    /// awkward one.
    #[test]
    fn a_mount_that_is_not_utf8_does_not_hide_the_others() {
        let scratch = ScratchDir::new("mount-nonutf8");
        let proc = proc_with_mounts(
            &scratch,
            b"/dev/odd /mnt/od\xffd ext4 rw 0 0\n/dev/sda1 / ext4 rw 0 0\n",
        );
        let mounts = read_mounts_in(&proc);
        assert_eq!(mounts.len(), 2, "both lines are still lines");
        assert!(
            mounts.iter().any(|m| m.mount_point == "/"),
            "the readable mount must still be listed"
        );
        // `\377`, not `\xff`: `quoting::escape_unprintable` uses *octal*
        // escapes, which is what GNU's own quoting does and what the rest of
        // this tree's diagnostics already print.
        assert!(
            mounts.iter().any(|m| m.mount_point.contains("\\377")),
            "and the odd one shows its byte escaped rather than vanishing"
        );
    }

    /// A missing `/proc/mounts` is an empty table, not a panic.
    #[test]
    fn a_missing_proc_mounts_is_empty() {
        let scratch = ScratchDir::new("mount-missing");
        let proc = procinfo::ProcFs::at(scratch.path("nonexistent"));
        assert!(read_mounts_in(&proc).is_empty());
    }

    /// The ordinary case still parses every field.
    #[test]
    fn an_ordinary_line_parses_all_four_fields() {
        let scratch = ScratchDir::new("mount-ordinary");
        let proc = proc_with_mounts(&scratch, b"/dev/sda1 / ext4 rw,relatime 0 0\n");
        let mounts = read_mounts_in(&proc);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].device, "/dev/sda1");
        assert_eq!(mounts[0].mount_point, "/");
        assert_eq!(mounts[0].fs_type, "ext4");
        assert_eq!(mounts[0].options, "rw,relatime");
    }
    use super::*;

    // mount/umount now issue the real SYS_FS_MOUNT / SYS_FS_UMOUNT syscalls
    // (652/653). On the build host the native syscall path is compiled out,
    // so do_mount/do_umount return the off-target fallback error; we exercise
    // the pure decode/validation helpers instead.

    #[test]
    fn test_canonical_fstype_known() {
        assert_eq!(canonical_fstype("ext4"), Some("ext4"));
        assert_eq!(canonical_fstype("ramfs"), Some("tmpfs"));
        assert_eq!(canonical_fstype("fat32"), Some("vfat"));
        assert_eq!(canonical_fstype("procfs"), Some("proc"));
    }

    #[test]
    fn test_canonical_fstype_auto_is_none() {
        assert_eq!(canonical_fstype("auto"), None);
        assert_eq!(canonical_fstype("xfs"), None);
    }

    #[test]
    fn test_kernel_errstr_known_codes() {
        assert!(kernel_errstr(-400).contains("permission"));
        assert!(kernel_errstr(-602).contains("busy"));
        assert!(kernel_errstr(-2).contains("not supported"));
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
