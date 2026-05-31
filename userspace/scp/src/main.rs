//! OurOS Secure File Copy (scp)
//!
//! Copies files between hosts over SSH (simplified). The initial version fully
//! supports local file copy with SSH-style syntax parsing. Remote transport
//! stubs are structured for future SSH library integration.
//!
//! # Usage
//!
//! ```text
//! scp [OPTION]... SOURCE... TARGET
//!
//! Copy files and directories.
//!
//!   -r              Recursively copy directories
//!   -p              Preserve timestamps and permissions
//!   -v              Verbose mode
//!   -q              Quiet mode (no progress)
//!   -C              Enable compression (reserved for SSH transport)
//!   -B              Batch mode (no password prompts)
//!   -P port         Connect on the specified port
//!   -i identity     Path to the identity (private key) file
//!   -l limit        Bandwidth limit in KB/s
//!   --help          Display this help and exit
//!   --version       Output version information and exit
//! ```
//!
//! # Examples
//!
//! ```text
//! scp file.txt user@host:/tmp/          Copy local file to remote host
//! scp user@host:/etc/config ./          Copy remote file to local
//! scp -r mydir user@host:/backup/       Recursive directory copy
//! scp -p file1 file2 ./dest/            Copy multiple files, preserve metadata
//! scp file1 file2                       Local-to-local copy
//! ```
//!
//! # Exit codes
//!
//! - 0: success
//! - 1: general error
//! - 2: usage / argument error

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use std::env;
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Buffer size for file I/O (64 KiB).
const COPY_BUF_SIZE: usize = 64 * 1024;

/// Native OurOS monotonic clock (kernel syscall/number.rs); no-arg, returns
/// boot-relative nanoseconds in rax.  (Syscall 30 is SYS_IRQ_REGISTER.)
const SYS_CLOCK_MONOTONIC: u64 = 10;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
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

/// Get monotonic time in nanoseconds.
fn clock_ns() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments; all three
    // extra arguments are zero/unused. Returns nanoseconds since boot.
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

// ============================================================================
// Error type
// ============================================================================

/// All errors produced by scp.
enum ScpError {
    /// Argument parsing or usage error.
    Usage(String),
    /// I/O error during file operations.
    Io(String),
    /// Remote operations are not yet implemented.
    RemoteNotSupported(String),
}

impl core::fmt::Display for ScpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Usage(msg) => write!(f, "scp: {msg}"),
            Self::Io(msg) => write!(f, "scp: {msg}"),
            Self::RemoteNotSupported(msg) => write!(f, "scp: remote transfer not yet supported: {msg}"),
        }
    }
}

// ============================================================================
// Target specification — local vs remote
// ============================================================================

/// A parsed scp path: either local or remote (user@host:path).
#[derive(Clone)]
enum Location {
    Local(String),
    Remote {
        user: Option<String>,
        host: String,
        path: String,
    },
}

impl Location {
    /// Parse an scp-style path argument.
    ///
    /// Remote paths look like `user@host:path` or `host:path`.
    /// Local paths are everything else (including absolute Windows-style paths
    /// which contain `:` only at position 1, e.g. `C:\foo`).
    fn parse(s: &str) -> Self {
        // A colon before any slash indicates a remote spec, BUT we must
        // exclude Windows-style drive letters like "C:\path" where the colon
        // is always at index 1 and followed by \ or /.
        if let Some(colon_pos) = s.find(':') {
            // Windows drive letter: single ASCII letter followed by ':'
            let is_drive_letter = colon_pos == 1
                && s.as_bytes().first().map_or(false, |b| b.is_ascii_alphabetic());

            if !is_drive_letter && colon_pos > 0 {
                // Check there is no slash before the colon (that would mean
                // it's a local path with a colon in a directory name).
                let before_colon = &s[..colon_pos];
                if !before_colon.contains('/') && !before_colon.contains('\\') {
                    let host_part = before_colon;
                    let path_part = &s[colon_pos + 1..];

                    // Split user@host if present.
                    if let Some(at_pos) = host_part.find('@') {
                        let user = &host_part[..at_pos];
                        let host = &host_part[at_pos + 1..];
                        if !host.is_empty() {
                            return Self::Remote {
                                user: if user.is_empty() {
                                    None
                                } else {
                                    Some(user.to_string())
                                },
                                host: host.to_string(),
                                path: if path_part.is_empty() {
                                    ".".to_string()
                                } else {
                                    path_part.to_string()
                                },
                            };
                        }
                    } else if !host_part.is_empty() {
                        return Self::Remote {
                            user: None,
                            host: host_part.to_string(),
                            path: if path_part.is_empty() {
                                ".".to_string()
                            } else {
                                path_part.to_string()
                            },
                        };
                    }
                }
            }
        }
        Self::Local(s.to_string())
    }

