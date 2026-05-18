//! OurOS File Synchronization Utility
//!
//! Synchronizes files and directories locally, inspired by rsync. Compares
//! source and destination using modification time + size (default) or SHA-256
//! checksums, and transfers only changed data using a simplified block-based
//! delta algorithm.
//!
//! # Usage
//!
//! ```text
//! rsync [OPTION]... SOURCE DEST
//! rsync [OPTION]... SOURCE... DEST/
//!
//! Synchronize files and directories.
//!
//!   -r, --recursive         Recurse into directories
//!   -a, --archive           Archive mode (equals -rlpt)
//!   -v, --verbose           Show files being transferred
//!   -n, --dry-run           Perform a trial run with no changes
//!       --delete            Delete extraneous files from destination
//!       --exclude=PATTERN   Exclude files matching PATTERN
//!       --include=PATTERN   Include files matching PATTERN (overrides exclude)
//!       --progress          Show per-file transfer progress
//!   -c, --checksum          Use checksum instead of size+time comparison
//!   -u, --update            Skip files that are newer in destination
//!   -h, --human-readable    Show sizes in human-readable format
//!       --stats             Show transfer statistics at end
//!   -p, --perms             Preserve permissions
//!   -t, --times             Preserve modification times
//!   -l, --links             Preserve symlinks (copy as symlinks)
//!   -i, --itemize-changes   Show change details per file
//!       --max-size=SIZE     Skip files larger than SIZE
//!       --min-size=SIZE     Skip files smaller than SIZE
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//! ```
//!
//! # Trailing-slash semantics
//!
//! - `rsync source/ dest/` -- copies contents of source into dest
//! - `rsync source dest/`  -- copies the directory itself into dest
//!
//! # Exit codes
//!
//! - 0: success
//! - 1: some files could not be transferred
//! - 2: usage / argument error

use std::collections::HashMap;
use std::env;
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Block size for delta-transfer comparison (4 KiB).
const DELTA_BLOCK_SIZE: usize = 4096;

/// Buffer size for file copying (64 KiB).
const COPY_BUF_SIZE: usize = 64 * 1024;

// ============================================================================
// Configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    sources: Vec<String>,
    dest: String,
    recursive: bool,
    verbose: bool,
    dry_run: bool,
    delete: bool,
    excludes: Vec<String>,
    includes: Vec<String>,
    progress: bool,
    checksum: bool,
    update: bool,
    human_readable: bool,
    stats: bool,
    preserve_perms: bool,
    preserve_times: bool,
    preserve_links: bool,
    itemize: bool,
    max_size: Option<u64>,
    min_size: Option<u64>,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

/// Transfer statistics collected during synchronization.
struct Stats {
    files_transferred: u64,
    files_skipped: u64,
    files_deleted: u64,
    bytes_transferred: u64,
    bytes_total: u64,
    errors: u64,
    dirs_created: u64,
    symlinks_created: u64,
}

impl Stats {
    fn new() -> Self {
        Self {
            files_transferred: 0,
            files_skipped: 0,
            files_deleted: 0,
            bytes_transferred: 0,
            bytes_total: 0,
            errors: 0,
            dirs_created: 0,
            symlinks_created: 0,
        }
    }
}

/// Change flags for itemized output (-i).
struct ItemizeFlags {
    /// Type: f=file, d=directory, L=symlink
    file_type: char,
    /// Was the content updated?
    content_changed: bool,
    /// Size changed?
    size_changed: bool,
    /// Timestamps changed?
    time_changed: bool,
    /// Permissions changed?
    perms_changed: bool,
    /// Newly created?
    is_new: bool,
}

