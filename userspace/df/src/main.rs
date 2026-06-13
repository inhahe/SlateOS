//! Slate OS Disk Free Space Utility
//!
//! Displays filesystem disk space usage by reading mount information from
//! `/proc/mounts` and querying per-filesystem statistics via the
//! `SYS_FS_STATVFS` syscall. Falls back to `/sys/block/*/size` and
//! `/proc/partitions` when the syscall is unavailable.
//!
//! # Usage
//!
//! ```text
//! df                        Show disk space for all real filesystems
//! df /home                  Show only the filesystem containing /home
//! df -h                     Human-readable sizes (powers of 1024)
//! df -H                     SI sizes (powers of 1000)
//! df -k                     1K-block columns (default)
//! df -m                     1M-block columns
//! df -T                     Include filesystem type column
//! df -i                     Show inode usage instead of block usage
//! df -t ext4                Only show ext4 filesystems
//! df -x tmpfs               Exclude tmpfs filesystems
//! df --total                Append a summary total row
//! df --json                 JSON output
//! df --help                 Show help
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================
//
// df queries per-filesystem space via SYS_FS_STATVFS=608 (fs zone 600-799).
// The previous version used 650, which on Slate OS is SYS_FS_SEEK_DATA, and ALSO
// passed the arguments in the wrong order (buffer where the length belongs):
// it could never have returned valid filesystem statistics. The real handler's
// ABI is arg0=path ptr, arg1=path len, arg2=output-buffer ptr.

/// Query filesystem space/configuration (`SYS_FS_STATVFS`).
const SYS_FS_STATVFS: u64 = 608;

/// Size of the `SYS_FS_STATVFS` output buffer, in bytes.
const FS_STATVFS_SIZE: usize = 64;

/// Parsed `SYS_FS_STATVFS` result.
///
/// Slate OS statvfs does not distinguish "free" from "available to unprivileged
/// users" (there is no reserved-block pool), so callers treat available == free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatVfs {
    /// Block size in bytes.
    block_size: u64,
    /// Total blocks on the filesystem.
    total_blocks: u64,
    /// Free blocks.
    free_blocks: u64,
    /// Total inodes.
    total_inodes: u64,
    /// Free inodes.
    free_inodes: u64,
    /// Maximum filename length.
    max_name_len: u64,
    /// Whether the filesystem is mounted read-only.
    read_only: bool,
}

/// Parse the 64-byte `SYS_FS_STATVFS` buffer.
///
/// Layout (all little-endian, matching the kernel's `sys_fs_statvfs`):
///   [0..8] block_size, [8..16] total_blocks, [16..24] free_blocks,
///   [24..32] total_inodes, [32..40] free_inodes, [40..48] max_name_len,
///   [48] read_only (u8). Split out for host unit testing.
fn parse_statvfs_buffer(buf: &[u8]) -> Option<StatVfs> {
    let read_u64 = |off: usize| -> Option<u64> {
        let b = buf.get(off..off + 8)?;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    };

    Some(StatVfs {
        block_size: read_u64(0)?,
        total_blocks: read_u64(8)?,
        free_blocks: read_u64(16)?,
        total_inodes: read_u64(24)?,
        free_inodes: read_u64(32)?,
        max_name_len: read_u64(40)?,
        read_only: *buf.get(48)? != 0,
    })
}

/// Invoke a 3-argument syscall via inline x86_64 assembly.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller is responsible for passing valid pointers and lengths.
    // The kernel validates all arguments before accessing userspace memory.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Call `SYS_FS_STATVFS` on a mount point path, returning parsed statistics on
/// success or the negative kernel error code on failure.
#[cfg(target_arch = "x86_64")]
fn fs_statvfs(path: &str) -> Result<StatVfs, i64> {
    let path_bytes = path.as_bytes();
    let mut buf = [0u8; FS_STATVFS_SIZE];

    // SAFETY: arg0/arg1 describe the path slice (pointer + length) and arg2 is
    // our stack buffer sized to the ABI-defined output length. All three stay
    // live for the duration of the syscall, and the kernel validates them.
    let ret = unsafe {
        syscall3(
            SYS_FS_STATVFS,
            path_bytes.as_ptr() as u64,
            path_bytes.len() as u64,
            buf.as_mut_ptr() as u64,
        )
    };

    if ret < 0 {
        return Err(ret);
    }
    // -1 stands in for "kernel returned success but the buffer was malformed",
    // which should never happen given a 64-byte stack buffer.
    parse_statvfs_buffer(&buf).ok_or(-1)
}

