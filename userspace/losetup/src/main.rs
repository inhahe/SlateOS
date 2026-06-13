//! Slate OS loop device management utility.
//!
//! Multi-personality binary providing:
//! - **losetup** — set up and control loop devices
//! - **lodetach** — detach loop devices (alias for losetup -d)
//!
//! Loop devices allow regular files to be accessed as block devices.
//! Manages `/dev/loop*` devices via `/sys/block/loop*/` sysfs interface
//! and ioctl-like control through `/dev/loop-control`.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const LOOP_CONTROL: &str = "/dev/loop-control";
const SYS_BLOCK: &str = "/sys/block";
const MAX_LOOP_DEVICES: u32 = 256;
/// Kernel-exported table of active swap areas (one per line after the header).
const PROC_SWAPS: &str = "/proc/swaps";

// ============================================================================
// Data structures
// ============================================================================

/// Information about a loop device.
#[derive(Clone, Debug)]
struct LoopInfo {
    /// Device path (e.g., /dev/loop0).
    device: String,
    /// Loop device number.
    number: u32,
    /// Backing file path (empty if not attached).
    backing_file: String,
    /// Offset into the backing file.
    offset: u64,
    /// Size limit (0 = no limit).
    sizelimit: u64,
    /// Whether the device is read-only.
    read_only: bool,
    /// Whether autoclear is set.
    autoclear: bool,
    /// Whether direct I/O is enabled.
    dio: bool,
    /// Partition scan enabled.
    partscan: bool,
}

/// Options for setting up a loop device.
struct SetupOptions {
    /// Specific loop device to use (None = auto-find).
    device: Option<String>,
    /// Backing file path.
    file: String,
    /// Offset into the file.
    offset: u64,
    /// Size limit.
    sizelimit: u64,
    /// Read-only mode.
    read_only: bool,
    /// Enable partition scanning.
    partscan: bool,
    /// Enable direct I/O.
    direct_io: bool,
    /// Show the assigned device path.
    show: bool,
    /// Find an existing loop for a file.
    find: bool,
}

// ============================================================================
// Loop device enumeration via sysfs
// ============================================================================

/// Enumerate all loop devices from /sys/block/loop*.
fn enumerate_loop_devices() -> Vec<LoopInfo> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir(SYS_BLOCK) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = match name.to_str() {
                Some(s) => s.to_string(),
                None => continue,
            };

            if !name_str.starts_with("loop") {
                continue;
            }

            let num_str = &name_str[4..];
            let number: u32 = match num_str.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };

            let info = read_loop_info(number);
            devices.push(info);
        }
    }

    devices.sort_by_key(|d| d.number);
    devices
}

/// Read info about a specific loop device from sysfs.
fn read_loop_info(number: u32) -> LoopInfo {
    let sysfs_dir = format!("{SYS_BLOCK}/loop{number}/loop");
    let device = format!("/dev/loop{number}");

    let backing_file = read_sysfs_str(&format!("{sysfs_dir}/backing_file"));
    let offset = read_sysfs_u64(&format!("{sysfs_dir}/offset"));
    let sizelimit = read_sysfs_u64(&format!("{sysfs_dir}/sizelimit"));
    let autoclear = read_sysfs_bool(&format!("{sysfs_dir}/autoclear"));
    let dio = read_sysfs_bool(&format!("{sysfs_dir}/dio"));
    let partscan = read_sysfs_bool(&format!("{sysfs_dir}/partscan"));
    let ro = read_sysfs_bool(&format!("{SYS_BLOCK}/loop{number}/ro"));

    LoopInfo {
        device,
        number,
        backing_file,
        offset,
        sizelimit,
        read_only: ro,
        autoclear,
        dio,
        partscan,
    }
}

