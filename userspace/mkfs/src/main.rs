//! SlateOS mkfs -- Create Filesystems
//!
//! Creates filesystems on disk devices or image files. Supports ext4, FAT32,
//! and tmpfs via the kernel's `SYS_FS_FORMAT` syscall (number 651 in the
//! filesystem zone, 600-799).
//!
//! # Usage
//!
//! ```text
//! mkfs -t <type> [options] <device>
//! mkfs.<type> [options] <device>        (detect type from argv[0])
//!
//! Filesystem types: ext4, fat32/vfat, tmpfs
//!
//! Common options:
//!   -t <type>              Filesystem type
//!   -L <label> / --label   Volume label
//!   -n / --dry-run         Show what would be done without doing it
//!   -v / --verbose         Verbose output
//!   -f / --force           Force creation even if device has existing fs
//!   -c / --check           Check device for bad blocks before formatting
//!   --json                 JSON output of created filesystem info
//!
//! ext4 options:
//!   -b <blocksize>         Block size in bytes (1024, 2048, 4096, 16384)
//!   -i <bytes-per-inode>   Bytes per inode ratio
//!   -N <num-inodes>        Number of inodes to create
//!   -J size=<N>            Journal size in MiB
//!   -O <features>          Comma-separated feature list
//!
//! FAT32 options:
//!   -F <fat-size>          FAT type: 12, 16, or 32
//!   -s <sectors/cluster>   Sectors per cluster
//!   -S <sector-size>       Logical sector size in bytes
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

/// Format a filesystem on a device.
/// arg1 = device path pointer, arg2 = device path length,
/// arg3 = filesystem type id (1=ext4, 2=fat32, 3=tmpfs).
const SYS_FS_FORMAT: u64 = 651;

/// Filesystem type identifiers accepted by the kernel.
const FS_TYPE_EXT4: u64 = 1;
const FS_TYPE_FAT32: u64 = 2;
const FS_TYPE_TMPFS: u64 = 3;

/// Invoke a syscall with 3 arguments.
///
/// The kernel receives: rdi=arg1, rsi=arg2, rdx=arg3.
/// Returns a negative errno on failure, 0 on success.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid pointers/values for the
    // given syscall number. The kernel validates all inputs and returns
    // a negative errno on failure.
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

/// Translate a negative syscall return into a human-readable error message.
fn syscall_error_msg(ret: i64) -> &'static str {
    match ret {
        -1 => "operation not permitted (EPERM)",
        -2 => "no such file or directory (ENOENT)",
        -5 => "I/O error (EIO)",
        -12 => "out of memory (ENOMEM)",
        -13 => "permission denied (EACCES)",
        -16 => "device or resource busy (EBUSY)",
        -19 => "no such device (ENODEV)",
        -22 => "invalid argument (EINVAL)",
        -28 => "no space left on device (ENOSPC)",
        -30 => "read-only filesystem (EROFS)",
        -38 => "function not implemented (ENOSYS)",
        _ => "unknown error",
    }
}

// ============================================================================
// Filesystem helpers
// ============================================================================

/// Read a sysfs/procfs file, returning its trimmed contents on success.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a sysfs file and parse as u64, returning 0 on failure.
fn read_u64(path: &str) -> u64 {
    read_file(path)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Check whether a device path exists (either in /dev or as a regular file).
fn device_exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// Check whether a device is currently mounted by scanning /proc/mounts.
fn is_mounted(dev_path: &str) -> bool {
    let content = match fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return false,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(mounted_dev) = parts.first()
            && *mounted_dev == dev_path {
                return true;
            }
    }
    false
}

/// Detect the existing filesystem type on a device by reading from sysfs.
/// Returns an empty string if no filesystem is detected.
fn detect_existing_fs(dev_name: &str) -> String {
    // Try /sys/block/<dev>/fstype first (whole disk).
    if let Some(ft) = read_file(&format!("/sys/block/{dev_name}/fstype"))
        && !ft.is_empty() {
            return ft;
        }

    // Try as a partition: scan parent disks.
    if let Ok(entries) = fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                let part_path = format!("/sys/block/{name}/{dev_name}/fstype");
                if let Some(ft) = read_file(&part_path)
                    && !ft.is_empty() {
                        return ft;
                    }
            }
        }
    }

    String::new()
}

/// Get the size in bytes of a block device from sysfs.
/// Returns 0 if the size cannot be determined.
fn get_device_size(dev_name: &str) -> u64 {
    // Try /sys/block/<dev>/size (whole disk, in 512-byte sectors).
    let sectors = read_u64(&format!("/sys/block/{dev_name}/size"));
    if sectors > 0 {
        return sectors.saturating_mul(512);
    }

    // Try as a partition under a parent disk.
    if let Ok(entries) = fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                let part_size = read_u64(&format!("/sys/block/{name}/{dev_name}/size"));
                if part_size > 0 {
                    return part_size.saturating_mul(512);
                }
            }
        }
    }

    // Fall back to file size for image files.
    if let Ok(meta) = fs::metadata(format!("/dev/{dev_name}")) {
        return meta.len();
    }

    0
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count as a human-readable string (e.g. "1.5 GiB").
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 10) / TIB;
        if frac > 0 {
            format!("{whole}.{frac} TiB")
        } else {
            format!("{whole} TiB")
        }
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 10) / GIB;
        if frac > 0 {
            format!("{whole}.{frac} GiB")
        } else {
            format!("{whole} GiB")
        }
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 10) / MIB;
        if frac > 0 {
            format!("{whole}.{frac} MiB")
        } else {
            format!("{whole} MiB")
        }
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 10) / KIB;
        if frac > 0 {
            format!("{whole}.{frac} KiB")
        } else {
            format!("{whole} KiB")
        }
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// JSON escape helper
// ============================================================================