/// Host fallback: the statvfs syscall cannot run on the build host.
#[cfg(not(target_arch = "x86_64"))]
fn fs_statvfs(_path: &str) -> Result<StatVfs, i64> {
    Err(-2)
}

// ============================================================================
// Mount entry parsing
// ============================================================================

/// One line from `/proc/mounts`.
struct MountEntry {
    /// Device path (e.g. `/dev/sda1`).
    device: String,
    /// Mount point (e.g. `/`).
    mountpoint: String,
    /// Filesystem type (e.g. `ext4`).
    fstype: String,
    /// Mount options string.
    #[allow(dead_code)]
    options: String,
}

/// Pseudo-filesystem types that are skipped by default.
const PSEUDO_FS: &[&str] = &[
    "proc", "sysfs", "devfs", "devpts", "devtmpfs", "tmpfs", "ramfs",
    "hugetlbfs", "mqueue", "debugfs", "tracefs", "securityfs", "configfs",
    "fusectl", "cgroup", "cgroup2", "pstore", "bpf", "autofs",
];

fn is_pseudo_fs(fstype: &str) -> bool {
    PSEUDO_FS.iter().any(|&p| p.eq_ignore_ascii_case(fstype))
}

/// Parse `/proc/mounts` into a list of mount entries.
fn read_mounts() -> Vec<MountEntry> {
    let contents = match fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("df: cannot read /proc/mounts: {e}");
            return Vec::new();
        }
    };

    let mut entries = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Format: device mountpoint fstype options dump pass
        let fields: Vec<&str> = line.splitn(6, ' ').collect();
        if fields.len() < 4 {
            continue;
        }
        entries.push(MountEntry {
            device: fields[0].to_string(),
            mountpoint: fields[1].to_string(),
            fstype: fields[2].to_string(),
            options: fields[3].to_string(),
        });
    }
    entries
}

// ============================================================================
// Fallback: /sys/block and /proc/partitions
// ============================================================================

/// Try to read device size (in bytes) from `/sys/block/<dev>/size`.
/// The sysfs `size` file reports sectors (512-byte units).
fn sysblock_size_bytes(device: &str) -> Option<u64> {
    // Strip leading `/dev/` to get the kernel name.
    let name = device.strip_prefix("/dev/").unwrap_or(device);
    let path = format!("/sys/block/{name}/size");
    let text = fs::read_to_string(&path).ok()?;
    let sectors: u64 = text.trim().parse().ok()?;
    Some(sectors.saturating_mul(512))
}

/// Try to find a device entry in `/proc/partitions` and return its size in
/// bytes.
fn procpart_size_bytes(device: &str) -> Option<u64> {
    let name = device.strip_prefix("/dev/").unwrap_or(device);
    let contents = fs::read_to_string("/proc/partitions").ok()?;
    for line in contents.lines() {
        let line = line.trim();
        // Skip header / blank lines.
        if line.is_empty() || line.starts_with("major") {
            continue;
        }
        // Format: major minor #blocks name
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 && fields[3] == name {
            let kblocks: u64 = fields[2].parse().ok()?;
            return Some(kblocks.saturating_mul(1024));
        }
    }
    None
}

// ============================================================================
// Filesystem statistics
// ============================================================================

/// Statistics gathered for one mounted filesystem.
struct FsInfo {
    device: String,
    mountpoint: String,
    fstype: String,
    /// Total size in bytes.
    total_bytes: u64,
    /// Used bytes.
    used_bytes: u64,
    /// Available bytes (to unprivileged users).
    avail_bytes: u64,
    /// Total inodes (0 if unknown).
    total_inodes: u64,
    /// Used inodes.
    used_inodes: u64,
    /// Free inodes.
    free_inodes: u64,
}

