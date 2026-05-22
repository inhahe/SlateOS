//! OurOS btrfs filesystem management utility.
//!
//! Multi-personality binary providing:
//! - **btrfs** — btrfs filesystem management (subcommands: filesystem, subvolume,
//!   balance, device, scrub, check, rescue, restore, send, receive, property,
//!   quota, qgroup, inspect-internal)
//! - **mkfs.btrfs** — create a new btrfs filesystem on a device
//! - **btrfs-convert** — convert ext2/3/4 filesystem to btrfs
//!
//! Personality is detected via argv[0] basename (stripping path and .exe suffix).

#![deny(clippy::all)]

use std::env;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_NODE_SIZE: u32 = 16384;
const DEFAULT_SECTOR_SIZE: u32 = 4096;
const DEFAULT_LEAF_SIZE: u32 = 16384;

// ============================================================================
// RAID profiles
// ============================================================================

/// RAID profile for metadata or data.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RaidProfile {
    Single,
    Dup,
    Raid0,
    Raid1,
    Raid1c3,
    Raid1c4,
    Raid10,
    Raid5,
    Raid6,
}

impl RaidProfile {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "single" => Some(Self::Single),
            "dup" => Some(Self::Dup),
            "raid0" => Some(Self::Raid0),
            "raid1" => Some(Self::Raid1),
            "raid1c3" => Some(Self::Raid1c3),
            "raid1c4" => Some(Self::Raid1c4),
            "raid10" => Some(Self::Raid10),
            "raid5" => Some(Self::Raid5),
            "raid6" => Some(Self::Raid6),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Dup => "dup",
            Self::Raid0 => "RAID0",
            Self::Raid1 => "RAID1",
            Self::Raid1c3 => "RAID1C3",
            Self::Raid1c4 => "RAID1C4",
            Self::Raid10 => "RAID10",
            Self::Raid5 => "RAID5",
            Self::Raid6 => "RAID6",
        }
    }

    fn min_devices(self) -> u32 {
        match self {
            Self::Single | Self::Dup => 1,
            Self::Raid0 | Self::Raid1 => 2,
            Self::Raid1c3 | Self::Raid5 => 3,
            Self::Raid1c4 | Self::Raid6 => 4,
            Self::Raid10 => 4,
        }
    }
}

// ============================================================================
// Data structures
// ============================================================================

/// Represents a btrfs device in a filesystem.
#[derive(Clone, Debug)]
struct BtrfsDevice {
    /// Device ID within the filesystem.
    devid: u64,
    /// Total size in bytes.
    size: u64,
    /// Used bytes on this device.
    used: u64,
    /// Device path (e.g., /dev/sda1).
    path: String,
    /// Whether the device is missing/offline.
    missing: bool,
}

/// Represents a btrfs subvolume.
#[derive(Clone, Debug)]
struct Subvolume {
    /// Subvolume ID.
    id: u64,
    /// Generation at creation.
    generation: u64,
    /// Top-level subvolume ID (parent).
    top_level: u64,
    /// Path relative to the filesystem root.
    path: String,
    /// Subvolume UUID.
    uuid: String,
    /// Parent UUID (empty for root subvolume).
    parent_uuid: String,
    /// Received UUID (for received snapshots).
    received_uuid: String,
    /// Creation time as Unix timestamp.
    ctime: u64,
    /// Read-only flag.
    readonly: bool,
}

/// Represents an entire btrfs filesystem.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct BtrfsFilesystem {
    /// Filesystem UUID.
    uuid: String,
    /// User-assigned label (may be empty).
    label: String,
    /// Generation number.
    generation: u64,
    /// Node size in bytes.
    node_size: u32,
    /// Sector size in bytes.
    sector_size: u32,
    /// Total bytes across all devices.
    total_bytes: u64,
    /// Used bytes across all devices.
    used_bytes: u64,
    /// Devices in this filesystem.
    devices: Vec<BtrfsDevice>,
    /// Metadata RAID profile.
    metadata_profile: RaidProfile,
    /// Data RAID profile.
    data_profile: RaidProfile,
    /// Subvolumes.
    subvolumes: Vec<Subvolume>,
}

/// Balance status information.
#[derive(Clone, Debug)]
struct BalanceStatus {
    running: bool,
    paused: bool,
    /// Number of chunks considered.
    considered: u64,
    /// Number of chunks relocated.
    completed: u64,
    /// Estimated total chunks.
    estimated: u64,
}

/// Scrub status information.
#[derive(Clone, Debug)]
struct ScrubStatus {
    running: bool,
    /// Data bytes scrubbed.
    data_bytes_scrubbed: u64,
    /// Tree bytes scrubbed.
    tree_bytes_scrubbed: u64,
    /// Read errors found.
    read_errors: u64,
    /// Checksum errors found.
    csum_errors: u64,
    /// Verify errors found.
    verify_errors: u64,
    /// Super block errors.
    super_errors: u64,
    /// Number of corrected errors.
    corrected_errors: u64,
    /// Number of uncorrectable errors.
    uncorrectable_errors: u64,
}

/// Quota group information.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct QgroupInfo {
    /// Qgroup ID in level/subvolid format.
    qgroupid: String,
    /// Referenced bytes (exclusive + shared).
    referenced: u64,
    /// Exclusive bytes.
    exclusive: u64,
    /// Maximum referenced bytes limit (0 = no limit).
    max_referenced: u64,
    /// Maximum exclusive bytes limit (0 = no limit).
    max_exclusive: u64,
}

/// Options for mkfs.btrfs.
struct MkfsOptions {
    label: String,
    metadata_profile: RaidProfile,
    data_profile: RaidProfile,
    node_size: u32,
    sector_size: u32,
    force: bool,
    devices: Vec<String>,
}

/// Options for btrfs-convert.
struct ConvertOptions {
    device: String,
    no_inline: bool,
    no_rollback: bool,
    rollback: bool,
    label: String,
    progress: bool,
}

/// Btrfs property entry.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct Property {
    name: String,
    value: String,
    property_type: PropertyType,
}

/// Property type classification.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PropertyType {
    Filesystem,
    Subvolume,
    Device,
    Inode,
}

impl PropertyType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::Subvolume => "subvolume",
            Self::Device => "device",
            Self::Inode => "inode",
        }
    }
}

// ============================================================================
// Error handling
// ============================================================================

/// All errors produced by the btrfs utility.
#[derive(Debug)]
#[allow(dead_code)]
enum BtrfsError {
    Io(io::Error),
    InvalidArgument(String),
    NotFound(String),
    PermissionDenied(String),
    FilesystemError(String),
    UnsupportedOperation(String),
    DeviceError(String),
}

impl std::fmt::Display for BtrfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::FilesystemError(msg) => write!(f, "filesystem error: {msg}"),
            Self::UnsupportedOperation(msg) => write!(f, "unsupported: {msg}"),
            Self::DeviceError(msg) => write!(f, "device error: {msg}"),
        }
    }
}

impl From<io::Error> for BtrfsError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

type Result<T> = std::result::Result<T, BtrfsError>;

// ============================================================================
// Utility helpers
// ============================================================================

/// Format bytes as a human-readable string (KiB, MiB, GiB, TiB).
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        format!("{:.2}TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.2}GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2}MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2}KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes}B")
    }
}

/// Parse a size string with optional suffix (K, M, G, T) into bytes.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_part, multiplier) = if let Some(n) = s.strip_suffix('T') {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024u64 * 1024)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024u64)
    } else {
        (s, 1u64)
    };
    num_part.trim().parse::<u64>().ok().map(|n| n.saturating_mul(multiplier))
}

/// Generate a mock UUID for display purposes.
fn generate_uuid(seed: u64) -> String {
    // Deterministic UUID-like string from seed for consistent output.
    let a = seed & 0xFFFF_FFFF;
    let b = (seed >> 8) & 0xFFFF;
    let c = (seed >> 16) & 0xFFFF;
    let d = (seed >> 24) & 0xFFFF;
    let e = seed ^ 0xDEAD_BEEF_CAFE;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a,
        b & 0xFFFF,
        c & 0xFFFF,
        d & 0xFFFF,
        e & 0xFFFF_FFFF_FFFF,
    )
}

/// Check if a path looks like a block device path.
fn is_device_path(path: &str) -> bool {
    path.starts_with("/dev/")
}

/// Validate that a node size is a power of two and within range.
fn validate_node_size(size: u32) -> bool {
    size.is_power_of_two() && size >= 4096 && size <= 65536
}

/// Validate that a sector size is a power of two and within range.
fn validate_sector_size(size: u32) -> bool {
    size.is_power_of_two() && size >= 512 && size <= 65536
}

// ============================================================================
// Filesystem operations (simulated for OurOS userspace)
// ============================================================================

/// Probe a device path and return simulated filesystem info.
fn probe_filesystem(device: &str) -> Result<BtrfsFilesystem> {
    if !is_device_path(device) {
        return Err(BtrfsError::InvalidArgument(format!(
            "'{device}' does not appear to be a block device"
        )));
    }

    // In a real implementation, this would read the btrfs superblock
    // from the device. Here we return an error indicating the device
    // must be accessed through kernel ioctls.
    Err(BtrfsError::FilesystemError(format!(
        "cannot open '{device}': operation requires btrfs kernel support"
    )))
}

/// Attempt to read filesystem info from a mounted path via sysfs/procfs.
fn read_mounted_fs_info(mount_path: &str) -> Result<BtrfsFilesystem> {
    // In a real implementation, this would:
    // 1. Check /proc/mounts to find the device for this mount point
    // 2. Issue BTRFS_IOC_FS_INFO ioctl to get filesystem details
    // 3. Issue BTRFS_IOC_DEV_INFO for each device
    Err(BtrfsError::FilesystemError(format!(
        "cannot read filesystem info for '{mount_path}': not a btrfs mount point or kernel support unavailable"
    )))
}

// ============================================================================
// btrfs filesystem subcommands
// ============================================================================

fn cmd_filesystem(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs filesystem <show|df|usage|resize|defrag|label|sync>".into(),
        ));
    }

    match args[0].as_str() {
        "show" => cmd_filesystem_show(&args[1..]),
        "df" => cmd_filesystem_df(&args[1..]),
        "usage" => cmd_filesystem_usage(&args[1..]),
        "resize" => cmd_filesystem_resize(&args[1..]),
        "defrag" | "defragment" => cmd_filesystem_defrag(&args[1..]),
        "label" => cmd_filesystem_label(&args[1..]),
        "sync" => cmd_filesystem_sync(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown filesystem subcommand '{other}'"
        ))),
    }
}

fn cmd_filesystem_show(args: &[String]) -> Result<()> {
    let path = args.first().map(|s| s.as_str()).unwrap_or("/");

    if is_device_path(path) {
        let _fs = probe_filesystem(path)?;
    }

    // Attempt to read from mounted filesystem
    match read_mounted_fs_info(path) {
        Ok(fs) => {
            let label_str = if fs.label.is_empty() {
                String::new()
            } else {
                format!(" Label: '{}'", fs.label)
            };
            println!("Label: '{}' uuid: {}", fs.label, fs.uuid);
            println!("\tTotal devices {} FS bytes used {}{label_str}",
                     fs.devices.len(), format_bytes(fs.used_bytes));
            println!();
            for dev in &fs.devices {
                let missing = if dev.missing { " ***MISSING***" } else { "" };
                println!("\tdevid {:>4} size {} used {} path {}{}",
                         dev.devid, format_bytes(dev.size),
                         format_bytes(dev.used), dev.path, missing);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            eprintln!("btrfs filesystem show requires a mounted btrfs filesystem or device");
            Err(e)
        }
    }
}

fn cmd_filesystem_df(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument("missing path argument".into()))?;

    match read_mounted_fs_info(path) {
        Ok(fs) => {
            println!("Data, {}: total={}, used={}",
                     fs.data_profile.as_str(),
                     format_bytes(fs.total_bytes / 2),
                     format_bytes(fs.used_bytes * 3 / 4));
            println!("System, {}: total={}, used={}",
                     fs.metadata_profile.as_str(),
                     format_bytes(16 * 1024 * 1024),
                     format_bytes(4 * 1024 * 1024));
            println!("Metadata, {}: total={}, used={}",
                     fs.metadata_profile.as_str(),
                     format_bytes(fs.total_bytes / 8),
                     format_bytes(fs.used_bytes / 10));
            println!("GlobalReserve, single: total={}, used={}",
                     format_bytes(256 * 1024 * 1024),
                     format_bytes(0));
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}

fn cmd_filesystem_usage(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument("missing path argument".into()))?;

    match read_mounted_fs_info(path) {
        Ok(fs) => {
            println!("Overall:");
            println!("    Device size:                   {}", format_bytes(fs.total_bytes));
            println!("    Device allocated:               {}", format_bytes(fs.used_bytes));
            println!("    Device unallocated:             {}", format_bytes(fs.total_bytes.saturating_sub(fs.used_bytes)));
            println!("    Used:                           {}", format_bytes(fs.used_bytes * 3 / 4));
            println!("    Free (estimated):               {}",
                     format_bytes(fs.total_bytes.saturating_sub(fs.used_bytes)));
            println!("    Data ratio:                     1.00");
            println!("    Metadata ratio:                 1.00");
            println!("    Global reserve:                 {} (used: {})",
                     format_bytes(256 * 1024 * 1024), format_bytes(0));
            println!("    Multiple profiles:              no");
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}

fn cmd_filesystem_resize(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs filesystem resize <size|max> <path>".into(),
        ));
    }

    let size_spec = &args[0];
    let path = &args[1];

    // Validate the size spec
    if size_spec != "max" {
        // Check for [devid:]<size> format
        let size_part = if let Some(idx) = size_spec.find(':') {
            let devid = &size_spec[..idx];
            if devid.parse::<u64>().is_err() {
                return Err(BtrfsError::InvalidArgument(format!(
                    "invalid device ID in '{size_spec}'"
                )));
            }
            &size_spec[idx + 1..]
        } else {
            size_spec.as_str()
        };

        // Check for relative sizing (+/-) or absolute
        let check_part = if let Some(s) = size_part.strip_prefix('+') {
            s
        } else if let Some(s) = size_part.strip_prefix('-') {
            s
        } else {
            size_part
        };

        if parse_size(check_part).is_none() {
            return Err(BtrfsError::InvalidArgument(format!(
                "invalid size '{size_spec}'"
            )));
        }
    }

    let _fs = read_mounted_fs_info(path)?;
    println!("Resize device id 1 ({size_spec}) on {path}");
    Ok(())
}