/// Escape a string for safe inclusion in JSON output.
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
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Parsed options
// ============================================================================

/// All options parsed from the command line.
struct MkfsOptions {
    /// Filesystem type: "ext4", "fat32", "tmpfs".
    fs_type: String,
    /// Device path (e.g. "/dev/sda1" or "disk.img").
    device: String,
    /// Volume label (-L / --label).
    label: String,
    /// Dry run mode (-n / --dry-run).
    dry_run: bool,
    /// Verbose output (-v / --verbose).
    verbose: bool,
    /// Force overwrite (-f / --force).
    force: bool,
    /// Check for bad blocks first (-c / --check).
    check: bool,
    /// JSON output (--json).
    json: bool,

    // -- ext4-specific --
    /// Block size in bytes (default 4096).
    ext4_block_size: u64,
    /// Bytes-per-inode ratio for inode count calculation.
    ext4_bytes_per_inode: u64,
    /// Explicit inode count (overrides bytes-per-inode).
    ext4_num_inodes: u64,
    /// Journal size in MiB (0 = default).
    ext4_journal_size_mib: u64,
    /// Feature list (comma-separated).
    ext4_features: String,

    // -- FAT-specific --
    /// FAT type: 12, 16, or 32 (default 32).
    fat_size: u32,
    /// Sectors per cluster (0 = auto).
    fat_sectors_per_cluster: u32,
    /// Logical sector size in bytes (default 512).
    fat_sector_size: u32,
}

impl MkfsOptions {
    fn new() -> Self {
        Self {
            fs_type: String::new(),
            device: String::new(),
            label: String::new(),
            dry_run: false,
            verbose: false,
            force: false,
            check: false,
            json: false,
            ext4_block_size: 4096,
            ext4_bytes_per_inode: 16384,
            ext4_num_inodes: 0,
            ext4_journal_size_mib: 0,
            ext4_features: String::new(),
            fat_size: 32,
            fat_sectors_per_cluster: 0,
            fat_sector_size: 512,
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Detect filesystem type from argv[0] (e.g. "mkfs.ext4" -> "ext4").
fn detect_type_from_argv0(argv0: &str) -> Option<String> {
    // Extract the basename, stripping any directory prefix.
    let basename = argv0.rsplit('/').next().unwrap_or(argv0);

    if let Some(suffix) = basename.strip_prefix("mkfs.") {
        let normalized = normalize_fs_type(suffix);
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }

    None
}

/// Normalize user-supplied filesystem type names to canonical form.
/// Returns empty string for unrecognized types.
fn normalize_fs_type(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "ext4" => "ext4".to_string(),
        "fat32" | "vfat" | "fat" => "fat32".to_string(),
        "tmpfs" => "tmpfs".to_string(),
        _ => String::new(),
    }
}

/// Map a canonical filesystem type name to the kernel's numeric type id.
fn fs_type_to_id(fs_type: &str) -> Option<u64> {
    match fs_type {
        "ext4" => Some(FS_TYPE_EXT4),
        "fat32" => Some(FS_TYPE_FAT32),
        "tmpfs" => Some(FS_TYPE_TMPFS),
        _ => None,
    }
}

/// Parse command-line arguments into MkfsOptions. Exits on error.
fn parse_args() -> MkfsOptions {
    let args: Vec<String> = env::args().collect();
    let mut opts = MkfsOptions::new();

    // Detect filesystem type from program name (mkfs.ext4, mkfs.fat, etc.).
    if let Some(argv0) = args.first()
        && let Some(detected) = detect_type_from_argv0(argv0) {
            opts.fs_type = detected;
        }

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-t" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -t requires a filesystem type argument");
                    process::exit(1);
                }
                let normalized = normalize_fs_type(&args[i]);
                if normalized.is_empty() {
                    eprintln!(
                        "mkfs: error: unsupported filesystem type '{}'",
                        args[i]
                    );
                    eprintln!("  Supported types: ext4, fat32 (vfat), tmpfs");
                    process::exit(1);
                }
                opts.fs_type = normalized;
            }

            "-L" | "--label" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -L/--label requires a label argument");
                    process::exit(1);
                }
                opts.label = args[i].clone();
            }

            "-n" | "--dry-run" => opts.dry_run = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-f" | "--force" => opts.force = true,
            "-c" | "--check" => opts.check = true,
            "--json" => opts.json = true,

            // ext4-specific options.
            "-b" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -b requires a block size argument");
                    process::exit(1);
                }
                match args[i].parse::<u64>() {
                    Ok(bs) => opts.ext4_block_size = bs,
                    Err(_) => {
                        eprintln!("mkfs: error: invalid block size '{}'", args[i]);
                        process::exit(1);
                    }
                }
            }

            "-i" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -i requires a bytes-per-inode argument");
                    process::exit(1);
                }
                match args[i].parse::<u64>() {
                    Ok(bpi) => opts.ext4_bytes_per_inode = bpi,
                    Err(_) => {
                        eprintln!(
                            "mkfs: error: invalid bytes-per-inode '{}'",
                            args[i]
                        );
                        process::exit(1);
                    }
                }
            }

            "-N" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -N requires an inode count argument");
                    process::exit(1);
                }
                match args[i].parse::<u64>() {
                    Ok(n) => opts.ext4_num_inodes = n,
                    Err(_) => {
                        eprintln!("mkfs: error: invalid inode count '{}'", args[i]);
                        process::exit(1);
                    }
                }
            }

            "-J" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -J requires a journal spec (e.g. size=128)");
                    process::exit(1);
                }
                // Parse "size=<N>" format.
                if let Some(val) = args[i].strip_prefix("size=") {
                    match val.parse::<u64>() {
                        Ok(sz) => opts.ext4_journal_size_mib = sz,
                        Err(_) => {
                            eprintln!(
                                "mkfs: error: invalid journal size '{}'",
                                args[i]
                            );
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!(
                        "mkfs: error: unrecognized -J parameter '{}' (expected size=<N>)",
                        args[i]
                    );
                    process::exit(1);
                }
            }

            "-O" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -O requires a feature list");
                    process::exit(1);
                }
                opts.ext4_features = args[i].clone();
            }

            // FAT-specific options.
            "-F" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -F requires a FAT size (12, 16, 32)");
                    process::exit(1);
                }
                match args[i].parse::<u32>() {
                    Ok(fs @ (12 | 16 | 32)) => opts.fat_size = fs,
                    Ok(other) => {
                        eprintln!(
                            "mkfs: error: invalid FAT size {other} (must be 12, 16, or 32)"
                        );
                        process::exit(1);
                    }
                    Err(_) => {
                        eprintln!("mkfs: error: invalid FAT size '{}'", args[i]);
                        process::exit(1);
                    }
                }
            }

            "-s" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -s requires sectors-per-cluster value");
                    process::exit(1);
                }
                match args[i].parse::<u32>() {
                    Ok(spc) => opts.fat_sectors_per_cluster = spc,
                    Err(_) => {
                        eprintln!(
                            "mkfs: error: invalid sectors-per-cluster '{}'",
                            args[i]
                        );
                        process::exit(1);
                    }
                }
            }

            "-S" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkfs: error: -S requires a sector size value");
                    process::exit(1);
                }
                match args[i].parse::<u32>() {
                    Ok(ss) => opts.fat_sector_size = ss,
                    Err(_) => {
                        eprintln!(
                            "mkfs: error: invalid sector size '{}'",
                            args[i]
                        );
                        process::exit(1);
                    }
                }
            }

            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }

            "--version" | "-V" => {
                println!("mkfs (SlateOS) 0.1.0");
                process::exit(0);
            }

            other => {
                // Treat as device path if it doesn't start with '-'.
                if other.starts_with('-') {
                    eprintln!("mkfs: error: unknown option '{other}'");
                    eprintln!("  Run 'mkfs --help' for usage.");
                    process::exit(1);
                }
                if !opts.device.is_empty() {
                    eprintln!("mkfs: error: multiple device arguments given");
                    eprintln!("  Already have '{}', got '{other}'", opts.device);
                    process::exit(1);
                }
                opts.device = other.to_string();
            }
        }
        i += 1;
    }

    opts
}

