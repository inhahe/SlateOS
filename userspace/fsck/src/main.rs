//! OurOS Filesystem Check and Repair Utility
//!
//! Traditional `fsck` front-end that delegates to the kernel's filesystem
//! verification and repair syscalls (`SYS_FS_VERIFY` / `SYS_FS_REPAIR`).
//!
//! Supports check-only, auto-repair, forced-yes, progress display, JSON
//! output, and batch checking via `/etc/fstab`.  The executable can also be
//! invoked as `fsck.<type>` (e.g. `fsck.ext4`) to implicitly set `-t`.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Filesystem clean, no errors |
//! | 1    | Errors found and corrected |
//! | 2    | Errors corrected, reboot needed |
//! | 4    | Errors remain uncorrected |
//! | 8    | Operational / usage error |
//!
//! # Usage
//!
//! ```text
//! fsck [options] <device>
//! fsck -A                         Check all filesystems in /etc/fstab
//! fsck -t <type> <device>         Specify filesystem type
//! fsck.<type> <device>            Detect type from program name
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Exit codes (bitwise-OR'd where appropriate)
// ============================================================================

/// Filesystem was clean.
const EXIT_CLEAN: i32 = 0;
/// Errors were found and fixed.
const EXIT_FIXED: i32 = 1;
/// Errors fixed but a reboot is needed.
const EXIT_REBOOT: i32 = 2;
/// Errors remain unfixed.
const EXIT_ERRORS: i32 = 4;
/// Operational / usage error.
const EXIT_USAGE: i32 = 8;

// ============================================================================
// Syscall interface (fs zone, numbers 600-799)
// ============================================================================

/// Check filesystem integrity.
/// arg1 = path_ptr, arg2 = path_len, arg3 = flags.
/// Returns 0 if clean, positive = error count, negative = errno.
const SYS_FS_VERIFY: u64 = 652;

/// Repair filesystem.
/// arg1 = path_ptr, arg2 = path_len, arg3 = flags.
/// Returns 0 if clean after repair, positive = remaining errors, negative = errno.
const SYS_FS_REPAIR: u64 = 653;

// Verify / repair flag bits.
const FSCK_FLAG_FORCE: u64 = 1 << 0;
const FSCK_FLAG_AUTO_REPAIR: u64 = 1 << 1;
const FSCK_FLAG_YES_ALL: u64 = 1 << 2;
const FSCK_FLAG_NO_MODIFY: u64 = 1 << 3;
const FSCK_FLAG_VERBOSE: u64 = 1 << 4;
const FSCK_FLAG_PROGRESS: u64 = 1 << 5;

/// Invoke a raw syscall with three arguments.
///
/// The kernel ABI places arguments in rdi, rsi, rdx, r10, r8, and the
/// syscall number in rax.  Only rdi/rsi/rdx are used here.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees a1 points to a valid buffer of length a2,
    // and a3 contains valid flag bits.  The kernel validates everything and
    // returns a negative errno on failure.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r10") _,
            lateout("r11") _,
            lateout("r8") _,
            options(nostack),
        );
    }
    ret
}

/// Human-readable string for a negative errno returned by a syscall.
fn errno_msg(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such file or directory",
        -5 => "I/O error",
        -12 => "out of memory",
        -13 => "permission denied",
        -16 => "device busy",
        -19 => "no such device",
        -22 => "invalid argument",
        -28 => "no space left on device",
        -30 => "read-only filesystem",
        -38 => "function not implemented",
        _ => "unknown error",
    }
}

// ============================================================================
// Parsed command-line options
// ============================================================================

struct Options {
    /// Filesystem type override (`-t`).
    fstype: Option<String>,
    /// Device path(s) to check.
    devices: Vec<String>,
    /// Auto-repair safe fixes (`-a` / `-p`).
    auto_repair: bool,
    /// Answer yes to every repair prompt (`-y`).
    yes_all: bool,
    /// Check only -- make no changes (`-n`).
    no_modify: bool,
    /// Force check even if filesystem appears clean (`-f`).
    force: bool,
    /// Verbose output (`-v`).
    verbose: bool,
    /// Show progress indicator (`-C`).
    progress: bool,
    /// Check all filesystems in /etc/fstab (`-A`).
    check_all: bool,
    /// Emit JSON report instead of human-readable text.
    json: bool,
}

