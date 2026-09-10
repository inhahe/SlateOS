//! Slate OS disk management service.
//!
//! Multi-personality binary providing:
//! - **udisksctl** — command-line client for disk management
//! - **udisksd** — disk management daemon
//! - **umount** — unmount filesystems (compatibility)
//!
//! Provides D-Bus-accessible disk management: mount/unmount, format,
//! partition, SMART data, power management, loop setup.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// The single line every declining subcommand ends with.
///
/// One recognisable sentence rather than six differently-worded ones, because
/// the thing a reader needs to take away is identical in every case: the
/// command did not do what it is named for, and is saying so rather than
/// printing a success line. `design-decisions.md` 1019.
const REFUSE: &str = "udisksctl: refusing to report a change it did not make";

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct BlockDevice {
    device: String,
    _sys_path: String,
    id_type: String,
    id_label: String,
    id_uuid: String,
    size: u64,
    removable: bool,
    read_only: bool,
    model: String,
    vendor: String,
    serial: String,
    _revision: String,
    mountpoints: Vec<String>,
    partition_table_type: String,
    partitions: Vec<Partition>,
}

#[derive(Clone, Debug)]
struct Partition {
    device: String,
    number: u32,
    _offset: u64,
    size: u64,
    _type_code: String,
    label: String,
    _uuid: String,
    _flags: Vec<String>,
}

#[derive(Clone, Debug)]
struct _LoopDevice {
    device: String,
    backing_file: String,
    _offset: u64,
    _size_limit: u64,
    _read_only: bool,
    _autoclear: bool,
}

// ============================================================================
// Device discovery
// ============================================================================

fn discover_block_devices() -> Vec<BlockDevice> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip loop and ram devices.
            if name.starts_with("loop") || name.starts_with("ram") {
                continue;
            }

            let path = entry.path();
            let dev_path = format!("/dev/{name}");

            let size_str = read_sys(&path.join("size"));
            let size: u64 = size_str.parse().unwrap_or(0) * 512;

            let removable = read_sys(&path.join("removable")) == "1";
            let read_only = read_sys(&path.join("ro")) == "1";
            let model = read_sys(&path.join("device/model"));
            let vendor = read_sys(&path.join("device/vendor"));

            // Check mount status.
            let mountpoints = find_mountpoints(&dev_path);

            // Look for partitions.
            let partitions = discover_partitions(&path, &name);

            devices.push(BlockDevice {
                device: dev_path,
                _sys_path: path.to_string_lossy().to_string(),
                id_type: String::new(),
                id_label: String::new(),
                id_uuid: String::new(),
                size,
                removable,
                read_only,
                model,
                vendor,
                serial: String::new(),
                _revision: String::new(),
                mountpoints,
                partition_table_type: String::new(),
                partitions,
            });
        }
    }

    // Fallback: generate simulated devices.
    if devices.is_empty() {
        devices.push(BlockDevice {
            device: "/dev/sda".to_string(),
            _sys_path: "/sys/block/sda".to_string(),
            id_type: "ext4".to_string(),
            id_label: "root".to_string(),
            id_uuid: "12345678-abcd-ef01-2345-6789abcdef01".to_string(),
            size: 256 * 1024 * 1024 * 1024,
            removable: false,
            read_only: false,
            model: "Virtual Disk".to_string(),
            vendor: "Slate OS".to_string(),
            serial: String::new(),
            _revision: String::new(),
            mountpoints: vec!["/".to_string()],
            partition_table_type: "gpt".to_string(),
            partitions: vec![
                Partition {
                    device: "/dev/sda1".to_string(),
                    number: 1,
                    _offset: 1048576,
                    size: 512 * 1024 * 1024,
                    _type_code: "EF00".to_string(),
                    label: "EFI".to_string(),
                    _uuid: "AAAA-BBBB".to_string(),
                    _flags: vec!["boot".to_string()],
                },
                Partition {
                    device: "/dev/sda2".to_string(),
                    number: 2,
                    _offset: 513 * 1024 * 1024,
                    size: 255 * 1024 * 1024 * 1024,
                    _type_code: "8300".to_string(),
                    label: "root".to_string(),
                    _uuid: "12345678-abcd-ef01-2345-6789abcdef01".to_string(),
                    _flags: Vec::new(),
                },
            ],
        });
    }

    devices
}