    /// Returns `true` if this is a remote location.
    fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

    /// Display string for diagnostics (used by remote transfer stubs).
    #[allow(dead_code)]
    fn display(&self) -> String {
        match self {
            Self::Local(p) => p.clone(),
            Self::Remote { user, host, path } => {
                if let Some(u) = user {
                    format!("{u}@{host}:{path}")
                } else {
                    format!("{host}:{path}")
                }
            }
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Source paths (at least one).
    sources: Vec<Location>,
    /// Destination path (exactly one).
    target: Location,
    /// Recursive directory copy.
    recursive: bool,
    /// Preserve timestamps and permissions.
    preserve: bool,
    /// Verbose output.
    verbose: bool,
    /// Quiet mode (suppress progress).
    quiet: bool,
    /// Enable compression (SSH transport option, currently a no-op stub).
    compress: bool,
    /// Batch mode (no interactive prompts). Used by SSH transport stub.
    #[allow(dead_code)]
    batch: bool,
    /// SSH port override. Used by SSH transport stub.
    #[allow(dead_code)]
    port: Option<u16>,
    /// Identity (private key) file path. Used by SSH transport stub.
    #[allow(dead_code)]
    identity: Option<String>,
    /// Bandwidth limit in KB/s (0 = unlimited).
    bandwidth_limit: u64,
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Result of parsing command-line arguments.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

fn parse_args(args: &[String]) -> Result<ParseResult, ScpError> {
    let mut recursive = false;
    let mut preserve = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut compress = false;
    let mut batch = false;
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut bandwidth_limit: u64 = 0;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" {
            return Ok(ParseResult::Help);
        }
        if arg == "--version" {
            return Ok(ParseResult::Version);
        }
        if arg == "--" {
            // Everything after -- is positional.
            for a in &args[i + 1..] {
                positional.push(a.clone());
            }
            break;
        }

        // Long options first.
        if arg.starts_with("--") {
            return Err(ScpError::Usage(format!("unknown option: {arg}")));
        }

        // Short options that take a value: -P, -i, -l
        // These consume the next argument.
        if arg == "-P" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                ScpError::Usage("-P requires a port number".into())
            })?;
            port = Some(val.parse::<u16>().map_err(|_| {
                ScpError::Usage(format!("invalid port: {val}"))
            })?);
            i += 1;
            continue;
        }
        if arg == "-i" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                ScpError::Usage("-i requires an identity file path".into())
            })?;
            identity = Some(val.clone());
            i += 1;
            continue;
        }
        if arg == "-l" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                ScpError::Usage("-l requires a bandwidth limit in KB/s".into())
            })?;
            bandwidth_limit = val.parse::<u64>().map_err(|_| {
                ScpError::Usage(format!("invalid bandwidth limit: {val}"))
            })?;
            i += 1;
            continue;
        }

        // Bundled short flags (e.g. -rpvC).
        if arg.starts_with('-') && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            for ch in chars {
                match ch {
                    'r' => recursive = true,
                    'p' => preserve = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'C' => compress = true,
                    'B' => batch = true,
                    _ => {
                        return Err(ScpError::Usage(format!(
                            "unknown option: -{ch}"
                        )));
                    }
                }
            }
            i += 1;
            continue;
        }

        // Positional argument.
        positional.push(arg.clone());
        i += 1;
    }

    if positional.len() < 2 {
        return Err(ScpError::Usage(
            "need at least a source and a target".into(),
        ));
    }

    // Last positional is always the target.
    let target_str = positional.pop().ok_or_else(|| {
        ScpError::Usage("missing target".into())
    })?;
    let target = Location::parse(&target_str);

    let sources: Vec<Location> = positional.iter().map(|s| Location::parse(s)).collect();

    // Validate: if multiple sources, target must look like a directory
    // (remote target or ends with / or is an existing directory).
    if sources.len() > 1 {
        if let Location::Local(ref p) = target {
            let path = Path::new(p);
            if !path.is_dir() && !p.ends_with('/') && !p.ends_with('\\') {
                return Err(ScpError::Usage(
                    "target must be a directory when copying multiple sources"
                        .into(),
                ));
            }
        }
    }

    Ok(ParseResult::Run(Config {
        sources,
        target,
        recursive,
        preserve,
        verbose,
        quiet,
        compress,
        batch,
        port,
        identity,
        bandwidth_limit,
    }))
}