/// Gather filesystem statistics for a mount entry. Returns `None` if we
/// cannot determine anything useful about the filesystem.
fn gather_fs_info(entry: &MountEntry) -> Option<FsInfo> {
    // Try the SYS_FS_STATVFS syscall first.
    if let Ok(st) = fs_statvfs(&entry.mountpoint)
        && st.block_size > 0
        && st.total_blocks > 0
    {
        let total = st.total_blocks.saturating_mul(st.block_size);
        let free = st.free_blocks.saturating_mul(st.block_size);
        // Slate OS statvfs has no reserved-block pool, so the space available to
        // unprivileged users equals the free space.
        let avail = free;
        let used = total.saturating_sub(free);
        let iused = st.total_inodes.saturating_sub(st.free_inodes);
        return Some(FsInfo {
            device: entry.device.clone(),
            mountpoint: entry.mountpoint.clone(),
            fstype: entry.fstype.clone(),
            total_bytes: total,
            used_bytes: used,
            avail_bytes: avail,
            total_inodes: st.total_inodes,
            used_inodes: iused,
            free_inodes: st.free_inodes,
        });
    }

    // Fallback: sysfs / procfs.
    let total = sysblock_size_bytes(&entry.device)
        .or_else(|| procpart_size_bytes(&entry.device))?;

    // Without the syscall we cannot distinguish free/avail, so report the
    // whole device as total and zero used. This is imprecise but better
    // than nothing.
    Some(FsInfo {
        device: entry.device.clone(),
        mountpoint: entry.mountpoint.clone(),
        fstype: entry.fstype.clone(),
        total_bytes: total,
        used_bytes: 0,
        avail_bytes: total,
        total_inodes: 0,
        used_inodes: 0,
        free_inodes: 0,
    })
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte count into human-readable form (powers of 1024).
fn human_readable(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T", "P"];
    if bytes == 0 {
        return "0".to_string();
    }
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1024.0 || unit == "P" {
            return if val >= 100.0 || (val - val.round()).abs() < 0.05 {
                format!("{:.0}{unit}", val)
            } else {
                format!("{val:.1}{unit}")
            };
        }
        val /= 1024.0;
    }
    format!("{bytes}")
}

/// Format a byte count into SI human-readable form (powers of 1000).
fn si_readable(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "kB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0".to_string();
    }
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1000.0 || unit == "PB" {
            return if val >= 100.0 || (val - val.round()).abs() < 0.05 {
                format!("{:.0}{unit}", val)
            } else {
                format!("{val:.1}{unit}")
            };
        }
        val /= 1000.0;
    }
    format!("{bytes}")
}

/// Format a block count (raw numeric string in the chosen unit).
fn block_str(bytes: u64, block_divisor: u64) -> String {
    let blocks = bytes.saturating_add(block_divisor.saturating_sub(1)) / block_divisor.max(1);
    format!("{blocks}")
}

/// Compute percentage used, returning a string like "42%". Returns "-" if
/// total is zero.
fn usage_pct(used: u64, total: u64) -> String {
    if total == 0 {
        return "-".to_string();
    }
    // Use the Linux convention: %Use = used / (used + avail), but we receive
    // used and total, so: pct = used * 100 / total, rounded up.
    let pct = (used as u128)
        .saturating_mul(100)
        .saturating_add(total as u128 - 1)
        / (total as u128).max(1);
    let pct = pct.min(100);
    format!("{pct}%")
}

/// ANSI color code for a percentage. Green <70, yellow 70-90, red >90.
fn pct_color(used: u64, total: u64) -> &'static str {
    if total == 0 {
        return "";
    }
    let pct = (used as u128).saturating_mul(100) / (total as u128).max(1);
    if pct > 90 {
        "\x1b[31m" // red
    } else if pct >= 70 {
        "\x1b[33m" // yellow
    } else {
        "\x1b[32m" // green
    }
}

const COLOR_RESET: &str = "\x1b[0m";

/// Returns true when stdout is likely a terminal that supports color.
fn stdout_is_tty() -> bool {
    // On Slate OS we assume /dev/tty is a terminal indicator. A more complete
    // implementation would use an ioctl, but this suffices for now.
    env::var_os("TERM").is_some()
}

// ============================================================================
// CLI configuration
// ============================================================================

struct Config {
    /// `-h`: human-readable (1024-based).
    human: bool,
    /// `-H`: SI units (1000-based).
    si: bool,
    /// Block size divisor (1024 for -k, 1048576 for -m).
    block_divisor: u64,
    /// Block unit label for the header ("1K-blocks", "1M-blocks").
    block_label: &'static str,
    /// `-T`: show filesystem type column.
    show_type: bool,
    /// `-i`: show inode info instead of block usage.
    inodes: bool,
    /// `-t <type>`: only show these filesystem types.
    include_types: Vec<String>,
    /// `-x <type>`: exclude these filesystem types.
    exclude_types: Vec<String>,
    /// `--total`: add a totals row.
    total: bool,
    /// `--json`: JSON output.
    json: bool,
    /// Positional arguments: filter to these mount points.
    filter_paths: Vec<String>,
    /// Whether color output is enabled.
    color: bool,
}