fn read_sysfs_str(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_sysfs_u64(path: &str) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn read_sysfs_bool(path: &str) -> bool {
    read_sysfs_u64(path) != 0
}

/// Return `true` if `device` is currently in use as an active swap area.
///
/// Reads `/proc/swaps`, whose first column is the swap area path. A loop
/// device backing an active swap must not be detached, so the detach path
/// consults this before tearing the device down. A missing or unreadable
/// `/proc/swaps` (e.g. the device path simply isn't listed) means "not
/// active", which is the safe default for the lookup itself — the detach
/// will still be attempted and the kernel remains the final arbiter.
fn is_swap_active(device: &str) -> bool {
    let Ok(content) = fs::read_to_string(PROC_SWAPS) else {
        return false;
    };
    // Skip the header line; match the first whitespace-delimited field.
    content
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .any(|filename| filename == device)
}

/// Find the first available (unattached) loop device.
fn find_free_loop() -> Option<u32> {
    // First try the loop-control device.
    if let Ok(content) = fs::read_to_string(LOOP_CONTROL)
        && let Ok(n) = content.trim().parse::<u32>()
    {
        return Some(n);
    }

    // Fallback: scan existing devices.
    for i in 0..MAX_LOOP_DEVICES {
        let info = read_loop_info(i);
        if info.backing_file.is_empty() {
            return Some(i);
        }
    }

    None
}

/// Find a loop device associated with a specific file.
fn find_loop_for_file(file: &str) -> Option<LoopInfo> {
    let devices = enumerate_loop_devices();
    // Resolve the target file's canonical path.
    let canonical = fs::canonicalize(file)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file.to_string());

    devices
        .into_iter()
        .find(|dev| dev.backing_file == canonical || dev.backing_file == file)
}

// ============================================================================
// Size formatting
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0".to_string();
    }
    if bytes >= 1_099_511_627_776 {
        let tib = bytes as f64 / 1_099_511_627_776.0;
        format!("{tib:.1}T")
    } else if bytes >= 1_073_741_824 {
        let gib = bytes as f64 / 1_073_741_824.0;
        format!("{gib:.1}G")
    } else if bytes >= 1_048_576 {
        let mib = bytes as f64 / 1_048_576.0;
        format!("{mib:.1}M")
    } else if bytes >= 1024 {
        let kib = bytes as f64 / 1024.0;
        format!("{kib:.1}K")
    } else {
        format!("{bytes}")
    }
}

/// Parse a size string with optional suffix: 512, 1K, 4M, 1G, etc.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('T') {
        (n, 1_099_511_627_776u64)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1_073_741_824)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1_048_576)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024)
    } else {
        (s, 1)
    };

    // A raw byte count (no suffix) must be an exact integer. A suffixed value
    // may carry a fractional part — e.g. "1.5G", and crucially the decimal
    // forms that `format_size` itself emits ("1.0K") — so scale a float by the
    // multiplier and round to the nearest byte. Integer-with-suffix stays exact.
    if multiplier == 1 {
        num_str.parse::<u64>().ok()
    } else if let Ok(n) = num_str.parse::<u64>() {
        n.checked_mul(multiplier)
    } else {
        let f = num_str.parse::<f64>().ok()?;
        if !f.is_finite() || f < 0.0 {
            return None;
        }
        let bytes = f * multiplier as f64;
        if bytes > u64::MAX as f64 {
            return None;
        }
        Some(bytes.round() as u64)
    }
}

