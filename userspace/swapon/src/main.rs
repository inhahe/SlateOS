//! Slate OS swap and memory management utilities.
//!
//! Multi-personality binary providing:
//! - **swapon** — enable swap space
//! - **swapoff** — disable swap space
//! - **free** — display memory and swap usage
//!
//! Reads `/proc/meminfo`, `/proc/swaps`, and `/etc/fstab` for system memory
//! and swap configuration.

#![deny(clippy::all)]

use quoting::quotef_os;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const PROC_SWAPS: &str = "/proc/swaps";
const FSTAB_PATH: &str = "/etc/fstab";

// ============================================================================
// Data structures
// ============================================================================

/// A swap entry from /proc/swaps.
#[derive(Clone, Debug)]
struct SwapEntry {
    filename: String,
    swap_type: String,
    size_kb: u64,
    used_kb: u64,
    priority: i32,
}

/// Fstab entry.
struct FstabEntry {
    device: String,
    /// Mirrors the `/etc/fstab` field vocabulary; parsed, not yet used.
    #[allow(dead_code)]
    mountpoint: String,
    fstype: String,
    /// Mirrors the `/etc/fstab` field vocabulary; parsed, not yet used.
    #[allow(dead_code)]
    options: String,
    _dump: u32,
    _pass: u32,
}

// ============================================================================
// Parsing
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn parse_proc_swaps() -> Vec<SwapEntry> {
    let content = match read_file(PROC_SWAPS) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 5 {
            entries.push(SwapEntry {
                filename: fields[0].to_string(),
                swap_type: fields[1].to_string(),
                size_kb: fields[2].parse().unwrap_or(0),
                used_kb: fields[3].parse().unwrap_or(0),
                priority: fields[4].parse().unwrap_or(-1),
            });
        }
    }
    entries
}