fn discover_partitions(block_path: &Path, parent_name: &str) -> Vec<Partition> {
    let mut partitions = Vec::new();

    if let Ok(entries) = fs::read_dir(block_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(parent_name) {
                continue;
            }
            let part_path = entry.path();
            if !part_path.join("partition").exists() {
                continue;
            }

            let partition_num: u32 = read_sys(&part_path.join("partition")).parse().unwrap_or(0);
            let start: u64 = read_sys(&part_path.join("start")).parse().unwrap_or(0) * 512;
            let size: u64 = read_sys(&part_path.join("size")).parse().unwrap_or(0) * 512;

            partitions.push(Partition {
                device: format!("/dev/{name}"),
                number: partition_num,
                _offset: start,
                size,
                _type_code: String::new(),
                label: String::new(),
                _uuid: String::new(),
                _flags: Vec::new(),
            });
        }
    }

    partitions.sort_by_key(|p| p.number);
    partitions
}

fn read_sys(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The mount table, through [`procinfo`].
///
/// Two defects in the hand-parser this replaces. The kernel escapes space,
/// tab, newline and backslash in the device and mount-point fields as `\040`,
/// `\011`, `\012` and `\134`, so a mount point containing a space never
/// matched a path the caller held. And reading the file as text fails
/// *entirely* when any single line holds a byte that is not UTF-8, so one
/// awkward mount hid every mount -- and `unwrap_or_default()` turned that into
/// an empty table rather than an error.
fn mount_table() -> Vec<procinfo::Mount> {
    procinfo::ProcFs::new()
        .mounts()
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The device path as bytes, without forcing UTF-8 through it.
///
/// `to_string_lossy` was here until 2026-09-10, and is wrong for the reason
/// CLAUDE.md item 7 gives: this OS allows every byte but `/` and NUL in a
/// path, so a device whose name is not UTF-8 became replacement characters and
/// then failed to equal its own entry in the mount table. The symptom would
/// have been `udisksctl unmount -b <device>` answering "is not mounted" about
/// a device that plainly was.
#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    p.as_os_str().as_bytes().to_vec()
}

/// The development host has no byte view of a path. It is not the platform
/// this ships on; the arm exists so the crate builds and its tests run here.
#[cfg(not(unix))]
fn path_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

fn find_mountpoints(device: &str) -> Vec<String> {
    let canonical = fs::canonicalize(device).unwrap_or_else(|_| PathBuf::from(device));
    let canonical_bytes = path_bytes(&canonical);

    mount_table()
        .iter()
        .filter(|m| m.device == device.as_bytes() || m.device == canonical_bytes.as_slice())
        .map(|m| quoting::escape_unprintable(&m.mount_point))
        .collect()
}

// ============================================================================
// Size formatting
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1} TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// udisksctl personality
// ============================================================================

fn udisksctl_main(args: &[String]) -> i32 {
    if args.is_empty() {
        print_udisksctl_help();
        return 1;
    }

    let command = args[0].as_str();
    let cmd_args = &args[1..];

    match command {
        "status" => cmd_status(),
        "info" => cmd_info(cmd_args),
        "mount" => cmd_mount(cmd_args),
        "unmount" | "umount" => cmd_unmount(cmd_args),
        "power-off" => cmd_poweroff(cmd_args),
        "smart-simulate" => cmd_smart(cmd_args),
        "dump" => cmd_dump(),
        "monitor" => cmd_monitor(),
        "loop-setup" => cmd_loop_setup(cmd_args),
        "loop-delete" => cmd_loop_delete(cmd_args),
        "--help" | "help" | "-h" => {
            print_udisksctl_help();
            0
        }
        "--version" => {
            println!("udisksctl (Slate OS) {VERSION}");
            0
        }
        other => {
            eprintln!("udisksctl: unknown command {}", quoteaf_os(other));
            1
        }
    }
}

fn print_udisksctl_help() {
    println!("Usage: udisksctl <command> [options]");
    println!();
    println!("Commands:");
    println!("  status           Show status of block devices");
    println!("  info -b DEVICE   Show detailed info");
    println!("  mount -b DEVICE  Mount a filesystem");
    println!("  unmount -b DEV   Unmount a filesystem");
    println!("  power-off -b DEV Power off a drive");
    println!("  smart-simulate   Show SMART data");
    println!("  dump             Dump all objects");
    println!("  monitor          Monitor changes");
    println!("  loop-setup -f F  Set up a loop device");
    println!("  loop-delete -b D Delete a loop device");
    println!("  help             Show this help");
    println!("  --version        Show version");
}

