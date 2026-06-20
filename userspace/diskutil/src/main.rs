//! Slate OS Disk Utility
//!
//! Comprehensive disk management tool for listing devices, displaying detailed
//! information, formatting partitions, verifying/repairing filesystems,
//! benchmarking I/O performance, querying S.M.A.R.T. status, and issuing TRIM.
//!
//! Reads from `/sys/block/`, `/proc/partitions`, and `/proc/mounts`.
//!
//! NOTE: `format` is wired to `SYS_FS_FORMAT` (654), `verify`/`repair` to
//! `SYS_FS_CHECK` (655) (both for the FAT family), `usage` to the real
//! `SYS_FS_STATVFS` (608), and `trim` to `SYS_FS_TRIM` (656, fstrim — discards
//! the free space of the mounted filesystem). All read-only listing/info logic
//! works via sysfs/procfs.
//!
//! # Usage
//!
//! ```text
//! diskutil list                    List all disks and partitions
//! diskutil info <device>           Detailed device information
//! diskutil format <device> <fs>    Format a partition (ext4, fat32, tmpfs)
//! diskutil verify <device>         Check filesystem integrity (read-only)
//! diskutil repair <device>         Attempt filesystem repair
//! diskutil usage <path>            Disk space usage for a mount point
//! diskutil benchmark <device>      Sequential read/write speed test
//! diskutil smart <device>          Show S.M.A.R.T. status
//! diskutil trim <device>           Issue TRIM/discard to SSD
//! diskutil partitions <device>     List partition table (MBR or GPT)
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::process;
use std::time::Instant;

// ============================================================================
// Filesystem-admin operations
// ============================================================================

/// `SYS_FS_FORMAT` — format a registered block device (FAT family only so far).
///
/// ABI: arg0/arg1 = device-name ptr+len (the block-device registry name, e.g.
/// "vda"/"sda" — NOT a `/dev/` path), arg2/arg3 = fstype ptr+len, arg4/arg5 =
/// optional label ptr+len (0/0 = none). Root-only and destructive.
const SYS_FS_FORMAT: u64 = 654;

/// `SYS_FS_CHECK` — check/repair a filesystem on a block device (fsck).
///
/// ABI: arg0/arg1 = device-name ptr+len (registry name, not a `/dev/` path),
/// arg2 = flags (bit 0 = repair). Returns the number of outstanding errors
/// (problems found in check-only mode, or remaining after repair), or a
/// negative `KernelError`. FAT family only so far. Root-only.
const SYS_FS_CHECK: u64 = 655;

/// Repair-mode flag bit for `SYS_FS_CHECK` (arg2 bit 0).
const FS_CHECK_REPAIR: u64 = 1 << 0;

/// `SYS_FS_STATVFS` — query filesystem space/config for the FS backing a path.
///
/// ABI: arg0/arg1 = path ptr+len (any path on the target filesystem), arg2 =
/// pointer to a 64-byte output buffer. Returns 0 on success or a negative
/// `KernelError`. Buffer layout (little-endian, all `u64` unless noted):
/// off 0 = block_size, 8 = total_blocks, 16 = free_blocks, 24 = total_inodes,
/// 32 = free_inodes, 40 = max_name_len, 48 = read_only (`u8`).
const SYS_FS_STATVFS: u64 = 608;

/// Size of the `SYS_FS_STATVFS` output buffer, in bytes.
const FS_STATVFS_SIZE: usize = 64;

/// `SYS_FS_TRIM` — discard the free space of a mounted filesystem (fstrim).
///
/// ABI: arg0/arg1 = device-name ptr+len (registry name, not a `/dev/` path).
/// Returns the number of bytes discarded (>= 0) or a negative `KernelError`
/// (e.g. the device is not mounted). Non-destructive: only free blocks are
/// trimmed. Root-only.
const SYS_FS_TRIM: u64 = 656;

// All fs-admin subcommands are now wired to real syscalls: `format` to
// `SYS_FS_FORMAT` (654), `verify`/`repair` to `SYS_FS_CHECK` (655), `usage` to
// `SYS_FS_STATVFS` (608), and `trim` to `SYS_FS_TRIM` (656). The host-build
// syscall fallback returns -38 (ENOSYS) directly.

/// Raw 6-argument native syscall. Returns the kernel's `i64` result (negative
/// values are `-KernelError` codes).
///
/// Register convention: rax = number; rdi/rsi/rdx/r10/r8/r9 = arg0..arg5.
/// rcx/r11 are clobbered by `syscall`, so the 4th argument goes in r10.
#[cfg(target_arch = "x86_64")]
fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    // SAFETY: register-only syscall; the kernel validates the pointers we pass.
    // All clobbers (rax result, rcx, r11) are declared.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host-build fallback so the tool compiles and unit-tests on the dev machine.
#[cfg(not(target_arch = "x86_64"))]
fn syscall6(_nr: u64, _a1: u64, _a2: u64, _a3: u64, _a4: u64, _a5: u64, _a6: u64) -> i64 {
    -38 // ENOSYS on non-target hosts.
}

/// Translate a negative syscall return into a human-readable error.
///
/// Covers both the native `KernelError` codes that `SYS_FS_FORMAT` returns and
/// the legacy POSIX-errno fallbacks used by the not-yet-wired ops.
fn syscall_error_msg(ret: i64) -> String {
    match ret {
        // Native KernelError codes (SYS_FS_FORMAT).
        -2 => "operation not supported".to_string(),
        -3 => "invalid argument".to_string(),
        -400 => "permission denied — must be root".to_string(),
        -500 => "no such device".to_string(),
        -509 => "read-only device".to_string(),
        -601 => "no such device".to_string(),
        -602 => "device busy".to_string(),
        // Legacy POSIX-errno fallbacks (ENOSYS path).
        -1 => "operation not permitted".to_string(),
        -5 => "I/O error".to_string(),
        -12 => "out of memory".to_string(),
        -13 => "permission denied".to_string(),
        -16 => "device busy".to_string(),
        -19 => "no such device".to_string(),
        -22 => "invalid argument".to_string(),
        -28 => "no space left on device".to_string(),
        -30 => "read-only filesystem".to_string(),
        -38 => "function not implemented".to_string(),
        other => format!("error {other}"),
    }
}

// ============================================================================
// Filesystem / sysfs helpers
// ============================================================================