// ============================================================================
// JSON output
// ============================================================================

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn print_loop_json(devices: &[LoopInfo]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"loopdevices\": [");
    for (i, dev) in devices.iter().enumerate() {
        let comma = if i + 1 < devices.len() { "," } else { "" };
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "      \"name\": \"{}\",", json_escape(&dev.device));
        let _ = writeln!(
            out,
            "      \"back-file\": \"{}\",",
            json_escape(&dev.backing_file)
        );
        let _ = writeln!(out, "      \"offset\": {},", dev.offset);
        let _ = writeln!(out, "      \"sizelimit\": {},", dev.sizelimit);
        let _ = writeln!(out, "      \"ro\": {},", dev.read_only);
        let _ = writeln!(out, "      \"autoclear\": {},", dev.autoclear);
        let _ = writeln!(out, "      \"dio\": {},", dev.dio);
        let _ = writeln!(out, "      \"partscan\": {}", dev.partscan);
        let _ = writeln!(out, "    }}{comma}");
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_losetup(args: &[String]) {
    let mut list_all = false;
    let mut detach: Option<String> = None;
    let mut detach_all = false;
    let mut json_output = false;
    let mut verbose = false;
    let mut find_flag = false;
    let mut setup_file: Option<String> = None;
    let mut setup_device: Option<String> = None;
    let mut offset: u64 = 0;
    let mut sizelimit: u64 = 0;
    let mut read_only = false;
    let mut partscan = false;
    let mut direct_io = false;
    let mut show_flag = false;
    let mut associated: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("losetup {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => list_all = true,
            "-l" | "--list" => list_all = true,
            "-f" | "--find" => find_flag = true,
            "-j" | "--associated" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("losetup: -j requires a file argument");
                    process::exit(1);
                }
                associated = Some(args[i].clone());
            }
            "-d" | "--detach" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("losetup: -d requires a device argument");
                    process::exit(1);
                }
                detach = Some(args[i].clone());
            }
            "-D" | "--detach-all" => detach_all = true,
            "-o" | "--offset" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("losetup: -o requires a size argument");
                    process::exit(1);
                }
                offset = parse_size(&args[i]).unwrap_or_else(|| {
                    eprintln!("losetup: invalid offset: {}", args[i]);
                    process::exit(1);
                });
            }
            "--sizelimit" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("losetup: --sizelimit requires a size argument");
                    process::exit(1);
                }
                sizelimit = parse_size(&args[i]).unwrap_or_else(|| {
                    eprintln!("losetup: invalid sizelimit: {}", args[i]);
                    process::exit(1);
                });
            }
            "-r" | "--read-only" => read_only = true,
            "-P" | "--partscan" => partscan = true,
            "--direct-io" => direct_io = true,
            "--show" => show_flag = true,
            "-J" | "--json" => json_output = true,
            "-v" | "--verbose" => verbose = true,
            s if !s.starts_with('-') => {
                if setup_device.is_none() && s.starts_with("/dev/loop") {
                    setup_device = Some(s.to_string());
                } else if setup_file.is_none() {
                    setup_file = Some(s.to_string());
                } else {
                    eprintln!("losetup: unexpected argument: {s}");
                    process::exit(1);
                }
            }
            other => {
                eprintln!("losetup: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    // Detach operations.
    if detach_all {
        do_detach_all(verbose);
        return;
    }

    if let Some(ref dev) = detach {
        do_detach(dev, verbose);
        return;
    }

    // Find associated device for a file.
    if let Some(ref file) = associated {
        do_associated(file, json_output);
        return;
    }

    // List all loop devices.
    if list_all {
        do_list(json_output);
        return;
    }

    // If -f without a file, just print the next free device.
    if find_flag && setup_file.is_none() {
        match find_free_loop() {
            Some(n) => println!("/dev/loop{n}"),
            None => {
                eprintln!("losetup: cannot find a free loop device");
                process::exit(1);
            }
        }
        return;
    }

    // Setup: either explicit device + file, or -f + file.
    if let Some(file) = setup_file {
        let opts = SetupOptions {
            device: setup_device,
            file,
            offset,
            sizelimit,
            read_only,
            partscan,
            direct_io,
            show: show_flag || find_flag,
            find: find_flag,
        };
        do_setup(&opts, verbose);
        return;
    }

    // If a specific device is given without a file, show info about it.
    if let Some(ref dev) = setup_device {
        do_show_device(dev, json_output);
        return;
    }

    // Default: list all.
    if args.is_empty() {
        do_list(json_output);
        return;
    }

    eprintln!("losetup: no operation specified");
    eprintln!("Try 'losetup --help' for more information.");
    process::exit(1);
}

fn print_usage() {
    println!("Usage:");
    println!("  losetup [options] [loopdev]");
    println!("  losetup [options] -f [file]       Find and setup a free loop device");
    println!("  losetup [options] loopdev file     Setup loop device");
    println!("  losetup -d loopdev                 Detach loop device");
    println!("  losetup -D                         Detach all loop devices");
    println!("  losetup -a                         List all loop devices");
    println!("  losetup -j file                    Show loop device for file");
    println!();
    println!("Options:");
    println!("  -a, --all              List all used loop devices");
    println!("  -d, --detach <dev>     Detach loop device");
    println!("  -D, --detach-all       Detach all loop devices");
    println!("  -f, --find             Find first unused device (or auto-setup)");
    println!("  -j, --associated <f>   Show loop device associated with file");
    println!("  -o, --offset <n>       Offset into the file");
    println!("  --sizelimit <n>        Limit size of the data");
    println!("  -r, --read-only        Set up read-only loop device");
    println!("  -P, --partscan         Create a partitioned loop device");
    println!("  --direct-io            Enable direct I/O");
    println!("  --show                 Print device name after setup");
    println!("  -J, --json             JSON output");
    println!("  -v, --verbose          Verbose mode");
    println!("  -h, --help             Show this help");
    println!("  -V, --version          Show version");
}

fn do_list(json: bool) {
    let devices = enumerate_loop_devices();
    let active: Vec<LoopInfo> = devices
        .into_iter()
        .filter(|d| !d.backing_file.is_empty())
        .collect();

    if json {
        print_loop_json(&active);
    } else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = writeln!(
            out,
            "{:<20} {:>6} {:>10} {:>10} {:>4} {:>5} {:>3} BACK-FILE",
            "NAME", "OFFSET", "SIZELIMIT", "FILESIZE", "RO", "AUTOCL", "DIO"
        );

        for dev in &active {
            let file_size = fs::metadata(&dev.backing_file)
                .map(|m| m.len())
                .unwrap_or(0);

            let _ = writeln!(
                out,
                "{:<20} {:>6} {:>10} {:>10} {:>4} {:>5} {:>3} {}",
                dev.device,
                dev.offset,
                if dev.sizelimit > 0 {
                    format_size(dev.sizelimit)
                } else {
                    "0".into()
                },
                format_size(file_size),
                if dev.read_only { "1" } else { "0" },
                if dev.autoclear { "1" } else { "0" },
                if dev.dio { "1" } else { "0" },
                dev.backing_file
            );
        }
    }
}

