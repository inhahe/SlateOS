//! OurOS removable media ejection utility.
//!
//! Multi-personality binary providing:
//! - **eject** — eject removable media (CD/DVD, USB, floppy)
//! - **volname** — display volume name of a CD-ROM
//!
//! Controls removable media devices: open/close tray, lock/unlock,
//! toggle auto-eject, and read volume names.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum EjectAction {
    Eject,
    Close,
    ToggleTray,
    Lock,
    Unlock,
    SetAutoEject(bool),
    DisplaySpeed,
    SetSpeed(u32),
}

#[derive(Clone, Debug)]
struct EjectOptions {
    device: String,
    action: EjectAction,
    force: bool,
    verbose: bool,
    no_unmount: bool,
    _proc_mount: bool,
    _removable_only: bool,
}

impl Default for EjectOptions {
    fn default() -> Self {
        Self {
            device: default_device(),
            action: EjectAction::Eject,
            force: false,
            verbose: false,
            no_unmount: false,
            _proc_mount: false,
            _removable_only: true,
        }
    }
}

// ============================================================================
// Device detection
// ============================================================================

fn default_device() -> String {
    // Check common CD/DVD device paths.
    let candidates = [
        "/dev/cdrom",
        "/dev/dvd",
        "/dev/sr0",
        "/dev/sr1",
        "/dev/fd0",
    ];
    for dev in &candidates {
        if std::path::Path::new(dev).exists() {
            return dev.to_string();
        }
    }
    "/dev/cdrom".to_string()
}

fn resolve_device(name: &str) -> String {
    // If it's already an absolute path, use it.
    if name.starts_with('/') {
        return name.to_string();
    }
    // Check if it's a mount point.
    if let Some(dev) = find_device_for_mountpoint(name) {
        return dev;
    }
    // Try prepending /dev/.
    format!("/dev/{name}")
}

fn find_device_for_mountpoint(mountpoint: &str) -> Option<String> {
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == mountpoint {
            return Some(parts[0].to_string());
        }
    }
    None
}

fn find_mountpoint_for_device(device: &str) -> Option<String> {
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    // Also check the canonical path of the device.
    let canonical = fs::canonicalize(device)
        .unwrap_or_else(|_| std::path::PathBuf::from(device));
    let canonical_str = canonical.to_string_lossy();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2
            && (parts[0] == device || parts[0] == canonical_str.as_ref()) {
                return Some(parts[1].to_string());
            }
    }
    None
}

// ============================================================================
// Device info
// ============================================================================

#[derive(Clone, Debug)]
struct DeviceInfo {
    name: String,
    removable: bool,
    _model: String,
    _vendor: String,
    device_type: String,
}

fn get_device_info(device: &str) -> DeviceInfo {
    // Extract base device name (e.g., "sr0" from "/dev/sr0").
    let base_name = device
        .rsplit('/')
        .next()
        .unwrap_or(device);

    let sys_path = format!("/sys/block/{base_name}");

    let removable = fs::read_to_string(format!("{sys_path}/removable"))
        .unwrap_or_default()
        .trim()
        == "1";

    let model = fs::read_to_string(format!("{sys_path}/device/model"))
        .unwrap_or_default()
        .trim()
        .to_string();

    let vendor = fs::read_to_string(format!("{sys_path}/device/vendor"))
        .unwrap_or_default()
        .trim()
        .to_string();

    let dev_type = fs::read_to_string(format!("{sys_path}/device/type"))
        .unwrap_or_else(|_| "0".to_string())
        .trim()
        .to_string();

    let device_type = match dev_type.as_str() {
        "5" => "cd/dvd".to_string(),
        "0" => "disk".to_string(),
        _ => "unknown".to_string(),
    };

    DeviceInfo {
        name: device.to_string(),
        removable,
        _model: model,
        _vendor: vendor,
        device_type,
    }
}

fn list_removable_devices() -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let dev_path = format!("/dev/{name}");
            let info = get_device_info(&dev_path);
            if info.removable {
                devices.push(info);
            }
        }
    }
    devices
}

// ============================================================================
// Volume name reading (ISO 9660)
// ============================================================================