impl Config {
    fn default_config() -> Self {
        Self {
            human: false,
            si: false,
            block_divisor: 1024,
            block_label: "1K-blocks",
            show_type: false,
            inodes: false,
            include_types: Vec::new(),
            exclude_types: Vec::new(),
            total: false,
            json: false,
            filter_paths: Vec::new(),
            color: stdout_is_tty(),
        }
    }
}

fn print_help() {
    let help = "\
Usage: df [OPTION]... [FILE]...

Show information about the filesystems on which each FILE resides,
or all filesystems by default.

Options:
  -h, --human-readable   Print sizes in human-readable format (1K=1024)
  -H, --si               Print sizes in SI format (1K=1000)
  -k                     Show sizes in 1K blocks (default)
  -m                     Show sizes in 1M blocks
  -T, --print-type       Show filesystem type
  -i, --inodes           Show inode information instead of block usage
  -t, --type <TYPE>      Only show filesystems of the given type
  -x, --exclude-type <TYPE>
                         Exclude filesystems of the given type
      --total            Show a total row
      --json             Output in JSON format
      --no-color         Disable colored output
      --help             Display this help and exit
      --version          Output version information and exit";
    println!("{help}");
}

fn print_version() {
    println!("df (Slate OS coreutils) 0.1.0");
}

fn parse_args() -> Config {
    let mut cfg = Config::default_config();
    let args: Vec<String> = env::args().collect();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--human-readable" => cfg.human = true,
            "-H" | "--si" => cfg.si = true,
            "-k" => {
                cfg.block_divisor = 1024;
                cfg.block_label = "1K-blocks";
            }
            "-m" => {
                cfg.block_divisor = 1_048_576;
                cfg.block_label = "1M-blocks";
            }
            "-T" | "--print-type" => cfg.show_type = true,
            "-i" | "--inodes" => cfg.inodes = true,
            "-t" | "--type" => {
                i += 1;
                if i < args.len() {
                    cfg.include_types.push(args[i].clone());
                } else {
                    eprintln!("df: option '{arg}' requires an argument");
                    process::exit(1);
                }
            }
            "-x" | "--exclude-type" => {
                i += 1;
                if i < args.len() {
                    cfg.exclude_types.push(args[i].clone());
                } else {
                    eprintln!("df: option '{arg}' requires an argument");
                    process::exit(1);
                }
            }
            "--total" => cfg.total = true,
            "--json" => cfg.json = true,
            "--no-color" => cfg.color = false,
            "--help" => {
                print_help();
                process::exit(0);
            }
            "--version" => {
                print_version();
                process::exit(0);
            }
            other if other.starts_with('-') => {
                eprintln!("df: unknown option '{other}'");
                eprintln!("Try 'df --help' for more information.");
                process::exit(1);
            }
            _ => cfg.filter_paths.push(arg.clone()),
        }
        i += 1;
    }

    // Human / SI overrides the block-based display.
    if cfg.si {
        cfg.human = false;
    }

    cfg
}

// ============================================================================
// Filtering
// ============================================================================

/// Decide whether a mount entry should be displayed.
fn should_display(entry: &MountEntry, cfg: &Config) -> bool {
    // Type include filter.
    if !cfg.include_types.is_empty()
        && !cfg.include_types.iter().any(|t| t.eq_ignore_ascii_case(&entry.fstype))
    {
        return false;
    }

    // Type exclude filter.
    if cfg.exclude_types.iter().any(|t| t.eq_ignore_ascii_case(&entry.fstype)) {
        return false;
    }

    // If positional paths were given, only show mounts that match.
    if !cfg.filter_paths.is_empty() {
        let dominated = cfg.filter_paths.iter().any(|p| {
            // A mount "matches" a path if the path starts with the mount point.
            p == &entry.mountpoint || p.starts_with(&format!("{}/", entry.mountpoint))
        });
        if !dominated {
            return false;
        }
    }

    // Skip pseudo-filesystems unless they were explicitly requested by -t or
    // by a positional path that matched above.
    if cfg.include_types.is_empty() && cfg.filter_paths.is_empty() && is_pseudo_fs(&entry.fstype) {
        return false;
    }

    true
}