fn do_show_device(device: &str, json: bool) {
    // Extract loop number from device path.
    let num_str = device.strip_prefix("/dev/loop").unwrap_or(device);
    let number: u32 = match num_str.parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("losetup: {device}: invalid loop device");
            process::exit(1);
        }
    };

    let info = read_loop_info(number);
    if info.backing_file.is_empty() {
        eprintln!("losetup: {device}: not attached");
        process::exit(1);
    }

    if json {
        print_loop_json(&[info]);
    } else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = writeln!(
            out,
            "{}: []:({}) {}",
            info.device, info.offset, info.backing_file
        );
    }
}

fn do_associated(file: &str, json: bool) {
    if let Some(info) = find_loop_for_file(file) {
        if json {
            print_loop_json(&[info]);
        } else {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "{}: []:({}) {}",
                info.device, info.offset, info.backing_file
            );
        }
    }
    // No output if no association found (matching Linux behavior).
}

fn do_setup(opts: &SetupOptions, verbose: bool) {
    // Verify the backing file exists.
    if !std::path::Path::new(&opts.file).exists() {
        eprintln!("losetup: {}: No such file or directory", opts.file);
        process::exit(1);
    }

    let device = if let Some(ref dev) = opts.device {
        dev.clone()
    } else if opts.find {
        match find_free_loop() {
            Some(n) => format!("/dev/loop{n}"),
            None => {
                eprintln!("losetup: cannot find a free loop device");
                process::exit(1);
            }
        }
    } else {
        eprintln!("losetup: no loop device specified (use -f to find one)");
        process::exit(1);
    };

    // In a real kernel, this would use ioctl(LOOP_SET_FD, LOOP_SET_STATUS64).
    // We simulate by writing to a control file.
    let setup_cmd = format!(
        "setup {} file={} offset={} sizelimit={} ro={} partscan={} dio={}",
        device,
        opts.file,
        opts.offset,
        opts.sizelimit,
        opts.read_only as u8,
        opts.partscan as u8,
        opts.direct_io as u8,
    );

    match fs::write(LOOP_CONTROL, &setup_cmd) {
        Ok(()) => {
            if verbose {
                eprintln!("losetup: {device}: attached to {}", opts.file);
            }
            if opts.show {
                println!("{device}");
            }
        }
        Err(e) => {
            eprintln!("losetup: {device}: failed to set up: {e}");
            process::exit(1);
        }
    }
}

fn do_detach(device: &str, verbose: bool) {
    // Refuse to detach a device that is backing an active swap area: tearing
    // it down would yank memory out from under the kernel's swap subsystem.
    if is_swap_active(device) {
        eprintln!("losetup: {device}: in use as swap, refusing to detach");
        process::exit(1);
    }
    let detach_cmd = format!("detach {device}");
    match fs::write(LOOP_CONTROL, &detach_cmd) {
        Ok(()) => {
            if verbose {
                eprintln!("losetup: {device}: detached");
            }
        }
        Err(e) => {
            eprintln!("losetup: {device}: failed to detach: {e}");
            process::exit(1);
        }
    }
}

fn do_detach_all(verbose: bool) {
    let devices = enumerate_loop_devices();
    for dev in &devices {
        if !dev.backing_file.is_empty() {
            do_detach(&dev.device, verbose);
        }
    }
}

// ============================================================================
// Personality: lodetach (alias for losetup -d)
// ============================================================================