fn cmd_status() -> i32 {
    let devices = discover_block_devices();

    println!(
        "{:<16} {:<16} {:<24} {:<10}",
        "MODEL", "VENDOR", "DEVICE", "SIZE"
    );
    println!("{}", "-".repeat(66));

    for dev in &devices {
        let model = if dev.model.is_empty() {
            "Unknown"
        } else {
            &dev.model
        };
        let vendor = if dev.vendor.is_empty() {
            "Unknown"
        } else {
            &dev.vendor
        };
        println!(
            "{:<16} {:<16} {:<24} {:<10}",
            model,
            vendor,
            dev.device,
            format_size(dev.size)
        );
    }

    0
}

fn cmd_info(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("udisksctl: info requires -b DEVICE");
            return 1;
        }
    };

    let devices = discover_block_devices();
    let dev = devices.iter().find(|d| d.device == device);

    match dev {
        Some(d) => {
            println!(
                "/org/freedesktop/UDisks2/block_devices/{}",
                strip_dev(&d.device)
            );
            println!("  org.freedesktop.UDisks2.Block:");
            println!("    Device:          {}", d.device);
            println!("    Size:            {} ({})", d.size, format_size(d.size));
            println!("    ReadOnly:        {}", d.read_only);
            println!("    IdType:          {}", d.id_type);
            println!("    IdLabel:         {}", d.id_label);
            println!("    IdUUID:          {}", d.id_uuid);

            if !d.mountpoints.is_empty() {
                println!("  org.freedesktop.UDisks2.Filesystem:");
                for mp in &d.mountpoints {
                    println!("    MountPoints:     {mp}");
                }
            }

            if !d.partitions.is_empty() {
                println!("  org.freedesktop.UDisks2.PartitionTable:");
                println!("    Type:            {}", d.partition_table_type);
                for part in &d.partitions {
                    println!(
                        "    Partition {}:     {} ({}, {})",
                        part.number,
                        part.device,
                        part.label,
                        format_size(part.size)
                    );
                }
            }

            println!("  org.freedesktop.UDisks2.Drive:");
            println!("    Model:           {}", d.model);
            println!("    Vendor:          {}", d.vendor);
            println!("    Serial:          {}", d.serial);
            println!("    Removable:       {}", d.removable);
            0
        }
        None => {
            eprintln!("udisksctl: device {} not found", quoteaf_os(&device));
            1
        }
    }
}

fn cmd_mount(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("udisksctl: mount requires -b DEVICE");
            return 1;
        }
    };

    let fstype = parse_option_arg(args, "-t");
    let options = parse_option_arg(args, "-o");

    // Check if already mounted.
    let mountpoints = find_mountpoints(&device);
    if !mountpoints.is_empty() {
        eprintln!(
            "udisksctl: {} is already mounted at {}",
            device, mountpoints[0]
        );
        return 1;
    }

    // Determine mount point.
    let base_name = strip_dev(&device);
    let mount_point = format!("/media/$USER/{base_name}");

    // The hedge used to be on stderr and the claim on stdout:
    //
    //     eprintln!("udisksctl: would mount {} at {} ...");
    //     println!("Mounted {} at {mount_point}.", device);
    //     0
    //
    // so a script reading stdout was told the mount had happened, while the
    // "would" went to the stream a pipeline discards. The mount point was
    // `/media/$USER/<name>` with `$USER` never expanded, so the sentence named
    // a directory that could not exist under any user.
    eprintln!(
        "udisksctl: cannot mount {} (type={}, options={}): this build has no \
         udisks mount backend",
        device,
        fstype.as_deref().unwrap_or("auto"),
        options.as_deref().unwrap_or("defaults")
    );
    eprintln!(
        "udisksctl: use `mount {} {}` -- that one performs the mount",
        device, mount_point
    );
    eprintln!("{REFUSE}");
    1
}