impl ItemizeFlags {
    /// Format as an rsync-style itemize string like `>f.st.....`
    fn format(&self) -> String {
        let mut buf = String::with_capacity(11);

        // Field 1: transfer direction (> = sent)
        if self.is_new {
            buf.push('>');
        } else if self.content_changed || self.size_changed || self.time_changed
            || self.perms_changed
        {
            buf.push('>');
        } else {
            buf.push('.');
        }

        // Field 2: file type
        buf.push(self.file_type);

        // Field 3: checksum/content
        buf.push(if self.content_changed { 'c' } else { '.' });

        // Field 4: size
        buf.push(if self.size_changed { 's' } else { '.' });

        // Field 5: timestamp
        buf.push(if self.time_changed { 't' } else { '.' });

        // Field 6: permissions
        buf.push(if self.perms_changed { 'p' } else { '.' });

        // Fields 7-11: owner, group, acl, xattr, reserved
        buf.push_str(".....");

        buf
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> Result<ParseResult, String> {
    let mut sources = Vec::new();
    let mut recursive = false;
    let mut verbose = false;
    let mut dry_run = false;
    let mut delete = false;
    let mut excludes = Vec::new();
    let mut includes = Vec::new();
    let mut progress = false;
    let mut checksum = false;
    let mut update = false;
    let mut human_readable = false;
    let mut stats = false;
    let mut preserve_perms = false;
    let mut preserve_times = false;
    let mut preserve_links = false;
    let mut itemize = false;
    let mut max_size: Option<u64> = None;
    let mut min_size: Option<u64> = None;
    let mut positional = Vec::new();

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
            positional.extend(args[i + 1..].iter().cloned());
            break;
        }

        if let Some(pat) = arg.strip_prefix("--exclude=") {
            excludes.push(pat.to_string());
            i += 1;
            continue;
        }
        if let Some(pat) = arg.strip_prefix("--include=") {
            includes.push(pat.to_string());
            i += 1;
            continue;
        }
        if let Some(val) = arg.strip_prefix("--max-size=") {
            max_size = Some(parse_size(val)?);
            i += 1;
            continue;
        }
        if let Some(val) = arg.strip_prefix("--min-size=") {
            min_size = Some(parse_size(val)?);
            i += 1;
            continue;
        }

        match arg.as_str() {
            "--recursive" => recursive = true,
            "--archive" => {
                recursive = true;
                preserve_links = true;
                preserve_perms = true;
                preserve_times = true;
            }
            "--verbose" => verbose = true,
            "--dry-run" => dry_run = true,
            "--delete" => delete = true,
            "--progress" => progress = true,
            "--checksum" => checksum = true,
            "--update" => update = true,
            "--human-readable" => human_readable = true,
            "--stats" => stats = true,
            "--perms" => preserve_perms = true,
            "--times" => preserve_times = true,
            "--links" => preserve_links = true,
            "--itemize-changes" => itemize = true,
            _ if arg.starts_with("--") => {
                return Err(format!("unknown option: {arg}"));
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                // Short flags: may be combined, e.g. -avz
                let chars: Vec<char> = arg[1..].chars().collect();
                for ch in chars {
                    match ch {
                        'r' => recursive = true,
                        'a' => {
                            recursive = true;
                            preserve_links = true;
                            preserve_perms = true;
                            preserve_times = true;
                        }
                        'v' => verbose = true,
                        'n' => dry_run = true,
                        'c' => checksum = true,
                        'u' => update = true,
                        'h' => human_readable = true,
                        'p' => preserve_perms = true,
                        't' => preserve_times = true,
                        'l' => preserve_links = true,
                        'i' => itemize = true,
                        _ => return Err(format!("unknown short option: -{ch}")),
                    }
                }
            }
            _ => {
                positional.push(arg.clone());
            }
        }

        i += 1;
    }

    if positional.len() < 2 {
        return Err("rsync: not enough arguments — need SOURCE and DEST".into());
    }

    let dest = positional.pop().expect("checked length above");
    sources = positional;

    Ok(ParseResult::Run(Config {
        sources,
        dest,
        recursive,
        verbose,
        dry_run,
        delete,
        excludes,
        includes,
        progress,
        checksum,
        update,
        human_readable,
        stats,
        preserve_perms,
        preserve_times,
        preserve_links,
        itemize,
        max_size,
        min_size,
    }))
}

/// Parse a human-readable size string into bytes.
/// Accepts: plain number, or number suffixed with K, M, G, T (case-insensitive).
fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size value".into());
    }

    let (num_part, multiplier) = if let Some(n) = s.strip_suffix(['k', 'K']) {
        (n, 1024u64)
    } else if let Some(n) = s.strip_suffix(['m', 'M']) {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix(['g', 'G']) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix(['t', 'T']) {
        (n, 1024 * 1024 * 1024 * 1024)
    } else {
        (s, 1u64)
    };

    let num: u64 = num_part
        .parse()
        .map_err(|_| format!("invalid size: {s}"))?;
    num.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflow: {s}"))
}

/// Format a byte count for human-readable display.
fn format_size(bytes: u64, human: bool) -> String {
    if !human {
        return bytes.to_string();
    }
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1024.0 || unit == "TiB" {
            if val < 10.0 {
                return format!("{val:.2}{unit}");
            } else if val < 100.0 {
                return format!("{val:.1}{unit}");
            } else {
                return format!("{val:.0}{unit}");
            }
        }
        val /= 1024.0;
    }
    format!("{bytes}")
}

fn print_help() {
    let help = "\
rsync 0.1.0 — OurOS file synchronization

Usage: rsync [OPTION]... SOURCE DEST
       rsync [OPTION]... SOURCE... DEST/

Synchronize files and directories locally.

Options:
  -r, --recursive         Recurse into directories
  -a, --archive           Archive mode (equals -rlpt)
  -v, --verbose           Show files being transferred
  -n, --dry-run           Perform a trial run with no changes
      --delete            Delete extraneous files from destination
      --exclude=PATTERN   Exclude files matching PATTERN
      --include=PATTERN   Include files matching PATTERN (overrides exclude)
      --progress          Show per-file transfer progress
  -c, --checksum          Use checksum instead of size+time comparison
  -u, --update            Skip files that are newer in destination
  -h, --human-readable    Show sizes in human-readable format
      --stats             Show transfer statistics at end
  -p, --perms             Preserve permissions
  -t, --times             Preserve modification times
  -l, --links             Preserve symlinks (copy as symlinks)
  -i, --itemize-changes   Show change details per file
      --max-size=SIZE     Skip files larger than SIZE
      --min-size=SIZE     Skip files smaller than SIZE
      --help              Display this help and exit
      --version           Output version information and exit

Trailing-slash semantics:
  rsync source/ dest/   copies contents of source into dest
  rsync source  dest/   copies the directory itself into dest

Size suffixes: K (KiB), M (MiB), G (GiB), T (TiB)
Patterns: * matches any chars, ? matches one char";
    println!("{help}");
}

// ============================================================================
// SHA-256 (minimal implementation for checksum mode)
// ============================================================================