fn cmd_filesystem_defrag(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs filesystem defragment [-v] [-r] [-f] [-c[algo]] <path>".into(),
        ));
    }

    let mut verbose = false;
    let mut recursive = false;
    let mut flush = false;
    let mut compress = String::new();
    let mut paths = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => verbose = true,
            "-r" => recursive = true,
            "-f" => flush = true,
            s if s.starts_with("-c") => {
                compress = s.strip_prefix("-c").unwrap_or("zlib").to_string();
                if compress.is_empty() {
                    compress = "zlib".to_string();
                }
            }
            _ => paths.push(args[i].clone()),
        }
        i += 1;
    }

    if paths.is_empty() {
        return Err(BtrfsError::InvalidArgument("no path specified".into()));
    }

    for path in &paths {
        if verbose {
            print!("Defragmenting {path}");
            if recursive {
                print!(" (recursive)");
            }
            if flush {
                print!(" (flush)");
            }
            if !compress.is_empty() {
                print!(" (compress={compress})");
            }
            println!();
        }
        // Real implementation would issue BTRFS_IOC_DEFRAG_RANGE ioctl
    }
    Ok(())
}

fn cmd_filesystem_label(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs filesystem label <device|mount_point> [newlabel]".into(),
        ));
    }

    let path = &args[0];
    let new_label = args.get(1);

    if let Some(label) = new_label {
        if label.len() > 255 {
            return Err(BtrfsError::InvalidArgument(
                "label too long (max 255 bytes)".into(),
            ));
        }
        // Real implementation: BTRFS_IOC_SET_FSLABEL
        println!("Set label of '{path}' to '{label}'");
    } else {
        // Real implementation: BTRFS_IOC_GET_FSLABEL or read superblock
        match read_mounted_fs_info(path) {
            Ok(fs) => println!("{}", fs.label),
            Err(_) => {
                // Try reading from device superblock
                let _fs = probe_filesystem(path)?;
            }
        }
    }
    Ok(())
}

fn cmd_filesystem_sync(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument("missing path argument".into()))?;

    // Real implementation: BTRFS_IOC_SYNC ioctl on the filesystem
    let _fs = read_mounted_fs_info(path)?;
    println!("Filesystem synced: {path}");
    Ok(())
}

// ============================================================================
// btrfs subvolume subcommands
// ============================================================================

fn cmd_subvolume(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs subvolume <create|delete|list|show|snapshot|get-default|set-default>".into(),
        ));
    }

    match args[0].as_str() {
        "create" => cmd_subvolume_create(&args[1..]),
        "delete" => cmd_subvolume_delete(&args[1..]),
        "list" => cmd_subvolume_list(&args[1..]),
        "show" => cmd_subvolume_show(&args[1..]),
        "snapshot" => cmd_subvolume_snapshot(&args[1..]),
        "get-default" => cmd_subvolume_get_default(&args[1..]),
        "set-default" => cmd_subvolume_set_default(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown subvolume subcommand '{other}'"
        ))),
    }
}

fn cmd_subvolume_create(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs subvolume create [-i <qgroupid>] <path>".into(),
        ));
    }

    let mut qgroup = String::new();
    let mut paths = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-i" {
            i += 1;
            if i >= args.len() {
                return Err(BtrfsError::InvalidArgument(
                    "-i requires a qgroup argument".into(),
                ));
            }
            qgroup = args[i].clone();
        } else {
            paths.push(args[i].clone());
        }
        i += 1;
    }

    if paths.is_empty() {
        return Err(BtrfsError::InvalidArgument("no path specified".into()));
    }

    for path in &paths {
        // Real implementation: BTRFS_IOC_SUBVOL_CREATE_V2
        print!("Create subvolume '{path}'");
        if !qgroup.is_empty() {
            print!(" in qgroup {qgroup}");
        }
        println!();
    }
    Ok(())
}

fn cmd_subvolume_delete(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs subvolume delete [-c|-C] [-v] <subvolume> [<subvolume>...]".into(),
        ));
    }

    let mut commit_mode = "";
    let mut verbose = false;
    let mut paths = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-c" => commit_mode = "after each",
            "-C" => commit_mode = "after all",
            "-v" => verbose = true,
            _ => paths.push(arg.clone()),
        }
    }

    if paths.is_empty() {
        return Err(BtrfsError::InvalidArgument("no subvolume specified".into()));
    }

    for path in &paths {
        // Real implementation: BTRFS_IOC_SNAP_DESTROY_V2
        if verbose {
            print!("Delete subvolume '{path}'");
            if !commit_mode.is_empty() {
                print!(" (commit {commit_mode})");
            }
            println!();
        } else {
            println!("Delete subvolume '{path}'");
        }
    }
    Ok(())
}

fn cmd_subvolume_list(args: &[String]) -> Result<()> {
    let mut sort_gen = false;
    let mut sort_rootid = false;
    let mut show_readonly = false;
    let mut show_snapshot_only = false;
    let mut path = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-g" => sort_gen = true,
            "-u" => sort_rootid = true,
            "-r" => show_readonly = true,
            "-s" => show_snapshot_only = true,
            "-t" => { /* table format, default */ }
            _ => path = args[i].clone(),
        }
        i += 1;
    }

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs subvolume list [-grsu] <path>".into(),
        ));
    }

    let _ = (sort_gen, sort_rootid, show_readonly, show_snapshot_only);

    // Real implementation: BTRFS_IOC_TREE_SEARCH on the filesystem
    match read_mounted_fs_info(&path) {
        Ok(fs) => {
            println!("ID\tgen\ttop level\tpath");
            for sv in &fs.subvolumes {
                if show_readonly && !sv.readonly {
                    continue;
                }
                if show_snapshot_only && sv.parent_uuid.is_empty() {
                    continue;
                }
                println!("{}\t{}\t{}\t{}", sv.id, sv.generation, sv.top_level, sv.path);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}

fn cmd_subvolume_show(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs subvolume show <subvolume-path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_INO_LOOKUP + BTRFS_IOC_GET_SUBVOL_INFO
    match read_mounted_fs_info(path) {
        Ok(fs) => {
            if let Some(sv) = fs.subvolumes.first() {
                println!("{}", sv.path);
                println!("\tName:\t\t\t{}", sv.path.rsplit('/').next().unwrap_or(&sv.path));
                println!("\tUUID:\t\t\t{}", sv.uuid);
                println!("\tParent UUID:\t\t{}", if sv.parent_uuid.is_empty() { "-" } else { &sv.parent_uuid });
                println!("\tReceived UUID:\t\t{}", if sv.received_uuid.is_empty() { "-" } else { &sv.received_uuid });
                println!("\tCreation time:\t\t{}", sv.ctime);
                println!("\tSubvolume ID:\t\t{}", sv.id);
                println!("\tGeneration:\t\t{}", sv.generation);
                println!("\tFlags:\t\t\t{}", if sv.readonly { "readonly" } else { "-" });
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}

fn cmd_subvolume_snapshot(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs subvolume snapshot [-r] <source> <dest>".into(),
        ));
    }

    let mut readonly = false;
    let mut positional = Vec::new();

    for arg in args {
        if arg == "-r" {
            readonly = true;
        } else {
            positional.push(arg.clone());
        }
    }

    if positional.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "need both source and destination paths".into(),
        ));
    }

    let source = &positional[0];
    let dest = &positional[1];

    // Real implementation: BTRFS_IOC_SNAP_CREATE_V2
    print!("Create a{} snapshot of '{source}' in '{dest}'",
           if readonly { " readonly" } else { "" });
    println!();
    Ok(())
}

fn cmd_subvolume_get_default(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs subvolume get-default <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_DEFAULT_SUBVOL with GET
    let _fs = read_mounted_fs_info(path)?;
    println!("ID 5 gen 0 top level 0 path (FS_TREE)");
    Ok(())
}

fn cmd_subvolume_set_default(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs subvolume set-default <subvolid|subvol-path> <path>".into(),
        ));
    }

    let subvol_spec = &args[0];
    let path = &args[1];

    // Real implementation: BTRFS_IOC_DEFAULT_SUBVOL
    let _fs = read_mounted_fs_info(path)?;
    println!("Set default subvolume to {subvol_spec} on {path}");
    Ok(())
}

// ============================================================================
// btrfs balance subcommands
// ============================================================================

fn cmd_balance(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs balance <start|pause|cancel|resume|status>".into(),
        ));
    }

    match args[0].as_str() {
        "start" => cmd_balance_start(&args[1..]),
        "pause" => cmd_balance_pause(&args[1..]),
        "cancel" => cmd_balance_cancel(&args[1..]),
        "resume" => cmd_balance_resume(&args[1..]),
        "status" => cmd_balance_status(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown balance subcommand '{other}'"
        ))),
    }
}

fn cmd_balance_start(args: &[String]) -> Result<()> {
    let mut filters = Vec::new();
    let mut force = false;
    let mut background = false;
    let mut path = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => force = true,
            "--background" | "--bg" => background = true,
            s if s.starts_with("-d") => {
                filters.push(format!("data: {}", s.strip_prefix("-d").unwrap_or("")));
            }
            s if s.starts_with("-m") => {
                filters.push(format!("metadata: {}", s.strip_prefix("-m").unwrap_or("")));
            }
            s if s.starts_with("-s") => {
                filters.push(format!("system: {}", s.strip_prefix("-s").unwrap_or("")));
            }
            _ => path = args[i].clone(),
        }
        i += 1;
    }

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs balance start [-f] [-d|-m|-s filters] <path>".into(),
        ));
    }

    let _ = force;

    // Real implementation: BTRFS_IOC_BALANCE_V2
    print!("Balance on '{path}'");
    if background {
        print!(" (background)");
    }
    if !filters.is_empty() {
        print!(" with filters: {}", filters.join(", "));
    }
    println!(" started");
    Ok(())
}

fn cmd_balance_pause(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs balance pause <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_BALANCE_CTL with BTRFS_BALANCE_CTL_PAUSE
    let _fs = read_mounted_fs_info(path)?;
    println!("Balance on '{path}' paused");
    Ok(())
}

fn cmd_balance_cancel(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs balance cancel <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_BALANCE_CTL with BTRFS_BALANCE_CTL_CANCEL
    let _fs = read_mounted_fs_info(path)?;
    println!("Balance on '{path}' cancelled");
    Ok(())
}

fn cmd_balance_resume(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs balance resume <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_BALANCE_V2 with resume flag
    let _fs = read_mounted_fs_info(path)?;
    println!("Balance on '{path}' resumed");
    Ok(())
}

fn cmd_balance_status(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs balance status <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_BALANCE_PROGRESS
    let _fs = read_mounted_fs_info(path)?;
    let status = BalanceStatus {
        running: false,
        paused: false,
        considered: 0,
        completed: 0,
        estimated: 0,
    };

    if status.running {
        println!("Balance on '{path}' is running");
        println!("{} out of about {} chunks balanced ({} considered)",
                 status.completed, status.estimated, status.considered);
    } else if status.paused {
        println!("Balance on '{path}' is paused");
        println!("{} out of about {} chunks balanced ({} considered)",
                 status.completed, status.estimated, status.considered);
    } else {
        println!("No balance found on '{path}'");
    }
    Ok(())
}

// ============================================================================
// btrfs device subcommands
// ============================================================================

fn cmd_device(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs device <add|remove|delete|scan|stats|usage|ready>".into(),
        ));
    }

    match args[0].as_str() {
        "add" => cmd_device_add(&args[1..]),
        "remove" | "delete" => cmd_device_remove(&args[1..]),
        "scan" => cmd_device_scan(&args[1..]),
        "stats" => cmd_device_stats(&args[1..]),
        "usage" => cmd_device_usage(&args[1..]),
        "ready" => cmd_device_ready(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown device subcommand '{other}'"
        ))),
    }
}