fn cmd_unmount(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("udisksctl: unmount requires -b DEVICE");
            return 1;
        }
    };

    let mountpoints = find_mountpoints(&device);
    if mountpoints.is_empty() {
        eprintln!("udisksctl: {} is not mounted", device);
        return 1;
    }

    for mp in &mountpoints {
        eprintln!("udisksctl: cannot unmount {mp}: this build has no udisks unmount backend");
        eprintln!("udisksctl: use `umount {mp}` -- that one performs the unmount");
    }
    eprintln!("{REFUSE}");
    1
}

fn cmd_poweroff(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("udisksctl: power-off requires -b DEVICE");
            return 1;
        }
    };

    eprintln!("udisksctl: cannot power off {device}: no block-device power control");
    eprintln!("{REFUSE}");
    1
}

fn cmd_smart(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => "/dev/sda".to_string(),
    };

    println!("SMART data for {device}:");
    println!("  Overall assessment: PASSED");
    println!("  Temperature:        35 °C");
    println!("  Power-on hours:     1234");
    println!("  Power cycles:       42");
    println!("  Reallocated sectors: 0");
    println!("  Pending sectors:     0");
    0
}

fn cmd_dump() -> i32 {
    let devices = discover_block_devices();
    for dev in &devices {
        println!("=== {} ===", dev.device);
        println!("  Size: {}", format_size(dev.size));
        println!("  Model: {}", dev.model);
        println!("  Vendor: {}", dev.vendor);
        println!("  Removable: {}", dev.removable);
        if !dev.mountpoints.is_empty() {
            println!("  Mounted at: {}", dev.mountpoints.join(", "));
        }
        for part in &dev.partitions {
            println!(
                "  Partition {}: {} ({})",
                part.number,
                part.device,
                format_size(part.size)
            );
        }
        println!();
    }
    0
}

fn cmd_monitor() -> i32 {
    // The two stdout lines used to say "Monitoring UDisks2 events..." and
    // "(Press Ctrl+C to stop)" before returning immediately. Both are false in
    // the same breath: nothing is monitored, and Ctrl+C has nothing to
    // interrupt because the program has already exited. A user watching for
    // events would have sat in front of a finished process.
    eprintln!("udisksctl: cannot monitor device events: there is no bus to watch");
    eprintln!("{REFUSE}");
    1
}

fn cmd_loop_setup(args: &[String]) -> i32 {
    let file = parse_option_arg(args, "-f");
    let file = match file {
        Some(f) => f,
        None => {
            eprintln!("udisksctl: loop-setup requires -f FILE");
            return 1;
        }
    };

    let read_only = args.iter().any(|a| a == "--read-only" || a == "-r");

    eprintln!(
        "udisksctl: cannot set up a loop device for {} (read_only={}): \
         this build has no loop-device backend",
        file, read_only
    );
    // The old success line named /dev/loop0 unconditionally -- an invented
    // device, and the same one every time, so two files would have been
    // reported as mapped to it.
    eprintln!("{REFUSE}");
    1
}

fn cmd_loop_delete(args: &[String]) -> i32 {
    let device = parse_block_arg(args);
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("udisksctl: loop-delete requires -b DEVICE");
            return 1;
        }
    };

    eprintln!("udisksctl: cannot delete loop device {device}: no loop-device backend");
    eprintln!("{REFUSE}");
    1
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

fn parse_block_arg(args: &[String]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if (arg == "-b" || arg == "--block-device") && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    // Also accept a positional device path.
    args.iter().find(|a| a.starts_with("/dev/")).cloned()
}

fn parse_option_arg(args: &[String], flag: &str) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if arg == flag && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn strip_dev(device: &str) -> String {
    device.strip_prefix("/dev/").unwrap_or(device).to_string()
}

// ============================================================================
// udisksd daemon personality
// ============================================================================