fn print_help() {
    let help = "\
scp 0.1.0 -- OurOS secure file copy

Usage: scp [OPTION]... SOURCE... TARGET

Copy files and directories between hosts.

Options:
  -r              Recursively copy directories
  -p              Preserve timestamps and permissions
  -v              Verbose mode
  -q              Quiet mode (no progress display)
  -C              Enable compression (SSH transport)
  -B              Batch mode (no password prompts)
  -P port         Connect on the specified port (note: uppercase P)
  -i identity     Path to private key file
  -l limit        Bandwidth limit in KB/s
  --help          Display this help and exit
  --version       Output version information and exit

Paths:
  Local:   /path/to/file   or   relative/path
  Remote:  user@host:/path  or  host:/path

Examples:
  scp file.txt user@host:/tmp/
  scp user@host:/etc/config ./
  scp -r mydir user@host:/backup/
  scp -p file1 file2 ./dest/
  scp file1 file2
";
    print!("{help}");
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte count in human-readable form.
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1024.0 || unit == "TB" {
            if val < 10.0 {
                return format!("{val:.1}{unit}");
            }
            return format!("{val:.0}{unit}");
        }
        val /= 1024.0;
    }
    format!("{bytes}B")
}

/// Format a transfer speed (bytes/sec) in human-readable form.
fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "0B/s".to_string();
    }
    let s = format_size(bytes_per_sec);
    format!("{s}/s")
}

/// Format a duration in seconds as MM:SS or HH:MM:SS.
fn format_eta(seconds: u64) -> String {
    if seconds >= 3600 {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        let s = seconds % 60;
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m:02}:{s:02}")
    }
}

// ============================================================================
// Progress display
// ============================================================================

/// Progress tracking state for a single file transfer.
struct Progress {
    /// File name being transferred.
    file_name: String,
    /// Total file size in bytes.
    total_bytes: u64,
    /// Bytes transferred so far.
    transferred: u64,
    /// Timestamp (ns) when transfer started.
    start_ns: u64,
    /// Last time progress was printed (ns), to throttle updates.
    last_print_ns: u64,
}

impl Progress {
    fn new(file_name: &str, total_bytes: u64) -> Self {
        let now = clock_ns();
        Self {
            file_name: file_name.to_string(),
            total_bytes,
            transferred: 0,
            start_ns: now,
            last_print_ns: 0,
        }
    }

    /// Update transferred count and optionally print progress.
    fn update(&mut self, additional: u64, quiet: bool) {
        self.transferred = self.transferred.saturating_add(additional);
        if quiet {
            return;
        }

        let now = clock_ns();
        // Throttle output to at most once per 200ms.
        let interval = 200_000_000u64; // 200ms in nanoseconds
        if now.saturating_sub(self.last_print_ns) < interval
            && self.transferred < self.total_bytes
        {
            return;
        }
        self.last_print_ns = now;

        self.print_line(now);
    }

    /// Print the final progress line (100%).
    fn finish(&mut self, quiet: bool) {
        if quiet {
            return;
        }
        self.transferred = self.total_bytes;
        let now = clock_ns();
        self.print_line(now);
        eprintln!();
    }

    /// Render and print the progress line to stderr.
    fn print_line(&self, now: u64) {
        let pct = if self.total_bytes > 0 {
            self.transferred.saturating_mul(100) / self.total_bytes
        } else {
            100
        };

        let elapsed_ns = now.saturating_sub(self.start_ns);
        let elapsed_sec = elapsed_ns / 1_000_000_000;

        let speed = if elapsed_sec > 0 {
            self.transferred / elapsed_sec
        } else {
            self.transferred
        };

        let eta = if speed > 0 && self.transferred < self.total_bytes {
            let remaining = self.total_bytes.saturating_sub(self.transferred);
            remaining / speed
        } else {
            0
        };

        // Truncate file name for display (max 24 chars).
        let display_name = if self.file_name.len() > 24 {
            let start = self.file_name.len() - 24;
            format!("...{}", &self.file_name[start + 3..])

        } else {
            self.file_name.clone()
        };

        let eta_str = if self.transferred >= self.total_bytes {
            "done".to_string()
        } else {
            format!("ETA {}", format_eta(eta))
        };

        eprint!(
            "\r{display_name:<24} {pct:>3}% {transferred:>8} {speed:>9} {eta_str}   ",
            transferred = format_size(self.transferred),
            speed = format_speed(speed),
        );
    }
}