/// When positional paths are given and multiple mounts match (e.g., both `/`
/// and `/home` match `/home/user`), keep only the most-specific (longest
/// mountpoint) for each requested path.
fn best_mount_for_paths(infos: &mut Vec<FsInfo>, filter_paths: &[String]) {
    if filter_paths.is_empty() {
        return;
    }

    let mut keep = Vec::new();
    for fpath in filter_paths {
        // Find the mount with the longest mountpoint that is a prefix.
        let best = infos
            .iter()
            .enumerate()
            .filter(|(_, fi)| {
                fpath == &fi.mountpoint
                    || fpath.starts_with(&format!("{}/", fi.mountpoint))
            })
            .max_by_key(|(_, fi)| fi.mountpoint.len());
        if let Some((idx, _)) = best
            && !keep.contains(&idx)
        {
            keep.push(idx);
        }
    }

    // Retain only the selected entries, preserving their original order.
    keep.sort_unstable();
    let mut idx = 0;
    infos.retain(|_| {
        let yes = keep.contains(&idx);
        idx += 1;
        yes
    });
}

// ============================================================================
// Table output
// ============================================================================

/// Format a size value according to the current config.
fn format_size(bytes: u64, cfg: &Config) -> String {
    if cfg.human {
        human_readable(bytes)
    } else if cfg.si {
        si_readable(bytes)
    } else {
        block_str(bytes, cfg.block_divisor)
    }
}

/// A single row in the output table (pre-formatted strings).
struct Row {
    filesystem: String,
    fstype: String,
    size: String,
    used: String,
    avail: String,
    pct: String,
    mountpoint: String,
    /// Raw percentage for coloring (0..=100, or u64::MAX for "N/A").
    raw_pct_numerator: u64,
    raw_pct_denominator: u64,
}

fn build_rows(infos: &[FsInfo], cfg: &Config) -> Vec<Row> {
    infos
        .iter()
        .map(|fi| {
            if cfg.inodes {
                let total = fi.total_inodes;
                let used = fi.used_inodes;
                let free = fi.free_inodes;
                Row {
                    filesystem: fi.device.clone(),
                    fstype: fi.fstype.clone(),
                    size: format!("{total}"),
                    used: format!("{used}"),
                    avail: format!("{free}"),
                    pct: usage_pct(used, total),
                    mountpoint: fi.mountpoint.clone(),
                    raw_pct_numerator: used,
                    raw_pct_denominator: total,
                }
            } else {
                Row {
                    filesystem: fi.device.clone(),
                    fstype: fi.fstype.clone(),
                    size: format_size(fi.total_bytes, cfg),
                    used: format_size(fi.used_bytes, cfg),
                    avail: format_size(fi.avail_bytes, cfg),
                    pct: usage_pct(fi.used_bytes, fi.total_bytes),
                    mountpoint: fi.mountpoint.clone(),
                    raw_pct_numerator: fi.used_bytes,
                    raw_pct_denominator: fi.total_bytes,
                }
            }
        })
        .collect()
}

fn build_total_row(infos: &[FsInfo], cfg: &Config) -> Row {
    let mut tb: u64 = 0;
    let mut ub: u64 = 0;
    let mut ab: u64 = 0;
    let mut ti: u64 = 0;
    let mut ui: u64 = 0;
    let mut fi_free: u64 = 0;

    for fi in infos {
        tb = tb.saturating_add(fi.total_bytes);
        ub = ub.saturating_add(fi.used_bytes);
        ab = ab.saturating_add(fi.avail_bytes);
        ti = ti.saturating_add(fi.total_inodes);
        ui = ui.saturating_add(fi.used_inodes);
        fi_free = fi_free.saturating_add(fi.free_inodes);
    }

    if cfg.inodes {
        Row {
            filesystem: "total".to_string(),
            fstype: "-".to_string(),
            size: format!("{ti}"),
            used: format!("{ui}"),
            avail: format!("{fi_free}"),
            pct: usage_pct(ui, ti),
            mountpoint: "-".to_string(),
            raw_pct_numerator: ui,
            raw_pct_denominator: ti,
        }
    } else {
        Row {
            filesystem: "total".to_string(),
            fstype: "-".to_string(),
            size: format_size(tb, cfg),
            used: format_size(ub, cfg),
            avail: format_size(ab, cfg),
            pct: usage_pct(ub, tb),
            mountpoint: "-".to_string(),
            raw_pct_numerator: ub,
            raw_pct_denominator: tb,
        }
    }
}