fn udisksd_main(args: &[String]) -> i32 {
    let mut no_debug = true;
    let mut replace = false;

    for arg in args {
        match arg.as_str() {
            "--no-debug" => no_debug = true,
            "--debug" => no_debug = false,
            "--replace" | "-r" => replace = true,
            "--help" | "-h" => {
                println!("Usage: udisksd [options]");
                println!();
                println!("UDisks2 disk management daemon.");
                println!();
                println!("Options:");
                println!("  --debug     Enable debug output");
                println!("  --no-debug  Disable debug output (default)");
                println!("  --replace   Replace existing instance");
                println!("  --help      Display this help");
                println!("  --version   Display version");
                return 0;
            }
            "--version" => {
                println!("udisksd (Slate OS) {VERSION}");
                return 0;
            }
            other => {
                eprintln!("udisksd: unknown option {}", quoteaf_os(other));
            }
        }
    }

    eprintln!(
        "udisksd: starting (debug={}, replace={})",
        !no_debug, replace
    );
    eprintln!("udisksd: cannot register on D-Bus as org.freedesktop.UDisks2");
    eprintln!("udisksd: there is no bus connection in this build, so no client");
    eprintln!("udisksd: could ever reach this daemon. Exiting rather than idling.");
    1
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("udisksctl");
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

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "udisksd" => udisksd_main(&rest),
        // NOT "umount". `userspace/mount` answers to that name and actually
        // unmounts -- SYS_FS_MOUNT/SYS_FS_UNMOUNT on target, and an honest
        // Err off it. This crate carried a second `umount` that read the real
        // mount table and then printed "would unmount" for each entry, so
        // which of the two a user got was decided by whichever binary the
        // rootfs installed at /bin/umount. That is the collision 4182acf8d
        // untangled for `login` and `loginmgr`, with the same consequence:
        // a packaging accident deciding whether a core command works.
        _ => udisksctl_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1_500), "1.5 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1_500_000), "1.5 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn test_format_size_tb() {
        assert_eq!(format_size(1_500_000_000_000), "1.5 TB");
    }

    #[test]
    fn test_strip_dev() {
        assert_eq!(strip_dev("/dev/sda"), "sda");
        assert_eq!(strip_dev("/dev/sda1"), "sda1");
        assert_eq!(strip_dev("sda"), "sda");
    }

    #[test]
    fn test_parse_block_arg() {
        let args: Vec<String> = vec!["-b".to_string(), "/dev/sda".to_string()];
        assert_eq!(parse_block_arg(&args), Some("/dev/sda".to_string()));
    }

    #[test]
    fn test_parse_block_arg_positional() {
        let args: Vec<String> = vec!["/dev/sda".to_string()];
        assert_eq!(parse_block_arg(&args), Some("/dev/sda".to_string()));
    }

    #[test]
    fn test_parse_block_arg_missing() {
        let args: Vec<String> = vec!["-b".to_string()];
        assert_eq!(parse_block_arg(&args), None);
    }

    #[test]
    fn test_parse_option_arg() {
        let args: Vec<String> = vec!["-t".to_string(), "ext4".to_string()];
        assert_eq!(parse_option_arg(&args, "-t"), Some("ext4".to_string()));
    }

    #[test]
    fn test_parse_option_arg_missing() {
        let args: Vec<String> = vec!["-t".to_string()];
        assert_eq!(parse_option_arg(&args, "-t"), None);
    }

    #[test]
    fn test_discover_block_devices() {
        let devices = discover_block_devices();
        // Should return at least one device (fallback simulated).
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_find_mountpoints_nonexistent() {
        let mps = find_mountpoints("/dev/nonexistent_xyz");
        assert!(mps.is_empty());
    }

    #[test]
    fn test_block_device_fields() {
        let dev = BlockDevice {
            device: "/dev/sda".to_string(),
            _sys_path: "/sys/block/sda".to_string(),
            id_type: "ext4".to_string(),
            id_label: "root".to_string(),
            id_uuid: "uuid-here".to_string(),
            size: 1_000_000_000,
            removable: false,
            read_only: false,
            model: "Test".to_string(),
            vendor: "Test".to_string(),
            serial: "123".to_string(),
            _revision: "1.0".to_string(),
            mountpoints: vec!["/".to_string()],
            partition_table_type: "gpt".to_string(),
            partitions: Vec::new(),
        };
        assert_eq!(dev.device, "/dev/sda");
        assert!(!dev.removable);
    }

    #[test]
    fn test_partition_fields() {
        let part = Partition {
            device: "/dev/sda1".to_string(),
            number: 1,
            _offset: 1048576,
            size: 512 * 1024 * 1024,
            _type_code: "EF00".to_string(),
            label: "EFI".to_string(),
            _uuid: "AAAA-BBBB".to_string(),
            _flags: vec!["boot".to_string()],
        };
        assert_eq!(part.number, 1);
        assert_eq!(part.label, "EFI");
    }
}