// ============================================================================
// Validation
// ============================================================================

/// Validate parsed options. Exits with an error message on failure.
fn validate_options(opts: &MkfsOptions) {
    if opts.fs_type.is_empty() {
        eprintln!("mkfs: error: no filesystem type specified");
        eprintln!("  Use -t <type> or invoke as mkfs.<type> (e.g. mkfs.ext4)");
        eprintln!("  Supported types: ext4, fat32 (vfat), tmpfs");
        process::exit(1);
    }

    if opts.device.is_empty() {
        eprintln!("mkfs: error: no device specified");
        eprintln!("  Usage: mkfs -t <type> <device>");
        process::exit(1);
    }

    // Validate ext4 block size (must be a power of 2, between 1024 and 65536).
    if opts.fs_type == "ext4" {
        let bs = opts.ext4_block_size;
        if !(1024..=65536).contains(&bs) || (bs & (bs - 1)) != 0 {
            eprintln!(
                "mkfs: error: invalid ext4 block size {bs} \
                 (must be a power of 2 between 1024 and 65536)"
            );
            process::exit(1);
        }
    }

    // Validate FAT sector size (must be a power of 2, between 512 and 4096).
    if opts.fs_type == "fat32" {
        let ss = opts.fat_sector_size;
        if !(512..=4096).contains(&ss) || (ss & (ss - 1)) != 0 {
            eprintln!(
                "mkfs: error: invalid FAT sector size {ss} \
                 (must be a power of 2 between 512 and 4096)"
            );
            process::exit(1);
        }

        if opts.fat_sectors_per_cluster > 0 {
            let spc = opts.fat_sectors_per_cluster;
            if spc > 128 || (spc & (spc - 1)) != 0 {
                eprintln!(
                    "mkfs: error: invalid sectors-per-cluster {spc} \
                     (must be a power of 2, max 128)"
                );
                process::exit(1);
            }
        }
    }

    // Validate label length.
    if !opts.label.is_empty() {
        let max_len = match opts.fs_type.as_str() {
            "ext4" => 16,
            "fat32" => 11,
            _ => 64,
        };
        if opts.label.len() > max_len {
            eprintln!(
                "mkfs: error: label '{}' too long for {} (max {} characters)",
                opts.label, opts.fs_type, max_len
            );
            process::exit(1);
        }
    }
}

// ============================================================================
// Device-path normalization
// ============================================================================