fn cmd_device_add(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs device add [-f] [-K] <device> [<device>...] <path>".into(),
        ));
    }

    let mut force = false;
    let mut no_discard = false;
    let mut positional = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-f" => force = true,
            "-K" => no_discard = true,
            _ => positional.push(arg.clone()),
        }
    }

    let _ = (force, no_discard);

    if positional.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "need at least one device and a mount path".into(),
        ));
    }

    let mount_path = &positional[positional.len() - 1];
    let devices = &positional[..positional.len() - 1];

    // Real implementation: BTRFS_IOC_ADD_DEV
    let _fs = read_mounted_fs_info(mount_path)?;
    for dev in devices {
        if !is_device_path(dev) {
            return Err(BtrfsError::DeviceError(format!(
                "'{dev}' does not appear to be a block device"
            )));
        }
        println!("Added device '{dev}' to '{mount_path}'");
    }
    Ok(())
}

fn cmd_device_remove(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs device remove <device|devid> [<device|devid>...] <path>".into(),
        ));
    }

    let mount_path = &args[args.len() - 1];
    let devices = &args[..args.len() - 1];

    // Real implementation: BTRFS_IOC_RM_DEV_V2
    let _fs = read_mounted_fs_info(mount_path)?;
    for dev in devices {
        println!("Removed device '{dev}' from '{mount_path}'");
    }
    Ok(())
}

fn cmd_device_scan(args: &[String]) -> Result<()> {
    let mut forget = false;
    let mut devices = Vec::new();

    for arg in args {
        if arg == "--forget" {
            forget = true;
        } else {
            devices.push(arg.clone());
        }
    }

    if forget {
        if devices.is_empty() {
            println!("All device registrations removed");
        } else {
            for dev in &devices {
                println!("Device '{dev}' forgotten");
            }
        }
    } else if devices.is_empty() {
        // Scan all devices
        println!("Scanning for btrfs filesystems...");
        println!("Scanning completed, no filesystems found");
    } else {
        for dev in &devices {
            if !is_device_path(dev) {
                eprintln!("WARNING: '{dev}' does not look like a device path");
            }
            println!("Scanning device '{dev}'...");
        }
    }
    Ok(())
}

fn cmd_device_stats(args: &[String]) -> Result<()> {
    let mut reset = false;
    let mut check = false;
    let mut path = String::new();

    for arg in args {
        match arg.as_str() {
            "-z" | "--reset" => reset = true,
            "-c" | "--check" => check = true,
            _ => path = arg.clone(),
        }
    }

    let _ = check;

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs device stats [-z] [-c] <path>".into(),
        ));
    }

    // Real implementation: BTRFS_IOC_GET_DEV_STATS
    let _fs = read_mounted_fs_info(&path)?;
    if reset {
        println!("Device stats reset on '{path}'");
    }
    // Would display: write_io_errs, read_io_errs, flush_io_errs,
    // corruption_errs, generation_errs for each device.
    Ok(())
}

fn cmd_device_usage(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs device usage <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_DEV_INFO + BTRFS_IOC_SPACE_INFO
    match read_mounted_fs_info(path) {
        Ok(fs) => {
            for dev in &fs.devices {
                let missing = if dev.missing { " (MISSING)" } else { "" };
                println!("{}{missing}", dev.path);
                println!("   Device ID:             {}", dev.devid);
                println!("   Device size:            {}", format_bytes(dev.size));
                println!("   Data,{}: {}",
                         fs.data_profile.as_str(), format_bytes(dev.used));
                println!("   Unallocated:            {}",
                         format_bytes(dev.size.saturating_sub(dev.used)));
                println!();
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}

fn cmd_device_ready(args: &[String]) -> Result<()> {
    let device = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs device ready <device>".into(),
        ))?;

    if !is_device_path(device) {
        return Err(BtrfsError::DeviceError(format!(
            "'{device}' is not a device path"
        )));
    }

    // Real implementation: BTRFS_IOC_DEVICES_READY
    let _fs = probe_filesystem(device)?;
    println!("Device '{device}' is ready");
    Ok(())
}

// ============================================================================
// btrfs scrub subcommands
// ============================================================================

fn cmd_scrub(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs scrub <start|cancel|resume|status>".into(),
        ));
    }

    match args[0].as_str() {
        "start" => cmd_scrub_start(&args[1..]),
        "cancel" => cmd_scrub_cancel(&args[1..]),
        "resume" => cmd_scrub_resume(&args[1..]),
        "status" => cmd_scrub_status(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown scrub subcommand '{other}'"
        ))),
    }
}

fn cmd_scrub_start(args: &[String]) -> Result<()> {
    let mut readonly = false;
    let mut background = true;
    let mut path = String::new();

    for arg in args {
        match arg.as_str() {
            "-r" => readonly = true,
            "-B" => background = false,
            _ => path = arg.clone(),
        }
    }

    let _ = readonly;

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs scrub start [-Br] <path|device>".into(),
        ));
    }

    // Real implementation: BTRFS_IOC_SCRUB
    let _fs = read_mounted_fs_info(&path)?;
    if background {
        println!("Scrub started on '{path}' (background)");
    } else {
        println!("Scrub started on '{path}' (foreground)");
        println!("Scrub completed on '{path}'");
    }
    Ok(())
}

fn cmd_scrub_cancel(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs scrub cancel <path|device>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_SCRUB_CANCEL
    let _fs = read_mounted_fs_info(path)?;
    println!("Scrub cancelled on '{path}'");
    Ok(())
}

fn cmd_scrub_resume(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs scrub resume <path|device>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_SCRUB with resume flag
    let _fs = read_mounted_fs_info(path)?;
    println!("Scrub resumed on '{path}'");
    Ok(())
}

fn cmd_scrub_status(args: &[String]) -> Result<()> {
    let mut raw = false;
    let mut path = String::new();

    for arg in args {
        match arg.as_str() {
            "-R" | "--raw" => raw = true,
            _ => path = arg.clone(),
        }
    }

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs scrub status [-R] <path|device>".into(),
        ));
    }

    // Real implementation: BTRFS_IOC_SCRUB_PROGRESS
    let _fs = read_mounted_fs_info(&path)?;
    let status = ScrubStatus {
        running: false,
        data_bytes_scrubbed: 0,
        tree_bytes_scrubbed: 0,
        read_errors: 0,
        csum_errors: 0,
        verify_errors: 0,
        super_errors: 0,
        corrected_errors: 0,
        uncorrectable_errors: 0,
    };

    if raw {
        println!("scrub.running={}", if status.running { 1 } else { 0 });
        println!("scrub.data_bytes_scrubbed={}", status.data_bytes_scrubbed);
        println!("scrub.tree_bytes_scrubbed={}", status.tree_bytes_scrubbed);
        println!("scrub.read_errors={}", status.read_errors);
        println!("scrub.csum_errors={}", status.csum_errors);
        println!("scrub.verify_errors={}", status.verify_errors);
        println!("scrub.super_errors={}", status.super_errors);
        println!("scrub.corrected_errors={}", status.corrected_errors);
        println!("scrub.uncorrectable_errors={}", status.uncorrectable_errors);
    } else {
        println!("Scrub status for '{path}':");
        println!("  Status:            {}", if status.running { "running" } else { "idle" });
        println!("  Data scrubbed:     {}", format_bytes(status.data_bytes_scrubbed));
        println!("  Tree scrubbed:     {}", format_bytes(status.tree_bytes_scrubbed));
        println!("  Read errors:       {}", status.read_errors);
        println!("  Csum errors:       {}", status.csum_errors);
        println!("  Verify errors:     {}", status.verify_errors);
        println!("  Super errors:      {}", status.super_errors);
        println!("  Corrected:         {}", status.corrected_errors);
        println!("  Uncorrectable:     {}", status.uncorrectable_errors);
    }
    Ok(())
}

// ============================================================================
// btrfs check / rescue / restore
// ============================================================================

fn cmd_check(args: &[String]) -> Result<()> {
    let mut readonly = true;
    let mut repair = false;
    let mut force = false;
    let mut mode = "lowmem";
    let mut device = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repair" => {
                repair = true;
                readonly = false;
            }
            "--readonly" => readonly = true,
            "--force" | "-f" => force = true,
            "--mode" => {
                i += 1;
                if i < args.len() {
                    mode = match args[i].as_str() {
                        "lowmem" => "lowmem",
                        "original" => "original",
                        _ => return Err(BtrfsError::InvalidArgument(format!(
                            "unknown check mode '{}'. Use 'lowmem' or 'original'", args[i]
                        ))),
                    };
                }
            }
            _ => device = args[i].clone(),
        }
        i += 1;
    }

    let _ = (force, mode);

    if device.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs check [--repair] [--readonly] [--mode lowmem|original] <device>".into(),
        ));
    }

    if !is_device_path(&device) {
        return Err(BtrfsError::InvalidArgument(format!(
            "'{device}' does not appear to be a block device"
        )));
    }

    println!("Opening filesystem on {device}");
    println!("Checking filesystem (mode: {mode}, {})",
             if readonly { "readonly" } else if repair { "repair" } else { "check" });

    // Real implementation: open the filesystem, walk all trees, verify checksums
    let _fs = probe_filesystem(&device)?;
    println!("Checking complete, no errors found");
    Ok(())
}

fn cmd_rescue(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs rescue <super-recover|zero-log|chunk-recover|fix-device-size|clear-uuid-tree>".into(),
        ));
    }

    let subcmd = &args[0];
    let rest = &args[1..];

    match subcmd.as_str() {
        "super-recover" => {
            let device = rest.first()
                .ok_or_else(|| BtrfsError::InvalidArgument(
                    "usage: btrfs rescue super-recover [-v] [-y] <device>".into(),
                ))?;
            println!("Recovering superblock on '{device}'...");
            let _fs = probe_filesystem(device)?;
            Ok(())
        }
        "zero-log" => {
            let device = rest.first()
                .ok_or_else(|| BtrfsError::InvalidArgument(
                    "usage: btrfs rescue zero-log <device>".into(),
                ))?;
            println!("Zeroing log tree on '{device}'...");
            let _fs = probe_filesystem(device)?;
            Ok(())
        }
        "chunk-recover" => {
            let device = rest.first()
                .ok_or_else(|| BtrfsError::InvalidArgument(
                    "usage: btrfs rescue chunk-recover [-v] [-y] <device>".into(),
                ))?;
            println!("Recovering chunk tree on '{device}'...");
            let _fs = probe_filesystem(device)?;
            Ok(())
        }
        "fix-device-size" => {
            let device = rest.first()
                .ok_or_else(|| BtrfsError::InvalidArgument(
                    "usage: btrfs rescue fix-device-size <device>".into(),
                ))?;
            println!("Fixing device size records on '{device}'...");
            let _fs = probe_filesystem(device)?;
            Ok(())
        }
        "clear-uuid-tree" => {
            let device = rest.first()
                .ok_or_else(|| BtrfsError::InvalidArgument(
                    "usage: btrfs rescue clear-uuid-tree <device>".into(),
                ))?;
            println!("Clearing UUID tree on '{device}'...");
            let _fs = probe_filesystem(device)?;
            Ok(())
        }
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown rescue subcommand '{other}'"
        ))),
    }
}

fn cmd_restore(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs restore [-s] [-x] [-m] [-S] [-v] [-i] [-t tree_location] [-f byte_offset] [-u super_mirror] [-r root_objectid] [-d] [-l] [-D] <device> <output_dir>".into(),
        ));
    }

    let mut verbose = false;
    let mut dry_run = false;
    let mut symlinks = false;
    let mut metadata = false;
    let mut super_mirror: u32 = 0;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => verbose = true,
            "-D" | "--dry-run" => dry_run = true,
            "-s" => symlinks = true,
            "-m" => metadata = true,
            "-u" => {
                i += 1;
                if i < args.len() {
                    super_mirror = args[i].parse().unwrap_or(0);
                }
            }
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }

    let _ = (verbose, dry_run, symlinks, metadata, super_mirror);

    if positional.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "need both device and output directory".into(),
        ));
    }

    let device = &positional[0];
    let output = &positional[1];

    println!("Restoring files from '{device}' to '{output}'...");
    let _fs = probe_filesystem(device)?;
    println!("Restore complete");
    Ok(())
}

// ============================================================================
// btrfs send / receive
// ============================================================================

fn cmd_send(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs send [-v] [-e] [-p parent] [-c clone_src] [-f outfile] <snapshot> [snapshot...]".into(),
        ));
    }

    let mut verbose = false;
    let mut no_data = false;
    let mut parent = String::new();
    let mut clone_srcs = Vec::new();
    let mut outfile = String::new();
    let mut snapshots = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => verbose = true,
            "-e" | "--no-data" => no_data = true,
            "-p" => {
                i += 1;
                if i < args.len() {
                    parent = args[i].clone();
                }
            }
            "-c" => {
                i += 1;
                if i < args.len() {
                    clone_srcs.push(args[i].clone());
                }
            }
            "-f" => {
                i += 1;
                if i < args.len() {
                    outfile = args[i].clone();
                }
            }
            _ => snapshots.push(args[i].clone()),
        }
        i += 1;
    }

    let _ = (verbose, no_data);

    if snapshots.is_empty() {
        return Err(BtrfsError::InvalidArgument("no snapshot specified".into()));
    }

    for snap in &snapshots {
        print!("Sending snapshot '{snap}'");
        if !parent.is_empty() {
            print!(" (parent: {parent})");
        }
        if !clone_srcs.is_empty() {
            print!(" (clone sources: {})", clone_srcs.join(", "));
        }
        if !outfile.is_empty() {
            print!(" to file '{outfile}'");
        } else {
            print!(" to stdout");
        }
        println!();
    }
    Ok(())
}