// ============================================================================
// Bandwidth limiter
// ============================================================================

/// Simple bandwidth throttle based on elapsed time.
struct Throttle {
    /// Maximum bytes per second (0 = unlimited).
    max_bps: u64,
    /// Total bytes transferred since throttle creation.
    total: u64,
    /// Timestamp when throttle was created.
    start_ns: u64,
}

impl Throttle {
    fn new(limit_kbps: u64) -> Self {
        Self {
            max_bps: limit_kbps.saturating_mul(1024),
            total: 0,
            start_ns: clock_ns(),
        }
    }

    /// Called after writing `n` bytes. Busy-waits if we're ahead of schedule.
    fn account(&mut self, n: u64) {
        if self.max_bps == 0 {
            return;
        }
        self.total = self.total.saturating_add(n);

        // Calculate how many nanoseconds we should have taken to transfer
        // self.total bytes at max_bps.
        let expected_ns = self.total.saturating_mul(1_000_000_000) / self.max_bps;
        loop {
            let elapsed = clock_ns().saturating_sub(self.start_ns);
            if elapsed >= expected_ns {
                break;
            }
            // Busy-wait. In a real OS with sleep syscalls, we'd yield here.
            core::hint::spin_loop();
        }
    }
}

// ============================================================================
// Metadata preservation
// ============================================================================

/// Preserved metadata from a source file/directory.
struct PreservedMeta {
    /// Last modification time as (seconds, nanoseconds) since epoch, if available.
    modified: Option<(i64, i64)>,
    /// Permission mode bits (Unix-style), if available.
    #[allow(dead_code)]
    permissions: Option<u32>,
}

/// Read metadata we want to preserve from a source path.
fn read_preserved_meta(meta: &Metadata) -> PreservedMeta {
    let modified = meta.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| {
            (d.as_secs() as i64, d.subsec_nanos() as i64)
        })
    });

    // On OurOS we'd read the actual Unix permission bits. For now we store
    // the read-only flag as a simplified approximation.
    let permissions = if meta.permissions().readonly() {
        Some(0o444u32)
    } else {
        Some(0o644u32)
    };

    PreservedMeta {
        modified,
        permissions,
    }
}

/// Apply preserved metadata to a destination path.
///
/// This uses std::fs operations. On OurOS, a future version could use
/// syscalls for utimensat and chmod equivalents for higher fidelity.
fn apply_preserved_meta(path: &Path, meta: &PreservedMeta) {
    // Apply modification time if we have it.
    if let Some((secs, _nanos)) = meta.modified {
        if let Some(t) = std::time::UNIX_EPOCH.checked_add(
            std::time::Duration::from_secs(secs as u64),
        ) {
            // std::fs::File::set_modified is not in std, but
            // filetime-equivalent syscalls would go here. For now we use
            // set_permissions as the only portable metadata we can write.
            let _ = t; // Timestamp application is a stub until utimensat is available.
        }
    }

    // Apply permissions.
    if let Some(mode) = meta.permissions {
        let readonly = mode & 0o222 == 0;
        if let Ok(current) = fs::metadata(path) {
            let mut perms = current.permissions();
            perms.set_readonly(readonly);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

// ============================================================================
// Local file copy engine
// ============================================================================

/// Statistics accumulated during a transfer session.
struct TransferStats {
    files: u64,
    directories: u64,
    bytes: u64,
    errors: u64,
}

impl TransferStats {
    fn new() -> Self {
        Self {
            files: 0,
            directories: 0,
            bytes: 0,
            errors: 0,
        }
    }
}

/// Copy a single file from `src` to `dst`, with progress and throttle.
fn copy_file(
    src: &Path,
    dst: &Path,
    config: &Config,
    throttle: &mut Throttle,
    stats: &mut TransferStats,
) -> Result<(), ScpError> {
    let src_meta = fs::metadata(src).map_err(|e| {
        ScpError::Io(format!("{}: {e}", src.display()))
    })?;

    if src_meta.is_dir() {
        return Err(ScpError::Io(format!(
            "{}: is a directory (use -r for recursive copy)",
            src.display()
        )));
    }

    let file_size = src_meta.len();
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| src.display().to_string());

    if config.verbose {
        eprintln!("{} -> {}", src.display(), dst.display());
    }

    let preserved = if config.preserve {
        Some(read_preserved_meta(&src_meta))
    } else {
        None
    };

    // Ensure parent directory exists.
    if let Some(parent) = dst.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| {
                ScpError::Io(format!(
                    "cannot create directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    let mut reader = File::open(src).map_err(|e| {
        ScpError::Io(format!("{}: {e}", src.display()))
    })?;
    let mut writer = File::create(dst).map_err(|e| {
        ScpError::Io(format!("{}: {e}", dst.display()))
    })?;

    let mut buf = [0u8; COPY_BUF_SIZE];
    let mut progress = Progress::new(&file_name, file_size);

    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            ScpError::Io(format!("read {}: {e}", src.display()))
        })?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| {
            ScpError::Io(format!("write {}: {e}", dst.display()))
        })?;

        throttle.account(n as u64);
        progress.update(n as u64, config.quiet);
    }

    progress.finish(config.quiet);

    // Apply preserved metadata after the file is fully written.
    if let Some(ref meta) = preserved {
        apply_preserved_meta(dst, meta);
    }

    stats.files = stats.files.saturating_add(1);
    stats.bytes = stats.bytes.saturating_add(file_size);

    Ok(())
}