/// Normalize a device argument to a full path.
/// Strips /dev/ prefix for sysfs lookups, adds /dev/ for syscall use.
///
/// Returns (dev_path_for_syscall, dev_name_for_sysfs).
fn normalize_device(device: &str) -> (String, String) {
    if let Some(name) = device.strip_prefix("/dev/") {
        (device.to_string(), name.to_string())
    } else if device.starts_with('/') || device.contains('.') {
        // Absolute path or image file -- use as-is.
        let name = device
            .rsplit('/')
            .next()
            .unwrap_or(device);
        (device.to_string(), name.to_string())
    } else {
        // Bare device name like "sda1" -- prepend /dev/.
        (format!("/dev/{device}"), device.to_string())
    }
}

// ============================================================================
// Pre-format checks
// ============================================================================

/// Run all pre-format safety checks. Returns true if it is safe to proceed.
fn preflight_checks(opts: &MkfsOptions, dev_path: &str, dev_name: &str) -> bool {
    // Check that the device exists.
    if !device_exists(dev_path) {
        eprintln!("mkfs: error: '{}' does not exist", dev_path);
        return false;
    }

    // Check if the device is currently mounted.
    if is_mounted(dev_path) {
        eprintln!(
            "mkfs: error: {} is currently mounted -- unmount it first",
            dev_path
        );
        return false;
    }

    // Check for existing filesystem.
    let existing_fs = detect_existing_fs(dev_name);
    if !existing_fs.is_empty() && !opts.force {
        eprintln!(
            "mkfs: error: {} already contains a {} filesystem",
            dev_path, existing_fs
        );
        eprintln!("  Use -f/--force to overwrite.");
        return false;
    } else if !existing_fs.is_empty() && opts.force && opts.verbose {
        eprintln!(
            "mkfs: warning: overwriting existing {} filesystem on {}",
            existing_fs, dev_path
        );
    }

    true
}

// ============================================================================
// Summary computation
// ============================================================================

/// Filesystem creation summary for display and JSON output.
struct FsSummary {
    fs_type: String,
    device: String,
    total_size: u64,
    block_size: u64,
    num_inodes: u64,
    journal_size_mib: u64,
    label: String,
    features: String,
    // FAT-specific.
    fat_type: u32,
    sectors_per_cluster: u32,
    sector_size: u32,
}

/// Compute a summary of the filesystem that will be (or was) created.
fn compute_summary(opts: &MkfsOptions, dev_name: &str) -> FsSummary {
    let total_size = get_device_size(dev_name);

    match opts.fs_type.as_str() {
        "ext4" => {
            let block_size = opts.ext4_block_size;
            let num_inodes = if opts.ext4_num_inodes > 0 {
                opts.ext4_num_inodes
            } else if total_size > 0 && opts.ext4_bytes_per_inode > 0 {
                total_size / opts.ext4_bytes_per_inode
            } else {
                0
            };

            // Default journal size: 128 MiB for devices >= 4 GiB, otherwise
            // 1/32 of device size (clamped to 4..1024 MiB).
            let journal_mib = if opts.ext4_journal_size_mib > 0 {
                opts.ext4_journal_size_mib
            } else if total_size > 0 {
                let default = total_size / (32 * 1024 * 1024);
                default.clamp(4, 1024)
            } else {
                128
            };

            FsSummary {
                fs_type: "ext4".to_string(),
                device: opts.device.clone(),
                total_size,
                block_size,
                num_inodes,
                journal_size_mib: journal_mib,
                label: opts.label.clone(),
                features: opts.ext4_features.clone(),
                fat_type: 0,
                sectors_per_cluster: 0,
                sector_size: 0,
            }
        }

        "fat32" => {
            let sector_size = opts.fat_sector_size;
            let spc = if opts.fat_sectors_per_cluster > 0 {
                opts.fat_sectors_per_cluster
            } else {
                // Auto-select based on device size (matching Windows defaults).
                auto_fat_cluster_size(total_size, sector_size)
            };

            FsSummary {
                fs_type: "fat32".to_string(),
                device: opts.device.clone(),
                total_size,
                block_size: u64::from(sector_size) * u64::from(spc),
                num_inodes: 0,
                journal_size_mib: 0,
                label: opts.label.clone(),
                features: String::new(),
                fat_type: opts.fat_size,
                sectors_per_cluster: spc,
                sector_size,
            }
        }

        "tmpfs" => FsSummary {
            fs_type: "tmpfs".to_string(),
            device: opts.device.clone(),
            total_size,
            block_size: 4096,
            num_inodes: 0,
            journal_size_mib: 0,
            label: opts.label.clone(),
            features: String::new(),
            fat_type: 0,
            sectors_per_cluster: 0,
            sector_size: 0,
        },

        _ => FsSummary {
            fs_type: opts.fs_type.clone(),
            device: opts.device.clone(),
            total_size: 0,
            block_size: 0,
            num_inodes: 0,
            journal_size_mib: 0,
            label: String::new(),
            features: String::new(),
            fat_type: 0,
            sectors_per_cluster: 0,
            sector_size: 0,
        },
    }
}

/// Select a default sectors-per-cluster for FAT based on device size,
/// following the same heuristic as Windows.
fn auto_fat_cluster_size(total_bytes: u64, sector_size: u32) -> u32 {
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    // If device size is unknown, pick a safe default.
    if total_bytes == 0 {
        return 8;
    }

    // sectors_per_cluster: larger devices get larger clusters to reduce FAT
    // table size; smaller devices use smaller clusters for less slack waste.
    let spc = if total_bytes <= 64 * MIB {
        1u64
    } else if total_bytes <= 128 * MIB {
        2
    } else if total_bytes <= 256 * MIB {
        4
    } else if total_bytes <= 8 * GIB {
        8
    } else if total_bytes <= 16 * GIB {
        16
    } else if total_bytes <= 32 * GIB {
        32
    } else {
        64
    };

    // Ensure cluster size does not exceed 32 KiB (FAT spec limit for most
    // implementations). Scale down spc if sector_size is large.
    let cluster_bytes = spc * u64::from(sector_size);
    if cluster_bytes > 32768 {
        let max_spc = 32768 / u64::from(sector_size);
        if max_spc >= 1 { max_spc as u32 } else { 1 }
    } else {
        spc as u32
    }
}