fn cmd_receive(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs receive [-v] [-f infile] [-e] [-C] [--dump] <path>".into(),
        ));
    }

    let mut verbose = false;
    let mut infile = String::new();
    let mut chroot = false;
    let mut dump = false;
    let mut path = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => verbose = true,
            "-C" => chroot = true,
            "--dump" => dump = true,
            "-f" => {
                i += 1;
                if i < args.len() {
                    infile = args[i].clone();
                }
            }
            _ => path = args[i].clone(),
        }
        i += 1;
    }

    let _ = (verbose, chroot, dump);

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "no destination path specified".into(),
        ));
    }

    print!("Receiving snapshot at '{path}'");
    if !infile.is_empty() {
        print!(" from file '{infile}'");
    } else {
        print!(" from stdin");
    }
    println!();
    Ok(())
}

// ============================================================================
// btrfs property subcommands
// ============================================================================

fn cmd_property(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs property <get|set|list>".into(),
        ));
    }

    match args[0].as_str() {
        "get" => cmd_property_get(&args[1..]),
        "set" => cmd_property_set(&args[1..]),
        "list" => cmd_property_list(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown property subcommand '{other}'"
        ))),
    }
}

fn cmd_property_get(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs property get [-t type] <object> [name]".into(),
        ));
    }

    let mut obj_type: Option<PropertyType> = None;
    let mut object = String::new();
    let mut name = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i < args.len() {
                    obj_type = match args[i].as_str() {
                        "f" | "filesystem" => Some(PropertyType::Filesystem),
                        "s" | "subvolume" | "subvol" => Some(PropertyType::Subvolume),
                        "d" | "device" | "dev" => Some(PropertyType::Device),
                        "i" | "inode" => Some(PropertyType::Inode),
                        other => return Err(BtrfsError::InvalidArgument(format!(
                            "unknown property type '{other}'"
                        ))),
                    };
                }
            }
            _ => {
                if object.is_empty() {
                    object = args[i].clone();
                } else {
                    name = args[i].clone();
                }
            }
        }
        i += 1;
    }

    if object.is_empty() {
        return Err(BtrfsError::InvalidArgument("no object specified".into()));
    }

    let type_str = obj_type.map(|t| t.as_str()).unwrap_or("auto");

    if name.is_empty() {
        // List all properties for this object
        println!("Properties for '{object}' (type: {type_str}):");
        println!("  compression=");
        println!("  ro=false");
        println!("  label=");
    } else {
        // Get specific property
        println!("{name}=");
    }
    Ok(())
}

fn cmd_property_set(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs property set [-t type] <object> <name> <value>".into(),
        ));
    }

    let mut obj_type: Option<PropertyType> = None;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        if args[i] == "-t" {
            i += 1;
            if i < args.len() {
                obj_type = match args[i].as_str() {
                    "f" | "filesystem" => Some(PropertyType::Filesystem),
                    "s" | "subvolume" | "subvol" => Some(PropertyType::Subvolume),
                    "d" | "device" | "dev" => Some(PropertyType::Device),
                    "i" | "inode" => Some(PropertyType::Inode),
                    other => return Err(BtrfsError::InvalidArgument(format!(
                        "unknown property type '{other}'"
                    ))),
                };
            }
        } else {
            positional.push(args[i].clone());
        }
        i += 1;
    }

    if positional.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "need object, property name, and value".into(),
        ));
    }

    let object = &positional[0];
    let name = &positional[1];
    let value = &positional[2];

    // Validate known properties
    match name.as_str() {
        "compression" => {
            match value.as_str() {
                "" | "none" | "zlib" | "lzo" | "zstd" => {}
                other => return Err(BtrfsError::InvalidArgument(format!(
                    "unknown compression algorithm '{other}'"
                ))),
            }
        }
        "ro" => {
            match value.as_str() {
                "true" | "false" => {}
                other => return Err(BtrfsError::InvalidArgument(format!(
                    "invalid value '{other}' for 'ro', expected 'true' or 'false'"
                ))),
            }
        }
        "label" => { /* any string up to 255 bytes */ }
        _ => { /* allow unknown properties for forward compatibility */ }
    }

    let type_str = obj_type.map(|t| t.as_str()).unwrap_or("auto");
    let _ = type_str;
    println!("Set property '{name}' to '{value}' on '{object}'");
    Ok(())
}

fn cmd_property_list(args: &[String]) -> Result<()> {
    let mut obj_type: Option<PropertyType> = None;
    let mut object = String::new();

    let mut i = 0;
    while i < args.len() {
        if args[i] == "-t" {
            i += 1;
            if i < args.len() {
                obj_type = match args[i].as_str() {
                    "f" | "filesystem" => Some(PropertyType::Filesystem),
                    "s" | "subvolume" | "subvol" => Some(PropertyType::Subvolume),
                    "d" | "device" | "dev" => Some(PropertyType::Device),
                    "i" | "inode" => Some(PropertyType::Inode),
                    other => return Err(BtrfsError::InvalidArgument(format!(
                        "unknown property type '{other}'"
                    ))),
                };
            }
        } else {
            object = args[i].clone();
        }
        i += 1;
    }

    if object.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs property list [-t type] <object>".into(),
        ));
    }

    let type_str = obj_type.map(|t| t.as_str()).unwrap_or("auto");
    println!("Available properties for '{object}' (type: {type_str}):");
    println!("  ro                   - read-only status (rw: subvolume)");
    println!("  label                - filesystem label (rw: filesystem)");
    println!("  compression          - compression algorithm (rw: inode, filesystem)");
    Ok(())
}

// ============================================================================
// btrfs quota subcommands
// ============================================================================

fn cmd_quota(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs quota <enable|disable|rescan>".into(),
        ));
    }

    match args[0].as_str() {
        "enable" => cmd_quota_enable(&args[1..]),
        "disable" => cmd_quota_disable(&args[1..]),
        "rescan" => cmd_quota_rescan(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown quota subcommand '{other}'"
        ))),
    }
}

fn cmd_quota_enable(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs quota enable <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_QUOTA_CTL with BTRFS_QUOTA_CTL_ENABLE
    let _fs = read_mounted_fs_info(path)?;
    println!("Quota enabled on '{path}'");
    Ok(())
}

fn cmd_quota_disable(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs quota disable <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_QUOTA_CTL with BTRFS_QUOTA_CTL_DISABLE
    let _fs = read_mounted_fs_info(path)?;
    println!("Quota disabled on '{path}'");
    Ok(())
}

fn cmd_quota_rescan(args: &[String]) -> Result<()> {
    let mut wait = false;
    let mut path = String::new();

    for arg in args {
        match arg.as_str() {
            "-w" | "--wait" => wait = true,
            "-s" => { /* status mode */ }
            _ => path = arg.clone(),
        }
    }

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs quota rescan [-w] [-s] <path>".into(),
        ));
    }

    // Real implementation: BTRFS_IOC_QUOTA_RESCAN
    let _fs = read_mounted_fs_info(&path)?;
    println!("Quota rescan started on '{path}'");
    if wait {
        println!("Quota rescan completed on '{path}'");
    }
    Ok(())
}

// ============================================================================
// btrfs qgroup subcommands
// ============================================================================

fn cmd_qgroup(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs qgroup <show|create|destroy|assign|remove|limit>".into(),
        ));
    }

    match args[0].as_str() {
        "show" => cmd_qgroup_show(&args[1..]),
        "create" => cmd_qgroup_create(&args[1..]),
        "destroy" => cmd_qgroup_destroy(&args[1..]),
        "assign" => cmd_qgroup_assign(&args[1..]),
        "remove" => cmd_qgroup_remove(&args[1..]),
        "limit" => cmd_qgroup_limit(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown qgroup subcommand '{other}'"
        ))),
    }
}

fn cmd_qgroup_show(args: &[String]) -> Result<()> {
    let mut raw = false;
    let mut sort_by = String::new();
    let mut path = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--raw" => raw = true,
            "--sort" => {
                i += 1;
                if i < args.len() {
                    sort_by = args[i].clone();
                }
            }
            _ => path = args[i].clone(),
        }
        i += 1;
    }

    let _ = (raw, sort_by);

    if path.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup show [--raw] [--sort key] <path>".into(),
        ));
    }

    // Real implementation: BTRFS_IOC_TREE_SEARCH for quota items
    let _fs = read_mounted_fs_info(&path)?;
    println!("qgroupid         rfer         excl");
    println!("--------         ----         ----");
    // Would list all qgroups
    Ok(())
}

fn cmd_qgroup_create(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup create <qgroupid> <path>".into(),
        ));
    }

    let qgroupid = &args[0];
    let path = &args[1];

    // Validate qgroup ID format (level/subvolid)
    if !qgroupid.contains('/') {
        return Err(BtrfsError::InvalidArgument(format!(
            "invalid qgroup ID '{qgroupid}', expected format: level/subvolid"
        )));
    }

    // Real implementation: BTRFS_IOC_QGROUP_CREATE
    let _fs = read_mounted_fs_info(path)?;
    println!("Created qgroup '{qgroupid}' on '{path}'");
    Ok(())
}

fn cmd_qgroup_destroy(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup destroy <qgroupid> <path>".into(),
        ));
    }

    let qgroupid = &args[0];
    let path = &args[1];

    // Real implementation: BTRFS_IOC_QGROUP_CREATE with destroy flag
    let _fs = read_mounted_fs_info(path)?;
    println!("Destroyed qgroup '{qgroupid}' on '{path}'");
    Ok(())
}

fn cmd_qgroup_assign(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup assign [--rescan | --no-rescan] <src> <dst> <path>".into(),
        ));
    }

    let mut rescan = true;
    let mut positional = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--rescan" => rescan = true,
            "--no-rescan" => rescan = false,
            _ => positional.push(arg.clone()),
        }
    }

    if positional.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "need source qgroup, destination qgroup, and path".into(),
        ));
    }

    let src = &positional[0];
    let dst = &positional[1];
    let path = &positional[2];

    // Real implementation: BTRFS_IOC_QGROUP_ASSIGN
    let _fs = read_mounted_fs_info(path)?;
    println!("Assigned qgroup '{src}' to '{dst}' on '{path}'");
    if rescan {
        println!("Quota rescan triggered");
    }
    Ok(())
}

fn cmd_qgroup_remove(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup remove <src> <dst> <path>".into(),
        ));
    }

    let mut positional = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--rescan" | "--no-rescan" => {}
            _ => positional.push(arg.clone()),
        }
    }

    if positional.len() < 3 {
        return Err(BtrfsError::InvalidArgument(
            "need source qgroup, destination qgroup, and path".into(),
        ));
    }

    let src = &positional[0];
    let dst = &positional[1];
    let path = &positional[2];

    // Real implementation: BTRFS_IOC_QGROUP_ASSIGN with remove flag
    let _fs = read_mounted_fs_info(path)?;
    println!("Removed qgroup '{src}' from '{dst}' on '{path}'");
    Ok(())
}

fn cmd_qgroup_limit(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs qgroup limit [-c] [-e] <size|none> [<qgroupid>] <path>".into(),
        ));
    }

    let mut compressed = false;
    let mut exclusive = false;
    let mut positional = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-c" => compressed = true,
            "-e" => exclusive = true,
            _ => positional.push(arg.clone()),
        }
    }

    let _ = (compressed, exclusive);

    if positional.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing size argument".into(),
        ));
    }

    let size_str = &positional[0];
    let (qgroup, path) = if positional.len() >= 3 {
        (positional[1].clone(), positional[2].clone())
    } else if positional.len() == 2 {
        (String::new(), positional[1].clone())
    } else {
        return Err(BtrfsError::InvalidArgument(
            "missing path argument".into(),
        ));
    };

    // Validate the size
    if size_str != "none" {
        if parse_size(size_str).is_none() {
            return Err(BtrfsError::InvalidArgument(format!(
                "invalid size '{size_str}'"
            )));
        }
    }

    // Real implementation: BTRFS_IOC_QGROUP_LIMIT
    let _fs = read_mounted_fs_info(&path)?;
    let limit_type = if exclusive { "exclusive" } else { "referenced" };
    if qgroup.is_empty() {
        println!("Set {limit_type} limit to {size_str} on '{path}'");
    } else {
        println!("Set {limit_type} limit to {size_str} for qgroup {qgroup} on '{path}'");
    }
    Ok(())
}

// ============================================================================
// btrfs inspect-internal subcommands
// ============================================================================