/// Recursively copy a directory from `src` to `dst`.
fn copy_directory(
    src: &Path,
    dst: &Path,
    config: &Config,
    throttle: &mut Throttle,
    stats: &mut TransferStats,
) -> Result<(), ScpError> {
    if !config.recursive {
        return Err(ScpError::Io(format!(
            "{}: is a directory (use -r)",
            src.display()
        )));
    }

    if config.verbose {
        eprintln!("d {}", dst.display());
    }

    // Create the destination directory.
    fs::create_dir_all(dst).map_err(|e| {
        ScpError::Io(format!(
            "cannot create directory {}: {e}",
            dst.display()
        ))
    })?;
    stats.directories = stats.directories.saturating_add(1);

    // Preserve metadata on the directory itself.
    if config.preserve {
        if let Ok(src_meta) = fs::metadata(src) {
            let preserved = read_preserved_meta(&src_meta);
            apply_preserved_meta(dst, &preserved);
        }
    }

    // Read directory entries and sort for deterministic output.
    let mut entries: Vec<_> = fs::read_dir(src)
        .map_err(|e| {
            ScpError::Io(format!(
                "cannot read directory {}: {e}",
                src.display()
            ))
        })?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let dst_child = dst.join(&entry_name);

        let ft = entry.file_type().map_err(|e| {
            ScpError::Io(format!("{}: {e}", entry_path.display()))
        })?;

        if ft.is_dir() {
            copy_directory(&entry_path, &dst_child, config, throttle, stats)?;
        } else if ft.is_file() {
            if let Err(e) = copy_file(
                &entry_path,
                &dst_child,
                config,
                throttle,
                stats,
            ) {
                eprintln!("{e}");
                stats.errors = stats.errors.saturating_add(1);
            }
        } else if ft.is_symlink() {
            // Copy symlinks by reading their target and creating a new link.
            match fs::read_link(&entry_path) {
                Ok(link_target) => {
                    // Remove existing destination if present, then create symlink.
                    let _ = fs::remove_file(&dst_child);
                    #[cfg(unix)]
                    {
                        let _ = std::os::unix::fs::symlink(&link_target, &dst_child);
                    }
                    // On non-Unix (build host), just copy the file the link points to.
                    #[cfg(not(unix))]
                    {
                        let _ = link_target; // suppress unused warning
                        if let Err(e) = copy_file(
                            &entry_path,
                            &dst_child,
                            config,
                            throttle,
                            stats,
                        ) {
                            eprintln!("{e}");
                            stats.errors = stats.errors.saturating_add(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("scp: {}: cannot read symlink: {e}", entry_path.display());
                    stats.errors = stats.errors.saturating_add(1);
                }
            }
        }
        // Skip other file types (sockets, block devices, etc.).
    }

    Ok(())
}

/// Copy a single source (file or directory) to a destination path.
fn copy_source_to_dest(
    src_path: &Path,
    dst_path: &Path,
    config: &Config,
    throttle: &mut Throttle,
    stats: &mut TransferStats,
) -> Result<(), ScpError> {
    let src_meta = fs::symlink_metadata(src_path).map_err(|e| {
        ScpError::Io(format!("{}: {e}", src_path.display()))
    })?;

    if src_meta.is_dir() {
        copy_directory(src_path, dst_path, config, throttle, stats)
    } else {
        copy_file(src_path, dst_path, config, throttle, stats)
    }
}

// ============================================================================
// Remote transfer stubs (SSH transport)
// ============================================================================

/// Stub: initiate an SSH connection to the remote host.
///
/// When the SSH library is available, this will:
/// 1. Resolve the hostname
/// 2. Open a TCP connection to the SSH port
/// 3. Perform SSH handshake and authentication
/// 4. Open a session channel
/// 5. Start the `scp` protocol on the remote end
#[allow(dead_code, unused_variables)]
fn ssh_connect(
    user: &Option<String>,
    host: &str,
    port: u16,
    identity: &Option<String>,
    batch: bool,
    verbose: bool,
) -> Result<(), ScpError> {
    Err(ScpError::RemoteNotSupported(format!(
        "SSH transport to {host} not yet implemented"
    )))
}

/// Stub: send a file to a remote host via the SCP protocol.
///
/// The SCP wire protocol sends:
/// 1. `C<mode> <size> <filename>\n` header
/// 2. File data (exactly `size` bytes)
/// 3. `\0` (success byte)
///
/// For directories (recursive mode):
/// 1. `D<mode> 0 <dirname>\n` to enter
/// 2. Files/subdirs within
/// 3. `E\n` to leave
#[allow(unused_variables)]
fn remote_send(
    src: &Path,
    user: &Option<String>,
    host: &str,
    remote_path: &str,
    config: &Config,
) -> Result<(), ScpError> {
    Err(ScpError::RemoteNotSupported(format!(
        "send to {host}:{remote_path}"
    )))
}

/// Stub: receive a file from a remote host via the SCP protocol.
///
/// The receiving side sends `\0` to acknowledge each protocol message,
/// then reads the file data according to the header.
#[allow(unused_variables)]
fn remote_recv(
    user: &Option<String>,
    host: &str,
    remote_path: &str,
    dst: &Path,
    config: &Config,
) -> Result<(), ScpError> {
    Err(ScpError::RemoteNotSupported(format!(
        "receive from {host}:{remote_path}"
    )))
}

// ============================================================================
// Transfer dispatch
// ============================================================================

/// Determine and execute the appropriate transfer mode.
fn execute_transfer(config: &Config) -> Result<TransferStats, ScpError> {
    let mut stats = TransferStats::new();
    let mut throttle = Throttle::new(config.bandwidth_limit);

    let any_remote_src = config.sources.iter().any(|s| s.is_remote());
    let remote_target = config.target.is_remote();

    // Case 1: Remote source -> local target.
    if any_remote_src && !remote_target {
        for src in &config.sources {
            match src {
                Location::Remote { user, host, path } => {
                    let dst = match &config.target {
                        Location::Local(p) => PathBuf::from(p),
                        Location::Remote { .. } => {
                            return Err(ScpError::Usage(
                                "remote-to-remote copy is not supported".into(),
                            ));
                        }
                    };
                    remote_recv(user, host, path, &dst, config)?;
                }
                Location::Local(_) => {
                    return Err(ScpError::Usage(
                        "cannot mix local and remote sources".into(),
                    ));
                }
            }
        }
        return Ok(stats);
    }

    // Case 2: Local source -> remote target.
    if !any_remote_src && remote_target {
        match &config.target {
            Location::Remote { user, host, path } => {
                for src in &config.sources {
                    if let Location::Local(p) = src {
                        let src_path = Path::new(p);
                        remote_send(src_path, user, host, path, config)?;
                    }
                }
            }
            Location::Local(_) => unreachable!(),
        }
        return Ok(stats);
    }

    // Case 3: Remote-to-remote (not supported).
    if any_remote_src && remote_target {
        return Err(ScpError::Usage(
            "remote-to-remote copy is not supported".into(),
        ));
    }

    // Case 4: Local-to-local copy.
    let target_str = match &config.target {
        Location::Local(p) => p.clone(),
        Location::Remote { .. } => unreachable!(),
    };
    let target_path = PathBuf::from(&target_str);

    // If there are multiple sources, the target must be a directory.
    let target_is_dir = target_path.is_dir()
        || target_str.ends_with('/')
        || target_str.ends_with('\\')
        || config.sources.len() > 1;

    if target_is_dir && !target_path.exists() {
        fs::create_dir_all(&target_path).map_err(|e| {
            ScpError::Io(format!(
                "cannot create directory {}: {e}",
                target_path.display()
            ))
        })?;
    }

    for src in &config.sources {
        let src_str = match src {
            Location::Local(p) => p.clone(),
            Location::Remote { .. } => unreachable!(),
        };
        let src_path = PathBuf::from(&src_str);

        if !src_path.exists() {
            eprintln!("scp: {}: No such file or directory", src_path.display());
            stats.errors = stats.errors.saturating_add(1);
            continue;
        }

        let dst_path = if target_is_dir {
            // Target is a directory: place source inside it.
            let name = src_path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_else(|| src_path.as_os_str().to_os_string());
            target_path.join(name)
        } else {
            target_path.clone()
        };

        if let Err(e) = copy_source_to_dest(
            &src_path,
            &dst_path,
            config,
            &mut throttle,
            &mut stats,
        ) {
            eprintln!("{e}");
            stats.errors = stats.errors.saturating_add(1);
        }
    }

    Ok(stats)
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Skip argv[0] (program name).
    let result = if args.len() > 1 {
        parse_args(&args[1..])
    } else {
        Err(ScpError::Usage("missing operand".into()))
    };

    match result {
        Ok(ParseResult::Help) => {
            print_help();
            process::exit(0);
        }
        Ok(ParseResult::Version) => {
            println!("scp {VERSION} (OurOS)");
            process::exit(0);
        }
        Ok(ParseResult::Run(config)) => {
            if config.compress && config.verbose {
                eprintln!("scp: compression flag noted (effective only with SSH transport)");
            }

            let start = clock_ns();

            match execute_transfer(&config) {
                Ok(stats) => {
                    if config.verbose {
                        let elapsed_ns = clock_ns().saturating_sub(start);
                        let elapsed_ms = elapsed_ns / 1_000_000;
                        eprintln!(
                            "Transferred {} file(s), {} dir(s), {} total in {}.{:03}s",
                            stats.files,
                            stats.directories,
                            format_size(stats.bytes),
                            elapsed_ms / 1000,
                            elapsed_ms % 1000,
                        );
                        if stats.errors > 0 {
                            eprintln!("{} error(s)", stats.errors);
                        }
                    }
                    if stats.errors > 0 {
                        process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            eprintln!("Try 'scp --help' for more information.");
            process::exit(2);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Location parsing -----------------------------------------------------

    #[test]
    fn parse_local_simple() {
        let loc = Location::parse("/tmp/file.txt");
        assert!(matches!(loc, Location::Local(ref p) if p == "/tmp/file.txt"));
    }

    #[test]
    fn parse_local_relative() {
        let loc = Location::parse("./foo/bar");
        assert!(matches!(loc, Location::Local(ref p) if p == "./foo/bar"));
    }

    #[test]
    fn parse_remote_with_user() {
        let loc = Location::parse("alice@myhost:/data/file");
        match loc {
            Location::Remote { user, host, path } => {
                assert_eq!(user.as_deref(), Some("alice"));
                assert_eq!(host, "myhost");
                assert_eq!(path, "/data/file");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn parse_remote_without_user() {
        let loc = Location::parse("myhost:/data/file");
        match loc {
            Location::Remote { user, host, path } => {
                assert!(user.is_none());
                assert_eq!(host, "myhost");
                assert_eq!(path, "/data/file");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn parse_remote_empty_path_defaults_to_dot() {
        let loc = Location::parse("user@host:");
        match loc {
            Location::Remote { path, .. } => {
                assert_eq!(path, ".");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn parse_local_with_slash_before_colon() {
        // A colon after a slash should be treated as local.
        let loc = Location::parse("/some/path:with:colons");
        assert!(matches!(loc, Location::Local(_)));
    }

    #[test]
    fn parse_windows_drive_letter() {
        let loc = Location::parse("C:\\Users\\file");
        assert!(matches!(loc, Location::Local(_)));
    }

    // -- Formatting -----------------------------------------------------------

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500B");
    }

    #[test]
    fn format_size_kb() {
        assert_eq!(format_size(2048), "2.0KB");
    }

    #[test]
    fn format_size_mb() {
        // 5 * 1024 * 1024 = 5242880
        assert_eq!(format_size(5_242_880), "5.0MB");
    }

    #[test]
    fn format_size_gb() {
        // 2 * 1024^3
        assert_eq!(format_size(2_147_483_648), "2.0GB");
    }

    #[test]
    fn format_speed_zero() {
        assert_eq!(format_speed(0), "0B/s");
    }

    #[test]
    fn format_speed_value() {
        assert_eq!(format_speed(1024), "1.0KB/s");
    }

    #[test]
    fn format_eta_seconds() {
        assert_eq!(format_eta(45), "00:45");
    }

    #[test]
    fn format_eta_minutes() {
        assert_eq!(format_eta(125), "02:05");
    }

    #[test]
    fn format_eta_hours() {
        assert_eq!(format_eta(3661), "01:01:01");
    }

    // -- Argument parsing -----------------------------------------------------

    #[test]
    fn parse_args_basic() {
        let args: Vec<String> = vec!["file1", "file2"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = parse_args(&args);
        assert!(matches!(result, Ok(ParseResult::Run(_))));
    }

    #[test]
    fn parse_args_recursive() {
        let args: Vec<String> = vec!["-r", "dir1", "dir2"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => assert!(cfg.recursive),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_preserve_verbose_quiet() {
        let args: Vec<String> = vec!["-pvq", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert!(cfg.preserve);
                assert!(cfg.verbose);
                assert!(cfg.quiet);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_port() {
        let args: Vec<String> = vec!["-P", "2222", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => assert_eq!(cfg.port, Some(2222)),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_identity() {
        let args: Vec<String> = vec!["-i", "/key", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert_eq!(cfg.identity.as_deref(), Some("/key"));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_bandwidth() {
        let args: Vec<String> = vec!["-l", "100", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => assert_eq!(cfg.bandwidth_limit, 100),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_help() {
        let args: Vec<String> = vec!["--help"].into_iter().map(String::from).collect();
        assert!(matches!(parse_args(&args), Ok(ParseResult::Help)));
    }

    #[test]
    fn parse_args_version() {
        let args: Vec<String> = vec!["--version"].into_iter().map(String::from).collect();
        assert!(matches!(parse_args(&args), Ok(ParseResult::Version)));
    }

    #[test]
    fn parse_args_too_few() {
        let args: Vec<String> = vec!["onlyone"].into_iter().map(String::from).collect();
        assert!(matches!(parse_args(&args), Err(ScpError::Usage(_))));
    }

    #[test]
    fn parse_args_unknown_flag() {
        let args: Vec<String> = vec!["-z", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(matches!(parse_args(&args), Err(ScpError::Usage(_))));
    }

    #[test]
    fn parse_args_compress_batch() {
        let args: Vec<String> = vec!["-CB", "a", "b"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert!(cfg.compress);
                assert!(cfg.batch);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_double_dash() {
        let args: Vec<String> = vec!["--", "-weird-file", "dest"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert_eq!(cfg.sources.len(), 1);
                if let Location::Local(ref p) = cfg.sources[0] {
                    assert_eq!(p, "-weird-file");
                } else {
                    panic!("expected local");
                }
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_remote_target() {
        let args: Vec<String> = vec!["file", "user@host:/tmp/"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert!(cfg.target.is_remote());
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_args_remote_source() {
        let args: Vec<String> = vec!["user@host:/etc/config", "./local"]
            .into_iter()
            .map(String::from)
            .collect();
        match parse_args(&args) {
            Ok(ParseResult::Run(cfg)) => {
                assert!(cfg.sources[0].is_remote());
                assert!(!cfg.target.is_remote());
            }
            _ => panic!("expected Run"),
        }
    }

    // -- Location display -----------------------------------------------------

    #[test]
    fn location_display_local() {
        let loc = Location::Local("/tmp/f".to_string());
        assert_eq!(loc.display(), "/tmp/f");
    }

    #[test]
    fn location_display_remote_with_user() {
        let loc = Location::Remote {
            user: Some("bob".to_string()),
            host: "srv".to_string(),
            path: "/data".to_string(),
        };
        assert_eq!(loc.display(), "bob@srv:/data");
    }

    #[test]
    fn location_display_remote_without_user() {
        let loc = Location::Remote {
            user: None,
            host: "srv".to_string(),
            path: "/data".to_string(),
        };
        assert_eq!(loc.display(), "srv:/data");
    }
}