// ============================================================================
// Display helpers
// ============================================================================

/// Print the human-readable summary of the filesystem to be created.
fn print_summary(summary: &FsSummary) {
    println!();
    println!("=== Filesystem Summary ===");
    println!("  Type:           {}", summary.fs_type);
    println!("  Device:         {}", summary.device);
    if summary.total_size > 0 {
        println!(
            "  Total size:     {} ({} bytes)",
            format_size(summary.total_size),
            summary.total_size
        );
    }

    match summary.fs_type.as_str() {
        "ext4" => {
            println!("  Block size:     {} bytes", summary.block_size);
            if summary.num_inodes > 0 {
                println!("  Inodes:         {}", summary.num_inodes);
            }
            if summary.journal_size_mib > 0 {
                println!("  Journal size:   {} MiB", summary.journal_size_mib);
            }
            if !summary.features.is_empty() {
                println!("  Features:       {}", summary.features);
            }
        }
        "fat32" => {
            println!("  FAT type:       FAT{}", summary.fat_type);
            println!("  Sector size:    {} bytes", summary.sector_size);
            println!(
                "  Cluster size:   {} bytes ({} sectors/cluster)",
                summary.block_size, summary.sectors_per_cluster
            );
        }
        "tmpfs" => {
            println!("  (in-memory filesystem, no on-disk structures)");
        }
        _ => {}
    }

    if !summary.label.is_empty() {
        println!("  Label:          {}", summary.label);
    }
    println!();
}

/// Print JSON output of the filesystem summary.
fn print_json(summary: &FsSummary) {
    println!("{{");
    println!("  \"fs_type\": \"{}\",", json_escape(&summary.fs_type));
    println!("  \"device\": \"{}\",", json_escape(&summary.device));
    println!("  \"total_size\": {},", summary.total_size);
    println!(
        "  \"total_size_human\": \"{}\",",
        json_escape(&format_size(summary.total_size))
    );
    println!("  \"block_size\": {},", summary.block_size);

    match summary.fs_type.as_str() {
        "ext4" => {
            println!("  \"inodes\": {},", summary.num_inodes);
            println!("  \"journal_size_mib\": {},", summary.journal_size_mib);
            if !summary.features.is_empty() {
                println!(
                    "  \"features\": \"{}\",",
                    json_escape(&summary.features)
                );
            }
        }
        "fat32" => {
            println!("  \"fat_type\": {},", summary.fat_type);
            println!("  \"sector_size\": {},", summary.sector_size);
            println!(
                "  \"sectors_per_cluster\": {},",
                summary.sectors_per_cluster
            );
        }
        _ => {}
    }

    if !summary.label.is_empty() {
        println!("  \"label\": \"{}\",", json_escape(&summary.label));
    }
    println!("  \"success\": true");
    println!("}}");
}

// ============================================================================
// Bad-block check stub
// ============================================================================

/// Perform a basic bad-block scan by reading every block on the device.
/// This is a read-only check -- it reports blocks that return I/O errors.
fn check_bad_blocks(dev_path: &str, verbose: bool) {
    println!("Checking {} for bad blocks (read-only)...", dev_path);

    // Open the device for reading and scan sequentially.
    let file = match fs::File::open(dev_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("mkfs: warning: cannot open {} for bad-block check: {}", dev_path, e);
            return;
        }
    };

    use std::io::Read;

    let mut reader = std::io::BufReader::with_capacity(1024 * 1024, file);
    let mut buf = vec![0u8; 1024 * 1024]; // 1 MiB read buffer
    let mut block_num: u64 = 0;
    let mut bad_count: u64 = 0;
    let mut total_read: u64 = 0;

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                total_read += n as u64;
                block_num += 1;
                if verbose && block_num.is_multiple_of(64) {
                    eprint!("\r  Checked {} ...", format_size(total_read));
                }
            }
            Err(e) => {
                bad_count += 1;
                if verbose || bad_count <= 10 {
                    eprintln!(
                        "\r  Bad block at offset ~{}: {}",
                        format_size(total_read),
                        e
                    );
                }
                // Skip ahead by seeking past the bad region.
                total_read += buf.len() as u64;
                block_num += 1;
            }
        }
    }

    if verbose {
        eprintln!(); // Clear the progress line.
    }

    if bad_count == 0 {
        println!("  No bad blocks found ({} checked).", format_size(total_read));
    } else {
        eprintln!(
            "  WARNING: {} bad block(s) detected in {}.",
            bad_count,
            format_size(total_read)
        );
        eprintln!("  The device may be failing. Proceed with caution.");
    }
}

// ============================================================================
// Format execution
// ============================================================================

/// Issue the SYS_FS_FORMAT syscall to create the filesystem.
fn do_format(dev_path: &str, fs_type_id: u64) -> Result<(), String> {
    let path_bytes = dev_path.as_bytes();
    let ret = unsafe {
        // SAFETY: path_bytes points to valid UTF-8 memory with known length.
        // The kernel will validate the pointer and length.
        syscall3(
            SYS_FS_FORMAT,
            path_bytes.as_ptr() as u64,
            path_bytes.len() as u64,
            fs_type_id,
        )
    };

    if ret < 0 {
        Err(format!(
            "SYS_FS_FORMAT failed (errno {}): {}",
            -ret,
            syscall_error_msg(ret)
        ))
    } else {
        Ok(())
    }
}