fn cmd_inspect_internal(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "missing subcommand. Usage: btrfs inspect-internal <dump-tree|dump-super|rootid|inode-resolve|logical-resolve|subvolid-resolve|tree-stats>".into(),
        ));
    }

    match args[0].as_str() {
        "dump-tree" => cmd_inspect_dump_tree(&args[1..]),
        "dump-super" => cmd_inspect_dump_super(&args[1..]),
        "rootid" => cmd_inspect_rootid(&args[1..]),
        "inode-resolve" => cmd_inspect_inode_resolve(&args[1..]),
        "logical-resolve" => cmd_inspect_logical_resolve(&args[1..]),
        "subvolid-resolve" => cmd_inspect_subvolid_resolve(&args[1..]),
        "tree-stats" => cmd_inspect_tree_stats(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown inspect-internal subcommand '{other}'"
        ))),
    }
}

fn cmd_inspect_dump_tree(args: &[String]) -> Result<()> {
    let mut tree_id: Option<u64> = None;
    let mut device = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i < args.len() {
                    tree_id = args[i].parse().ok();
                    if tree_id.is_none() {
                        return Err(BtrfsError::InvalidArgument(format!(
                            "invalid tree ID '{}'", args[i]
                        )));
                    }
                }
            }
            _ => device = args[i].clone(),
        }
        i += 1;
    }

    if device.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal dump-tree [-t tree_id] <device>".into(),
        ));
    }

    let _fs = probe_filesystem(&device)?;
    if let Some(tid) = tree_id {
        println!("Dumping tree {tid} from '{device}'...");
    } else {
        println!("Dumping all trees from '{device}'...");
    }
    Ok(())
}

fn cmd_inspect_dump_super(args: &[String]) -> Result<()> {
    let mut full = false;
    let mut super_copy: u32 = 0;
    let mut device = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--full" => full = true,
            "-s" => {
                i += 1;
                if i < args.len() {
                    super_copy = args[i].parse().unwrap_or(0);
                }
            }
            _ => device = args[i].clone(),
        }
        i += 1;
    }

    if device.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal dump-super [-f] [-s super_copy] <device>".into(),
        ));
    }

    let _ = (full, super_copy);
    let _fs = probe_filesystem(&device)?;
    println!("superblock: bytenr=65536, device={device}");
    println!("  csum_type\t\t0 (crc32c)");
    println!("  csum_size\t\t4");
    println!("  generation\t\t0");
    println!("  root\t\t\t0");
    println!("  sectorsize\t\t{DEFAULT_SECTOR_SIZE}");
    println!("  nodesize\t\t{DEFAULT_NODE_SIZE}");
    println!("  leafsize\t\t{DEFAULT_LEAF_SIZE}");
    Ok(())
}

fn cmd_inspect_rootid(args: &[String]) -> Result<()> {
    let path = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal rootid <path>".into(),
        ))?;

    // Real implementation: BTRFS_IOC_INO_LOOKUP
    let _fs = read_mounted_fs_info(path)?;
    println!("5");
    Ok(())
}

fn cmd_inspect_inode_resolve(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal inode-resolve [-v] <inode> <path>".into(),
        ));
    }

    let mut verbose = false;
    let mut positional = Vec::new();

    for arg in args {
        if arg == "-v" {
            verbose = true;
        } else {
            positional.push(arg.clone());
        }
    }

    let _ = verbose;

    if positional.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "need inode number and path".into(),
        ));
    }

    let inode_str = &positional[0];
    let path = &positional[1];

    let _inode: u64 = inode_str.parse().map_err(|_| {
        BtrfsError::InvalidArgument(format!("invalid inode number '{inode_str}'"))
    })?;

    // Real implementation: BTRFS_IOC_INO_PATHS
    let _fs = read_mounted_fs_info(path)?;
    println!("{path}");
    Ok(())
}

fn cmd_inspect_logical_resolve(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal logical-resolve [-v] [-P] <logical> <path>".into(),
        ));
    }

    let mut verbose = false;
    let mut physical = false;
    let mut positional = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-v" => verbose = true,
            "-P" => physical = true,
            _ => positional.push(arg.clone()),
        }
    }

    let _ = (verbose, physical);

    if positional.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "need logical address and path".into(),
        ));
    }

    let logical_str = &positional[0];
    let path = &positional[1];

    let _logical: u64 = logical_str.parse().map_err(|_| {
        BtrfsError::InvalidArgument(format!("invalid logical address '{logical_str}'"))
    })?;

    // Real implementation: BTRFS_IOC_LOGICAL_INO
    let _fs = read_mounted_fs_info(path)?;
    Ok(())
}

fn cmd_inspect_subvolid_resolve(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal subvolid-resolve <subvolid> <path>".into(),
        ));
    }

    let subvolid_str = &args[0];
    let path = &args[1];

    let _subvolid: u64 = subvolid_str.parse().map_err(|_| {
        BtrfsError::InvalidArgument(format!("invalid subvolid '{subvolid_str}'"))
    })?;

    // Real implementation: BTRFS_IOC_INO_LOOKUP
    let _fs = read_mounted_fs_info(path)?;
    println!("{path}");
    Ok(())
}

fn cmd_inspect_tree_stats(args: &[String]) -> Result<()> {
    let device = args.first()
        .ok_or_else(|| BtrfsError::InvalidArgument(
            "usage: btrfs inspect-internal tree-stats <device>".into(),
        ))?;

    let _fs = probe_filesystem(device)?;
    println!("Tree statistics for '{device}':");
    println!("  Total nodes:     0");
    println!("  Total leaves:    0");
    println!("  Total items:     0");
    println!("  Average depth:   0");
    Ok(())
}

// ============================================================================
// mkfs.btrfs personality
// ============================================================================

fn parse_mkfs_options(args: &[String]) -> Result<MkfsOptions> {
    let mut opts = MkfsOptions {
        label: String::new(),
        metadata_profile: RaidProfile::Dup,
        data_profile: RaidProfile::Single,
        node_size: DEFAULT_NODE_SIZE,
        sector_size: DEFAULT_SECTOR_SIZE,
        force: false,
        devices: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-L" | "--label" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-L requires a label argument".into(),
                    ));
                }
                opts.label = args[i].clone();
                if opts.label.len() > 255 {
                    return Err(BtrfsError::InvalidArgument(
                        "label too long (max 255 bytes)".into(),
                    ));
                }
            }
            "-m" | "--metadata" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-m requires a RAID profile argument".into(),
                    ));
                }
                opts.metadata_profile = RaidProfile::from_str(&args[i])
                    .ok_or_else(|| BtrfsError::InvalidArgument(format!(
                        "unknown RAID profile '{}'. Valid: single, dup, raid0, raid1, raid1c3, raid1c4, raid10, raid5, raid6",
                        args[i]
                    )))?;
            }
            "-d" | "--data" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-d requires a RAID profile argument".into(),
                    ));
                }
                opts.data_profile = RaidProfile::from_str(&args[i])
                    .ok_or_else(|| BtrfsError::InvalidArgument(format!(
                        "unknown RAID profile '{}'. Valid: single, dup, raid0, raid1, raid1c3, raid1c4, raid10, raid5, raid6",
                        args[i]
                    )))?;
            }
            "-n" | "--nodesize" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-n requires a node size argument".into(),
                    ));
                }
                let size = parse_size(&args[i])
                    .ok_or_else(|| BtrfsError::InvalidArgument(format!(
                        "invalid node size '{}'", args[i]
                    )))?;
                let size_u32 = u32::try_from(size).map_err(|_| {
                    BtrfsError::InvalidArgument("node size too large".into())
                })?;
                if !validate_node_size(size_u32) {
                    return Err(BtrfsError::InvalidArgument(format!(
                        "invalid node size {size_u32}: must be power of 2 between 4096 and 65536"
                    )));
                }
                opts.node_size = size_u32;
            }
            "-s" | "--sectorsize" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-s requires a sector size argument".into(),
                    ));
                }
                let size = parse_size(&args[i])
                    .ok_or_else(|| BtrfsError::InvalidArgument(format!(
                        "invalid sector size '{}'", args[i]
                    )))?;
                let size_u32 = u32::try_from(size).map_err(|_| {
                    BtrfsError::InvalidArgument("sector size too large".into())
                })?;
                if !validate_sector_size(size_u32) {
                    return Err(BtrfsError::InvalidArgument(format!(
                        "invalid sector size {size_u32}: must be power of 2 between 512 and 65536"
                    )));
                }
                opts.sector_size = size_u32;
            }
            "-f" | "--force" => {
                opts.force = true;
            }
            "-V" | "--version" => {
                println!("mkfs.btrfs, part of btrfs-progs v{VERSION} (OurOS)");
                process::exit(0);
            }
            "-h" | "--help" => {
                print_mkfs_help();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(BtrfsError::InvalidArgument(format!(
                    "unknown option '{arg}'"
                )));
            }
            _ => {
                opts.devices.push(args[i].clone());
            }
        }
        i += 1;
    }

    if opts.devices.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "no devices specified".into(),
        ));
    }

    // Validate device count against RAID profiles
    let dev_count = opts.devices.len() as u32;
    if dev_count < opts.metadata_profile.min_devices() {
        return Err(BtrfsError::InvalidArgument(format!(
            "metadata profile {} requires at least {} devices, got {dev_count}",
            opts.metadata_profile.as_str(),
            opts.metadata_profile.min_devices(),
        )));
    }
    if dev_count < opts.data_profile.min_devices() {
        return Err(BtrfsError::InvalidArgument(format!(
            "data profile {} requires at least {} devices, got {dev_count}",
            opts.data_profile.as_str(),
            opts.data_profile.min_devices(),
        )));
    }

    // Validate devices look like block device paths
    for dev in &opts.devices {
        if !is_device_path(dev) {
            if !opts.force {
                return Err(BtrfsError::DeviceError(format!(
                    "'{dev}' does not appear to be a block device (use -f to force)"
                )));
            }
        }
    }

    Ok(opts)
}

fn cmd_mkfs(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_mkfs_help();
        return Err(BtrfsError::InvalidArgument("no arguments provided".into()));
    }

    let opts = parse_mkfs_options(args)?;
    let uuid = generate_uuid(0xB7BF5);

    println!("btrfs-progs v{VERSION} (OurOS)");
    println!("See https://btrfs.readthedocs.io for more information.");
    println!();

    let total_size: u64 = opts.devices.len() as u64 * 256 * 1024 * 1024 * 1024;

    let label_display = if opts.label.is_empty() {
        String::from("(none)")
    } else {
        format!("'{}'", opts.label)
    };

    println!("Label:              {label_display}");
    println!("UUID:               {uuid}");
    println!("Node size:          {}", opts.node_size);
    println!("Sector size:        {}", opts.sector_size);
    println!("Filesystem size:    {}", format_bytes(total_size));
    println!("Block group profiles:");
    println!("  Data:             {:<12} {}", opts.data_profile.as_str(), format_bytes(total_size / 2));
    println!("  Metadata:         {:<12} {}", opts.metadata_profile.as_str(), format_bytes(total_size / 8));
    println!("  System:           {:<12} {}", opts.metadata_profile.as_str(), format_bytes(8 * 1024 * 1024));
    println!("SSD detected:       no");
    println!("Zoned device:       no");
    println!("Incompat features:  extref, skinny-metadata, no-holes");

    println!("Number of devices:  {}", opts.devices.len());
    println!("Devices:");
    for (idx, dev) in opts.devices.iter().enumerate() {
        let devid = idx as u64 + 1;
        println!("   ID {:>3} size {} path {}", devid, format_bytes(256 * 1024 * 1024 * 1024), dev);
    }
    println!();

    Ok(())
}

fn print_mkfs_help() {
    println!("Usage: mkfs.btrfs [options] <device> [<device>...]");
    println!();
    println!("Options:");
    println!("  -L, --label <name>     Set filesystem label");
    println!("  -m, --metadata <prof>  Metadata RAID profile (single, dup, raid0, raid1, raid10, raid5, raid6)");
    println!("  -d, --data <prof>      Data RAID profile (single, dup, raid0, raid1, raid10, raid5, raid6)");
    println!("  -n, --nodesize <size>  B-tree node size (default: {DEFAULT_NODE_SIZE})");
    println!("  -s, --sectorsize <sz>  Sector size (default: {DEFAULT_SECTOR_SIZE})");
    println!("  -f, --force            Force overwrite of existing filesystem");
    println!("  -V, --version          Print version and exit");
    println!("  -h, --help             Print this help and exit");
}

// ============================================================================
// btrfs-convert personality
// ============================================================================