/// Minimal SHA-256 for file checksums. We implement it here to avoid external
/// dependencies, since the OurOS sysroot may not have a crypto library.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        // Fill partial buffer first.
        if self.buf_len > 0 {
            let space = 64 - self.buf_len;
            let copy_len = space.min(data.len());
            self.buffer[self.buf_len..self.buf_len + copy_len]
                .copy_from_slice(&data[..copy_len]);
            self.buf_len += copy_len;
            offset = copy_len;
            if self.buf_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buf_len = 0;
            }
        }

        // Process full 64-byte blocks directly.
        while offset + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[offset..offset + 64]);
            self.compress(&block);
            offset += 64;
        }

        // Buffer remaining bytes.
        let remaining = data.len() - offset;
        if remaining > 0 {
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buf_len = remaining;
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);

        // Append 0x80 byte.
        let mut pad = vec![0x80u8];

        // Pad to 56 mod 64 bytes.
        let current = (self.buf_len + 1) % 64;
        let zeros_needed = if current <= 56 { 56 - current } else { 120 - current };
        pad.resize(1 + zeros_needed, 0);

        // Append length as big-endian 64-bit.
        pad.extend_from_slice(&bit_len.to_be_bytes());

        self.update(&pad);

        let mut result = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            let base = i * 4;
            result[base] = (word >> 24) as u8;
            result[base + 1] = (word >> 16) as u8;
            result[base + 2] = (word >> 8) as u8;
            result[base + 3] = word as u8;
        }
        result
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let base = i * 4;
            w[i] = u32::from_be_bytes([
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Compute SHA-256 hash of a file.
fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file =
        File::open(path).map_err(|e| format!("cannot open '{}': {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; COPY_BUF_SIZE];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read '{}': {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

// ============================================================================
// Pattern matching (glob-style for --exclude/--include)
// ============================================================================

/// Match a path component against a glob pattern.
/// Supports `*` (any sequence) and `?` (any single char).
fn glob_match(pattern: &str, name: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

/// Check whether a relative path should be excluded based on include/exclude
/// rules. Include patterns take priority: if a file matches an include, it is
/// NOT excluded even if it also matches an exclude.
fn is_excluded(rel_path: &str, config: &Config) -> bool {
    // Extract the filename component for matching.
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);

    // Check include first (overrides exclude).
    for inc in &config.includes {
        if glob_match(inc, file_name) || glob_match(inc, rel_path) {
            return false;
        }
    }

    // Check exclude patterns.
    for exc in &config.excludes {
        if glob_match(exc, file_name) || glob_match(exc, rel_path) {
            return true;
        }
    }

    false
}

// ============================================================================
// Metadata helpers
// ============================================================================

/// Extract modification time as seconds since the Unix epoch.
fn get_mtime(meta: &Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Get file permissions as a Unix mode word. Returns 0o644 on non-Unix.
#[cfg(unix)]
fn get_mode(meta: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode()
}

#[cfg(not(unix))]
fn get_mode(_meta: &Metadata) -> u32 {
    if _meta.is_dir() {
        0o755
    } else if _meta.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

/// Set file permissions.
#[cfg(unix)]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {e}", path.display()))
}

#[cfg(not(unix))]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), String> {
    // On non-Unix, we can only set readonly.
    let readonly = (mode & 0o200) == 0;
    let mut perms = fs::metadata(path)
        .map_err(|e| format!("read metadata '{}': {e}", path.display()))?
        .permissions();
    perms.set_readonly(readonly);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {e}", path.display()))
}

/// Set modification time on a file.
fn set_file_mtime(path: &Path, mtime_secs: u64) -> Result<(), String> {
    // Use filetime via raw syscall if available, otherwise try
    // the std approach. On our OS target (Unix-family) we use utimensat.
    #[cfg(target_family = "unix")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| format!("invalid path '{}': contains null byte", path.display()))?;

        // timespec: [tv_sec, tv_nsec]
        // We set both atime and mtime to the same value.
        let times: [i64; 4] = [
            mtime_secs as i64, 0, // atime
            mtime_secs as i64, 0, // mtime
        ];

        // SAFETY: utimensat with AT_FDCWD (-100) sets timestamps on the file
        // at the given path. The c_path is a valid NUL-terminated string, and
        // times is a valid pointer to two timespec structs. flags=0 means
        // follow symlinks.
        let ret = unsafe {
            utimensat(-100, c_path.as_ptr(), times.as_ptr(), 0)
        };
        if ret != 0 {
            return Err(format!(
                "set mtime on '{}': utimensat failed",
                path.display()
            ));
        }
        return Ok(());
    }

    #[cfg(not(target_family = "unix"))]
    {
        // No portable way to set mtime without dependencies; silently skip.
        let _ = (path, mtime_secs);
        Ok(())
    }
}

#[cfg(target_family = "unix")]
unsafe extern "C" {
    fn utimensat(dirfd: i32, pathname: *const i8, times: *const i64, flags: i32) -> i32;
}

// ============================================================================
// File comparison
// ============================================================================

/// Determine whether a source file needs to be transferred to dest.
/// Returns `true` if the file should be copied.
fn needs_transfer(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    config: &Config,
) -> Result<bool, String> {
    let dst_meta = match fs::symlink_metadata(dst_path) {
        Ok(m) => m,
        Err(_) => return Ok(true), // dest doesn't exist
    };

    // If update mode, skip if dest is newer.
    if config.update {
        let src_mtime = get_mtime(src_meta);
        let dst_mtime = get_mtime(&dst_meta);
        if dst_mtime > src_mtime {
            return Ok(false);
        }
    }

    if config.checksum {
        // Compare by SHA-256 hash.
        let src_hash = sha256_file(src_path)?;
        let dst_hash = sha256_file(dst_path)?;
        Ok(src_hash != dst_hash)
    } else {
        // Compare by size + mtime.
        let size_match = src_meta.len() == dst_meta.len();
        let mtime_match = get_mtime(src_meta) == get_mtime(&dst_meta);
        Ok(!size_match || !mtime_match)
    }
}

/// Compute itemize flags for a file transfer.
fn compute_itemize(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    file_type: char,
) -> ItemizeFlags {
    let dst_meta = fs::symlink_metadata(dst_path).ok();

    let is_new = dst_meta.is_none();

    let (content_changed, size_changed, time_changed, perms_changed) =
        if let Some(ref dm) = dst_meta {
            let sc = src_meta.len() != dm.len()
                || get_mtime(src_meta) != get_mtime(dm);
            let sz = src_meta.len() != dm.len();
            let tm = get_mtime(src_meta) != get_mtime(dm);
            let pm = get_mode(src_meta) != get_mode(dm);
            (sc, sz, tm, pm)
        } else {
            (true, true, true, true)
        };

    ItemizeFlags {
        file_type,
        content_changed,
        size_changed,
        time_changed,
        perms_changed,
        is_new,
    }
}

// ============================================================================
// File transfer (with optional delta)
// ============================================================================

/// Copy a file from src to dst, optionally showing progress.
/// Uses a block-based delta approach: if the destination already exists and
/// has the same size, only rewrite blocks that differ.
fn transfer_file(
    src_path: &Path,
    dst_path: &Path,
    config: &Config,
    stats: &mut Stats,
) -> Result<u64, String> {
    let src_meta = fs::metadata(src_path)
        .map_err(|e| format!("stat '{}': {e}", src_path.display()))?;
    let file_size = src_meta.len();
    stats.bytes_total = stats.bytes_total.wrapping_add(file_size);

    if config.dry_run {
        return Ok(file_size);
    }

    // Try delta transfer if dest exists and has the same size.
    let dst_exists = dst_path.is_file();
    if dst_exists {
        if let Ok(dst_meta) = fs::metadata(dst_path) {
            if dst_meta.len() == file_size && file_size > 0 {
                return delta_transfer(src_path, dst_path, file_size, config, stats);
            }
        }
    }

    // Full copy.
    full_copy(src_path, dst_path, file_size, config, stats)
}

/// Full file copy (no delta).
fn full_copy(
    src_path: &Path,
    dst_path: &Path,
    file_size: u64,
    config: &Config,
    stats: &mut Stats,
) -> Result<u64, String> {
    let mut src = File::open(src_path)
        .map_err(|e| format!("open '{}': {e}", src_path.display()))?;
    let mut dst = File::create(dst_path)
        .map_err(|e| format!("create '{}': {e}", dst_path.display()))?;

    let mut buf = [0u8; COPY_BUF_SIZE];
    let mut written: u64 = 0;
    let mut last_progress_pct: u64 = u64::MAX;

    loop {
        let n = src
            .read(&mut buf)
            .map_err(|e| format!("read '{}': {e}", src_path.display()))?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])
            .map_err(|e| format!("write '{}': {e}", dst_path.display()))?;
        written = written.wrapping_add(n as u64);

        if config.progress && file_size > 0 {
            let pct = written.saturating_mul(100) / file_size;
            if pct != last_progress_pct {
                last_progress_pct = pct;
                eprint!(
                    "\r  {}/{} ({pct}%)",
                    format_size(written, config.human_readable),
                    format_size(file_size, config.human_readable),
                );
            }
        }
    }

    if config.progress && file_size > 0 {
        eprintln!();
    }

    stats.bytes_transferred = stats.bytes_transferred.wrapping_add(written);
    Ok(written)
}

/// Delta transfer: compare blocks and only rewrite those that differ.
fn delta_transfer(
    src_path: &Path,
    dst_path: &Path,
    file_size: u64,
    config: &Config,
    stats: &mut Stats,
) -> Result<u64, String> {
    let mut src = File::open(src_path)
        .map_err(|e| format!("open '{}': {e}", src_path.display()))?;
    let mut dst = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dst_path)
        .map_err(|e| format!("open '{}' for delta: {e}", dst_path.display()))?;

    let mut src_buf = [0u8; DELTA_BLOCK_SIZE];
    let mut dst_buf = [0u8; DELTA_BLOCK_SIZE];
    let mut offset: u64 = 0;
    let mut bytes_written: u64 = 0;
    let mut last_progress_pct: u64 = u64::MAX;

    loop {
        let src_n = read_full(&mut src, &mut src_buf)
            .map_err(|e| format!("read '{}': {e}", src_path.display()))?;
        if src_n == 0 {
            break;
        }

        let dst_n = read_full(&mut dst, &mut dst_buf[..src_n])
            .map_err(|e| format!("read '{}': {e}", dst_path.display()))?;

        // If the blocks differ, seek back and write the source block.
        if dst_n != src_n || src_buf[..src_n] != dst_buf[..src_n] {
            use std::io::Seek;
            dst.seek(std::io::SeekFrom::Start(offset))
                .map_err(|e| format!("seek '{}': {e}", dst_path.display()))?;
            dst.write_all(&src_buf[..src_n])
                .map_err(|e| format!("write '{}': {e}", dst_path.display()))?;
            // Seek forward past what we just wrote so the next read_full
            // reads from the correct position.
            dst.seek(std::io::SeekFrom::Start(offset + src_n as u64))
                .map_err(|e| format!("seek '{}': {e}", dst_path.display()))?;
            bytes_written = bytes_written.wrapping_add(src_n as u64);
        }

        offset = offset.wrapping_add(src_n as u64);

        if config.progress && file_size > 0 {
            let pct = offset.saturating_mul(100) / file_size;
            if pct != last_progress_pct {
                last_progress_pct = pct;
                eprint!(
                    "\r  {}/{} ({pct}%) delta",
                    format_size(offset, config.human_readable),
                    format_size(file_size, config.human_readable),
                );
            }
        }
    }

    if config.progress && file_size > 0 {
        eprintln!();
    }

    stats.bytes_transferred = stats.bytes_transferred.wrapping_add(bytes_written);
    Ok(bytes_written)
}