fn parse_fstab() -> Vec<FstabEntry> {
    let content = match read_file(FSTAB_PATH) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 {
            entries.push(FstabEntry {
                device: fields[0].to_string(),
                mountpoint: fields[1].to_string(),
                fstype: fields[2].to_string(),
                options: fields[3].to_string(),
                _dump: fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                _pass: fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }
    entries
}

fn get_swap_fstab_entries() -> Vec<FstabEntry> {
    parse_fstab()
        .into_iter()
        .filter(|e| e.fstype == "swap")
        .collect()
}

fn is_swap_active(device: &str) -> bool {
    let active = parse_proc_swaps();
    active.iter().any(|s| s.filename == device)
}

// ============================================================================
// Size formatting
// ============================================================================

// ============================================================================
// Personality: swapon
// ============================================================================

fn cmd_swapon(args: &[String]) {
    let mut show_summary = false;
    let mut all_flag = false;
    let mut priority: Option<i32> = None;
    let mut discard = false;
    let mut verbose = false;
    let mut devices: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: swapon [options] [device...]");
                println!();
                println!("Enable devices and files for paging and swapping.");
                println!();
                println!("Options:");
                println!("  -a, --all              Enable all swaps from /etc/fstab");
                println!("  -d, --discard          Enable discard/TRIM on swap");
                println!("  -p, --priority <prio>  Set swap priority (-1 to 32767)");
                println!("  -s, --summary          Display swap usage summary");
                println!("  --show                 Display swap entries (default cols)");
                println!("  -v, --verbose          Verbose output");
                println!("  -h, --help             Show this help");
                println!("  --version              Show version");
                process::exit(0);
            }
            "--version" => {
                println!("swapon {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all_flag = true,
            "-d" | "--discard" => discard = true,
            "-s" | "--summary" | "--show" => show_summary = true,
            "-v" | "--verbose" => verbose = true,
            "-p" | "--priority" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("swapon: -p requires an argument");
                    process::exit(1);
                }
                priority = Some(parse_priority(&args[i]).unwrap_or_else(|msg| {
                    eprintln!("swapon: {msg}");
                    process::exit(1);
                }));
            }
            s if !s.starts_with('-') => {
                devices.push(s.to_string());
            }
            other => {
                eprintln!("swapon: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    // Default action: show summary if no args.
    if args.is_empty() || show_summary {
        show_swap_summary();
        if args.is_empty() || (show_summary && devices.is_empty() && !all_flag) {
            return;
        }
    }

    if all_flag {
        // Enable all swap entries from fstab.
        let fstab_swaps = get_swap_fstab_entries();
        if fstab_swaps.is_empty() {
            eprintln!("swapon: no swap entries found in {FSTAB_PATH}");
            return;
        }
        for entry in &fstab_swaps {
            if is_swap_active(&entry.device) {
                if verbose {
                    println!("swapon: {}: already active", entry.device);
                }
                continue;
            }
            activate_swap(&entry.device, priority, discard, verbose);
        }
    } else if devices.is_empty() {
        // No devices and no -a: already showed summary above.
    } else {
        for device in &devices {
            if is_swap_active(device) {
                eprintln!("swapon: {}: already active", quotef_os(device));
                continue;
            }
            activate_swap(device, priority, discard, verbose);
        }
    }
}

fn show_swap_summary() {
    let entries = parse_proc_swaps();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(
        out,
        "{:<40} {:>6} {:>10} {:>10} {:>5}",
        "Filename", "Type", "Size", "Used", "Priority"
    );

    for e in &entries {
        let _ = writeln!(
            out,
            "{:<40} {:>6} {:>10} {:>10} {:>5}",
            e.filename, e.swap_type, e.size_kb, e.used_kb, e.priority
        );
    }
}

/// A `-p` argument, or why it is not one.
///
/// **The range check is the point.** The priority is carried in the low 15
/// bits of `swapflags`, so `swap_flags` masks it -- and a value outside
/// `0..=32767` does not fail, it *becomes a different priority*. `-p -1`
/// masked to `0x7FFF`, which is the **maximum**: the caller asked for the
/// lowest and would have got the highest. `-p 99999` became 1695. Both were
/// accepted silently.
///
/// Rejecting out-of-range values is also what `util-linux` does, and it is the
/// only way the flag word can be built by masking without the mask changing
/// the answer.
fn parse_priority(arg: &str) -> Result<i32, String> {
    let Ok(p) = arg.parse::<i32>() else {
        return Err(format!("invalid priority: {arg}"));
    };
    if !(0..=libcall::SWAP_FLAG_PRIO_MASK).contains(&p) {
        return Err(format!(
            "priority out of range: {p} (must be 0..={})",
            libcall::SWAP_FLAG_PRIO_MASK
        ));
    }
    Ok(p)
}

/// The `swapflags` word `swapon(2)` takes, built from the options given.
///
/// Split out because it is the only part of activation that decides anything;
/// the rest is a libc call. `SWAP_FLAG_PREFER` is what makes the low 15 bits
/// mean a priority at all -- without it the kernel ignores them, so a
/// `-p` that set the bits and not the flag would be silently dropped.
fn swap_flags(priority: Option<i32>, discard: bool) -> i32 {
    let mut flags = 0;
    if let Some(p) = priority {
        flags |= libcall::SWAP_FLAG_PREFER;
        flags |= p & libcall::SWAP_FLAG_PRIO_MASK;
    }
    if discard {
        flags |= libcall::SWAP_FLAG_DISCARD;
    }
    flags
}

/// Enable swapping on `device`, through `swapon(2)`.
///
/// # This used to write to a path that does not exist
///
/// The code here was:
///
/// ```ignore
/// // We simulate by writing to a hypothetical /proc/sys/swap/activate.
/// match fs::write("/proc/sys/swap/activate", &cmd) {
/// ```
///
/// and the comment was accurate: the kernel serves no `/proc/sys/swap`, so
/// every activation failed with "no such file or directory". That is at least
/// a failure rather than a false success -- but it is the *wrong* failure, and
/// it says the wrong thing to whoever reads it: the problem is not a missing
/// file, it is that swap is not implemented.
///
/// `swapon(2)` is the real interface and has been there all along. It
/// validates its arguments in Linux's own order and returns `ENOSYS`, so the
/// message now names the actual state of the system -- and when the kernel
/// grows swap, this program starts working with no change here.
///
/// Reached through `libcall`, not through `posix` as a Rust dependency. That
/// distinction cost the whole tree a red `main` on 2026-09-10: this function
/// called the `posix::unistd` Rust path for `swapon`, then read its `errno`
/// through the same rlib's `get_errno` -- and *both* were the wrong copy of
/// the library. The Rust path compiles a second libc with every syscall
/// stubbed to `-ENOSYS`, and the `errno` it reports is that second copy's
/// cell, which the linked library never wrote. So the program called a
/// `swapon` that could not have worked and then asked a different library
/// why. That is the archetype `design-decisions.md` 768 describes, reproduced
/// six days after that decision was written, by the lane that wrote it.
fn activate_swap(device: &str, priority: Option<i32>, discard: bool, verbose: bool) {
    let Ok(path) = std::ffi::CString::new(device) else {
        // A NUL inside the argument. `swapon(2)` takes a C string, so such a
        // path cannot be expressed to it at all; saying so beats truncating.
        eprintln!("swapon: {}: path contains a NUL byte", quotef_os(device));
        return;
    };
    // The `errno` arrives with the failure instead of being fetched after it,
    // so there is no second library left to read it from by mistake.
    match libcall::swapon(&path, swap_flags(priority, discard)) {
        Ok(()) => {
            if verbose {
                println!(
                    "swapon: {}: activated{}",
                    quotef_os(device),
                    priority
                        .map(|p| format!(" (priority {p})"))
                        .unwrap_or_default()
                );
            }
        }
        Err(e) => {
            eprintln!(
                "swapon: {}: failed to activate: {}",
                quotef_os(device),
                errno_text(e)
            );
        }
    }
}

/// A short description of an `errno` this program can actually receive.
///
/// Only the values `swapon(2)` and `swapoff(2)` document, plus a numeric
/// fallback. A table of every errno would be a second `strerror`, and the
/// point here is to say something true about *these* calls.
fn errno_text(e: i32) -> String {
    match e {
        libcall::EPERM => "not permitted (needs CAP_SYS_ADMIN)".to_string(),
        libcall::EINVAL => "invalid flags".to_string(),
        libcall::ENOENT => "no such file".to_string(),
        libcall::EFAULT => "bad path".to_string(),
        libcall::ENOSYS => "swap is not implemented on this kernel".to_string(),
        other => format!("errno {other}"),
    }
}

// ============================================================================
// Personality: swapoff
// ============================================================================

fn cmd_swapoff(args: &[String]) {
    let mut all_flag = false;
    let mut verbose = false;
    let mut devices: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: swapoff [options] [device...]");
                println!();
                println!("Disable devices and files for paging and swapping.");
                println!();
                println!("Options:");
                println!("  -a, --all      Disable all swaps");
                println!("  -v, --verbose  Verbose output");
                println!("  -h, --help     Show this help");
                println!("  --version      Show version");
                process::exit(0);
            }
            "--version" => {
                println!("swapoff {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all_flag = true,
            "-v" | "--verbose" => verbose = true,
            s if !s.starts_with('-') => {
                devices.push(s.to_string());
            }
            other => {
                eprintln!("swapoff: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    if all_flag {
        let active = parse_proc_swaps();
        if active.is_empty() {
            if verbose {
                println!("swapoff: no swap entries active");
            }
            return;
        }
        for entry in &active {
            deactivate_swap(&entry.filename, verbose);
        }
    } else if devices.is_empty() {
        eprintln!("swapoff: no device specified");
        eprintln!("Try 'swapoff --help' for more information.");
        process::exit(1);
    } else {
        for device in &devices {
            if !is_swap_active(device) {
                eprintln!("swapoff: {}: not currently active", quotef_os(device));
                continue;
            }
            deactivate_swap(device, verbose);
        }
    }
}

/// Disable swapping on `device`, through `swapoff(2)`. See [`activate_swap`]
/// for what this replaces.
fn deactivate_swap(device: &str, verbose: bool) {
    let Ok(path) = std::ffi::CString::new(device) else {
        eprintln!("swapoff: {}: path contains a NUL byte", quotef_os(device));
        return;
    };
    match libcall::swapoff(&path) {
        Ok(()) => {
            if verbose {
                println!("swapoff: {}: deactivated", quotef_os(device));
            }
        }
        Err(e) => {
            eprintln!(
                "swapoff: {}: failed to deactivate: {}",
                quotef_os(device),
                errno_text(e)
            );
        }
    }
}

// ============================================================================
// Personality: free
// ============================================================================

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("swapon");
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
        "swapoff" => cmd_swapoff(&rest),
        _ => cmd_swapon(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {

    // ---- the swapflags word ----

    #[test]
    fn no_options_is_no_flags() {
        assert_eq!(swap_flags(None, false), 0);
    }

    /// **A priority without `SWAP_FLAG_PREFER` is ignored by the kernel.**
    ///
    /// The low 15 bits only mean a priority when that flag is set, so setting
    /// the bits alone would have been a `-p` silently dropped.
    #[test]
    fn a_priority_sets_the_prefer_flag_as_well_as_the_bits() {
        let flags = swap_flags(Some(5), false);
        assert_ne!(
            flags & libcall::SWAP_FLAG_PREFER,
            0,
            "without PREFER the kernel ignores the priority bits"
        );
        assert_eq!(flags & libcall::SWAP_FLAG_PRIO_MASK, 5);
    }

    #[test]
    fn discard_is_independent_of_priority() {
        assert_eq!(
            swap_flags(None, true) & libcall::SWAP_FLAG_DISCARD,
            libcall::SWAP_FLAG_DISCARD
        );
        let both = swap_flags(Some(7), true);
        assert_ne!(both & libcall::SWAP_FLAG_PREFER, 0);
        assert_ne!(both & libcall::SWAP_FLAG_DISCARD, 0);
        assert_eq!(both & libcall::SWAP_FLAG_PRIO_MASK, 7);
    }

    // ---- the priority argument ----

    #[test]
    fn an_ordinary_priority_is_accepted() {
        assert_eq!(parse_priority("0"), Ok(0));
        assert_eq!(parse_priority("100"), Ok(100));
        assert_eq!(parse_priority("32767"), Ok(libcall::SWAP_FLAG_PRIO_MASK));
    }

    /// **A negative priority used to become the maximum.**
    ///
    /// `swap_flags` masks with `0x7FFF`, so `-1` became `32767` -- the caller
    /// asked for the lowest priority and would have got the highest, with no
    /// diagnostic. This is the test that would have caught it.
    #[test]
    fn a_negative_priority_is_refused_rather_than_wrapped() {
        assert!(parse_priority("-1").is_err());
        // Through a binding so it is not constant-folded away: the point is
        // to show what the old masking did to the value, not to assert an
        // identity about a literal.
        let asked_for: i32 = -1;
        assert_eq!(asked_for & libcall::SWAP_FLAG_PRIO_MASK, 32767);
    }

    /// And one above the range became an unrelated number: 99999 masks to
    /// 1695.
    #[test]
    fn a_priority_above_the_range_is_refused_rather_than_masked() {
        assert!(parse_priority("32768").is_err());
        assert!(parse_priority("99999").is_err());
        let asked_for: i32 = 99999;
        assert_eq!(asked_for & libcall::SWAP_FLAG_PRIO_MASK, 1695);
    }

    #[test]
    fn a_priority_that_is_not_a_number_is_refused() {
        assert!(parse_priority("high").is_err());
        assert!(parse_priority("").is_err());
    }
    use super::*;

    #[test]
    fn test_parse_proc_swaps() {
        // parse_proc_swaps reads from /proc/swaps which may not exist.
        // Just verify it doesn't panic and returns a vec.
        let entries = parse_proc_swaps();
        // entries may be empty or populated.
        let _ = entries.len();
    }

    #[test]
    fn test_parse_fstab() {
        let entries = parse_fstab();
        // Just verify no panic.
        let _ = entries.len();
    }

    #[test]
    fn test_get_swap_fstab_entries() {
        let entries = get_swap_fstab_entries();
        for e in &entries {
            assert_eq!(e.fstype, "swap");
        }
    }

    #[test]
    fn test_swap_entry_fields() {
        let entry = SwapEntry {
            filename: "/dev/sda2".to_string(),
            swap_type: "partition".to_string(),
            size_kb: 8388604,
            used_kb: 1024,
            priority: -2,
        };
        assert_eq!(entry.filename, "/dev/sda2");
        assert_eq!(entry.swap_type, "partition");
        assert_eq!(entry.size_kb, 8388604);
        assert_eq!(entry.used_kb, 1024);
        assert_eq!(entry.priority, -2);
    }

    #[test]
    fn test_fstab_entry() {
        let entry = FstabEntry {
            device: "/dev/sda2".to_string(),
            mountpoint: "none".to_string(),
            fstype: "swap".to_string(),
            options: "sw".to_string(),
            _dump: 0,
            _pass: 0,
        };
        assert_eq!(entry.fstype, "swap");
        assert_eq!(entry.mountpoint, "none");
    }

    #[test]
    fn test_is_swap_active() {
        // Test with a non-existent device.
        assert!(!is_swap_active("/dev/nonexistent"));
    }

    #[test]
    fn test_personality_detection() {
        // Test basename extraction logic.
        let test_cases = [
            ("/usr/sbin/swapon", "swapon"),
            ("/usr/sbin/swapoff", "swapoff"),
            ("free", "free"),
            ("C:\\Windows\\swapon.exe", "swapon"),
            ("/bin/free.exe", "free"),
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
}