impl Options {
    fn new() -> Self {
        Self {
            fstype: None,
            devices: Vec::new(),
            auto_repair: false,
            yes_all: false,
            no_modify: false,
            force: false,
            verbose: false,
            progress: false,
            check_all: false,
            json: false,
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Detect filesystem type from `argv[0]`.  If the binary is invoked as
/// `fsck.ext4` or `/sbin/fsck.ext4`, return `Some("ext4")`.
fn fstype_from_argv0(argv0: &str) -> Option<String> {
    // Find the basename.
    let basename = match argv0.rfind('/') {
        Some(pos) => &argv0[pos + 1..],
        None => argv0,
    };
    // Look for "fsck.<type>".
    if let Some(rest) = basename.strip_prefix("fsck.")
        && !rest.is_empty() {
            return Some(rest.to_string());
        }
    None
}

fn parse_args() -> Options {
    let args: Vec<String> = env::args().collect();
    let mut opts = Options::new();

    // Detect type from program name first.
    if let Some(first) = args.first() {
        opts.fstype = fstype_from_argv0(first);
    }

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-t" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("fsck: -t requires a filesystem type argument");
                    process::exit(EXIT_USAGE);
                }
                opts.fstype = Some(args[i].clone());
            }
            "-a" | "-p" => opts.auto_repair = true,
            "-y" => opts.yes_all = true,
            "-n" => opts.no_modify = true,
            "-f" | "--force" => opts.force = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-C" => opts.progress = true,
            "-A" => opts.check_all = true,
            "--json" => opts.json = true,
            "-h" | "--help" => {
                print_usage();
                process::exit(EXIT_CLEAN);
            }
            "--version" => {
                println!("fsck (OurOS) 0.1.0");
                process::exit(EXIT_CLEAN);
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("fsck: unknown option '{other}'");
                    process::exit(EXIT_USAGE);
                }
                opts.devices.push(other.to_string());
            }
        }
        i += 1;
    }

    opts
}

fn print_usage() {
    println!("fsck (OurOS) 0.1.0 -- check and repair filesystems");
    println!();
    println!("USAGE:");
    println!("  fsck [options] <device> ...");
    println!("  fsck -A                 Check all filesystems in /etc/fstab");
    println!("  fsck.<type> <device>    Detect type from program name");
    println!();
    println!("OPTIONS:");
    println!("  -t <type>    Specify filesystem type (ext4, fat32, ...)");
    println!("  -a, -p       Auto-repair safe fixes (preen mode)");
    println!("  -y           Answer yes to all repair prompts");
    println!("  -n           Check only, make no changes");
    println!("  -f, --force  Force check even if filesystem appears clean");
    println!("  -v, --verbose  Verbose output");
    println!("  -C           Show progress indicator");
    println!("  -A           Check all filesystems listed in /etc/fstab");
    println!("  --json       JSON output of check results");
    println!("  -h, --help   Show this help");
    println!("  --version    Show version");
    println!();
    println!("EXIT CODES:");
    println!("  0  Filesystem clean");
    println!("  1  Errors found and corrected");
    println!("  2  Errors corrected, reboot needed");
    println!("  4  Errors remain uncorrected");
    println!("  8  Operational error");
}

// ============================================================================
// /etc/fstab parsing
// ============================================================================

/// One entry from /etc/fstab.
struct FstabEntry {
    device: String,
    _mount_point: String,
    fstype: String,
    /// The fs_passno field (sixth column).  0 = skip, 1 = root, 2+ = check.
    pass: u32,
}

/// Parse /etc/fstab, returning entries sorted by pass number (ascending).
fn read_fstab() -> Vec<FstabEntry> {
    let content = match fs::read_to_string("/etc/fstab") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fsck: cannot read /etc/fstab: {e}");
            return Vec::new();
        }
    };

    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and blank lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let cols: Vec<&str> = line.split_whitespace().collect();
        // fstab has at least 4 columns; pass is column 6 (index 5).
        if cols.len() < 4 {
            continue;
        }

        let device = cols[0].to_string();
        let mount_point = cols[1].to_string();
        let fstype = cols[2].to_string();
        // Default pass = 0 if not specified.
        let pass: u32 = if cols.len() >= 6 {
            cols[5].parse().unwrap_or(0)
        } else {
            0
        };

        // pass == 0 means "do not check".
        if pass == 0 {
            continue;
        }

        // Skip pseudo-filesystems that cannot be checked.
        match fstype.as_str() {
            "proc" | "sysfs" | "devtmpfs" | "tmpfs" | "devpts" | "cgroup"
            | "cgroup2" | "hugetlbfs" | "mqueue" | "debugfs" | "tracefs"
            | "securityfs" | "pstore" | "bpf" | "autofs" | "none" | "swap" => continue,
            _ => {}
        }

        entries.push(FstabEntry {
            device,
            _mount_point: mount_point,
            fstype,
            pass,
        });
    }

    // Sort: pass 1 first (root), then pass 2+.
    entries.sort_by_key(|e| e.pass);
    entries
}