/// Read exactly `buf.len()` bytes (or until EOF). Returns the number of bytes
/// actually read.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = reader.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

// ============================================================================
// Symlink handling
// ============================================================================

/// Copy a symlink from src to dst. Reads the link target and creates a new
/// symlink at dst pointing to the same target.
#[cfg(target_family = "unix")]
fn copy_symlink(src: &Path, dst: &Path, dry_run: bool) -> Result<(), String> {
    let target = fs::read_link(src)
        .map_err(|e| format!("readlink '{}': {e}", src.display()))?;

    if dry_run {
        return Ok(());
    }

    // Remove existing destination if present.
    if dst.exists() || dst.symlink_metadata().is_ok() {
        fs::remove_file(dst)
            .or_else(|_| fs::remove_dir(dst))
            .map_err(|e| format!("remove '{}': {e}", dst.display()))?;
    }

    std::os::unix::fs::symlink(&target, dst)
        .map_err(|e| format!("symlink '{}' -> '{}': {e}", dst.display(), target.display()))
}

#[cfg(not(target_family = "unix"))]
fn copy_symlink(src: &Path, dst: &Path, dry_run: bool) -> Result<(), String> {
    // On non-Unix, copy symlinks as regular files.
    if dry_run {
        return Ok(());
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("copy symlink '{}' -> '{}': {e}", src.display(), dst.display()))
}