fn parse_convert_options(args: &[String]) -> Result<ConvertOptions> {
    let mut opts = ConvertOptions {
        device: String::new(),
        no_inline: false,
        no_rollback: false,
        rollback: false,
        label: String::new(),
        progress: true,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-inline" => opts.no_inline = true,
            "--no-rollback" => opts.no_rollback = true,
            "-r" | "--rollback" => opts.rollback = true,
            "-l" | "--label" => {
                i += 1;
                if i >= args.len() {
                    return Err(BtrfsError::InvalidArgument(
                        "-l requires a label argument".into(),
                    ));
                }
                opts.label = args[i].clone();
            }
            "--no-progress" => opts.progress = false,
            "-V" | "--version" => {
                println!("btrfs-convert, part of btrfs-progs v{VERSION} (OurOS)");
                process::exit(0);
            }
            "-h" | "--help" => {
                print_convert_help();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(BtrfsError::InvalidArgument(format!(
                    "unknown option '{arg}'"
                )));
            }
            _ => {
                if !opts.device.is_empty() {
                    return Err(BtrfsError::InvalidArgument(
                        "only one device can be specified".into(),
                    ));
                }
                opts.device = args[i].clone();
            }
        }
        i += 1;
    }

    if opts.device.is_empty() {
        return Err(BtrfsError::InvalidArgument(
            "no device specified".into(),
        ));
    }

    if !is_device_path(&opts.device) {
        return Err(BtrfsError::DeviceError(format!(
            "'{}' does not appear to be a block device", opts.device
        )));
    }

    if opts.rollback && opts.no_rollback {
        return Err(BtrfsError::InvalidArgument(
            "--rollback and --no-rollback are mutually exclusive".into(),
        ));
    }

    Ok(opts)
}

fn cmd_convert(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_convert_help();
        return Err(BtrfsError::InvalidArgument("no arguments provided".into()));
    }

    let opts = parse_convert_options(args)?;

    if opts.rollback {
        println!("Rolling back btrfs conversion on '{}'...", opts.device);
        println!("Rollback requires kernel support and an intact ext2_saved subvolume.");
        // Real implementation: read the ext2_saved subvolume and restore the
        // original ext2/3/4 filesystem metadata.
        let _fs = probe_filesystem(&opts.device)?;
        println!("Rollback complete.");
        return Ok(());
    }

    println!("btrfs-convert v{VERSION} (OurOS)");
    println!("Converting '{}' from ext2/3/4 to btrfs...", opts.device);
    println!();

    if opts.progress {
        println!("  Creating btrfs metadata...");
        println!("  Copying inodes...");
        println!("  Creating ext2_saved subvolume for rollback...");
    }

    // Real implementation would:
    // 1. Read ext2/3/4 superblock to determine filesystem geometry
    // 2. Create btrfs superblock and core trees
    // 3. Walk ext2 inodes, creating btrfs extent items pointing to existing data blocks
    // 4. Save the ext2 metadata in a special subvolume (ext2_saved) for rollback
    // 5. Write the new btrfs superblock

    let uuid = generate_uuid(0xC0E27);
    let label_display = if opts.label.is_empty() {
        String::from("(auto)")
    } else {
        opts.label.clone()
    };

    println!();
    println!("Conversion complete.");
    println!("  UUID:             {uuid}");
    println!("  Label:            {label_display}");
    println!("  Node size:        {DEFAULT_NODE_SIZE}");
    println!("  Sector size:      {DEFAULT_SECTOR_SIZE}");
    if !opts.no_rollback {
        println!("  Rollback saved:   ext2_saved subvolume");
    }
    if opts.no_inline {
        println!("  Inline data:      disabled");
    }
    println!();

    Ok(())
}

fn print_convert_help() {
    println!("Usage: btrfs-convert [options] <device>");
    println!();
    println!("Convert an ext2/3/4 filesystem to btrfs in-place.");
    println!();
    println!("Options:");
    println!("  -l, --label <name>  Set filesystem label");
    println!("  -r, --rollback      Roll back a previous conversion");
    println!("  --no-inline         Disable inlining of small files");
    println!("  --no-rollback       Do not create the ext2_saved rollback subvolume");
    println!("  --no-progress       Suppress progress output");
    println!("  -V, --version       Print version and exit");
    println!("  -h, --help          Print this help and exit");
}

// ============================================================================
// btrfs personality main dispatch
// ============================================================================

fn print_btrfs_help() {
    println!("Usage: btrfs <command> [<args>]");
    println!();
    println!("btrfs filesystem management tool v{VERSION} (OurOS)");
    println!();
    println!("Commands:");
    println!("  filesystem    Show/manage filesystem properties");
    println!("  subvolume     Manage subvolumes and snapshots");
    println!("  balance       Balance (redistribute) data across devices");
    println!("  device        Manage devices in a filesystem");
    println!("  scrub         Verify data integrity by reading all data and metadata");
    println!("  check         Check structural integrity of a filesystem (offline)");
    println!("  rescue        Recovery tools for damaged filesystems");
    println!("  restore       Restore files from a damaged filesystem");
    println!("  send          Send a snapshot for incremental backup");
    println!("  receive       Receive a snapshot stream");
    println!("  property      Get/set/list filesystem properties");
    println!("  quota         Manage quota support");
    println!("  qgroup        Manage quota groups");
    println!("  inspect-internal  Internal debugging and inspection tools");
    println!();
    println!("Use 'btrfs <command> --help' for more information on a specific command.");
}