/// Read a file and return its trimmed contents, or None on failure.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a file and parse it as u64, or return 0.
fn read_u64(path: &str) -> u64 {
    read_file(path)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Read a file and parse it as a boolean flag (1 = true).
fn read_bool(path: &str) -> bool {
    read_file(path).is_some_and(|s| s == "1")
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count as a human-readable string (e.g. "1.5G").
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 10) / TIB;
        if frac > 0 {
            format!("{whole}.{frac}T")
        } else {
            format!("{whole}T")
        }
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 10) / GIB;
        if frac > 0 {
            format!("{whole}.{frac}G")
        } else {
            format!("{whole}G")
        }
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 10) / MIB;
        if frac > 0 {
            format!("{whole}.{frac}M")
        } else {
            format!("{whole}M")
        }
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 10) / KIB;
        if frac > 0 {
            format!("{whole}.{frac}K")
        } else {
            format!("{whole}K")
        }
    } else {
        format!("{bytes}B")
    }
}

/// Format bytes per second as a throughput string (e.g. "152.3 MB/s").
fn format_throughput(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000_000.0 {
        format!("{:.1} GB/s", bytes_per_sec / 1_000_000_000.0)
    } else if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 1_000.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

// ============================================================================
// Block device data
// ============================================================================

/// A block device (disk or partition) discovered from sysfs or procfs.
#[allow(dead_code)]
struct BlockDevice {
    /// Kernel name (e.g. "sda", "sda1", "nvme0n1").
    name: String,
    /// Size in 512-byte sectors.
    size_sectors: u64,
    /// "disk" or "part".
    dev_type: String,
    /// Filesystem type if known (e.g. "ext4", "fat32").
    fstype: String,
    /// Filesystem label if known.
    label: String,
    /// Filesystem UUID if known.
    uuid: String,
    /// Mount point if currently mounted.
    mountpoint: String,
    /// Device model string.
    model: String,
    /// Device serial number.
    serial: String,
    /// Firmware revision string.
    firmware: String,
    /// Read-only flag.
    read_only: bool,
    /// Removable media flag.
    removable: bool,
    /// Whether this appears to be an SSD (rotational == 0).
    is_ssd: bool,
    /// Logical sector size in bytes (typically 512).
    logical_sector_size: u64,
    /// Physical sector size in bytes (typically 512 or 4096).
    physical_sector_size: u64,
    /// Child partitions (only populated for disks).
    children: Vec<BlockDevice>,
}

impl BlockDevice {
    /// Size in bytes (each sector is 512 bytes in /sys and /proc).
    fn size_bytes(&self) -> u64 {
        self.size_sectors.saturating_mul(512)
    }
}

// ============================================================================
// Mount table parsing
// ============================================================================

/// Parse /proc/mounts and build a map of kernel device name -> mount point.
fn parse_mounts() -> HashMap<String, String> {
    let mut mounts = HashMap::new();

    let content = match read_file("/proc/mounts") {
        Some(c) => c,
        None => return mounts,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let dev = parts[0];
            let mount = parts[1];
            // Strip "/dev/" prefix to get the kernel name.
            let name = dev.strip_prefix("/dev/").unwrap_or(dev);
            mounts.insert(name.to_string(), mount.to_string());
        }
    }

    mounts
}

/// Parse /proc/mounts and build a map of mount point -> (device, fstype, options).
fn parse_mounts_by_path() -> HashMap<String, (String, String, String)> {
    let mut mounts = HashMap::new();

    let content = match read_file("/proc/mounts") {
        Some(c) => c,
        None => return mounts,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let dev = parts[0].to_string();
            let mount = parts[1].to_string();
            let fstype = parts[2].to_string();
            let opts = parts[3].to_string();
            mounts.insert(mount, (dev, fstype, opts));
        }
    }

    mounts
}

// ============================================================================
// Filesystem info lookup
// ============================================================================

/// Read filesystem type, label, and UUID for a device from sysfs and /dev/disk/.
fn read_fsinfo(dev_name: &str, parent_name: Option<&str>) -> (String, String, String) {
    let paths_to_try: Vec<String> = if let Some(parent) = parent_name {
        vec![
            format!("/sys/block/{parent}/{dev_name}"),
            format!("/sys/block/{dev_name}"),
        ]
    } else {
        vec![format!("/sys/block/{dev_name}")]
    };

    let mut fstype = String::new();
    let mut label = String::new();
    let mut uuid = String::new();

    for base in &paths_to_try {
        if fstype.is_empty()
            && let Some(ft) = read_file(&format!("{base}/fstype"))
                && !ft.is_empty() {
                    fstype = ft;
                }
        if label.is_empty()
            && let Some(lb) = read_file(&format!("{base}/label"))
                && !lb.is_empty() {
                    label = lb;
                }
        if uuid.is_empty()
            && let Some(id) = read_file(&format!("{base}/uuid"))
                && !id.is_empty() {
                    uuid = id;
                }
    }

    // Fall back to /dev/disk/by-uuid and /dev/disk/by-label symlinks.
    if uuid.is_empty()
        && let Ok(entries) = fs::read_dir("/dev/disk/by-uuid") {
            for entry in entries.flatten() {
                if let Ok(target) = fs::read_link(entry.path()) {
                    let target_name = target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if target_name == dev_name
                        && let Some(u) = entry.file_name().to_str() {
                            uuid = u.to_string();
                        }
                }
            }
        }
    if label.is_empty()
        && let Ok(entries) = fs::read_dir("/dev/disk/by-label") {
            for entry in entries.flatten() {
                if let Ok(target) = fs::read_link(entry.path()) {
                    let target_name = target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if target_name == dev_name
                        && let Some(l) = entry.file_name().to_str() {
                            label = l.to_string();
                        }
                }
            }
        }

    (fstype, label, uuid)
}

// ============================================================================
// Block device scanning
// ============================================================================