// ============================================================================
// Directory scanning
// ============================================================================

/// An entry in the file tree with its relative path and metadata.
struct FileEntry {
    /// Path relative to the sync root (uses `/` separator).
    rel_path: String,
    /// Full absolute path.
    full_path: PathBuf,
    /// Cached metadata (symlink_metadata).
    meta: Metadata,
}

/// Recursively scan a directory tree, returning all entries with relative paths.
/// Directories are included as entries too.
fn scan_tree(root: &Path, config: &Config) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    scan_tree_inner(root, root, config, &mut entries)?;
    // Sort for deterministic order.
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(entries)
}

fn scan_tree_inner(
    root: &Path,
    current: &Path,
    config: &Config,
    entries: &mut Vec<FileEntry>,
) -> Result<(), String> {
    let read_dir = fs::read_dir(current)
        .map_err(|e| format!("read dir '{}': {e}", current.display()))?;

    for entry in read_dir {
        let entry =
            entry.map_err(|e| format!("read dir entry in '{}': {e}", current.display()))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("stat '{}': {e}", path.display()))?;

        // Compute relative path using forward slashes.
        let rel = path
            .strip_prefix(root)
            .map_err(|_| format!("path '{}' not under root '{}'", path.display(), root.display()))?
            .to_string_lossy()
            .replace('\\', "/");

        // Check exclusion.
        if is_excluded(&rel, config) {
            continue;
        }

        // Check size limits for files.
        if meta.is_file() {
            if let Some(max) = config.max_size {
                if meta.len() > max {
                    continue;
                }
            }
            if let Some(min) = config.min_size {
                if meta.len() < min {
                    continue;
                }
            }
        }

        entries.push(FileEntry {
            rel_path: rel,
            full_path: path.clone(),
            meta: meta.clone(),
        });

        if meta.is_dir() {
            scan_tree_inner(root, &path, config, entries)?;
        }
    }

    Ok(())
}

// ============================================================================
// Synchronization engine
// ============================================================================