/// Compute the column widths needed to fit all rows (including header).
struct ColWidths {
    filesystem: usize,
    fstype: usize,
    size: usize,
    used: usize,
    avail: usize,
    pct: usize,
    mountpoint: usize,
}

fn compute_widths(rows: &[Row], cfg: &Config) -> ColWidths {
    let fs_header = "Filesystem";
    let type_header = "Type";
    let (size_header, used_header, avail_header) = if cfg.inodes {
        ("Inodes", "IUsed", "IFree")
    } else if cfg.human || cfg.si {
        ("Size", "Used", "Avail", )
    } else {
        (cfg.block_label, "Used", "Available")
    };
    let pct_header = if cfg.inodes { "IUse%" } else { "Use%" };
    let mount_header = "Mounted on";

    let mut w = ColWidths {
        filesystem: fs_header.len(),
        fstype: type_header.len(),
        size: size_header.len(),
        used: used_header.len(),
        avail: avail_header.len(),
        pct: pct_header.len(),
        mountpoint: mount_header.len(),
    };

    for r in rows {
        w.filesystem = w.filesystem.max(r.filesystem.len());
        w.fstype = w.fstype.max(r.fstype.len());
        w.size = w.size.max(r.size.len());
        w.used = w.used.max(r.used.len());
        w.avail = w.avail.max(r.avail.len());
        w.pct = w.pct.max(r.pct.len());
        w.mountpoint = w.mountpoint.max(r.mountpoint.len());
    }

    w
}