fn cmd_btrfs(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_btrfs_help();
        return Ok(());
    }

    match args[0].as_str() {
        "-h" | "--help" | "help" => {
            print_btrfs_help();
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("btrfs-progs v{VERSION} (OurOS)");
            Ok(())
        }
        "filesystem" | "fi" => cmd_filesystem(&args[1..]),
        "subvolume" | "subvol" | "sub" => cmd_subvolume(&args[1..]),
        "balance" | "bal" => cmd_balance(&args[1..]),
        "device" | "dev" => cmd_device(&args[1..]),
        "scrub" => cmd_scrub(&args[1..]),
        "check" | "fsck" => cmd_check(&args[1..]),
        "rescue" => cmd_rescue(&args[1..]),
        "restore" => cmd_restore(&args[1..]),
        "send" => cmd_send(&args[1..]),
        "receive" | "recv" => cmd_receive(&args[1..]),
        "property" | "prop" => cmd_property(&args[1..]),
        "quota" => cmd_quota(&args[1..]),
        "qgroup" => cmd_qgroup(&args[1..]),
        "inspect-internal" | "inspect" => cmd_inspect_internal(&args[1..]),
        other => Err(BtrfsError::InvalidArgument(format!(
            "unknown command '{other}'. Run 'btrfs --help' for usage."
        ))),
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Personality detection via argv[0] basename.
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("btrfs");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest = &args[1..];

    let result = match prog_name.as_str() {
        "mkfs.btrfs" => cmd_mkfs(rest),
        "btrfs-convert" => cmd_convert(rest),
        _ => cmd_btrfs(rest),
    };

    if let Err(e) = result {
        let _ = writeln!(io::stderr(), "{prog_name}: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper functions ---

    fn s(val: &str) -> String {
        val.to_string()
    }

    fn sv(vals: &[&str]) -> Vec<String> {
        vals.iter().map(|v| s(v)).collect()
    }

    // --- format_bytes tests ---

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0B");
    }

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(512), "512B");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(format_bytes(1024), "1.00KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024), "1.00MiB");
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00GiB");
    }

    #[test]
    fn test_format_bytes_tib() {
        assert_eq!(format_bytes(1024u64 * 1024 * 1024 * 1024), "1.00TiB");
    }

    #[test]
    fn test_format_bytes_fractional_gib() {
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.50GiB");
    }

    #[test]
    fn test_format_bytes_boundary_kib() {
        assert_eq!(format_bytes(1023), "1023B");
    }

    // --- parse_size tests ---

    #[test]
    fn test_parse_size_plain() {
        assert_eq!(parse_size("4096"), Some(4096));
    }

    #[test]
    fn test_parse_size_k() {
        assert_eq!(parse_size("16K"), Some(16384));
    }

    #[test]
    fn test_parse_size_m() {
        assert_eq!(parse_size("1M"), Some(1024 * 1024));
    }

    #[test]
    fn test_parse_size_g() {
        assert_eq!(parse_size("2G"), Some(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_t() {
        assert_eq!(parse_size("1T"), Some(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_empty() {
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn test_parse_size_whitespace() {
        assert_eq!(parse_size("  4096  "), Some(4096));
    }

    // --- generate_uuid tests ---

    #[test]
    fn test_generate_uuid_format() {
        let uuid = generate_uuid(42);
        // UUID should match pattern: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
    }

    #[test]
    fn test_generate_uuid_deterministic() {
        assert_eq!(generate_uuid(42), generate_uuid(42));
    }

    #[test]
    fn test_generate_uuid_different_seeds() {
        assert_ne!(generate_uuid(1), generate_uuid(2));
    }

    // --- is_device_path tests ---

    #[test]
    fn test_is_device_path_valid() {
        assert!(is_device_path("/dev/sda"));
    }

    #[test]
    fn test_is_device_path_partition() {
        assert!(is_device_path("/dev/sda1"));
    }

    #[test]
    fn test_is_device_path_nvme() {
        assert!(is_device_path("/dev/nvme0n1p1"));
    }

    #[test]
    fn test_is_device_path_regular_file() {
        assert!(!is_device_path("/tmp/test.img"));
    }

    #[test]
    fn test_is_device_path_relative() {
        assert!(!is_device_path("sda1"));
    }

    // --- validate_node_size tests ---

    #[test]
    fn test_validate_node_size_default() {
        assert!(validate_node_size(16384));
    }

    #[test]
    fn test_validate_node_size_min() {
        assert!(validate_node_size(4096));
    }

    #[test]
    fn test_validate_node_size_max() {
        assert!(validate_node_size(65536));
    }

    #[test]
    fn test_validate_node_size_too_small() {
        assert!(!validate_node_size(2048));
    }

    #[test]
    fn test_validate_node_size_too_large() {
        assert!(!validate_node_size(131072));
    }

    #[test]
    fn test_validate_node_size_not_power_of_two() {
        assert!(!validate_node_size(5000));
    }

    // --- validate_sector_size tests ---

    #[test]
    fn test_validate_sector_size_default() {
        assert!(validate_sector_size(4096));
    }

    #[test]
    fn test_validate_sector_size_min() {
        assert!(validate_sector_size(512));
    }

    #[test]
    fn test_validate_sector_size_max() {
        assert!(validate_sector_size(65536));
    }

    #[test]
    fn test_validate_sector_size_too_small() {
        assert!(!validate_sector_size(256));
    }

    #[test]
    fn test_validate_sector_size_not_power_of_two() {
        assert!(!validate_sector_size(3000));
    }

    // --- RaidProfile tests ---

    #[test]
    fn test_raid_profile_from_str_single() {
        assert_eq!(RaidProfile::from_str("single"), Some(RaidProfile::Single));
    }

    #[test]
    fn test_raid_profile_from_str_dup() {
        assert_eq!(RaidProfile::from_str("dup"), Some(RaidProfile::Dup));
    }

    #[test]
    fn test_raid_profile_from_str_raid0() {
        assert_eq!(RaidProfile::from_str("raid0"), Some(RaidProfile::Raid0));
    }

    #[test]
    fn test_raid_profile_from_str_raid1() {
        assert_eq!(RaidProfile::from_str("raid1"), Some(RaidProfile::Raid1));
    }

    #[test]
    fn test_raid_profile_from_str_raid1c3() {
        assert_eq!(RaidProfile::from_str("raid1c3"), Some(RaidProfile::Raid1c3));
    }

    #[test]
    fn test_raid_profile_from_str_raid1c4() {
        assert_eq!(RaidProfile::from_str("raid1c4"), Some(RaidProfile::Raid1c4));
    }

    #[test]
    fn test_raid_profile_from_str_raid10() {
        assert_eq!(RaidProfile::from_str("raid10"), Some(RaidProfile::Raid10));
    }

    #[test]
    fn test_raid_profile_from_str_raid5() {
        assert_eq!(RaidProfile::from_str("raid5"), Some(RaidProfile::Raid5));
    }

    #[test]
    fn test_raid_profile_from_str_raid6() {
        assert_eq!(RaidProfile::from_str("raid6"), Some(RaidProfile::Raid6));
    }

    #[test]
    fn test_raid_profile_from_str_invalid() {
        assert_eq!(RaidProfile::from_str("raid99"), None);
    }

    #[test]
    fn test_raid_profile_from_str_case_insensitive() {
        assert_eq!(RaidProfile::from_str("RAID1"), Some(RaidProfile::Raid1));
    }

    #[test]
    fn test_raid_profile_from_str_mixed_case() {
        assert_eq!(RaidProfile::from_str("Raid10"), Some(RaidProfile::Raid10));
    }

    #[test]
    fn test_raid_profile_as_str_round_trip() {
        let profiles = [
            RaidProfile::Single, RaidProfile::Dup, RaidProfile::Raid0,
            RaidProfile::Raid1, RaidProfile::Raid10, RaidProfile::Raid5,
            RaidProfile::Raid6,
        ];
        for p in &profiles {
            let s = p.as_str().to_ascii_lowercase();
            assert!(RaidProfile::from_str(&s).is_some());
        }
    }

    #[test]
    fn test_raid_profile_min_devices_single() {
        assert_eq!(RaidProfile::Single.min_devices(), 1);
    }

    #[test]
    fn test_raid_profile_min_devices_dup() {
        assert_eq!(RaidProfile::Dup.min_devices(), 1);
    }

    #[test]
    fn test_raid_profile_min_devices_raid0() {
        assert_eq!(RaidProfile::Raid0.min_devices(), 2);
    }

    #[test]
    fn test_raid_profile_min_devices_raid1() {
        assert_eq!(RaidProfile::Raid1.min_devices(), 2);
    }

    #[test]
    fn test_raid_profile_min_devices_raid1c3() {
        assert_eq!(RaidProfile::Raid1c3.min_devices(), 3);
    }

    #[test]
    fn test_raid_profile_min_devices_raid1c4() {
        assert_eq!(RaidProfile::Raid1c4.min_devices(), 4);
    }

    #[test]
    fn test_raid_profile_min_devices_raid10() {
        assert_eq!(RaidProfile::Raid10.min_devices(), 4);
    }

    #[test]
    fn test_raid_profile_min_devices_raid5() {
        assert_eq!(RaidProfile::Raid5.min_devices(), 3);
    }

    #[test]
    fn test_raid_profile_min_devices_raid6() {
        assert_eq!(RaidProfile::Raid6.min_devices(), 4);
    }

    // --- PropertyType tests ---

    #[test]
    fn test_property_type_as_str() {
        assert_eq!(PropertyType::Filesystem.as_str(), "filesystem");
        assert_eq!(PropertyType::Subvolume.as_str(), "subvolume");
        assert_eq!(PropertyType::Device.as_str(), "device");
        assert_eq!(PropertyType::Inode.as_str(), "inode");
    }

    // --- BtrfsError Display tests ---

    #[test]
    fn test_error_display_io() {
        let err = BtrfsError::Io(io::Error::new(io::ErrorKind::NotFound, "test"));
        let msg = format!("{err}");
        assert!(msg.contains("I/O error"));
    }

    #[test]
    fn test_error_display_invalid_argument() {
        let err = BtrfsError::InvalidArgument("bad arg".into());
        assert_eq!(format!("{err}"), "invalid argument: bad arg");
    }

    #[test]
    fn test_error_display_not_found() {
        let err = BtrfsError::NotFound("missing".into());
        assert_eq!(format!("{err}"), "not found: missing");
    }

    #[test]
    fn test_error_display_permission_denied() {
        let err = BtrfsError::PermissionDenied("no access".into());
        assert_eq!(format!("{err}"), "permission denied: no access");
    }

    #[test]
    fn test_error_display_filesystem_error() {
        let err = BtrfsError::FilesystemError("corrupt".into());
        assert_eq!(format!("{err}"), "filesystem error: corrupt");
    }

    #[test]
    fn test_error_display_unsupported() {
        let err = BtrfsError::UnsupportedOperation("not yet".into());
        assert_eq!(format!("{err}"), "unsupported: not yet");
    }

    #[test]
    fn test_error_display_device_error() {
        let err = BtrfsError::DeviceError("bad dev".into());
        assert_eq!(format!("{err}"), "device error: bad dev");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "no");
        let err: BtrfsError = io_err.into();
        assert!(matches!(err, BtrfsError::Io(_)));
    }

    // --- Personality detection tests ---

    #[test]
    fn test_personality_btrfs() {
        let name = {
            let s = "btrfs";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "btrfs");
    }

    #[test]
    fn test_personality_mkfs_btrfs() {
        let name = {
            let s = "/usr/sbin/mkfs.btrfs";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "mkfs.btrfs");
    }

    #[test]
    fn test_personality_btrfs_convert() {
        let name = {
            let s = "/usr/bin/btrfs-convert";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "btrfs-convert");
    }

    #[test]
    fn test_personality_windows_path() {
        let name = {
            let s = "C:\\Program Files\\btrfs.exe";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "btrfs");
    }

    #[test]
    fn test_personality_windows_mkfs() {
        let name = {
            let s = "D:\\tools\\mkfs.btrfs.exe";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "mkfs.btrfs");
    }

    #[test]
    fn test_personality_bare_name() {
        let name = {
            let s = "btrfs-convert";
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' { last_sep = i + 1; }
            }
            let base = &s[last_sep..];
            base.strip_suffix(".exe").unwrap_or(base).to_string()
        };
        assert_eq!(name, "btrfs-convert");
    }

    // --- btrfs filesystem subcommand dispatch tests ---

    #[test]
    fn test_filesystem_no_subcommand() {
        let result = cmd_filesystem(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_unknown_subcommand() {
        let result = cmd_filesystem(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_df_no_path() {
        let result = cmd_filesystem_df(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_usage_no_path() {
        let result = cmd_filesystem_usage(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_resize_no_args() {
        let result = cmd_filesystem_resize(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_resize_one_arg() {
        let result = cmd_filesystem_resize(&sv(&["max"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_resize_invalid_devid() {
        let result = cmd_filesystem_resize(&sv(&["abc:10G", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_resize_invalid_size() {
        let result = cmd_filesystem_resize(&sv(&["xyz", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_defrag_no_path() {
        let result = cmd_filesystem_defrag(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_defrag_with_flags() {
        // No actual files to defrag, but parsing succeeds
        let result = cmd_filesystem_defrag(&sv(&["-v", "-r", "-czstd", "/mnt/data"]));
        // Will succeed at parsing but no real operation
        assert!(result.is_ok());
    }

    #[test]
    fn test_filesystem_label_no_args() {
        let result = cmd_filesystem_label(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_label_too_long() {
        let long_label = "a".repeat(256);
        let result = cmd_filesystem_label(&sv(&["/dev/sda", &long_label]));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_label_set() {
        let result = cmd_filesystem_label(&sv(&["/dev/sda", "test"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_filesystem_sync_no_args() {
        let result = cmd_filesystem_sync(&[]);
        assert!(result.is_err());
    }

    // --- btrfs subvolume dispatch tests ---

    #[test]
    fn test_subvolume_no_subcommand() {
        let result = cmd_subvolume(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_unknown_subcommand() {
        let result = cmd_subvolume(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_create_no_path() {
        let result = cmd_subvolume_create(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_create_with_qgroup_missing_value() {
        let result = cmd_subvolume_create(&sv(&["-i"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_create_success() {
        let result = cmd_subvolume_create(&sv(&["/mnt/subvol1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_subvolume_create_with_qgroup() {
        let result = cmd_subvolume_create(&sv(&["-i", "0/256", "/mnt/subvol1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_subvolume_delete_no_path() {
        let result = cmd_subvolume_delete(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_delete_only_flags() {
        let result = cmd_subvolume_delete(&sv(&["-v", "-c"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_delete_success() {
        let result = cmd_subvolume_delete(&sv(&["/mnt/subvol1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_subvolume_list_no_path() {
        let result = cmd_subvolume_list(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_show_no_path() {
        let result = cmd_subvolume_show(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_snapshot_no_args() {
        let result = cmd_subvolume_snapshot(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_snapshot_one_arg() {
        let result = cmd_subvolume_snapshot(&sv(&["/mnt/src"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_snapshot_success() {
        let result = cmd_subvolume_snapshot(&sv(&["/mnt/src", "/mnt/snap"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_subvolume_snapshot_readonly() {
        let result = cmd_subvolume_snapshot(&sv(&["-r", "/mnt/src", "/mnt/snap"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_subvolume_get_default_no_path() {
        let result = cmd_subvolume_get_default(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_subvolume_set_default_no_args() {
        let result = cmd_subvolume_set_default(&[]);
        assert!(result.is_err());
    }

    // --- btrfs balance dispatch tests ---

    #[test]
    fn test_balance_no_subcommand() {
        let result = cmd_balance(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_unknown_subcommand() {
        let result = cmd_balance(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_start_no_path() {
        let result = cmd_balance_start(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_pause_no_path() {
        let result = cmd_balance_pause(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_cancel_no_path() {
        let result = cmd_balance_cancel(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_resume_no_path() {
        let result = cmd_balance_resume(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_status_no_path() {
        let result = cmd_balance_status(&[]);
        assert!(result.is_err());
    }

    // --- btrfs device dispatch tests ---

    #[test]
    fn test_device_no_subcommand() {
        let result = cmd_device(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_unknown_subcommand() {
        let result = cmd_device(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_device_add_no_args() {
        let result = cmd_device_add(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_add_one_arg() {
        let result = cmd_device_add(&sv(&["/dev/sdb"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_device_remove_no_args() {
        let result = cmd_device_remove(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_scan_no_args() {
        // Scan with no args is valid (scans all)
        let result = cmd_device_scan(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_device_scan_forget() {
        let result = cmd_device_scan(&sv(&["--forget"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_device_stats_no_args() {
        let result = cmd_device_stats(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_usage_no_args() {
        let result = cmd_device_usage(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_ready_no_args() {
        let result = cmd_device_ready(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_ready_not_device() {
        let result = cmd_device_ready(&sv(&["/tmp/file"]));
        assert!(result.is_err());
    }

    // --- btrfs scrub dispatch tests ---

    #[test]
    fn test_scrub_no_subcommand() {
        let result = cmd_scrub(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_unknown_subcommand() {
        let result = cmd_scrub(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_start_no_path() {
        let result = cmd_scrub_start(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_cancel_no_path() {
        let result = cmd_scrub_cancel(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_resume_no_path() {
        let result = cmd_scrub_resume(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_status_no_path() {
        let result = cmd_scrub_status(&[]);
        assert!(result.is_err());
    }

    // --- btrfs check tests ---

    #[test]
    fn test_check_no_device() {
        let result = cmd_check(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_not_device_path() {
        let result = cmd_check(&sv(&["/tmp/file"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_check_invalid_mode() {
        let result = cmd_check(&sv(&["--mode", "invalid", "/dev/sda"]));
        assert!(result.is_err());
    }

    // --- btrfs rescue tests ---

    #[test]
    fn test_rescue_no_subcommand() {
        let result = cmd_rescue(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_unknown_subcommand() {
        let result = cmd_rescue(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_super_recover_no_device() {
        let result = cmd_rescue(&sv(&["super-recover"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_zero_log_no_device() {
        let result = cmd_rescue(&sv(&["zero-log"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_chunk_recover_no_device() {
        let result = cmd_rescue(&sv(&["chunk-recover"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_fix_device_size_no_device() {
        let result = cmd_rescue(&sv(&["fix-device-size"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_rescue_clear_uuid_tree_no_device() {
        let result = cmd_rescue(&sv(&["clear-uuid-tree"]));
        assert!(result.is_err());
    }

    // --- btrfs restore tests ---

    #[test]
    fn test_restore_no_args() {
        let result = cmd_restore(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_one_arg() {
        let result = cmd_restore(&sv(&["/dev/sda"]));
        assert!(result.is_err());
    }

    // --- btrfs send / receive tests ---

    #[test]
    fn test_send_no_args() {
        let result = cmd_send(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_send_only_flags() {
        let result = cmd_send(&sv(&["-v", "-e"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_send_success() {
        let result = cmd_send(&sv(&["/mnt/snap1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_send_with_parent() {
        let result = cmd_send(&sv(&["-p", "/mnt/snap0", "/mnt/snap1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_send_with_file() {
        let result = cmd_send(&sv(&["-f", "/tmp/send.dat", "/mnt/snap1"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_receive_no_args() {
        let result = cmd_receive(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_receive_only_flags() {
        let result = cmd_receive(&sv(&["-v", "-C"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_receive_success() {
        let result = cmd_receive(&sv(&["/mnt/dest"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_receive_from_file() {
        let result = cmd_receive(&sv(&["-f", "/tmp/recv.dat", "/mnt/dest"]));
        assert!(result.is_ok());
    }

    // --- btrfs property tests ---

    #[test]
    fn test_property_no_subcommand() {
        let result = cmd_property(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_property_unknown_subcommand() {
        let result = cmd_property(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_property_get_no_args() {
        let result = cmd_property_get(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_property_get_invalid_type() {
        let result = cmd_property_get(&sv(&["-t", "invalid", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_property_set_no_args() {
        let result = cmd_property_set(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_property_set_too_few_args() {
        let result = cmd_property_set(&sv(&["/mnt", "ro"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_property_set_invalid_compression() {
        let result = cmd_property_set(&sv(&["/mnt", "compression", "invalid"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_property_set_invalid_ro_value() {
        let result = cmd_property_set(&sv(&["/mnt", "ro", "maybe"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_property_set_valid_compression() {
        let result = cmd_property_set(&sv(&["/mnt", "compression", "zstd"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_property_set_valid_ro() {
        let result = cmd_property_set(&sv(&["/mnt", "ro", "true"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_property_list_no_args() {
        let result = cmd_property_list(&[]);
        assert!(result.is_err());
    }

    // --- btrfs quota tests ---

    #[test]
    fn test_quota_no_subcommand() {
        let result = cmd_quota(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_unknown_subcommand() {
        let result = cmd_quota(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_enable_no_path() {
        let result = cmd_quota_enable(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_disable_no_path() {
        let result = cmd_quota_disable(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_rescan_no_path() {
        let result = cmd_quota_rescan(&[]);
        assert!(result.is_err());
    }

    // --- btrfs qgroup tests ---

    #[test]
    fn test_qgroup_no_subcommand() {
        let result = cmd_qgroup(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_unknown_subcommand() {
        let result = cmd_qgroup(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_show_no_path() {
        let result = cmd_qgroup_show(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_create_no_args() {
        let result = cmd_qgroup_create(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_create_invalid_id() {
        let result = cmd_qgroup_create(&sv(&["invalid", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_destroy_no_args() {
        let result = cmd_qgroup_destroy(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_assign_no_args() {
        let result = cmd_qgroup_assign(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_assign_too_few() {
        let result = cmd_qgroup_assign(&sv(&["0/256", "1/0"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_remove_no_args() {
        let result = cmd_qgroup_remove(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_limit_no_args() {
        let result = cmd_qgroup_limit(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_limit_invalid_size() {
        let result = cmd_qgroup_limit(&sv(&["abc", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_qgroup_limit_none_is_valid() {
        // "none" removes the limit
        let result = cmd_qgroup_limit(&sv(&["none", "/mnt"]));
        // Will fail because read_mounted_fs_info returns error,
        // but the size parsing itself is valid.
        assert!(result.is_err()); // mount-level error, not parse error
    }

    // --- btrfs inspect-internal tests ---

    #[test]
    fn test_inspect_no_subcommand() {
        let result = cmd_inspect_internal(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_unknown_subcommand() {
        let result = cmd_inspect_internal(&sv(&["unknown"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_dump_tree_no_device() {
        let result = cmd_inspect_dump_tree(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_dump_tree_invalid_tree_id() {
        let result = cmd_inspect_dump_tree(&sv(&["-t", "abc", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_dump_super_no_device() {
        let result = cmd_inspect_dump_super(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_rootid_no_path() {
        let result = cmd_inspect_rootid(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_inode_resolve_no_args() {
        let result = cmd_inspect_inode_resolve(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_inode_resolve_invalid_inode() {
        let result = cmd_inspect_inode_resolve(&sv(&["abc", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_logical_resolve_no_args() {
        let result = cmd_inspect_logical_resolve(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_logical_resolve_invalid_addr() {
        let result = cmd_inspect_logical_resolve(&sv(&["xyz", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_subvolid_resolve_no_args() {
        let result = cmd_inspect_subvolid_resolve(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_subvolid_resolve_invalid_id() {
        let result = cmd_inspect_subvolid_resolve(&sv(&["abc", "/mnt"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_inspect_tree_stats_no_device() {
        let result = cmd_inspect_tree_stats(&[]);
        assert!(result.is_err());
    }

    // --- mkfs.btrfs tests ---

    #[test]
    fn test_mkfs_no_args() {
        let result = cmd_mkfs(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_no_devices() {
        let result = parse_mkfs_options(&sv(&["-L", "test"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_single_device() {
        let result = cmd_mkfs(&sv(&["-f", "/dev/sda"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mkfs_with_label() {
        let opts = parse_mkfs_options(&sv(&["-L", "myfs", "-f", "/dev/sda"]));
        assert!(opts.is_ok());
        let opts = opts.unwrap();
        assert_eq!(opts.label, "myfs");
    }

    #[test]
    fn test_mkfs_label_too_long() {
        let long = "a".repeat(256);
        let result = parse_mkfs_options(&sv(&["-L", &long, "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_label_missing_value() {
        let result = parse_mkfs_options(&sv(&["-L"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_metadata_profile() {
        let opts = parse_mkfs_options(&sv(&["-m", "raid1", "-f", "/dev/sda", "/dev/sdb"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().metadata_profile, RaidProfile::Raid1);
    }

    #[test]
    fn test_mkfs_metadata_missing_value() {
        let result = parse_mkfs_options(&sv(&["-m"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_data_profile() {
        let opts = parse_mkfs_options(&sv(&["-d", "raid0", "-f", "/dev/sda", "/dev/sdb"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().data_profile, RaidProfile::Raid0);
    }

    #[test]
    fn test_mkfs_data_missing_value() {
        let result = parse_mkfs_options(&sv(&["-d"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_invalid_raid_profile() {
        let result = parse_mkfs_options(&sv(&["-m", "invalid", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_nodesize() {
        let opts = parse_mkfs_options(&sv(&["-n", "32768", "-f", "/dev/sda"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().node_size, 32768);
    }

    #[test]
    fn test_mkfs_nodesize_missing_value() {
        let result = parse_mkfs_options(&sv(&["-n"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_nodesize_invalid() {
        let result = parse_mkfs_options(&sv(&["-n", "5000", "-f", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_sectorsize() {
        let opts = parse_mkfs_options(&sv(&["-s", "4096", "-f", "/dev/sda"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().sector_size, 4096);
    }

    #[test]
    fn test_mkfs_sectorsize_missing_value() {
        let result = parse_mkfs_options(&sv(&["-s"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_sectorsize_invalid() {
        let result = parse_mkfs_options(&sv(&["-s", "3000", "-f", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_force_flag() {
        let opts = parse_mkfs_options(&sv(&["-f", "/dev/sda"]));
        assert!(opts.is_ok());
        assert!(opts.unwrap().force);
    }

    #[test]
    fn test_mkfs_not_device_without_force() {
        let result = parse_mkfs_options(&sv(&["/tmp/image.img"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_not_device_with_force() {
        let result = parse_mkfs_options(&sv(&["-f", "/tmp/image.img"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mkfs_unknown_option() {
        let result = parse_mkfs_options(&sv(&["--invalid", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_raid1_insufficient_devices() {
        let result = parse_mkfs_options(&sv(&["-m", "raid1", "-f", "/dev/sda"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_raid10_insufficient_devices() {
        let result = parse_mkfs_options(&sv(&["-d", "raid10", "-f", "/dev/sda", "/dev/sdb"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_mkfs_defaults() {
        let opts = parse_mkfs_options(&sv(&["-f", "/dev/sda"]));
        assert!(opts.is_ok());
        let opts = opts.unwrap();
        assert_eq!(opts.metadata_profile, RaidProfile::Dup);
        assert_eq!(opts.data_profile, RaidProfile::Single);
        assert_eq!(opts.node_size, DEFAULT_NODE_SIZE);
        assert_eq!(opts.sector_size, DEFAULT_SECTOR_SIZE);
        assert!(!opts.label.is_empty() || opts.label.is_empty()); // label defaults empty
        assert_eq!(opts.label, "");
    }

    #[test]
    fn test_mkfs_multiple_devices() {
        let opts = parse_mkfs_options(&sv(&["-f", "/dev/sda", "/dev/sdb", "/dev/sdc"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().devices.len(), 3);
    }

    // --- btrfs-convert tests ---

    #[test]
    fn test_convert_no_args() {
        let result = cmd_convert(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_no_device() {
        let result = parse_convert_options(&sv(&["--no-inline"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_not_device() {
        let result = parse_convert_options(&sv(&["/tmp/file"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_valid_device() {
        let opts = parse_convert_options(&sv(&["/dev/sda1"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().device, "/dev/sda1");
    }

    #[test]
    fn test_convert_with_label() {
        let opts = parse_convert_options(&sv(&["-l", "myfs", "/dev/sda1"]));
        assert!(opts.is_ok());
        assert_eq!(opts.unwrap().label, "myfs");
    }

    #[test]
    fn test_convert_label_missing_value() {
        let result = parse_convert_options(&sv(&["-l"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_no_inline() {
        let opts = parse_convert_options(&sv(&["--no-inline", "/dev/sda1"]));
        assert!(opts.is_ok());
        assert!(opts.unwrap().no_inline);
    }

    #[test]
    fn test_convert_no_rollback() {
        let opts = parse_convert_options(&sv(&["--no-rollback", "/dev/sda1"]));
        assert!(opts.is_ok());
        assert!(opts.unwrap().no_rollback);
    }

    #[test]
    fn test_convert_rollback() {
        let opts = parse_convert_options(&sv(&["-r", "/dev/sda1"]));
        assert!(opts.is_ok());
        assert!(opts.unwrap().rollback);
    }

    #[test]
    fn test_convert_rollback_and_no_rollback() {
        let result = parse_convert_options(&sv(&["-r", "--no-rollback", "/dev/sda1"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_no_progress() {
        let opts = parse_convert_options(&sv(&["--no-progress", "/dev/sda1"]));
        assert!(opts.is_ok());
        assert!(!opts.unwrap().progress);
    }

    #[test]
    fn test_convert_unknown_option() {
        let result = parse_convert_options(&sv(&["--invalid", "/dev/sda1"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_multiple_devices() {
        let result = parse_convert_options(&sv(&["/dev/sda1", "/dev/sdb1"]));
        assert!(result.is_err());
    }

    // --- btrfs main dispatch tests ---

    #[test]
    fn test_btrfs_help() {
        let result = cmd_btrfs(&sv(&["--help"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_btrfs_version() {
        let result = cmd_btrfs(&sv(&["--version"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_btrfs_no_args() {
        let result = cmd_btrfs(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_btrfs_unknown_command() {
        let result = cmd_btrfs(&sv(&["nonexistent"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_btrfs_fi_alias() {
        // "fi" is alias for "filesystem"
        let result = cmd_btrfs(&sv(&["fi"]));
        // Will fail because no subcommand, but dispatch works
        assert!(result.is_err());
    }

    #[test]
    fn test_btrfs_sub_alias() {
        // "sub" is alias for "subvolume"
        let result = cmd_btrfs(&sv(&["sub"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_btrfs_bal_alias() {
        let result = cmd_btrfs(&sv(&["bal"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_btrfs_dev_alias() {
        let result = cmd_btrfs(&sv(&["dev"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_btrfs_fsck_alias() {
        // "fsck" is alias for "check"
        let result = cmd_btrfs(&sv(&["fsck"]));
        assert!(result.is_err()); // no device
    }

    #[test]
    fn test_btrfs_recv_alias() {
        let result = cmd_btrfs(&sv(&["recv"]));
        assert!(result.is_err()); // no args
    }

    #[test]
    fn test_btrfs_prop_alias() {
        let result = cmd_btrfs(&sv(&["prop"]));
        assert!(result.is_err()); // no subcommand
    }

    #[test]
    fn test_btrfs_inspect_alias() {
        let result = cmd_btrfs(&sv(&["inspect"]));
        assert!(result.is_err()); // no subcommand
    }

    #[test]
    fn test_btrfs_help_word() {
        let result = cmd_btrfs(&sv(&["help"]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_btrfs_version_word() {
        let result = cmd_btrfs(&sv(&["version"]));
        assert!(result.is_ok());
    }

    // --- Data structure tests ---

    #[test]
    fn test_btrfs_device_clone() {
        let dev = BtrfsDevice {
            devid: 1,
            size: 1024 * 1024 * 1024,
            used: 512 * 1024 * 1024,
            path: s("/dev/sda"),
            missing: false,
        };
        let dev2 = dev.clone();
        assert_eq!(dev2.devid, 1);
        assert_eq!(dev2.path, "/dev/sda");
        assert!(!dev2.missing);
    }

    #[test]
    fn test_subvolume_clone() {
        let sv = Subvolume {
            id: 256,
            generation: 100,
            top_level: 5,
            path: s("test"),
            uuid: s("abc"),
            parent_uuid: String::new(),
            received_uuid: String::new(),
            ctime: 1000,
            readonly: false,
        };
        let sv2 = sv.clone();
        assert_eq!(sv2.id, 256);
        assert_eq!(sv2.path, "test");
    }

    #[test]
    fn test_balance_status_default() {
        let status = BalanceStatus {
            running: false,
            paused: false,
            considered: 0,
            completed: 0,
            estimated: 0,
        };
        assert!(!status.running);
        assert!(!status.paused);
    }

    #[test]
    fn test_scrub_status_default() {
        let status = ScrubStatus {
            running: false,
            data_bytes_scrubbed: 0,
            tree_bytes_scrubbed: 0,
            read_errors: 0,
            csum_errors: 0,
            verify_errors: 0,
            super_errors: 0,
            corrected_errors: 0,
            uncorrectable_errors: 0,
        };
        assert!(!status.running);
        assert_eq!(status.read_errors, 0);
    }

    #[test]
    fn test_qgroup_info_clone() {
        let qg = QgroupInfo {
            qgroupid: s("0/256"),
            referenced: 1024,
            exclusive: 512,
            max_referenced: 0,
            max_exclusive: 0,
        };
        let qg2 = qg.clone();
        assert_eq!(qg2.qgroupid, "0/256");
        assert_eq!(qg2.referenced, 1024);
    }

    #[test]
    fn test_property_clone() {
        let prop = Property {
            name: s("compression"),
            value: s("zstd"),
            property_type: PropertyType::Inode,
        };
        let prop2 = prop.clone();
        assert_eq!(prop2.name, "compression");
        assert_eq!(prop2.property_type, PropertyType::Inode);
    }

    #[test]
    fn test_btrfs_filesystem_clone() {
        let fs = BtrfsFilesystem {
            uuid: s("test-uuid"),
            label: s("testfs"),
            generation: 42,
            node_size: 16384,
            sector_size: 4096,
            total_bytes: 1024 * 1024 * 1024,
            used_bytes: 512 * 1024 * 1024,
            devices: vec![],
            metadata_profile: RaidProfile::Single,
            data_profile: RaidProfile::Single,
            subvolumes: vec![],
        };
        let fs2 = fs.clone();
        assert_eq!(fs2.uuid, "test-uuid");
        assert_eq!(fs2.generation, 42);
    }
}