/// Synchronize a single source to a destination directory.
fn sync_one(
    src_path: &Path,
    dst_base: &Path,
    config: &Config,
    stats: &mut Stats,
) -> Result<(), String> {
    let src_meta = fs::symlink_metadata(src_path)
        .map_err(|e| format!("stat '{}': {e}", src_path.display()))?;

    if src_meta.is_file() {
        // Single file sync.
        return sync_file_entry(src_path, dst_base, &src_meta, "", config, stats);
    }

    if src_meta.is_symlink() && config.preserve_links {
        return sync_symlink_entry(src_path, dst_base, "", config, stats);
    }

    if !src_meta.is_dir() {
        return Err(format!(
            "'{}' is not a file or directory",
            src_path.display()
        ));
    }

    // Directory sync.
    if !config.recursive {
        return Err(format!(
            "skipping directory '{}' (use -r for recursive)",
            src_path.display()
        ));
    }

    // Ensure destination directory exists.
    if !config.dry_run {
        fs::create_dir_all(dst_base)
            .map_err(|e| format!("create dir '{}': {e}", dst_base.display()))?;
    }

    // Scan source tree.
    let src_entries = scan_tree(src_path, config)?;

    // Build a set of source relative paths for delete detection.
    let src_rel_set: HashMap<&str, ()> = src_entries
        .iter()
        .map(|e| (e.rel_path.as_str(), ()))
        .collect();

    // Process source entries.
    for entry in &src_entries {
        let dst_entry_path = dst_base.join(&entry.rel_path);

        if entry.meta.is_dir() {
            // Create directory if needed.
            if !dst_entry_path.is_dir() {
                if config.verbose || config.itemize {
                    let prefix = if config.itemize {
                        let flags = compute_itemize(
                            &entry.full_path,
                            &dst_entry_path,
                            &entry.meta,
                            'd',
                        );
                        format!("{} ", flags.format())
                    } else {
                        String::new()
                    };
                    println!("{prefix}created directory {}/", entry.rel_path);
                }
                if !config.dry_run {
                    fs::create_dir_all(&dst_entry_path)
                        .map_err(|e| {
                            format!("create dir '{}': {e}", dst_entry_path.display())
                        })?;
                }
                stats.dirs_created += 1;
            }
            // Preserve permissions on directory.
            if config.preserve_perms && !config.dry_run && dst_entry_path.is_dir() {
                if let Err(e) = set_file_permissions(&dst_entry_path, get_mode(&entry.meta)) {
                    eprintln!("rsync: warning: {e}");
                }
            }
        } else if entry.meta.is_symlink() && config.preserve_links {
            sync_symlink_entry(
                &entry.full_path,
                &dst_entry_path,
                &entry.rel_path,
                config,
                stats,
            )?;
        } else if entry.meta.is_file() {
            sync_file_entry(
                &entry.full_path,
                &dst_entry_path,
                &entry.meta,
                &entry.rel_path,
                config,
                stats,
            )?;
        }
    }

    // Delete phase: remove files in dest not in source.
    if config.delete {
        delete_extraneous(dst_base, &src_rel_set, config, stats)?;
    }

    Ok(())
}

/// Synchronize a single file entry.
fn sync_file_entry(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    rel_path: &str,
    config: &Config,
    stats: &mut Stats,
) -> Result<(), String> {
    let transfer = needs_transfer(src_path, dst_path, src_meta, config)?;

    if !transfer {
        stats.files_skipped += 1;
        return Ok(());
    }

    let display_path = if rel_path.is_empty() {
        src_path.display().to_string()
    } else {
        rel_path.to_string()
    };

    if config.itemize {
        let flags = compute_itemize(src_path, dst_path, src_meta, 'f');
        println!("{} {display_path}", flags.format());
    } else if config.verbose {
        println!("{display_path}");
    }

    // Ensure parent directory exists.
    if !config.dry_run {
        if let Some(parent) = dst_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("create dir '{}': {e}", parent.display()))?;
            }
        }
    }

    transfer_file(src_path, dst_path, config, stats)?;

    stats.files_transferred += 1;

    // Preserve metadata.
    if !config.dry_run {
        if config.preserve_perms {
            if let Err(e) = set_file_permissions(dst_path, get_mode(src_meta)) {
                eprintln!("rsync: warning: {e}");
            }
        }
        if config.preserve_times {
            if let Err(e) = set_file_mtime(dst_path, get_mtime(src_meta)) {
                eprintln!("rsync: warning: {e}");
            }
        }
    }

    Ok(())
}

/// Synchronize a symlink entry.
fn sync_symlink_entry(
    src_path: &Path,
    dst_path: &Path,
    rel_path: &str,
    config: &Config,
    stats: &mut Stats,
) -> Result<(), String> {
    let display_path = if rel_path.is_empty() {
        src_path.display().to_string()
    } else {
        rel_path.to_string()
    };

    if config.itemize {
        let src_meta = fs::symlink_metadata(src_path)
            .map_err(|e| format!("stat '{}': {e}", src_path.display()))?;
        let flags = compute_itemize(src_path, dst_path, &src_meta, 'L');
        println!("{} {display_path}", flags.format());
    } else if config.verbose {
        println!("{display_path} -> symlink");
    }

    copy_symlink(src_path, dst_path, config.dry_run)?;
    stats.symlinks_created += 1;

    Ok(())
}