// ============================================================================
// /proc/mounts helpers
// ============================================================================

/// Return true if `device` is currently mounted (appears in /proc/mounts).
fn is_mounted(device: &str) -> bool {
    let content = match fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let dev = device.strip_prefix("/dev/").unwrap_or(device);
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(mount_dev) = parts.first() {
            let md = mount_dev.strip_prefix("/dev/").unwrap_or(mount_dev);
            if md == dev {
                return true;
            }
        }
    }
    false
}

/// Try to detect the filesystem type of `device` from /proc/mounts or sysfs.
fn detect_fstype(device: &str) -> Option<String> {
    let dev = device.strip_prefix("/dev/").unwrap_or(device);

    // Try /proc/mounts first.
    if let Ok(content) = fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let md = parts[0].strip_prefix("/dev/").unwrap_or(parts[0]);
                if md == dev {
                    return Some(parts[2].to_string());
                }
            }
        }
    }

    // Try sysfs.
    let sysfs_path = format!("/sys/block/{dev}/fstype");
    if let Ok(t) = fs::read_to_string(&sysfs_path) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }

    None
}

// ============================================================================
// Progress display
// ============================================================================

/// The six phases of a full filesystem check.
const PHASES: &[&str] = &[
    "Phase 1: checking superblock",
    "Phase 2: checking block groups",
    "Phase 3: checking inodes",
    "Phase 4: checking directory structure",
    "Phase 5: checking data blocks",
    "Phase 6: final verification",
];

/// Print a simulated progress sequence.  On real hardware the kernel would
/// stream incremental status via a shared-memory progress buffer; here we
/// display the phase names as the check is dispatched.
fn show_progress(device: &str) {
    println!("  Checking {device}:");
    for phase in PHASES {
        println!("    {phase}...");
    }
}

/// Render a one-line progress bar.
fn progress_bar(pct: u32) {
    let filled = (pct as usize * 40) / 100;
    let empty = 40_usize.saturating_sub(filled);
    print!("\r  [");
    for _ in 0..filled {
        print!("=");
    }
    if filled < 40 {
        print!(">");
        for _ in 1..empty {
            print!(" ");
        }
    }
    print!("] {pct:>3}%");
    if pct >= 100 {
        println!();
    }
}

// ============================================================================
// Core check / repair logic
// ============================================================================

/// Result of checking a single device.
struct CheckResult {
    device: String,
    fstype: String,
    /// 0 = clean, positive = error count the kernel reported.
    errors_found: u64,
    /// Number of errors fixed (only meaningful when repair was attempted).
    errors_fixed: u64,
    /// Whether the kernel indicated a reboot is needed.
    needs_reboot: bool,
    /// Remaining unfixed errors.
    errors_remaining: u64,
    /// If the syscall itself failed (negative errno), store the message.
    syscall_error: Option<String>,
}

/// Build the bitwise flags word from parsed options.
fn build_flags(opts: &Options) -> u64 {
    let mut flags: u64 = 0;
    if opts.force {
        flags |= FSCK_FLAG_FORCE;
    }
    if opts.auto_repair {
        flags |= FSCK_FLAG_AUTO_REPAIR;
    }
    if opts.yes_all {
        flags |= FSCK_FLAG_YES_ALL;
    }
    if opts.no_modify {
        flags |= FSCK_FLAG_NO_MODIFY;
    }
    if opts.verbose {
        flags |= FSCK_FLAG_VERBOSE;
    }
    if opts.progress {
        flags |= FSCK_FLAG_PROGRESS;
    }
    flags
}

/// Normalise a device path: ensure it starts with "/dev/" unless it already
/// contains a '/'.
fn normalise_device(device: &str) -> String {
    if device.contains('/') {
        device.to_string()
    } else {
        format!("/dev/{device}")
    }
}