fn cmd_lodetach(args: &[String]) {
    if args.is_empty() {
        eprintln!("lodetach: no device specified");
        eprintln!("Usage: lodetach <device>");
        process::exit(1);
    }

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: lodetach <device>...");
                println!("Detach one or more loop devices.");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lodetach {VERSION}");
                process::exit(0);
            }
            _ => {
                do_detach(arg, true);
            }
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("losetup");
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

    match prog_name.as_str() {
        "lodetach" => cmd_lodetach(&rest),
        _ => cmd_losetup(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0");
        assert_eq!(format_size(512), "512");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(1048576), "1.0M");
        assert_eq!(format_size(1073741824), "1.0G");
        assert_eq!(format_size(1099511627776), "1.0T");
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("0"), Some(0));
        assert_eq!(parse_size("512"), Some(512));
        assert_eq!(parse_size("1K"), Some(1024));
        assert_eq!(parse_size("4M"), Some(4 * 1048576));
        assert_eq!(parse_size("2G"), Some(2 * 1073741824));
        assert_eq!(parse_size("1T"), Some(1099511627776));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn test_parse_size_roundtrip() {
        let sizes = [0u64, 512, 1024, 1048576, 1073741824, 1099511627776];
        for &s in &sizes {
            let formatted = format_size(s);
            // format_size and parse_size don't perfectly roundtrip for all values
            // due to formatting precision, but they should be reasonable.
            if s >= 1024 {
                assert!(parse_size(&formatted).is_some());
            }
        }
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
        assert_eq!(json_escape("line\nnew"), "line\\nnew");
        assert_eq!(json_escape("/dev/loop0"), "/dev/loop0");
    }

    #[test]
    fn test_loop_info_defaults() {
        let info = LoopInfo {
            device: "/dev/loop0".to_string(),
            number: 0,
            backing_file: String::new(),
            offset: 0,
            sizelimit: 0,
            read_only: false,
            autoclear: false,
            dio: false,
            partscan: false,
        };
        assert!(info.backing_file.is_empty());
        assert_eq!(info.number, 0);
        assert!(!info.read_only);
    }

    #[test]
    fn test_loop_info_attached() {
        let info = LoopInfo {
            device: "/dev/loop5".to_string(),
            number: 5,
            backing_file: "/home/user/disk.img".to_string(),
            offset: 1048576,
            sizelimit: 10485760,
            read_only: true,
            autoclear: true,
            dio: false,
            partscan: true,
        };
        assert!(!info.backing_file.is_empty());
        assert_eq!(info.number, 5);
        assert!(info.read_only);
        assert!(info.autoclear);
        assert!(info.partscan);
        assert!(!info.dio);
        assert_eq!(info.offset, 1048576);
        assert_eq!(info.sizelimit, 10485760);
    }

    #[test]
    fn test_setup_options() {
        let opts = SetupOptions {
            device: Some("/dev/loop0".to_string()),
            file: "/tmp/disk.img".to_string(),
            offset: 0,
            sizelimit: 0,
            read_only: false,
            partscan: false,
            direct_io: false,
            show: false,
            find: false,
        };
        assert_eq!(opts.file, "/tmp/disk.img");
        assert!(!opts.read_only);
    }

    #[test]
    fn test_enumerate_returns_vec() {
        // enumerate_loop_devices reads /sys/block which may not exist.
        let devices = enumerate_loop_devices();
        // Should not panic; may return empty vec.
        let _ = devices.len();
    }

    #[test]
    fn test_find_loop_for_nonexistent() {
        let result = find_loop_for_file("/nonexistent/file.img");
        assert!(result.is_none());
    }

    #[test]
    fn test_format_size_edge_cases() {
        assert_eq!(format_size(1), "1");
        assert_eq!(format_size(1023), "1023");
        assert_eq!(
            format_size(u64::MAX),
            format!("{:.1}T", u64::MAX as f64 / 1_099_511_627_776.0)
        );
    }

    #[test]
    fn test_personality_detection() {
        let test_cases = [
            ("/usr/sbin/losetup", "losetup"),
            ("lodetach", "lodetach"),
            ("/bin/losetup.exe", "losetup"),
            ("C:\\tools\\lodetach.exe", "lodetach"),
        ];

        for (input, expected) in &test_cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let basename = &input[last_sep..];
            let basename = basename.strip_suffix(".exe").unwrap_or(basename);
            assert_eq!(basename, *expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_read_sysfs_str_missing() {
        assert_eq!(read_sysfs_str("/nonexistent/path"), "");
    }

    #[test]
    fn test_read_sysfs_u64_missing() {
        assert_eq!(read_sysfs_u64("/nonexistent/path"), 0);
    }

    #[test]
    fn test_read_sysfs_bool_missing() {
        assert!(!read_sysfs_bool("/nonexistent/path"));
    }

    #[test]
    fn test_is_swap_active_nonexistent() {
        // is_swap_active looks at /proc/swaps; a nonexistent device should not be active.
        assert!(!is_swap_active("/dev/nonexistent_loop_device_xyz"));
    }
}