/// Delete files in destination that are not present in the source set.
fn delete_extraneous(
    dst_base: &Path,
    src_rel_set: &HashMap<&str, ()>,
    config: &Config,
    stats: &mut Stats,
) -> Result<(), String> {
    let dst_entries = match scan_tree(dst_base, config) {
        Ok(e) => e,
        Err(_) => return Ok(()), // destination might not have a tree yet
    };

    // Process in reverse order so files are deleted before their parent dirs.
    for entry in dst_entries.iter().rev() {
        if src_rel_set.contains_key(entry.rel_path.as_str()) {
            continue;
        }

        if config.verbose || config.itemize {
            let prefix = if config.itemize {
                "*deleting ".to_string()
            } else {
                "deleting ".to_string()
            };
            println!("{prefix}{}", entry.rel_path);
        }

        if !config.dry_run {
            if entry.meta.is_dir() {
                // Only remove if empty (files inside should have been removed
                // already since we iterate in reverse sorted order).
                let _ = fs::remove_dir(&entry.full_path);
            } else {
                fs::remove_file(&entry.full_path)
                    .map_err(|e| {
                        format!("delete '{}': {e}", entry.full_path.display())
                    })?;
            }
        }

        stats.files_deleted += 1;
    }

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn run() -> Result<bool, String> {
    let args: Vec<String> = env::args().skip(1).collect();

    let config = match parse_args(&args)? {
        ParseResult::Help => {
            print_help();
            return Ok(true);
        }
        ParseResult::Version => {
            println!("rsync {VERSION}");
            return Ok(true);
        }
        ParseResult::Run(c) => c,
    };

    if config.dry_run && config.verbose {
        eprintln!("(dry run)");
    }

    let mut stats = Stats::new();
    let mut had_error = false;

    for source in &config.sources {
        // Determine the effective source path and whether trailing slash.
        let trailing_slash = source.ends_with('/') || source.ends_with('\\');
        let src_path = PathBuf::from(source);

        // Determine destination path.
        let dst_path = if config.sources.len() == 1
            && !PathBuf::from(&config.dest).is_dir()
            && !config.dest.ends_with('/')
            && !config.dest.ends_with('\\')
        {
            // Single source to non-directory dest: direct mapping.
            PathBuf::from(&config.dest)
        } else if src_path.is_dir() && !trailing_slash {
            // No trailing slash on directory source: copy the directory itself
            // into dest. E.g. `rsync source dest/` -> `dest/source/...`
            let dir_name = src_path
                .file_name()
                .ok_or_else(|| format!("cannot determine name for '{source}'"))?;
            PathBuf::from(&config.dest).join(dir_name)
        } else {
            // Trailing slash on directory source: copy contents into dest.
            PathBuf::from(&config.dest)
        };

        if let Err(e) = sync_one(&src_path, &dst_path, &config, &mut stats) {
            eprintln!("rsync: {e}");
            stats.errors += 1;
            had_error = true;
        }
    }

    // Print statistics if requested.
    if config.stats {
        eprintln!();
        eprintln!("Number of files transferred: {}", stats.files_transferred);
        eprintln!("Number of files skipped: {}", stats.files_skipped);
        if config.delete {
            eprintln!("Number of files deleted: {}", stats.files_deleted);
        }
        eprintln!("Number of directories created: {}", stats.dirs_created);
        if config.preserve_links {
            eprintln!("Number of symlinks created: {}", stats.symlinks_created);
        }
        eprintln!(
            "Total transferred: {}",
            format_size(stats.bytes_transferred, config.human_readable)
        );
        eprintln!(
            "Total size: {}",
            format_size(stats.bytes_total, config.human_readable)
        );
        if stats.errors > 0 {
            eprintln!("Errors: {}", stats.errors);
        }
    }

    Ok(!had_error)
}

fn main() {
    match run() {
        Ok(true) => process::exit(0),
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("rsync: {e}");
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

    // -- parse_size -----------------------------------------------------------

    #[test]
    fn test_parse_size_plain() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_size_kilo() {
        assert_eq!(parse_size("4K").unwrap(), 4096);
        assert_eq!(parse_size("4k").unwrap(), 4096);
    }

    #[test]
    fn test_parse_size_mega() {
        assert_eq!(parse_size("2M").unwrap(), 2 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_giga() {
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_tera() {
        assert_eq!(parse_size("1T").unwrap(), 1024u64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_empty() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc").is_err());
    }

    // -- format_size ----------------------------------------------------------

    #[test]
    fn test_format_size_not_human() {
        assert_eq!(format_size(12345, false), "12345");
    }

    #[test]
    fn test_format_size_human_bytes() {
        assert_eq!(format_size(500, true), "500B");
    }

    #[test]
    fn test_format_size_human_kib() {
        let s = format_size(2048, true);
        assert!(s.contains("KiB"), "got: {s}");
    }

    #[test]
    fn test_format_size_human_mib() {
        let s = format_size(5 * 1024 * 1024, true);
        assert!(s.contains("MiB"), "got: {s}");
    }

    // -- glob_match -----------------------------------------------------------

    #[test]
    fn test_glob_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.txt", "foo.txt"));
        assert!(!glob_match("*.txt", "foo.rs"));
        assert!(glob_match("foo*", "foobar"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
    }

    #[test]
    fn test_glob_combined() {
        assert!(glob_match("test_*.rs", "test_foo.rs"));
        assert!(!glob_match("test_*.rs", "test_foo.txt"));
    }

    // -- is_excluded ----------------------------------------------------------

    #[test]
    fn test_is_excluded_basic() {
        let config = Config {
            sources: vec![],
            dest: String::new(),
            recursive: false,
            verbose: false,
            dry_run: false,
            delete: false,
            excludes: vec!["*.tmp".to_string()],
            includes: vec![],
            progress: false,
            checksum: false,
            update: false,
            human_readable: false,
            stats: false,
            preserve_perms: false,
            preserve_times: false,
            preserve_links: false,
            itemize: false,
            max_size: None,
            min_size: None,
        };
        assert!(is_excluded("foo.tmp", &config));
        assert!(!is_excluded("foo.txt", &config));
    }

    #[test]
    fn test_include_overrides_exclude() {
        let config = Config {
            sources: vec![],
            dest: String::new(),
            recursive: false,
            verbose: false,
            dry_run: false,
            delete: false,
            excludes: vec!["*.log".to_string()],
            includes: vec!["important.log".to_string()],
            progress: false,
            checksum: false,
            update: false,
            human_readable: false,
            stats: false,
            preserve_perms: false,
            preserve_times: false,
            preserve_links: false,
            itemize: false,
            max_size: None,
            min_size: None,
        };
        assert!(!is_excluded("important.log", &config));
        assert!(is_excluded("debug.log", &config));
    }

    // -- parse_args -----------------------------------------------------------

    #[test]
    fn test_parse_args_basic() {
        let args: Vec<String> = vec!["src".into(), "dst".into()];
        match parse_args(&args).unwrap() {
            ParseResult::Run(c) => {
                assert_eq!(c.sources, vec!["src"]);
                assert_eq!(c.dest, "dst");
                assert!(!c.recursive);
                assert!(!c.verbose);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_args_archive() {
        let args: Vec<String> = vec!["-a".into(), "src/".into(), "dst/".into()];
        match parse_args(&args).unwrap() {
            ParseResult::Run(c) => {
                assert!(c.recursive);
                assert!(c.preserve_links);
                assert!(c.preserve_perms);
                assert!(c.preserve_times);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_args_combined_short() {
        let args: Vec<String> = vec!["-rvn".into(), "a".into(), "b".into()];
        match parse_args(&args).unwrap() {
            ParseResult::Run(c) => {
                assert!(c.recursive);
                assert!(c.verbose);
                assert!(c.dry_run);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_args_exclude_include() {
        let args: Vec<String> = vec![
            "--exclude=*.o".into(),
            "--include=keep.o".into(),
            "s".into(),
            "d".into(),
        ];
        match parse_args(&args).unwrap() {
            ParseResult::Run(c) => {
                assert_eq!(c.excludes, vec!["*.o"]);
                assert_eq!(c.includes, vec!["keep.o"]);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_args_help() {
        let args: Vec<String> = vec!["--help".into()];
        assert!(matches!(parse_args(&args).unwrap(), ParseResult::Help));
    }

    #[test]
    fn test_parse_args_version() {
        let args: Vec<String> = vec!["--version".into()];
        assert!(matches!(parse_args(&args).unwrap(), ParseResult::Version));
    }

    #[test]
    fn test_parse_args_not_enough() {
        let args: Vec<String> = vec!["only_one".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_unknown_long() {
        let args: Vec<String> = vec!["--bogus".into(), "a".into(), "b".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_unknown_short() {
        let args: Vec<String> = vec!["-z".into(), "a".into(), "b".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_max_min_size() {
        let args: Vec<String> = vec![
            "--max-size=10M".into(),
            "--min-size=1K".into(),
            "a".into(),
            "b".into(),
        ];
        match parse_args(&args).unwrap() {
            ParseResult::Run(c) => {
                assert_eq!(c.max_size, Some(10 * 1024 * 1024));
                assert_eq!(c.min_size, Some(1024));
            }
            _ => panic!("expected Run"),
        }
    }

    // -- SHA-256 --------------------------------------------------------------

    #[test]
    fn test_sha256_empty() {
        let hash = Sha256::new().finalize();
        // SHA-256 of "" = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(hash[0], 0xe3);
        assert_eq!(hash[1], 0xb0);
        assert_eq!(hash[31], 0x55);
    }

    #[test]
    fn test_sha256_abc() {
        let mut h = Sha256::new();
        h.update(b"abc");
        let hash = h.finalize();
        // SHA-256 of "abc" = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(hash[0], 0xba);
        assert_eq!(hash[1], 0x78);
        assert_eq!(hash[31], 0xad);
    }

    #[test]
    fn test_sha256_longer() {
        let mut h = Sha256::new();
        // Input longer than one block (64 bytes).
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        h.update(data);
        let hash = h.finalize();
        // SHA-256 of that string = 248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1
        assert_eq!(hash[0], 0x24);
        assert_eq!(hash[1], 0x8d);
        assert_eq!(hash[31], 0xc1);
    }

    #[test]
    fn test_sha256_incremental() {
        // Feeding data byte-by-byte should produce the same result.
        let mut h1 = Sha256::new();
        h1.update(b"hello world");
        let hash1 = h1.finalize();

        let mut h2 = Sha256::new();
        for &b in b"hello world" {
            h2.update(&[b]);
        }
        let hash2 = h2.finalize();

        assert_eq!(hash1, hash2);
    }

    // -- ItemizeFlags ---------------------------------------------------------

    #[test]
    fn test_itemize_new_file() {
        let flags = ItemizeFlags {
            file_type: 'f',
            content_changed: true,
            size_changed: true,
            time_changed: true,
            perms_changed: true,
            is_new: true,
        };
        let s = flags.format();
        assert_eq!(s.len(), 11);
        assert_eq!(&s[..2], ">f");
        assert!(s.contains('c'));
        assert!(s.contains('s'));
        assert!(s.contains('t'));
        assert!(s.contains('p'));
    }

    #[test]
    fn test_itemize_unchanged() {
        let flags = ItemizeFlags {
            file_type: 'f',
            content_changed: false,
            size_changed: false,
            time_changed: false,
            perms_changed: false,
            is_new: false,
        };
        let s = flags.format();
        assert_eq!(&s, ".f...........");
    }

    // -- read_full ------------------------------------------------------------

    #[test]
    fn test_read_full_exact() {
        let data = b"hello world!";
        let mut cursor = std::io::Cursor::new(data);
        let mut buf = [0u8; 5];
        let n = read_full(&mut cursor, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn test_read_full_short() {
        let data = b"hi";
        let mut cursor = std::io::Cursor::new(data);
        let mut buf = [0u8; 10];
        let n = read_full(&mut cursor, &mut buf).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], b"hi");
    }
}