fn print_table(rows: &[Row], cfg: &Config) {
    let w = compute_widths(rows, cfg);
    let color = cfg.color;

    // Header.
    let (size_header, used_header, avail_header) = if cfg.inodes {
        ("Inodes", "IUsed", "IFree")
    } else if cfg.human || cfg.si {
        ("Size", "Used", "Avail")
    } else {
        (cfg.block_label, "Used", "Available")
    };
    let pct_header = if cfg.inodes { "IUse%" } else { "Use%" };

    if cfg.show_type {
        println!(
            "{:<fw$} {:<tw$} {:>sw$} {:>uw$} {:>aw$} {:>pw$} Mounted on",
            "Filesystem",
            "Type",
            size_header,
            used_header,
            avail_header,
            pct_header,
            fw = w.filesystem,
            tw = w.fstype,
            sw = w.size,
            uw = w.used,
            aw = w.avail,
            pw = w.pct,
        );
    } else {
        println!(
            "{:<fw$} {:>sw$} {:>uw$} {:>aw$} {:>pw$} Mounted on",
            "Filesystem",
            size_header,
            used_header,
            avail_header,
            pct_header,
            fw = w.filesystem,
            sw = w.size,
            uw = w.used,
            aw = w.avail,
            pw = w.pct,
        );
    }

    // Data rows.
    for r in rows {
        let pct_str = if color {
            let c = pct_color(r.raw_pct_numerator, r.raw_pct_denominator);
            if c.is_empty() {
                format!("{:>pw$}", r.pct, pw = w.pct)
            } else {
                format!("{c}{:>pw$}{COLOR_RESET}", r.pct, pw = w.pct)
            }
        } else {
            format!("{:>pw$}", r.pct, pw = w.pct)
        };

        if cfg.show_type {
            // The ANSI escapes make pct_str longer than the visible width.
            // We've already right-aligned the visible text inside `pct_str`,
            // so print it without a width specifier.
            println!(
                "{:<fw$} {:<tw$} {:>sw$} {:>uw$} {:>aw$} {pct_str} {}",
                r.filesystem,
                r.fstype,
                r.size,
                r.used,
                r.avail,
                r.mountpoint,
                fw = w.filesystem,
                tw = w.fstype,
                sw = w.size,
                uw = w.used,
                aw = w.avail,
            );
        } else {
            println!(
                "{:<fw$} {:>sw$} {:>uw$} {:>aw$} {pct_str} {}",
                r.filesystem,
                r.size,
                r.used,
                r.avail,
                r.mountpoint,
                fw = w.filesystem,
                sw = w.size,
                uw = w.used,
                aw = w.avail,
            );
        }
    }
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for JSON (handles backslash, quotes, and control chars).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // \uXXXX encoding for other control characters.
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    let _ = std::fmt::Write::write_fmt(
                        &mut out,
                        format_args!("\\u{unit:04x}"),
                    );
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn print_json(infos: &[FsInfo], cfg: &Config) {
    println!("[");
    let last = infos.len().saturating_sub(1);
    for (idx, fi) in infos.iter().enumerate() {
        let comma = if idx < last { "," } else { "" };
        if cfg.inodes {
            println!(
                "  {{\
                 \"filesystem\":\"{dev}\",\
                 \"type\":\"{tp}\",\
                 \"inodes\":{ti},\
                 \"iused\":{iu},\
                 \"ifree\":{ifr},\
                 \"iuse_pct\":\"{pct}\",\
                 \"mounted_on\":\"{mp}\"\
                 }}{comma}",
                dev = json_escape(&fi.device),
                tp = json_escape(&fi.fstype),
                ti = fi.total_inodes,
                iu = fi.used_inodes,
                ifr = fi.free_inodes,
                pct = usage_pct(fi.used_inodes, fi.total_inodes),
                mp = json_escape(&fi.mountpoint),
            );
        } else {
            println!(
                "  {{\
                 \"filesystem\":\"{dev}\",\
                 \"type\":\"{tp}\",\
                 \"size\":{sz},\
                 \"used\":{us},\
                 \"available\":{av},\
                 \"use_pct\":\"{pct}\",\
                 \"mounted_on\":\"{mp}\"\
                 }}{comma}",
                dev = json_escape(&fi.device),
                tp = json_escape(&fi.fstype),
                sz = fi.total_bytes,
                us = fi.used_bytes,
                av = fi.avail_bytes,
                pct = usage_pct(fi.used_bytes, fi.total_bytes),
                mp = json_escape(&fi.mountpoint),
            );
        }
    }
    println!("]");
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let cfg = parse_args();
    let mounts = read_mounts();

    if mounts.is_empty() {
        eprintln!("df: no filesystems found in /proc/mounts");
        return 1;
    }

    // Filter and gather stats.
    let mut infos: Vec<FsInfo> = Vec::new();
    for entry in &mounts {
        if !should_display(entry, &cfg) {
            continue;
        }
        // A filesystem may have been unmounted between reading /proc/mounts
        // and calling SYS_FS_STAT -- silently skip if we cannot stat it.
        if let Some(fi) = gather_fs_info(entry) {
            infos.push(fi);
        }
    }

    // Deduplicate: if the same device is mounted in multiple places, keep
    // all entries (the user might want to see bind mounts).

    // If positional paths were given, narrow to the best match per path.
    best_mount_for_paths(&mut infos, &cfg.filter_paths);

    if infos.is_empty() {
        if cfg.filter_paths.is_empty() {
            eprintln!("df: no matching filesystems");
        } else {
            for p in &cfg.filter_paths {
                eprintln!("df: {p}: no file system information available");
            }
        }
        return 1;
    }

    if cfg.json {
        print_json(&infos, &cfg);
        return 0;
    }

    // Build displayable rows.
    let mut rows = build_rows(&infos, &cfg);

    if cfg.total {
        rows.push(build_total_row(&infos, &cfg));
    }

    print_table(&rows, &cfg);

    0
}

fn main() {
    process::exit(run());
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(device: &str, mountpoint: &str, fstype: &str) -> MountEntry {
        MountEntry {
            device: device.to_string(),
            mountpoint: mountpoint.to_string(),
            fstype: fstype.to_string(),
            options: "rw".to_string(),
        }
    }

    // ---- statvfs buffer parsing --------------------------------------------

    #[test]
    fn statvfs_buffer_parses_all_fields() {
        let mut buf = [0u8; FS_STATVFS_SIZE];
        buf[0..8].copy_from_slice(&16384u64.to_le_bytes()); // block_size (16 KiB)
        buf[8..16].copy_from_slice(&1000u64.to_le_bytes()); // total_blocks
        buf[16..24].copy_from_slice(&400u64.to_le_bytes()); // free_blocks
        buf[24..32].copy_from_slice(&256u64.to_le_bytes()); // total_inodes
        buf[32..40].copy_from_slice(&200u64.to_le_bytes()); // free_inodes
        buf[40..48].copy_from_slice(&255u64.to_le_bytes()); // max_name_len
        buf[48] = 1; // read_only

        let st = parse_statvfs_buffer(&buf).unwrap();
        assert_eq!(st.block_size, 16384);
        assert_eq!(st.total_blocks, 1000);
        assert_eq!(st.free_blocks, 400);
        assert_eq!(st.total_inodes, 256);
        assert_eq!(st.free_inodes, 200);
        assert_eq!(st.max_name_len, 255);
        assert!(st.read_only);
    }

    #[test]
    fn statvfs_buffer_read_only_false() {
        let buf = [0u8; FS_STATVFS_SIZE];
        let st = parse_statvfs_buffer(&buf).unwrap();
        assert!(!st.read_only);
    }

    #[test]
    fn statvfs_buffer_too_small_returns_none() {
        let buf = [0u8; 40];
        assert!(parse_statvfs_buffer(&buf).is_none());
    }

    // ---- human / SI formatting ---------------------------------------------

    #[test]
    fn human_readable_units() {
        assert_eq!(human_readable(0), "0");
        assert_eq!(human_readable(512), "512B");
        // Exact multiples drop the fractional part (".0").
        assert_eq!(human_readable(1024), "1K");
        assert_eq!(human_readable(1536), "1.5K");
        assert_eq!(human_readable(1024 * 1024), "1M");
    }

    #[test]
    fn si_readable_units() {
        assert_eq!(si_readable(0), "0");
        assert_eq!(si_readable(1000), "1kB");
        assert_eq!(si_readable(1500), "1.5kB");
        assert_eq!(si_readable(1_000_000), "1MB");
    }

    #[test]
    fn block_str_rounds_up() {
        // 1 KiB blocks: 1025 bytes -> 2 blocks (ceil).
        assert_eq!(block_str(1025, 1024), "2");
        assert_eq!(block_str(1024, 1024), "1");
        assert_eq!(block_str(0, 1024), "0");
    }

    // ---- usage percentage --------------------------------------------------

    #[test]
    fn usage_pct_basics() {
        assert_eq!(usage_pct(0, 0), "-");
        assert_eq!(usage_pct(0, 100), "0%");
        assert_eq!(usage_pct(100, 100), "100%");
        // Linux rounds the percentage up.
        assert_eq!(usage_pct(1, 100), "1%");
        assert_eq!(usage_pct(101, 1000), "11%");
    }

    // ---- pseudo-fs detection -----------------------------------------------

    #[test]
    fn pseudo_fs_detection() {
        assert!(is_pseudo_fs("proc"));
        assert!(is_pseudo_fs("TMPFS"));
        assert!(!is_pseudo_fs("ext4"));
    }

    // ---- display filtering -------------------------------------------------

    #[test]
    fn should_display_skips_pseudo_by_default() {
        let cfg = Config::default_config();
        assert!(!should_display(&mount("tmpfs", "/tmp", "tmpfs"), &cfg));
        assert!(should_display(&mount("/dev/sda1", "/", "ext4"), &cfg));
    }

    #[test]
    fn should_display_include_type_filter() {
        let mut cfg = Config::default_config();
        cfg.include_types.push("ext4".to_string());
        assert!(should_display(&mount("/dev/sda1", "/", "ext4"), &cfg));
        assert!(!should_display(&mount("/dev/sdb1", "/data", "xfs"), &cfg));
    }

    #[test]
    fn should_display_exclude_type_filter() {
        let mut cfg = Config::default_config();
        cfg.exclude_types.push("ext4".to_string());
        assert!(!should_display(&mount("/dev/sda1", "/", "ext4"), &cfg));
    }

    // ---- best-mount selection ----------------------------------------------

    #[test]
    fn best_mount_picks_longest_prefix() {
        let mut infos = vec![
            FsInfo {
                device: "/dev/sda1".to_string(),
                mountpoint: "/".to_string(),
                fstype: "ext4".to_string(),
                total_bytes: 0,
                used_bytes: 0,
                avail_bytes: 0,
                total_inodes: 0,
                used_inodes: 0,
                free_inodes: 0,
            },
            FsInfo {
                device: "/dev/sda2".to_string(),
                mountpoint: "/home".to_string(),
                fstype: "ext4".to_string(),
                total_bytes: 0,
                used_bytes: 0,
                avail_bytes: 0,
                total_inodes: 0,
                used_inodes: 0,
                free_inodes: 0,
            },
        ];
        best_mount_for_paths(&mut infos, &["/home/alice".to_string()]);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].mountpoint, "/home");
    }

    // ---- JSON escaping -----------------------------------------------------

    #[test]
    fn json_escape_specials() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("x\ty"), "x\\ty");
        assert_eq!(json_escape("plain"), "plain");
    }
}