// ============================================================================
// Main flow
// ============================================================================

fn print_usage() {
    println!("mkfs (SlateOS) 0.1.0 -- Create filesystems on disk devices or image files");
    println!();
    println!("USAGE:");
    println!("  mkfs -t <type> [options] <device>");
    println!("  mkfs.<type> [options] <device>");
    println!();
    println!("FILESYSTEM TYPES:");
    println!("  ext4           Linux ext4 filesystem");
    println!("  fat32 / vfat   FAT32 filesystem");
    println!("  tmpfs          In-memory temporary filesystem");
    println!();
    println!("COMMON OPTIONS:");
    println!("  -t <type>      Filesystem type (required unless argv[0] is mkfs.<type>)");
    println!("  -L <label>     Volume label");
    println!("  --label <l>    Same as -L");
    println!("  -n, --dry-run  Show what would be done, without writing");
    println!("  -v, --verbose  Verbose output");
    println!("  -f, --force    Force creation, even if device has existing filesystem");
    println!("  -c, --check    Check device for bad blocks before formatting");
    println!("  --json         Output filesystem info as JSON");
    println!("  -h, --help     Show this help message");
    println!("  -V, --version  Show version");
    println!();
    println!("EXT4 OPTIONS:");
    println!("  -b <size>      Block size in bytes (1024, 2048, 4096, 16384, ...)");
    println!("  -i <ratio>     Bytes-per-inode ratio (controls inode count)");
    println!("  -N <count>     Explicit number of inodes to create");
    println!("  -J size=<N>    Journal size in MiB");
    println!("  -O <features>  Comma-separated feature list (e.g. dir_index,extent)");
    println!();
    println!("FAT OPTIONS:");
    println!("  -F <12|16|32>  FAT type (default: 32)");
    println!("  -s <spc>       Sectors per cluster");
    println!("  -S <size>      Logical sector size in bytes (default: 512)");
    println!();
    println!("EXAMPLES:");
    println!("  mkfs -t ext4 /dev/sda1");
    println!("  mkfs -t fat32 -L BOOT /dev/sda2");
    println!("  mkfs.ext4 -b 4096 -L rootfs /dev/nvme0n1p3");
    println!("  mkfs.fat -F 32 -n /dev/sdb1");
    println!("  mkfs -t ext4 -J size=256 -O dir_index,extent -v /dev/vda1");
    println!("  mkfs -t tmpfs tmpfs");
}