fn read_volume_name(device: &str) -> Option<String> {
    // ISO 9660 primary volume descriptor is at sector 16 (2048 bytes/sector).
    // Volume ID is at offset 40 within the PVD, 32 bytes.
    let data = fs::read(device).ok()?;
    let pvd_offset = 16 * 2048; // Sector 16
    if data.len() < pvd_offset + 40 + 32 {
        return None;
    }

    // Check PVD signature: type 1 at byte 0, "CD001" at bytes 1-5.
    if data[pvd_offset] != 1 {
        return None;
    }
    if &data[pvd_offset + 1..pvd_offset + 6] != b"CD001" {
        return None;
    }

    // Volume identifier at offset 40, length 32.
    let vol_id = &data[pvd_offset + 40..pvd_offset + 72];
    let name = String::from_utf8_lossy(vol_id).trim().to_string();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

// ============================================================================
// Unmount helper
// ============================================================================

fn unmount_device(device: &str, verbose: bool) -> bool {
    if let Some(mountpoint) = find_mountpoint_for_device(device) {
        if verbose {
            eprintln!("eject: unmounting {mountpoint}");
        }
        // In a real OS, call umount syscall.
        eprintln!("eject: would call umount(\"{}\")", mountpoint);
        true
    } else {
        // Not mounted.
        true
    }
}

// ============================================================================
// Eject operations (stubs for actual ioctl calls)
// ============================================================================

fn do_eject(opts: &EjectOptions) -> i32 {
    let device = &opts.device;
    let info = get_device_info(device);

    if opts.verbose {
        eprintln!("eject: device '{}' is {} (removable={})",
            device, info.device_type, info.removable);
    }

    if !opts.force && !info.removable {
        eprintln!("eject: {device} is not a removable device");
        eprintln!("eject: use --force to override");
        return 1;
    }

    match &opts.action {
        EjectAction::Eject => {
            if !opts.no_unmount
                && !unmount_device(device, opts.verbose) {
                    eprintln!("eject: unmount of {device} failed");
                    if !opts.force {
                        return 1;
                    }
                }
            if opts.verbose {
                eprintln!("eject: ejecting {device}");
            }
            // Would call ioctl(fd, CDROMEJECT, 0) or similar.
            eprintln!("eject: would call CDROMEJECT on {device}");
            0
        }
        EjectAction::Close => {
            if opts.verbose {
                eprintln!("eject: closing tray on {device}");
            }
            eprintln!("eject: would call CDROMCLOSETRAY on {device}");
            0
        }
        EjectAction::ToggleTray => {
            if opts.verbose {
                eprintln!("eject: toggling tray on {device}");
            }
            eprintln!("eject: would toggle tray on {device}");
            0
        }
        EjectAction::Lock => {
            if opts.verbose {
                eprintln!("eject: locking {device}");
            }
            eprintln!("eject: would call CDROM_LOCKDOOR(1) on {device}");
            0
        }
        EjectAction::Unlock => {
            if opts.verbose {
                eprintln!("eject: unlocking {device}");
            }
            eprintln!("eject: would call CDROM_LOCKDOOR(0) on {device}");
            0
        }
        EjectAction::SetAutoEject(enable) => {
            let state = if *enable { "on" } else { "off" };
            if opts.verbose {
                eprintln!("eject: setting auto-eject {state} on {device}");
            }
            eprintln!("eject: would call CDROMEJECT_SW({}) on {device}",
                if *enable { 1 } else { 0 });
            0
        }
        EjectAction::DisplaySpeed => {
            if opts.verbose {
                eprintln!("eject: querying speed of {device}");
            }
            eprintln!("eject: would query CDROM_SELECT_SPEED on {device}");
            0
        }
        EjectAction::SetSpeed(speed) => {
            if opts.verbose {
                eprintln!("eject: setting speed to {speed}x on {device}");
            }
            eprintln!("eject: would call CDROM_SELECT_SPEED({speed}) on {device}");
            0
        }
    }
}

// ============================================================================
// eject personality
// ============================================================================

fn eject_main(args: &[String]) -> i32 {
    let mut opts = EjectOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-t" | "--trayclose" => opts.action = EjectAction::Close,
            "-T" | "--traytoggle" => opts.action = EjectAction::ToggleTray,
            "-i" | "--manualeject" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "on" | "1" => opts.action = EjectAction::SetAutoEject(true),
                        _ => opts.action = EjectAction::SetAutoEject(false),
                    }
                }
            }
            "-l" | "--lock" => opts.action = EjectAction::Lock,
            "-L" | "--unlock" => opts.action = EjectAction::Unlock,
            "-x" | "--cdspeed" => {
                i += 1;
                if i < args.len() {
                    if let Ok(speed) = args[i].parse::<u32>() {
                        opts.action = EjectAction::SetSpeed(speed);
                    } else {
                        opts.action = EjectAction::DisplaySpeed;
                    }
                }
            }
            "-f" | "--force" => opts.force = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-n" | "--noop" | "--no-unmount" => opts.no_unmount = true,
            "-d" | "--default" => {
                println!("eject: default device: {}", default_device());
                return 0;
            }
            "-p" | "--proc" => opts._proc_mount = true,
            "--list" => {
                let devices = list_removable_devices();
                if devices.is_empty() {
                    println!("No removable devices found.");
                } else {
                    for dev in &devices {
                        println!("{} ({})", dev.name, dev.device_type);
                    }
                }
                return 0;
            }
            "--help" | "-h" => {
                println!("Usage: eject [options] [device|mountpoint]");
                println!();
                println!("Eject removable media.");
                println!();
                println!("Options:");
                println!("  -t, --trayclose    Close the tray");
                println!("  -T, --traytoggle   Toggle tray open/close");
                println!("  -l, --lock         Lock the drive door");
                println!("  -L, --unlock       Unlock the drive door");
                println!("  -x, --cdspeed N    Set CD-ROM speed");
                println!("  -f, --force        Force eject (non-removable)");
                println!("  -v, --verbose      Verbose output");
                println!("  -n, --no-unmount   Don't unmount before ejecting");
                println!("  -d, --default      Display default device");
                println!("  --list             List removable devices");
                println!("  -h, --help         Display this help");
                println!("  --version          Display version");
                return 0;
            }
            "--version" => {
                println!("eject (OurOS coreutils) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                opts.device = resolve_device(s);
            }
            other => {
                eprintln!("eject: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    do_eject(&opts)
}

// ============================================================================
// volname personality
// ============================================================================

fn volname_main(args: &[String]) -> i32 {
    let device = if args.is_empty() {
        default_device()
    } else {
        match args[0].as_str() {
            "--help" | "-h" => {
                println!("Usage: volname [device]");
                println!();
                println!("Display the volume name of a CD-ROM.");
                println!("Default device: {}", default_device());
                return 0;
            }
            "--version" => {
                println!("volname (OurOS coreutils) {VERSION}");
                return 0;
            }
            s => resolve_device(s),
        }
    };

    match read_volume_name(&device) {
        Some(name) => {
            println!("{name}");
            0
        }
        None => {
            eprintln!("volname: cannot read volume name from {device}");
            1
        }
    }
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("eject");
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
        "volname" => volname_main(&rest),
        _ => eject_main(&rest),
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
    fn test_resolve_device_absolute() {
        assert_eq!(resolve_device("/dev/sr0"), "/dev/sr0");
    }

    #[test]
    fn test_resolve_device_relative() {
        let result = resolve_device("sr0");
        assert!(result.contains("sr0"));
    }

    #[test]
    fn test_default_device() {
        let dev = default_device();
        assert!(!dev.is_empty());
    }

    #[test]
    fn test_device_info_nonexistent() {
        let info = get_device_info("/dev/nonexistent_device_xyz");
        assert!(!info.removable);
    }

    #[test]
    fn test_find_mountpoint_nonexistent() {
        let mp = find_mountpoint_for_device("/dev/nonexistent_device_xyz");
        // Should be None — no such device mounted.
        assert!(mp.is_none());
    }

    #[test]
    fn test_find_device_for_mountpoint_nonexistent() {
        let dev = find_device_for_mountpoint("/nonexistent_mountpoint_xyz");
        assert!(dev.is_none());
    }

    #[test]
    fn test_list_removable_devices() {
        // Should not panic regardless of system state.
        let _devices = list_removable_devices();
    }

    #[test]
    fn test_read_volume_name_nonexistent() {
        let name = read_volume_name("/dev/nonexistent_device_xyz");
        assert!(name.is_none());
    }

    #[test]
    fn test_eject_options_default() {
        let opts = EjectOptions::default();
        assert_eq!(opts.action, EjectAction::Eject);
        assert!(!opts.force);
        assert!(!opts.verbose);
        assert!(!opts.no_unmount);
    }

    #[test]
    fn test_unmount_nonexistent() {
        // Should succeed (not mounted = nothing to unmount).
        assert!(unmount_device("/dev/nonexistent_device_xyz", false));
    }
}