/// Scan /sys/block/ for disks and their partitions.
fn scan_devices(mounts: &HashMap<String, String>) -> Vec<BlockDevice> {
    let mut devices = Vec::new();

    let block_dir = "/sys/block";
    let entries = match fs::read_dir(block_dir) {
        Ok(e) => e,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Skip loop and ram devices with zero size.
        if name.starts_with("loop") || name.starts_with("ram") {
            let sz = read_u64(&format!("{block_dir}/{name}/size"));
            if sz == 0 {
                continue;
            }
        }

        let dev_path = format!("{block_dir}/{name}");
        let size_sectors = read_u64(&format!("{dev_path}/size"));
        let read_only = read_bool(&format!("{dev_path}/ro"));
        let removable = read_bool(&format!("{dev_path}/removable"));
        let model = read_file(&format!("{dev_path}/device/model")).unwrap_or_default();
        let serial = read_file(&format!("{dev_path}/device/serial")).unwrap_or_default();
        let firmware = read_file(&format!("{dev_path}/device/firmware_rev"))
            .or_else(|| read_file(&format!("{dev_path}/device/rev")))
            .unwrap_or_default();
        let mountpoint = mounts.get(&name).cloned().unwrap_or_default();
        let is_ssd = read_file(&format!("{dev_path}/queue/rotational"))
            .is_some_and(|v| v == "0");
        let logical_sector_size =
            read_u64(&format!("{dev_path}/queue/logical_block_size"));
        let physical_sector_size =
            read_u64(&format!("{dev_path}/queue/physical_block_size"));

        let (fstype, label, uuid) = read_fsinfo(&name, None);

        let children = scan_partitions(&dev_path, &name, mounts);

        devices.push(BlockDevice {
            name,
            size_sectors,
            dev_type: "disk".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model,
            serial,
            firmware,
            read_only,
            removable,
            is_ssd,
            logical_sector_size,
            physical_sector_size,
            children,
        });
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

/// Scan partitions of a disk from /sys/block/<disk>/<partition>/.
fn scan_partitions(
    disk_path: &str,
    disk_name: &str,
    mounts: &HashMap<String, String>,
) -> Vec<BlockDevice> {
    let mut parts = Vec::new();

    let entries = match fs::read_dir(disk_path) {
        Ok(e) => e,
        Err(_) => return parts,
    };

    for entry in entries.flatten() {
        let part_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Partitions are subdirectories starting with the parent disk name.
        if !part_name.starts_with(disk_name) {
            continue;
        }

        let part_path = format!("{disk_path}/{part_name}");

        // Verify it is actually a partition directory.
        let is_partition = fs::metadata(format!("{part_path}/partition")).is_ok()
            || fs::metadata(format!("{part_path}/size")).is_ok();
        if !is_partition {
            continue;
        }

        let size_sectors = read_u64(&format!("{part_path}/size"));
        let read_only = read_bool(&format!("{part_path}/ro"));
        let mountpoint = mounts.get(&part_name).cloned().unwrap_or_default();

        let (fstype, label, uuid) = read_fsinfo(&part_name, Some(disk_name));

        parts.push(BlockDevice {
            name: part_name,
            size_sectors,
            dev_type: "part".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model: String::new(),
            serial: String::new(),
            firmware: String::new(),
            read_only,
            removable: false,
            is_ssd: false,
            logical_sector_size: 0,
            physical_sector_size: 0,
            children: Vec::new(),
        });
    }

    parts.sort_by(|a, b| a.name.cmp(&b.name));
    parts
}

/// Fallback: scan /proc/partitions when /sys/block is unavailable.
fn scan_proc_partitions(mounts: &HashMap<String, String>) -> Vec<BlockDevice> {
    let content = match read_file("/proc/partitions") {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries: Vec<(String, u64)> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("major") || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[3].to_string();
        // /proc/partitions gives 1024-byte blocks; convert to 512-byte sectors.
        let blocks: u64 = parts[2].parse().unwrap_or(0);
        let sectors = blocks.saturating_mul(2);

        entries.push((name, sectors));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut disks: Vec<BlockDevice> = Vec::new();
    let mut i = 0;

    while i < entries.len() {
        let (ref disk_name, disk_sectors) = entries[i];
        let (fstype, label, uuid) = read_fsinfo(disk_name, None);
        let mountpoint = mounts.get(disk_name).cloned().unwrap_or_default();

        let mut children = Vec::new();
        let mut j = i + 1;
        while j < entries.len() && entries[j].0.starts_with(disk_name.as_str()) {
            let (ref part_name, part_sectors) = entries[j];
            let part_mp = mounts.get(part_name).cloned().unwrap_or_default();
            let (pfs, plb, puu) = read_fsinfo(part_name, Some(disk_name));

            children.push(BlockDevice {
                name: part_name.clone(),
                size_sectors: part_sectors,
                dev_type: "part".to_string(),
                fstype: pfs,
                label: plb,
                uuid: puu,
                mountpoint: part_mp,
                model: String::new(),
                serial: String::new(),
                firmware: String::new(),
                read_only: false,
                removable: false,
                is_ssd: false,
                logical_sector_size: 0,
                physical_sector_size: 0,
                children: Vec::new(),
            });
            j += 1;
        }

        disks.push(BlockDevice {
            name: disk_name.clone(),
            size_sectors: disk_sectors,
            dev_type: "disk".to_string(),
            fstype,
            label,
            uuid,
            mountpoint,
            model: String::new(),
            serial: String::new(),
            firmware: String::new(),
            read_only: false,
            removable: false,
            is_ssd: false,
            logical_sector_size: 0,
            physical_sector_size: 0,
            children,
        });

        i = j;
    }

    disks
}

/// Retrieve all block devices, preferring sysfs with procfs fallback.
fn get_all_devices() -> Vec<BlockDevice> {
    let mounts = parse_mounts();
    let mut devices = scan_devices(&mounts);
    if devices.is_empty() {
        devices = scan_proc_partitions(&mounts);
    }
    devices
}

/// Find a specific device by kernel name (e.g. "sda", "sda1", "nvme0n1p1").
/// Strips a leading "/dev/" if present.
fn find_device(name: &str) -> Option<BlockDevice> {
    let name = name.strip_prefix("/dev/").unwrap_or(name);
    let devices = get_all_devices();

    for dev in devices {
        if dev.name == name {
            return Some(dev);
        }
        for child in dev.children {
            if child.name == name {
                return Some(child);
            }
        }
    }

    None
}

/// Find a disk (parent) device by kernel name, returning it with children.
fn find_disk(name: &str) -> Option<BlockDevice> {
    let name = name.strip_prefix("/dev/").unwrap_or(name);
    get_all_devices().into_iter().find(|dev| dev.name == name)
}

// ============================================================================
// JSON helpers
// ============================================================================

/// Escape a string for safe inclusion in JSON output.
#[allow(dead_code)]
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Control characters: emit as \uXXXX.
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Subcommand: list
// ============================================================================

/// List all disks and partitions in a table.
fn cmd_list() {
    let devices = get_all_devices();

    if devices.is_empty() {
        eprintln!("No block devices found (is /sys/block or /proc/partitions available?)");
        process::exit(1);
    }

    // Header.
    println!(
        "{:<12} {:>8} {:<6} {:<8} {:<6} {:<4} MOUNTPOINT",
        "DEVICE", "SIZE", "TYPE", "FSTYPE", "RO", "SSD"
    );
    println!(
        "{:<12} {:>8} {:<6} {:<8} {:<6} {:<4} ----------",
        "------", "----", "----", "------", "--", "---"
    );

    for dev in &devices {
        let ssd_str = if dev.is_ssd { "yes" } else { "no" };
        println!(
            "{:<12} {:>8} {:<6} {:<8} {:<6} {:<4} {}",
            dev.name,
            format_size(dev.size_bytes()),
            dev.dev_type,
            if dev.fstype.is_empty() { "-" } else { &dev.fstype },
            if dev.read_only { "yes" } else { "no" },
            ssd_str,
            if dev.mountpoint.is_empty() { "-" } else { &dev.mountpoint },
        );

        for (idx, child) in dev.children.iter().enumerate() {
            let is_last = idx == dev.children.len() - 1;
            let prefix = if is_last { "\u{2514}\u{2500} " } else { "\u{251c}\u{2500} " };
            let display_name = format!("{prefix}{}", child.name);
            println!(
                "{:<12} {:>8} {:<6} {:<8} {:<6} {:<4} {}",
                display_name,
                format_size(child.size_bytes()),
                child.dev_type,
                if child.fstype.is_empty() { "-" } else { &child.fstype },
                if child.read_only { "yes" } else { "no" },
                "-",
                if child.mountpoint.is_empty() {
                    "-"
                } else {
                    &child.mountpoint
                },
            );
        }
    }
}

// ============================================================================
// Subcommand: info
// ============================================================================

/// Show detailed information about a disk or partition.
fn cmd_info(device_name: &str) {
    // First try as a full disk (with children) so we can display partition info.
    if let Some(dev) = find_disk(device_name) {
        print_device_info(&dev);
        return;
    }

    // Try as a partition.
    if let Some(dev) = find_device(device_name) {
        print_device_info(&dev);
        return;
    }

    eprintln!("error: device '{}' not found", device_name);
    eprintln!("  Try 'diskutil list' to see available devices.");
    process::exit(1);
}

fn print_device_info(dev: &BlockDevice) {
    println!("=== Device: /dev/{} ===", dev.name);
    println!();
    println!("  Type:               {}", dev.dev_type);
    println!("  Size:               {} ({} bytes)", format_size(dev.size_bytes()), dev.size_bytes());
    println!("  Sectors:            {} (512-byte)", dev.size_sectors);

    if dev.logical_sector_size > 0 {
        println!("  Logical sector:     {} bytes", dev.logical_sector_size);
    }
    if dev.physical_sector_size > 0 {
        println!("  Physical sector:    {} bytes", dev.physical_sector_size);
    }

    if !dev.model.is_empty() {
        println!("  Model:              {}", dev.model);
    }
    if !dev.serial.is_empty() {
        println!("  Serial:             {}", dev.serial);
    }
    if !dev.firmware.is_empty() {
        println!("  Firmware:           {}", dev.firmware);
    }

    println!("  Read-only:          {}", if dev.read_only { "yes" } else { "no" });
    println!("  Removable:          {}", if dev.removable { "yes" } else { "no" });
    println!("  SSD:                {}", if dev.is_ssd { "yes" } else { "no" });

    if !dev.fstype.is_empty() {
        println!("  Filesystem:         {}", dev.fstype);
    }
    if !dev.label.is_empty() {
        println!("  Label:              {}", dev.label);
    }
    if !dev.uuid.is_empty() {
        println!("  UUID:               {}", dev.uuid);
    }
    if !dev.mountpoint.is_empty() {
        println!("  Mount point:        {}", dev.mountpoint);
    }

    // Show scheduler info if available.
    let sched_path = format!("/sys/block/{}/queue/scheduler", dev.name);
    if let Some(sched) = read_file(&sched_path)
        && !sched.is_empty() {
            println!("  I/O scheduler:      {}", sched);
        }

    if !dev.children.is_empty() {
        println!();
        println!("  Partitions ({}):", dev.children.len());
        println!(
            "    {:<14} {:>10} {:<8} {:<8} MOUNTPOINT",
            "NAME", "SIZE", "FSTYPE", "LABEL"
        );
        for child in &dev.children {
            println!(
                "    {:<14} {:>10} {:<8} {:<8} {}",
                child.name,
                format_size(child.size_bytes()),
                if child.fstype.is_empty() { "-" } else { &child.fstype },
                if child.label.is_empty() { "-" } else { &child.label },
                if child.mountpoint.is_empty() { "-" } else { &child.mountpoint },
            );
        }
    }
}

// ============================================================================
// Subcommand: format
// ============================================================================

/// Format a partition with the specified filesystem type.
fn cmd_format(device_name: &str, fstype: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    // Validate filesystem type.
    match fstype {
        "ext4" | "fat32" | "vfat" | "tmpfs" => {}
        other => {
            eprintln!("error: unsupported filesystem type '{other}'");
            eprintln!("  Supported types: ext4, fat32, tmpfs");
            process::exit(1);
        }
    }

    // Verify the device exists.
    if find_device(dev_name).is_none() {
        eprintln!("error: device '{}' not found", dev_name);
        process::exit(1);
    }

    // Check if the device is currently mounted.
    let mounts = parse_mounts();
    if mounts.contains_key(dev_name) {
        eprintln!("error: /dev/{} is currently mounted", dev_name);
        eprintln!("  Unmount it first with: mount -u /dev/{}", dev_name);
        process::exit(1);
    }

    // The in-kernel mkfs backend only formats the FAT family so far.
    let kernel_fstype = match fstype {
        "fat32" | "vfat" => "vfat",
        other => {
            eprintln!(
                "error: the kernel cannot format '{other}' yet — only the FAT \
                 family (fat32/vfat) has an in-kernel mkfs backend"
            );
            process::exit(1);
        }
    };

    println!("Formatting /dev/{} as {}...", dev_name, fstype);

    // SYS_FS_FORMAT takes the block-device registry name (no /dev/ prefix), the
    // fstype string, and no label (0/0).
    let dev_bytes = dev_name.as_bytes();
    let fstype_bytes = kernel_fstype.as_bytes();
    let ret = syscall6(
        SYS_FS_FORMAT,
        dev_bytes.as_ptr() as u64,
        dev_bytes.len() as u64,
        fstype_bytes.as_ptr() as u64,
        fstype_bytes.len() as u64,
        0,
        0,
    );

    if ret < 0 {
        eprintln!("error: format failed: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    println!("Formatted /dev/{} as {} successfully.", dev_name, fstype);
}

// ============================================================================
// Subcommand: verify
// ============================================================================

/// Check filesystem integrity (read-only).
fn cmd_verify(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    let dev = match find_device(dev_name) {
        Some(d) => d,
        None => {
            eprintln!("error: device '{}' not found", dev_name);
            process::exit(1);
        }
    };

    if dev.fstype.is_empty() {
        eprintln!("warning: no filesystem type detected on /dev/{}", dev_name);
        eprintln!("  The device may be unformatted or the filesystem is unrecognized.");
    }

    println!("Verifying filesystem on /dev/{}...", dev_name);
    if !dev.fstype.is_empty() {
        println!("  Filesystem type: {}", dev.fstype);
    }
    println!("  Mode: read-only check");

    // Check-only run via SYS_FS_CHECK (FAT family only in the kernel so far).
    let dev_bytes = dev_name.as_bytes();
    let ret = syscall6(
        SYS_FS_CHECK,
        dev_bytes.as_ptr() as u64,
        dev_bytes.len() as u64,
        0, // flags: check-only.
        0,
        0,
        0,
    );

    if ret < 0 {
        eprintln!("Verification FAILED: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    if ret == 0 {
        println!("  Result: clean (no errors found).");
    } else {
        println!(
            "  Result: {} error{} found. Run 'diskutil repair /dev/{}' to fix.",
            ret,
            if ret == 1 { "" } else { "s" },
            dev_name
        );
        process::exit(1);
    }
}

// ============================================================================
// Subcommand: repair
// ============================================================================

/// Attempt to repair a filesystem.
fn cmd_repair(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    if find_device(dev_name).is_none() {
        eprintln!("error: device '{}' not found", dev_name);
        process::exit(1);
    }

    // Ensure not mounted.
    let mounts = parse_mounts();
    if mounts.contains_key(dev_name) {
        eprintln!("error: /dev/{} is currently mounted", dev_name);
        eprintln!("  Unmount it first before attempting repair.");
        process::exit(1);
    }

    println!("Repairing filesystem on /dev/{}...", dev_name);

    // Repair run via SYS_FS_CHECK with the repair bit set (FAT family only).
    let dev_bytes = dev_name.as_bytes();
    let ret = syscall6(
        SYS_FS_CHECK,
        dev_bytes.as_ptr() as u64,
        dev_bytes.len() as u64,
        FS_CHECK_REPAIR,
        0,
        0,
        0,
    );

    if ret < 0 {
        eprintln!("Repair FAILED: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    if ret == 0 {
        println!("  Result: filesystem is clean (all errors repaired).");
    } else {
        println!(
            "  Result: {} error{} could not be repaired.",
            ret,
            if ret == 1 { "" } else { "s" }
        );
        process::exit(1);
    }
}

// ============================================================================
// Subcommand: usage
// ============================================================================

/// Parsed result of a `SYS_FS_STATVFS` call (see the syscall doc for the
/// on-wire 64-byte buffer layout).
struct FsStatvfs {
    /// Fundamental block (allocation-unit) size in bytes.
    block_size: u64,
    /// Total number of blocks on the filesystem.
    total_blocks: u64,
    /// Number of free blocks. The kernel has no separate
    /// "available to unprivileged" count, so available == free here.
    free_blocks: u64,
    /// Total inodes / directory entries (0 if the concept doesn't apply).
    total_inodes: u64,
    /// Free inodes / directory entries.
    free_inodes: u64,
}

/// Read a `u64` from a little-endian byte slice at `off`, or 0 if out of range.
fn read_u64_le(buf: &[u8], off: usize) -> u64 {
    buf.get(off..off + 8)
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map_or(0, u64::from_le_bytes)
}

/// Query filesystem space info for `path` via `SYS_FS_STATVFS`.
///
/// Returns the parsed stats on success, or the negative `KernelError` code the
/// syscall returned on failure (so callers can render an honest message).
fn fs_statvfs(path: &str) -> Result<FsStatvfs, i64> {
    let mut buf = [0u8; FS_STATVFS_SIZE];
    let ret = syscall6(
        SYS_FS_STATVFS,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        0,
        0,
        0,
    );
    if ret < 0 {
        return Err(ret);
    }
    Ok(FsStatvfs {
        block_size: read_u64_le(&buf, 0),
        total_blocks: read_u64_le(&buf, 8),
        free_blocks: read_u64_le(&buf, 16),
        total_inodes: read_u64_le(&buf, 24),
        free_inodes: read_u64_le(&buf, 32),
    })
}

/// Show disk space usage for a mount point or path.
fn cmd_usage(path: &str) {
    // Try to find which mount point covers this path.
    let mounts_by_path = parse_mounts_by_path();

    // Find the longest mount-point prefix that matches.
    let mut best_mount = String::new();
    let mut best_dev = String::new();
    let mut best_fstype = String::new();

    for (mount, (dev, fstype, _opts)) in &mounts_by_path {
        if path.starts_with(mount.as_str()) && mount.len() > best_mount.len() {
            best_mount.clone_from(mount);
            best_dev.clone_from(dev);
            best_fstype.clone_from(fstype);
        }
    }

    if best_mount.is_empty() {
        // If no mount found, treat path as "/" by default.
        best_mount = "/".to_string();
        if let Some((dev, fstype, _opts)) = mounts_by_path.get("/") {
            best_dev.clone_from(dev);
            best_fstype.clone_from(fstype);
        }
    }

    println!("Disk usage for: {}", path);
    println!("  Mount point:   {}", best_mount);
    if !best_dev.is_empty() {
        println!("  Device:        {}", best_dev);
    }
    if !best_fstype.is_empty() {
        println!("  Filesystem:    {}", best_fstype);
    }
    println!();

    // Issue the real statvfs syscall on the path.
    let result = match fs_statvfs(path) {
        Ok(r) => r,
        Err(ret) => {
            eprintln!(
                "  (statvfs syscall failed: {}; showing estimate from sysfs)",
                syscall_error_msg(ret)
            );
            show_usage_estimate(path, &best_dev);
            return;
        }
    };

    let block_size = if result.block_size > 0 {
        result.block_size
    } else {
        4096
    };

    let total_bytes = result.total_blocks.saturating_mul(block_size);
    let free_bytes = result.free_blocks.saturating_mul(block_size);
    // The kernel exposes a single free count; available == free here.
    let avail_bytes = free_bytes;
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    let used_pct = if total_bytes > 0 {
        ((used_bytes as f64 / total_bytes as f64) * 100.0) as u64
    } else {
        0
    };

    println!("  Total:         {}", format_size(total_bytes));
    println!("  Used:          {} ({}%)", format_size(used_bytes), used_pct);
    println!("  Free:          {}", format_size(free_bytes));
    println!("  Available:     {}", format_size(avail_bytes));

    if result.total_inodes > 0 {
        let used_inodes = result.total_inodes.saturating_sub(result.free_inodes);
        let inode_pct = ((used_inodes as f64 / result.total_inodes as f64) * 100.0) as u64;
        println!();
        println!(
            "  Inodes:        {} / {} ({}% used)",
            used_inodes, result.total_inodes, inode_pct
        );
    }

    // Render a simple bar chart.
    println!();
    print_usage_bar(used_pct);
}

/// Show an estimated usage when statfs is not available, using sysfs data.
fn show_usage_estimate(path: &str, dev_name: &str) {
    let dev_name = dev_name.strip_prefix("/dev/").unwrap_or(dev_name);

    if let Some(dev) = find_device(dev_name) {
        println!("  Total:         {}", format_size(dev.size_bytes()));
        println!("  (exact usage data not available without statfs)");
    } else {
        println!("  Could not determine usage for '{}'", path);
    }
}

/// Print a horizontal bar showing percent usage.
fn print_usage_bar(pct: u64) {
    let bar_width: u64 = 40;
    let filled = (pct.saturating_mul(bar_width)) / 100;
    let empty = bar_width.saturating_sub(filled);

    let bar_char = if pct >= 90 {
        '!'
    } else if pct >= 75 {
        '#'
    } else {
        '='
    };

    let label = if pct >= 90 {
        "CRITICAL"
    } else if pct >= 75 {
        "WARNING"
    } else {
        "OK"
    };

    print!("  [");
    for _ in 0..filled {
        print!("{bar_char}");
    }
    for _ in 0..empty {
        print!(" ");
    }
    println!("] {}% {}", pct, label);
}

// ============================================================================
// Subcommand: benchmark
// ============================================================================

/// Run a sequential I/O benchmark on a device (via its mount point).
fn cmd_benchmark(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    // Find where the device is mounted so we can write a test file.
    let mounts = parse_mounts();

    let mount_point = match mounts.get(dev_name) {
        Some(mp) => mp.clone(),
        None => {
            // If the device itself is not in the mount table, check children.
            let devices = get_all_devices();
            let mut found = None;
            'outer: for d in &devices {
                if d.name == dev_name {
                    if !d.mountpoint.is_empty() {
                        found = Some(d.mountpoint.clone());
                        break;
                    }
                    for c in &d.children {
                        if !c.mountpoint.is_empty() {
                            found = Some(c.mountpoint.clone());
                            break 'outer;
                        }
                    }
                }
                for c in &d.children {
                    if c.name == dev_name && !c.mountpoint.is_empty() {
                        found = Some(c.mountpoint.clone());
                        break 'outer;
                    }
                }
            }

            match found {
                Some(mp) => mp,
                None => {
                    eprintln!("error: /dev/{} is not mounted", dev_name);
                    eprintln!("  Mount it first, then re-run the benchmark.");
                    eprintln!("  (The benchmark writes a temporary file to the mount point.)");
                    process::exit(1);
                }
            }
        }
    };

    println!("=== Disk Benchmark: /dev/{} ===", dev_name);
    println!("  Mount point: {}", mount_point);
    println!();

    let test_file = format!("{}/.diskutil_benchmark_tmp", mount_point);

    // Benchmark parameters.
    // Write 64 MiB in 1 MiB chunks for sequential write test.
    let chunk_size: usize = 1024 * 1024; // 1 MiB
    let total_size: usize = 64 * chunk_size; // 64 MiB
    let chunk_count = total_size / chunk_size;

    // Generate a data pattern (sequential bytes to avoid compression effects).
    let mut pattern = vec![0u8; chunk_size];
    for (i, byte) in pattern.iter_mut().enumerate() {
        *byte = (i & 0xFF) as u8;
    }

    // -- Sequential Write --
    println!("  Sequential write ({} x 1 MiB)...", chunk_count);

    let write_result = benchmark_write(&test_file, &pattern, chunk_count);

    match write_result {
        Ok((duration_secs, bytes_written)) => {
            let throughput = bytes_written as f64 / duration_secs;
            println!(
                "    Write: {} in {:.2}s = {}",
                format_size(bytes_written as u64),
                duration_secs,
                format_throughput(throughput)
            );
        }
        Err(e) => {
            eprintln!("    Write FAILED: {}", e);
            // Clean up and bail.
            let _ = fs::remove_file(&test_file);
            process::exit(1);
        }
    }

    // -- Sequential Read --
    println!("  Sequential read ({} x 1 MiB)...", chunk_count);

    let read_result = benchmark_read(&test_file, chunk_size, chunk_count);

    match read_result {
        Ok((duration_secs, bytes_read)) => {
            let throughput = bytes_read as f64 / duration_secs;
            println!(
                "    Read:  {} in {:.2}s = {}",
                format_size(bytes_read as u64),
                duration_secs,
                format_throughput(throughput)
            );
        }
        Err(e) => {
            eprintln!("    Read FAILED: {}", e);
        }
    }

    // Clean up.
    if let Err(e) = fs::remove_file(&test_file) {
        eprintln!("  warning: could not remove test file: {}", e);
    }

    println!();
    println!("  Benchmark complete.");
}

/// Write `chunk_count` copies of `pattern` to a file, return (seconds, bytes_written).
fn benchmark_write(
    path: &str,
    pattern: &[u8],
    chunk_count: usize,
) -> Result<(f64, usize), String> {
    let mut file = fs::File::create(path).map_err(|e| format!("create: {e}"))?;

    let start = Instant::now();

    let mut total_bytes = 0usize;
    for _ in 0..chunk_count {
        file.write_all(pattern).map_err(|e| format!("write: {e}"))?;
        total_bytes += pattern.len();
    }

    file.flush().map_err(|e| format!("flush: {e}"))?;
    // Ensure data hits the storage device, not just the page cache.
    file.sync_all().map_err(|e| format!("sync: {e}"))?;

    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();

    Ok((secs, total_bytes))
}

/// Read `chunk_count` chunks of `chunk_size` from a file, return (seconds, bytes_read).
fn benchmark_read(
    path: &str,
    chunk_size: usize,
    chunk_count: usize,
) -> Result<(f64, usize), String> {
    let mut file = fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut buf = vec![0u8; chunk_size];

    let start = Instant::now();

    let mut total_bytes = 0usize;
    for _ in 0..chunk_count {
        let n = file.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        total_bytes += n;
    }

    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();

    Ok((secs, total_bytes))
}

// ============================================================================
// Subcommand: smart
// ============================================================================

/// Show S.M.A.R.T. status for a disk.
fn cmd_smart(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    let dev = match find_disk(dev_name) {
        Some(d) => d,
        None => match find_device(dev_name) {
            Some(d) => d,
            None => {
                eprintln!("error: device '{}' not found", dev_name);
                process::exit(1);
            }
        },
    };

    println!("=== S.M.A.R.T. Status: /dev/{} ===", dev.name);
    println!();

    if !dev.model.is_empty() {
        println!("  Model:          {}", dev.model);
    }
    if !dev.serial.is_empty() {
        println!("  Serial:         {}", dev.serial);
    }
    if !dev.firmware.is_empty() {
        println!("  Firmware:       {}", dev.firmware);
    }

    println!("  Type:           {}", if dev.is_ssd { "SSD" } else { "HDD" });
    println!();

    // Read SMART attributes from /sys/block/<dev>/device/ subdirectories.
    // Slate OS exposes these in a simplified sysfs interface.
    let smart_base = format!("/sys/block/{}/device", dev.name);

    let smart_attrs: &[(&str, &str)] = &[
        ("smart/status", "Overall status"),
        ("smart/temperature", "Temperature"),
        ("smart/power_on_hours", "Power-on hours"),
        ("smart/power_cycle_count", "Power cycle count"),
        ("smart/reallocated_sectors", "Reallocated sectors"),
        ("smart/pending_sectors", "Pending sectors"),
        ("smart/uncorrectable_errors", "Uncorrectable errors"),
        ("smart/wear_leveling_count", "Wear leveling count"),
        ("smart/percentage_used", "Percentage used"),
        ("smart/available_spare", "Available spare"),
        ("smart/data_units_read", "Data units read"),
        ("smart/data_units_written", "Data units written"),
    ];

    let mut any_found = false;

    for (attr_path, label) in smart_attrs {
        let full_path = format!("{smart_base}/{attr_path}");
        if let Some(val) = read_file(&full_path)
            && !val.is_empty() {
                println!("  {:<24} {}", format!("{}:", label), val);
                any_found = true;
            }
    }

    if !any_found {
        println!("  S.M.A.R.T. data not available.");
        println!("  (The kernel may not expose SMART attributes for this device,");
        println!("   or the device does not support S.M.A.R.T.)");
    }

    // Also check for the overall health indicator.
    let health_path = format!("{smart_base}/smart/status");
    if let Some(status) = read_file(&health_path) {
        println!();
        if status.to_ascii_lowercase().contains("pass")
            || status.to_ascii_lowercase().contains("ok")
            || status.to_ascii_lowercase().contains("good")
        {
            println!("  Health: PASSED");
        } else if status.to_ascii_lowercase().contains("fail") {
            println!("  Health: FAILED -- Back up your data immediately!");
        } else {
            println!("  Health: {}", status);
        }
    }
}

// ============================================================================
// Subcommand: trim
// ============================================================================

/// Issue TRIM/discard command to an SSD.
fn cmd_trim(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    let dev = match find_device(dev_name) {
        Some(d) => d,
        None => {
            eprintln!("error: device '{}' not found", dev_name);
            process::exit(1);
        }
    };

    if !dev.is_ssd {
        eprintln!("warning: /dev/{} does not appear to be an SSD (rotational != 0)", dev.name);
        eprintln!("  TRIM is typically only meaningful for SSDs.");
        eprintln!("  Proceeding anyway...");
    }

    // Check discard_granularity to confirm TRIM support.
    let discard_gran = read_u64(&format!(
        "/sys/block/{}/queue/discard_granularity",
        dev.name
    ));
    if discard_gran == 0 {
        eprintln!("error: /dev/{} does not support TRIM (discard_granularity = 0)", dev.name);
        process::exit(1);
    }

    println!("Issuing TRIM to /dev/{}...", dev.name);
    println!("  Size: {}", format_size(dev.size_bytes()));

    // fstrim semantics: discard the *free* space of the filesystem mounted on
    // this device (non-destructive). The kernel resolves the device to its
    // mount and walks the free-space metadata, so the device must be mounted.
    let dev_bytes = dev.name.as_bytes();
    let ret = syscall6(
        SYS_FS_TRIM,
        dev_bytes.as_ptr() as u64,
        dev_bytes.len() as u64,
        0,
        0,
        0,
        0,
    );

    if ret < 0 {
        eprintln!("TRIM failed: {}", syscall_error_msg(ret));
        eprintln!("  (fstrim needs the device to be mounted; only free space is discarded.)");
        process::exit(1);
    }

    // Non-negative return is the number of bytes discarded.
    let discarded = ret as u64;
    println!("  Discarded: {}", format_size(discarded));
    println!("TRIM complete.");
}

// ============================================================================
// Subcommand: partitions
// ============================================================================

/// List the partition table of a disk.
fn cmd_partitions(device_name: &str) {
    let dev_name = device_name.strip_prefix("/dev/").unwrap_or(device_name);

    let dev = match find_disk(dev_name) {
        Some(d) => d,
        None => {
            eprintln!("error: device '{}' not found (or it is a partition, not a disk)", dev_name);
            eprintln!("  Provide a whole-disk device name (e.g. sda, nvme0n1).");
            process::exit(1);
        }
    };

    println!("=== Partition Table: /dev/{} ===", dev.name);
    println!("  Disk size: {}", format_size(dev.size_bytes()));
    println!();

    // Try to detect partition table type from sysfs.
    let table_type = detect_partition_table_type(&dev.name);
    println!("  Table type:  {}", table_type);
    println!();

    if dev.children.is_empty() {
        println!("  (no partitions found)");
        return;
    }

    println!(
        "  {:<4} {:<14} {:>12} {:>12} {:>10} {:<8} LABEL",
        "#", "NAME", "START", "END", "SIZE", "FSTYPE"
    );
    println!(
        "  {:<4} {:<14} {:>12} {:>12} {:>10} {:<8} -----",
        "-", "----", "-----", "---", "----", "------"
    );

    for (idx, child) in dev.children.iter().enumerate() {
        // Read partition start offset from sysfs.
        let start_sectors = read_u64(&format!(
            "/sys/block/{}/{}/start",
            dev.name, child.name
        ));
        let end_sectors = start_sectors.saturating_add(child.size_sectors).saturating_sub(1);

        let part_num = idx.saturating_add(1);

        println!(
            "  {:<4} {:<14} {:>12} {:>12} {:>10} {:<8} {}",
            part_num,
            child.name,
            start_sectors,
            end_sectors,
            format_size(child.size_bytes()),
            if child.fstype.is_empty() { "-" } else { &child.fstype },
            if child.label.is_empty() { "-" } else { &child.label },
        );
    }

    // Show disk GUID if GPT.
    if table_type == "GPT" {
        let guid_path = format!("/sys/block/{}/device/wwid", dev.name);
        if let Some(guid) = read_file(&guid_path)
            && !guid.is_empty() {
                println!();
                println!("  Disk GUID: {}", guid);
            }
    }
}

/// Detect whether a disk uses MBR or GPT by reading sysfs hints or
/// examining the partition layout.
fn detect_partition_table_type(disk_name: &str) -> &'static str {
    // Check /sys/block/<disk>/device/partition_table_type if our kernel exposes it.
    let ptt_path = format!("/sys/block/{disk_name}/device/partition_table_type");
    if let Some(ptt) = read_file(&ptt_path) {
        let lower = ptt.to_ascii_lowercase();
        if lower.contains("gpt") {
            return "GPT";
        }
        if lower.contains("mbr") || lower.contains("dos") {
            return "MBR";
        }
    }

    // Heuristic: if any partition number > 4, it is likely GPT (MBR only
    // supports 4 primary partitions natively, though logical partitions exist).
    // Also, NVMe devices almost always use GPT.
    if disk_name.starts_with("nvme") {
        return "GPT (assumed)";
    }

    // Check partition numbers via sysfs.
    let block_path = format!("/sys/block/{disk_name}");
    if let Ok(entries) = fs::read_dir(&block_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(suffix) = name_str.strip_prefix(disk_name) {
                // NVMe partitions have a 'p' prefix before the number.
                let suffix = suffix.strip_prefix('p').unwrap_or(suffix);
                if let Ok(num) = suffix.parse::<u32>()
                    && num > 4 {
                        return "GPT (inferred)";
                    }
            }
        }
    }

    "Unknown"
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    println!("Slate OS Disk Utility v0.1.0");
    println!();
    println!("Disk management, diagnostics, and benchmarking.");
    println!();
    println!("USAGE:");
    println!("  diskutil <command> [arguments]");
    println!();
    println!("COMMANDS:");
    println!("  list                      List all disks and partitions");
    println!("  info <device>             Detailed device information");
    println!("  format <device> <fstype>  Format a partition (ext4, fat32, tmpfs)");
    println!("  verify <device>           Check filesystem integrity (read-only)");
    println!("  repair <device>           Attempt filesystem repair");
    println!("  usage <path>              Disk space usage for a mount point");
    println!("  benchmark <device>        Sequential read/write speed test");
    println!("  smart <device>            Show S.M.A.R.T. status");
    println!("  trim <device>             Issue TRIM/discard to SSD");
    println!("  partitions <device>       List partition table (MBR or GPT)");
    println!();
    println!("EXAMPLES:");
    println!("  diskutil list");
    println!("  diskutil info sda");
    println!("  diskutil info /dev/nvme0n1");
    println!("  diskutil format /dev/sda1 ext4");
    println!("  diskutil verify sda1");
    println!("  diskutil usage /home");
    println!("  diskutil benchmark sda");
    println!("  diskutil smart sda");
    println!("  diskutil trim nvme0n1");
    println!("  diskutil partitions sda");
    println!();
    println!("DEVICE NAMES:");
    println!("  Devices can be specified with or without the /dev/ prefix.");
    println!("  Examples: sda, sda1, nvme0n1, nvme0n1p1, vda, /dev/sda");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    let command = args[1].as_str();

    match command {
        "list" | "ls" => {
            cmd_list();
        }

        "info" | "show" => {
            if args.len() < 3 {
                eprintln!("error: 'info' requires a device name");
                eprintln!("  Usage: diskutil info <device>");
                process::exit(1);
            }
            cmd_info(&args[2]);
        }

        "format" | "mkfs" => {
            if args.len() < 4 {
                eprintln!("error: 'format' requires a device name and filesystem type");
                eprintln!("  Usage: diskutil format <device> <fstype>");
                eprintln!("  Types: ext4, fat32, tmpfs");
                process::exit(1);
            }
            cmd_format(&args[2], &args[3]);
        }

        "verify" | "check" | "fsck" => {
            if args.len() < 3 {
                eprintln!("error: 'verify' requires a device name");
                eprintln!("  Usage: diskutil verify <device>");
                process::exit(1);
            }
            cmd_verify(&args[2]);
        }

        "repair" | "fix" => {
            if args.len() < 3 {
                eprintln!("error: 'repair' requires a device name");
                eprintln!("  Usage: diskutil repair <device>");
                process::exit(1);
            }
            cmd_repair(&args[2]);
        }

        "usage" | "df" => {
            if args.len() < 3 {
                // Default to root filesystem.
                cmd_usage("/");
            } else {
                cmd_usage(&args[2]);
            }
        }

        "benchmark" | "bench" => {
            if args.len() < 3 {
                eprintln!("error: 'benchmark' requires a device name");
                eprintln!("  Usage: diskutil benchmark <device>");
                process::exit(1);
            }
            cmd_benchmark(&args[2]);
        }

        "smart" => {
            if args.len() < 3 {
                eprintln!("error: 'smart' requires a device name");
                eprintln!("  Usage: diskutil smart <device>");
                process::exit(1);
            }
            cmd_smart(&args[2]);
        }

        "trim" | "discard" => {
            if args.len() < 3 {
                eprintln!("error: 'trim' requires a device name");
                eprintln!("  Usage: diskutil trim <device>");
                process::exit(1);
            }
            cmd_trim(&args[2]);
        }

        "partitions" | "parts" | "parttable" => {
            if args.len() < 3 {
                eprintln!("error: 'partitions' requires a device name");
                eprintln!("  Usage: diskutil partitions <device>");
                process::exit(1);
            }
            cmd_partitions(&args[2]);
        }

        "--help" | "-h" | "help" => {
            print_usage();
        }

        unknown => {
            eprintln!("error: unknown command '{unknown}'");
            eprintln!("  Run 'diskutil --help' for usage.");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, read_u64_le, syscall_error_msg};

    #[test]
    fn read_u64_le_basic() {
        let buf = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(read_u64_le(&buf, 0), 0x0807_0605_0403_0201);
    }

    #[test]
    fn read_u64_le_offset_and_zero() {
        // A 64-byte statvfs buffer laid out like the kernel writes it:
        // block_size@0, total_blocks@8, free_blocks@16.
        let mut buf = [0u8; 64];
        buf[0..8].copy_from_slice(&4096u64.to_le_bytes());
        buf[8..16].copy_from_slice(&1000u64.to_le_bytes());
        buf[16..24].copy_from_slice(&250u64.to_le_bytes());
        assert_eq!(read_u64_le(&buf, 0), 4096);
        assert_eq!(read_u64_le(&buf, 8), 1000);
        assert_eq!(read_u64_le(&buf, 16), 250);
        // Inodes left at zero.
        assert_eq!(read_u64_le(&buf, 24), 0);
    }

    #[test]
    fn read_u64_le_out_of_range_is_zero() {
        let buf = [0u8; 4];
        // Not enough bytes for a full u64 -> 0 rather than a panic.
        assert_eq!(read_u64_le(&buf, 0), 0);
        assert_eq!(read_u64_le(&buf, 100), 0);
    }

    #[test]
    fn syscall_error_msg_maps_kernel_codes() {
        assert_eq!(syscall_error_msg(-400), "permission denied — must be root");
        assert_eq!(syscall_error_msg(-601), "no such device");
        assert_eq!(syscall_error_msg(-38), "function not implemented");
    }

    #[test]
    fn format_size_is_human_readable() {
        // Sanity: 4096 blocks * 4096 bytes worth of formatting doesn't panic
        // and produces a non-empty, byte-sensible string.
        assert!(!format_size(0).is_empty());
        assert!(!format_size(4096).is_empty());
        assert!(!format_size(1024 * 1024 * 1024).is_empty());
    }
}