fn main() {
    let opts = parse_args();

    // If no arguments at all, show usage.
    if opts.fs_type.is_empty() && opts.device.is_empty() {
        print_usage();
        process::exit(0);
    }

    // Validate all options.
    validate_options(&opts);

    let (dev_path, dev_name) = normalize_device(&opts.device);

    // Compute the filesystem summary before doing anything destructive.
    let summary = compute_summary(&opts, &dev_name);

    if opts.verbose {
        println!(
            "mkfs: creating {} filesystem on {}",
            opts.fs_type, dev_path
        );
    }

    // --- Dry-run mode ---
    if opts.dry_run {
        println!("mkfs: dry run -- no changes will be written");
        print_summary(&summary);
        if opts.json {
            print_json(&summary);
        }
        println!("mkfs: dry run complete (no changes made)");
        process::exit(0);
    }

    // --- Pre-flight checks ---
    if !preflight_checks(&opts, &dev_path, &dev_name) {
        process::exit(1);
    }

    // --- Bad-block check (if requested) ---
    if opts.check {
        check_bad_blocks(&dev_path, opts.verbose);
    }

    // --- Resolve the kernel fs_type_id ---
    let fs_type_id = match fs_type_to_id(&opts.fs_type) {
        Some(id) => id,
        None => {
            // Should never happen after validation, but be defensive.
            eprintln!(
                "mkfs: internal error: no type id for '{}'",
                opts.fs_type
            );
            process::exit(1);
        }
    };

    // --- Verbose pre-format info ---
    if opts.verbose {
        if !opts.label.is_empty() {
            println!("  Label:        {}", opts.label);
        }
        match opts.fs_type.as_str() {
            "ext4" => {
                println!("  Block size:   {} bytes", opts.ext4_block_size);
                if summary.num_inodes > 0 {
                    println!("  Inodes:       {}", summary.num_inodes);
                }
                if summary.journal_size_mib > 0 {
                    println!("  Journal:      {} MiB", summary.journal_size_mib);
                }
                if !opts.ext4_features.is_empty() {
                    println!("  Features:     {}", opts.ext4_features);
                }
            }
            "fat32" => {
                println!("  FAT type:     FAT{}", opts.fat_size);
                println!("  Sector size:  {} bytes", opts.fat_sector_size);
                if summary.sectors_per_cluster > 0 {
                    println!(
                        "  Cluster size: {} bytes",
                        summary.block_size
                    );
                }
            }
            _ => {}
        }
    }

    // --- Perform the format ---
    println!("Creating {} filesystem on {} ...", opts.fs_type, dev_path);

    match do_format(&dev_path, fs_type_id) {
        Ok(()) => {
            println!(
                "mkfs: successfully created {} filesystem on {}",
                opts.fs_type, dev_path
            );
        }
        Err(e) => {
            eprintln!("mkfs: error: {}", e);
            process::exit(1);
        }
    }

    // --- Post-format summary ---
    if opts.json {
        print_json(&summary);
    } else {
        print_summary(&summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- syscall_error_msg -------------------------------------------------

    #[test]
    fn syscall_error_msg_known_codes() {
        assert_eq!(syscall_error_msg(-2), "no such file or directory (ENOENT)");
        assert_eq!(syscall_error_msg(-13), "permission denied (EACCES)");
        assert_eq!(syscall_error_msg(-22), "invalid argument (EINVAL)");
        assert_eq!(syscall_error_msg(-28), "no space left on device (ENOSPC)");
        assert_eq!(syscall_error_msg(-38), "function not implemented (ENOSYS)");
    }

    #[test]
    fn syscall_error_msg_unknown_returns_unknown_error() {
        assert_eq!(syscall_error_msg(-999), "unknown error");
        assert_eq!(syscall_error_msg(0), "unknown error");
    }

    // ---- normalize_fs_type -------------------------------------------------

    #[test]
    fn normalize_fs_type_known_names() {
        assert_eq!(normalize_fs_type("ext4"), "ext4");
        assert_eq!(normalize_fs_type("EXT4"), "ext4");
        assert_eq!(normalize_fs_type("Ext4"), "ext4");
    }

    #[test]
    fn normalize_fs_type_fat_aliases_to_fat32() {
        assert_eq!(normalize_fs_type("fat32"), "fat32");
        assert_eq!(normalize_fs_type("vfat"), "fat32");
        assert_eq!(normalize_fs_type("fat"), "fat32");
        assert_eq!(normalize_fs_type("VFAT"), "fat32");
    }

    #[test]
    fn normalize_fs_type_tmpfs() {
        assert_eq!(normalize_fs_type("tmpfs"), "tmpfs");
        assert_eq!(normalize_fs_type("TMPFS"), "tmpfs");
    }

    #[test]
    fn normalize_fs_type_unknown_returns_empty() {
        assert_eq!(normalize_fs_type("zfs"), "");
        assert_eq!(normalize_fs_type(""), "");
        assert_eq!(normalize_fs_type("ntfs"), "");
    }

    // ---- detect_type_from_argv0 --------------------------------------------

    #[test]
    fn detect_type_from_argv0_with_dot_suffix() {
        assert_eq!(detect_type_from_argv0("mkfs.ext4"), Some("ext4".to_string()));
        assert_eq!(detect_type_from_argv0("mkfs.fat32"), Some("fat32".to_string()));
        // Through alias normalization.
        assert_eq!(detect_type_from_argv0("mkfs.vfat"), Some("fat32".to_string()));
        assert_eq!(detect_type_from_argv0("mkfs.tmpfs"), Some("tmpfs".to_string()));
    }

    #[test]
    fn detect_type_from_argv0_strips_directory_prefix() {
        assert_eq!(
            detect_type_from_argv0("/usr/sbin/mkfs.ext4"),
            Some("ext4".to_string()),
        );
    }

    #[test]
    fn detect_type_from_argv0_plain_mkfs_returns_none() {
        // Bare "mkfs" with no suffix means caller must use -t.
        assert_eq!(detect_type_from_argv0("mkfs"), None);
        assert_eq!(detect_type_from_argv0("/bin/mkfs"), None);
    }

    #[test]
    fn detect_type_from_argv0_unknown_suffix_returns_none() {
        // mkfs.zfs isn't supported -> normalize_fs_type returns "" -> None.
        assert_eq!(detect_type_from_argv0("mkfs.zfs"), None);
    }

    #[test]
    fn detect_type_from_argv0_unrelated_program_returns_none() {
        assert_eq!(detect_type_from_argv0("ls"), None);
        assert_eq!(detect_type_from_argv0("fsck.ext4"), None);
    }

    // ---- fs_type_to_id -----------------------------------------------------

    #[test]
    fn fs_type_to_id_known_types() {
        assert_eq!(fs_type_to_id("ext4"), Some(FS_TYPE_EXT4));
        assert_eq!(fs_type_to_id("fat32"), Some(FS_TYPE_FAT32));
        assert_eq!(fs_type_to_id("tmpfs"), Some(FS_TYPE_TMPFS));
    }

    #[test]
    fn fs_type_to_id_unknown_returns_none() {
        assert_eq!(fs_type_to_id("zfs"), None);
        assert_eq!(fs_type_to_id(""), None);
    }

    // ---- normalize_device --------------------------------------------------

    #[test]
    fn normalize_device_strips_dev_prefix_for_sysfs_name() {
        let (path, name) = normalize_device("/dev/sda1");
        assert_eq!(path, "/dev/sda1");
        assert_eq!(name, "sda1");
    }

    #[test]
    fn normalize_device_bare_name_prepends_dev() {
        let (path, name) = normalize_device("sda1");
        assert_eq!(path, "/dev/sda1");
        assert_eq!(name, "sda1");
    }

    #[test]
    fn normalize_device_image_file_uses_basename() {
        let (path, name) = normalize_device("./images/disk.img");
        assert_eq!(path, "./images/disk.img");
        assert_eq!(name, "disk.img");
    }

    #[test]
    fn normalize_device_absolute_path_outside_dev() {
        let (path, name) = normalize_device("/tmp/loopback");
        // Has '/' prefix so treated as absolute; name is the basename.
        assert_eq!(path, "/tmp/loopback");
        assert_eq!(name, "loopback");
    }

    // ---- format_size -------------------------------------------------------

    #[test]
    fn format_size_below_kib_is_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kib_range() {
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(1024 + 512), "1.5 KiB");
    }

    #[test]
    fn format_size_mib_gib_tib() {
        assert_eq!(format_size(1024 * 1024), "1 MiB");
        assert_eq!(format_size(1024_u64.pow(3)), "1 GiB");
        assert_eq!(format_size(1024_u64.pow(4)), "1 TiB");
    }

    // ---- json_escape -------------------------------------------------------

    #[test]
    fn json_escape_basics() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape(r#"has "quotes""#), r#"has \"quotes\""#);
        assert_eq!(json_escape(r"a\b"), r"a\\b");
        assert_eq!(json_escape("a\nb\tc"), "a\\nb\\tc");
    }

    #[test]
    fn json_escape_control_chars_get_u_escaped() {
        assert_eq!(json_escape("\x01"), "\\u0001");
        assert_eq!(json_escape("\x1f"), "\\u001f");
    }

    // ---- auto_fat_cluster_size ---------------------------------------------

    #[test]
    fn auto_fat_cluster_size_zero_returns_safe_default() {
        // Size unknown -> default 8 sectors/cluster.
        assert_eq!(auto_fat_cluster_size(0, 512), 8);
    }

    #[test]
    fn auto_fat_cluster_size_small_devices_use_small_clusters() {
        // <= 64 MiB -> 1 spc.
        assert_eq!(auto_fat_cluster_size(32 * 1024 * 1024, 512), 1);
        // 64 < x <= 128 MiB -> 2.
        assert_eq!(auto_fat_cluster_size(100 * 1024 * 1024, 512), 2);
        // 128 < x <= 256 MiB -> 4.
        assert_eq!(auto_fat_cluster_size(200 * 1024 * 1024, 512), 4);
    }

    #[test]
    fn auto_fat_cluster_size_medium_device_uses_8_spc() {
        // 1 GiB -> 8 spc.
        assert_eq!(auto_fat_cluster_size(1024_u64.pow(3), 512), 8);
    }

    #[test]
    fn auto_fat_cluster_size_large_devices_increase_spc() {
        // 12 GiB -> 16 spc.
        assert_eq!(auto_fat_cluster_size(12 * 1024_u64.pow(3), 512), 16);
        // 24 GiB -> 32 spc.
        assert_eq!(auto_fat_cluster_size(24 * 1024_u64.pow(3), 512), 32);
        // 64 GiB -> 64 spc.
        assert_eq!(auto_fat_cluster_size(64 * 1024_u64.pow(3), 512), 64);
    }

    #[test]
    fn auto_fat_cluster_size_clamps_cluster_to_32k_max() {
        // With sector_size=4096 and spc=64 we'd get 256 KiB clusters,
        // way over the 32 KiB FAT limit; spc must be clamped to 8 (4096*8=32K).
        assert_eq!(auto_fat_cluster_size(64 * 1024_u64.pow(3), 4096), 8);
    }

    #[test]
    fn auto_fat_cluster_size_handles_huge_sector_size_floor_to_1() {
        // A 64 KiB sector can't host even one cluster within 32 KiB; the
        // function falls back to 1 to keep some progress.
        assert_eq!(auto_fat_cluster_size(64 * 1024_u64.pow(3), 65536), 1);
    }

    // ---- compute_summary ---------------------------------------------------

    fn opts_for(fs_type: &str) -> MkfsOptions {
        let mut o = MkfsOptions::new();
        o.fs_type = fs_type.to_string();
        o.device = "/tmp/test.img".to_string();
        o
    }

    #[test]
    fn compute_summary_ext4_picks_explicit_inodes_when_set() {
        let mut o = opts_for("ext4");
        o.ext4_num_inodes = 4096;
        // dev_name pointing to nothing -> total_size 0; we should still get
        // the explicit inode count since it doesn't depend on device size.
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.num_inodes, 4096);
        assert_eq!(s.block_size, 4096);  // default
        assert_eq!(s.fs_type, "ext4");
    }

    #[test]
    fn compute_summary_ext4_uses_journal_default_128_when_size_unknown() {
        let o = opts_for("ext4");
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.journal_size_mib, 128);
    }

    #[test]
    fn compute_summary_ext4_respects_user_journal_size() {
        let mut o = opts_for("ext4");
        o.ext4_journal_size_mib = 64;
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.journal_size_mib, 64);
    }

    #[test]
    fn compute_summary_ext4_copies_features_and_label() {
        let mut o = opts_for("ext4");
        o.ext4_features = "dir_index,extent".to_string();
        o.label = "rootfs".to_string();
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.features, "dir_index,extent");
        assert_eq!(s.label, "rootfs");
    }

    #[test]
    fn compute_summary_fat32_block_size_is_sector_times_spc() {
        let mut o = opts_for("fat32");
        o.fat_sector_size = 512;
        o.fat_sectors_per_cluster = 8;
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.sector_size, 512);
        assert_eq!(s.sectors_per_cluster, 8);
        assert_eq!(s.block_size, 4096);
        assert_eq!(s.fat_type, 32);
    }

    #[test]
    fn compute_summary_tmpfs_has_4k_block_no_inodes() {
        let o = opts_for("tmpfs");
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.fs_type, "tmpfs");
        assert_eq!(s.block_size, 4096);
        assert_eq!(s.num_inodes, 0);
        assert_eq!(s.journal_size_mib, 0);
    }

    #[test]
    fn compute_summary_unknown_type_zeroes_everything() {
        let o = opts_for("zfs"); // not in the match arms
        let s = compute_summary(&o, "nonexistent-device");
        assert_eq!(s.block_size, 0);
        assert_eq!(s.total_size, 0);
        assert_eq!(s.fs_type, "zfs"); // type is copied through
    }
}