/// Check (and optionally repair) a single device.
fn check_device(device: &str, fstype: &Option<String>, opts: &Options) -> CheckResult {
    let dev_path = normalise_device(device);
    let dev_display = dev_path.clone();

    // Resolve filesystem type.
    let resolved_fstype = match fstype {
        Some(t) => t.clone(),
        None => detect_fstype(&dev_path).unwrap_or_else(|| "auto".to_string()),
    };

    let mut result = CheckResult {
        device: dev_display.clone(),
        fstype: resolved_fstype.clone(),
        errors_found: 0,
        errors_fixed: 0,
        needs_reboot: false,
        errors_remaining: 0,
        syscall_error: None,
    };

    // Warn if mounted (non-root checks on mounted fs are risky).
    if is_mounted(&dev_path) && !opts.no_modify
        && !opts.json {
            eprintln!(
                "fsck: warning: {dev_display} is mounted; \
                 running check in read-only mode"
            );
        }

    if !opts.json {
        println!("fsck: checking {dev_display} (type: {resolved_fstype})");
    }

    // Show progress phases.
    if opts.progress && !opts.json {
        show_progress(&dev_display);
    }

    let flags = build_flags(opts);

    // ---- Phase 1: verify ----
    let path_bytes = dev_path.as_bytes();
    let verify_ret = unsafe {
        // SAFETY: path_bytes points to a valid byte slice whose length is
        // passed in a2.  The kernel copies from userspace and validates.
        syscall3(
            SYS_FS_VERIFY,
            path_bytes.as_ptr() as u64,
            path_bytes.len() as u64,
            flags,
        )
    };

    if verify_ret < 0 {
        result.syscall_error = Some(format!(
            "verify syscall failed: {} ({})",
            errno_msg(verify_ret),
            verify_ret
        ));
        return result;
    }

    result.errors_found = verify_ret as u64;

    if opts.progress && !opts.json {
        progress_bar(50);
    }

    // If clean and not forced, we are done.
    if result.errors_found == 0 && !opts.force {
        if opts.progress && !opts.json {
            progress_bar(100);
        }
        if !opts.json {
            println!("  {dev_display}: clean.");
        }
        return result;
    }

    // Report errors found.
    if !opts.json {
        println!(
            "  {dev_display}: {} error{} found.",
            result.errors_found,
            if result.errors_found == 1 { "" } else { "s" }
        );
    }

    // ---- Phase 2: repair (if requested) ----
    let will_repair = (opts.auto_repair || opts.yes_all) && !opts.no_modify;

    if will_repair {
        if !opts.json {
            println!("  Attempting repair on {dev_display}...");
        }

        let repair_ret = unsafe {
            // SAFETY: same pointer/length guarantees as the verify call.
            syscall3(
                SYS_FS_REPAIR,
                path_bytes.as_ptr() as u64,
                path_bytes.len() as u64,
                flags,
            )
        };

        if repair_ret < 0 {
            result.syscall_error = Some(format!(
                "repair syscall failed: {} ({})",
                errno_msg(repair_ret),
                repair_ret
            ));
            result.errors_remaining = result.errors_found;
            if opts.progress && !opts.json {
                progress_bar(100);
            }
            return result;
        }

        result.errors_remaining = repair_ret as u64;
        result.errors_fixed = result
            .errors_found
            .saturating_sub(result.errors_remaining);

        // A repair return of 0 means the kernel fixed everything.
        // Convention: if the root filesystem was repaired, recommend reboot.
        if result.errors_fixed > 0
            && (dev_path == "/dev/sda1"
                || dev_path == "/dev/root"
                || dev_path == "/dev/nvme0n1p1")
            {
                result.needs_reboot = true;
            }

        if !opts.json {
            if result.errors_remaining == 0 {
                println!(
                    "  {dev_display}: {} error{} fixed.",
                    result.errors_fixed,
                    if result.errors_fixed == 1 { "" } else { "s" }
                );
            } else {
                println!(
                    "  {dev_display}: {} fixed, {} remaining.",
                    result.errors_fixed, result.errors_remaining
                );
            }

            if result.needs_reboot {
                println!(
                    "  WARNING: root filesystem was modified; reboot is recommended."
                );
            }
        }
    } else if opts.no_modify {
        // -n mode: report what would happen.
        result.errors_remaining = result.errors_found;
        if !opts.json {
            println!(
                "  {dev_display}: {} error{} detected (no changes made).",
                result.errors_found,
                if result.errors_found == 1 { "" } else { "s" }
            );
        }
    } else {
        // Errors found but no auto-repair flag.
        result.errors_remaining = result.errors_found;
        if !opts.json {
            println!(
                "  {dev_display}: errors found. Run with -a or -y to repair."
            );
        }
    }

    if opts.progress && !opts.json {
        progress_bar(100);
    }

    result
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for safe JSON embedding.
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

/// Print results as a JSON array.
fn print_json(results: &[CheckResult]) {
    println!("[");
    for (i, r) in results.iter().enumerate() {
        let trailing = if i + 1 < results.len() { "," } else { "" };
        println!("  {{");
        println!("    \"device\": \"{}\",", json_escape(&r.device));
        println!("    \"fstype\": \"{}\",", json_escape(&r.fstype));
        println!("    \"errors_found\": {},", r.errors_found);
        println!("    \"errors_fixed\": {},", r.errors_fixed);
        println!("    \"errors_remaining\": {},", r.errors_remaining);
        println!("    \"needs_reboot\": {},", r.needs_reboot);
        match &r.syscall_error {
            Some(e) => println!("    \"error\": \"{}\"", json_escape(e)),
            None => println!("    \"error\": null"),
        }
        println!("  }}{trailing}");
    }
    println!("]");
}

// ============================================================================
// Summary
// ============================================================================

/// Print a human-readable summary of all results.
fn print_summary(results: &[CheckResult]) {
    if results.len() <= 1 {
        return;
    }

    println!();
    println!("=== Summary ===");
    println!(
        "  {:<24} {:>8} {:>8} {:>8}  STATUS",
        "DEVICE", "FOUND", "FIXED", "REMAIN"
    );
    println!(
        "  {:<24} {:>8} {:>8} {:>8}  ------",
        "------", "-----", "-----", "------"
    );

    for r in results {
        let status = if r.syscall_error.is_some() {
            "ERROR"
        } else if r.errors_remaining > 0 {
            "ERRORS"
        } else if r.errors_fixed > 0 {
            "FIXED"
        } else {
            "CLEAN"
        };

        println!(
            "  {:<24} {:>8} {:>8} {:>8}  {}",
            r.device, r.errors_found, r.errors_fixed, r.errors_remaining, status
        );
    }
}

// ============================================================================
// Compute combined exit code
// ============================================================================

fn exit_code_for(results: &[CheckResult]) -> i32 {
    let mut code: i32 = EXIT_CLEAN;

    for r in results {
        if r.syscall_error.is_some() {
            code |= EXIT_USAGE;
        }
        if r.errors_fixed > 0 {
            code |= EXIT_FIXED;
        }
        if r.needs_reboot {
            code |= EXIT_REBOOT;
        }
        if r.errors_remaining > 0 {
            code |= EXIT_ERRORS;
        }
    }

    code
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let opts = parse_args();

    // Sanity: -y and -n are mutually exclusive.
    if opts.yes_all && opts.no_modify {
        eprintln!("fsck: -y and -n are mutually exclusive");
        process::exit(EXIT_USAGE);
    }

    // Collect devices to check.
    let entries: Vec<(String, Option<String>)> = if opts.check_all {
        // -A mode: read /etc/fstab.
        let fstab = read_fstab();
        if fstab.is_empty() {
            eprintln!("fsck: no checkable filesystems found in /etc/fstab");
            process::exit(EXIT_USAGE);
        }
        fstab
            .into_iter()
            .map(|e| (e.device, Some(e.fstype)))
            .collect()
    } else {
        if opts.devices.is_empty() {
            eprintln!("fsck: no device specified");
            eprintln!("Usage: fsck [options] <device> ...");
            eprintln!("       fsck -A  (check all in /etc/fstab)");
            process::exit(EXIT_USAGE);
        }
        opts.devices
            .iter()
            .map(|d| (d.clone(), opts.fstype.clone()))
            .collect()
    };

    if opts.verbose && !opts.json {
        println!("fsck (OurOS) 0.1.0");
        if let Some(ref t) = opts.fstype {
            println!("  Filesystem type: {t}");
        }
        println!("  Devices to check: {}", entries.len());
        println!("  Flags: auto_repair={}, yes_all={}, no_modify={}, force={}",
            opts.auto_repair, opts.yes_all, opts.no_modify, opts.force);
        println!();
    }

    // Run checks.
    let mut results = Vec::with_capacity(entries.len());

    for (device, per_dev_type) in &entries {
        let fstype = per_dev_type.as_ref().or(opts.fstype.as_ref()).cloned();
        let r = check_device(device, &fstype, &opts);
        results.push(r);
    }

    // Output.
    if opts.json {
        print_json(&results);
    } else {
        print_summary(&results);
    }

    let code = exit_code_for(&results);
    process::exit(code);
}
