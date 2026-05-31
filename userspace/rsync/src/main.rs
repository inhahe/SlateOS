//! OurOS File Synchronization Utility (rsync / scp)
//!
//! Multi-personality binary providing:
//! - **rsync** -- file synchronization with delta transfer
//! - **scp**   -- secure copy (simplified local mode, SSH syntax parsing)
//!
//! Personality is detected from `argv[0]` basename: if it contains `scp`,
//! the binary runs in SCP mode; otherwise it runs as rsync.
//!
//! # rsync usage
//!
//! ```text
//! rsync [OPTION]... SOURCE DEST
//! rsync [OPTION]... SOURCE... DEST/
//!
//!   -r, --recursive         Recurse into directories
//!   -a, --archive           Archive mode (equals -rlpt)
//!   -v, --verbose           Show files being transferred
//!   -n, --dry-run           Perform a trial run with no changes
//!   -z                      Compression flag (reserved)
//!       --delete            Delete extraneous files from destination
//!       --exclude=PATTERN   Exclude files matching PATTERN
//!       --include=PATTERN   Include files matching PATTERN (overrides exclude)
//!       --progress          Show per-file transfer progress
//!   -c, --checksum          Use checksum instead of size+time comparison
//!   -u, --update            Skip files newer in destination
//!   -h, --human-readable    Show sizes in human-readable format
//!       --stats             Show transfer statistics at end
//!   -p, --perms             Preserve permissions
//!   -t, --times             Preserve modification times
//!   -l, --links             Preserve symlinks
//!   -i, --itemize-changes   Show change details per file
//!       --max-size=SIZE     Skip files larger than SIZE
//!       --min-size=SIZE     Skip files smaller than SIZE
//! ```
//!
//! # scp usage
//!
//! ```text
//! scp [OPTION]... SOURCE... TARGET
//!
//!   -r              Recursively copy directories
//!   -p              Preserve timestamps and permissions
//!   -v              Verbose mode
//! ```
//!
//! Remote path syntax (`host:path` or `user@host:path`) is parsed and
//! recognized, producing a clear error since SSH transport is not yet wired.

#![cfg_attr(not(test), no_main)]
// Under `cfg(test)` the freestanding `extern "C" fn main` entry point (and
// therefore the entire production call tree it reaches: rsync_run/scp_run,
// transfer/sync helpers, etc.) is excluded, because the test harness supplies
// its own `main` and exercises the individual functions directly.  That makes
// the production tree legitimately unreachable in the test build, so silence
// the resulting dead-code noise there only; the non-test build still warns.
#![cfg_attr(test, allow(dead_code))]

use std::collections::HashMap;
// `env::args` is only used by the freestanding `main`, which is excluded under
// `cfg(test)`; gate the import to avoid an unused-import warning in tests.
#[cfg(not(test))]
use std::env;
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Block size for rolling-checksum delta comparison (4 KiB).
const DELTA_BLOCK_SIZE: usize = 4096;

/// Buffer size for bulk file I/O (64 KiB).
const COPY_BUF_SIZE: usize = 64 * 1024;

/// Adler32 modulus.
const ADLER_MOD: u32 = 65521;

/// Native OurOS monotonic clock (kernel syscall/number.rs); no-arg, returns
/// boot-relative nanoseconds in rax.  (Syscall 30 is SYS_IRQ_REGISTER.)
const SYS_CLOCK_MONOTONIC: u64 = 10;

// ============================================================================
// Personality detection
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Personality {
    Rsync,
    Scp,
}

/// Detect personality from `argv[0]` basename.
fn detect_personality(argv0: &[u8]) -> Personality {
    // Extract basename (after last `/` or `\`).
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };
    // Strip `.exe` suffix if present.
    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe")
    {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"scp") {
        Personality::Scp
    } else {
        Personality::Rsync
    }
}

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// Caller must ensure `nr` is a valid syscall number and all arguments are
/// valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. `syscall` clobbers
    // rcx and r11 per the x86_64 ABI.
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
#[cfg(target_arch = "x86_64")]
fn clock_ns() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments.
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

#[cfg(not(target_arch = "x86_64"))]
fn clock_ns() -> u64 {
    0
}

// ============================================================================
// Output helpers (byte-slice based)
// ============================================================================

fn write_stdout(data: &[u8]) {
    let _ = io::stdout().write_all(data);
}

fn write_stderr(data: &[u8]) {
    let _ = io::stderr().write_all(data);
}

// ============================================================================
// Rolling checksum (Adler32-style)
// ============================================================================

/// Rolling Adler32-style checksum for delta detection.
///
/// `new`/`roll`/`reset_with` (and the `count` field they depend on) implement
/// the *sliding-window* primitive of the classic rsync algorithm: advancing the
/// window one byte at a time to find matching blocks at arbitrary offsets after
/// insertions/deletions.  The current local file-to-file `delta_transfer` only
/// compares blocks at identical offsets (sufficient to skip rewriting unchanged
/// blocks on disk), so the sliding-window methods are not yet wired into a
/// production path — true rolling-window delta only pays off over a network
/// transport, which this binary does not yet implement.  They are kept (and
/// unit-tested) as the correct primitive for that future path; see todo.txt.
#[derive(Clone, Copy)]
struct RollingChecksum {
    a: u32,
    b: u32,
    // Read only by `roll` (below), which is reserved for the sliding-window path.
    #[allow(dead_code)]
    count: usize,
}

impl RollingChecksum {
    #[allow(dead_code)] // sliding-window primitive; see struct-level note
    fn new() -> Self {
        Self {
            a: 1,
            b: 0,
            count: 0,
        }
    }

    /// Compute checksum over a full block.
    fn from_block(data: &[u8]) -> Self {
        let mut a: u32 = 1;
        let mut b: u32 = 0;
        for &byte in data {
            a = (a.wrapping_add(byte as u32)) % ADLER_MOD;
            b = (b.wrapping_add(a)) % ADLER_MOD;
        }
        Self {
            a,
            b,
            count: data.len(),
        }
    }

    /// Digest value combining both halves.
    fn digest(self) -> u32 {
        (self.b << 16) | self.a
    }

    /// Roll the checksum: remove `old_byte`, add `new_byte`.
    #[allow(dead_code)] // sliding-window primitive; see struct-level note
    fn roll(&mut self, old_byte: u8, new_byte: u8) {
        self.a = (self
            .a
            .wrapping_add(new_byte as u32)
            .wrapping_sub(old_byte as u32))
            % ADLER_MOD;
        self.b = (self.b.wrapping_add(self.a).wrapping_sub(
            (self.count as u32)
                .wrapping_mul(old_byte as u32)
                .wrapping_add(1),
        )) % ADLER_MOD;
    }

    /// Reset and compute over a new block.
    #[allow(dead_code)] // sliding-window primitive; see struct-level note
    fn reset_with(&mut self, data: &[u8]) {
        *self = Self::from_block(data);
    }
}

// ============================================================================
// SHA-256 (minimal implementation for checksum mode)
// ============================================================================

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        if self.buf_len > 0 {
            let space = 64 - self.buf_len;
            let copy_len = space.min(data.len());
            self.buffer[self.buf_len..self.buf_len + copy_len].copy_from_slice(&data[..copy_len]);
            self.buf_len += copy_len;
            offset = copy_len;
            if self.buf_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buf_len = 0;
            }
        }

        while offset + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[offset..offset + 64]);
            self.compress(&block);
            offset += 64;
        }

        let remaining = data.len() - offset;
        if remaining > 0 {
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buf_len = remaining;
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let current = (self.buf_len + 1) % 64;
        let zeros_needed = if current <= 56 {
            56 - current
        } else {
            120 - current
        };
        pad.resize(1 + zeros_needed, 0);
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
        for (i, wi) in w.iter_mut().enumerate().take(16) {
            let base = i * 4;
            *wi = u32::from_be_bytes([
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (i, &wi) in w.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(wi);
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
// Glob pattern matching (for --exclude / --include)
// ============================================================================

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

fn is_excluded(rel_path: &str, excludes: &[String], includes: &[String]) -> bool {
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);

    // Include overrides exclude.
    for inc in includes {
        if glob_match(inc, file_name) || glob_match(inc, rel_path) {
            return false;
        }
    }
    for exc in excludes {
        if glob_match(exc, file_name) || glob_match(exc, rel_path) {
            return true;
        }
    }
    false
}

// ============================================================================
// Size parsing / formatting
// ============================================================================

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
    let num: u64 = num_part.parse().map_err(|_| format!("invalid size: {s}"))?;
    num.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflow: {s}"))
}

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

fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "0B/s".into();
    }
    let s = format_size(bytes_per_sec, true);
    format!("{s}/s")
}

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
// Metadata helpers
// ============================================================================

fn get_mtime(meta: &Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

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

#[cfg(unix)]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {e}", path.display()))
}

#[cfg(not(unix))]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), String> {
    let readonly = (mode & 0o200) == 0;
    let mut perms = fs::metadata(path)
        .map_err(|e| format!("read metadata '{}': {e}", path.display()))?
        .permissions();
    perms.set_readonly(readonly);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {e}", path.display()))
}

fn set_file_mtime(path: &Path, mtime_secs: u64) -> Result<(), String> {
    #[cfg(target_family = "unix")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| format!("invalid path '{}': contains null byte", path.display()))?;

        let times: [i64; 4] = [
            mtime_secs as i64,
            0, // atime
            mtime_secs as i64,
            0, // mtime
        ];

        // SAFETY: utimensat with AT_FDCWD (-100) sets timestamps on the file
        // at the given path. c_path is a valid NUL-terminated string, times is
        // a pointer to two timespec structs. flags=0 follows symlinks.
        let ret = unsafe { utimensat(-100, c_path.as_ptr(), times.as_ptr(), 0) };
        if ret != 0 {
            return Err(format!(
                "set mtime on '{}': utimensat failed",
                path.display()
            ));
        }
        Ok(())
    }

    #[cfg(not(target_family = "unix"))]
    {
        let _ = (path, mtime_secs);
        Ok(())
    }
}

#[cfg(target_family = "unix")]
unsafe extern "C" {
    fn utimensat(dirfd: i32, pathname: *const i8, times: *const i64, flags: i32) -> i32;
}

// ============================================================================
// File list exchange (scan tree)
// ============================================================================

struct FileEntry {
    rel_path: String,
    full_path: PathBuf,
    meta: Metadata,
}

fn scan_tree(
    root: &Path,
    excludes: &[String],
    includes: &[String],
    max_size: Option<u64>,
    min_size: Option<u64>,
) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    scan_tree_inner(
        root,
        root,
        excludes,
        includes,
        max_size,
        min_size,
        &mut entries,
    )?;
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(entries)
}

fn scan_tree_inner(
    root: &Path,
    current: &Path,
    excludes: &[String],
    includes: &[String],
    max_size: Option<u64>,
    min_size: Option<u64>,
    entries: &mut Vec<FileEntry>,
) -> Result<(), String> {
    let read_dir =
        fs::read_dir(current).map_err(|e| format!("read dir '{}': {e}", current.display()))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("read dir entry in '{}': {e}", current.display()))?;
        let path = entry.path();
        let meta =
            fs::symlink_metadata(&path).map_err(|e| format!("stat '{}': {e}", path.display()))?;

        let rel = path
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "path '{}' not under root '{}'",
                    path.display(),
                    root.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");

        if is_excluded(&rel, excludes, includes) {
            continue;
        }

        if meta.is_file() {
            if let Some(max) = max_size
                && meta.len() > max
            {
                continue;
            }
            if let Some(min) = min_size
                && meta.len() < min
            {
                continue;
            }
        }

        entries.push(FileEntry {
            rel_path: rel,
            full_path: path.clone(),
            meta: meta.clone(),
        });

        if meta.is_dir() {
            scan_tree_inner(root, &path, excludes, includes, max_size, min_size, entries)?;
        }
    }
    Ok(())
}

// ============================================================================
// Itemize flags
// ============================================================================

struct ItemizeFlags {
    file_type: char,
    content_changed: bool,
    size_changed: bool,
    time_changed: bool,
    perms_changed: bool,
    is_new: bool,
}

impl ItemizeFlags {
    fn format(&self) -> String {
        let mut buf = String::with_capacity(11);
        if self.is_new
            || self.content_changed
            || self.size_changed
            || self.time_changed
            || self.perms_changed
        {
            buf.push('>');
        } else {
            buf.push('.');
        }
        buf.push(self.file_type);
        buf.push(if self.content_changed { 'c' } else { '.' });
        buf.push(if self.size_changed { 's' } else { '.' });
        buf.push(if self.time_changed { 't' } else { '.' });
        buf.push(if self.perms_changed { 'p' } else { '.' });
        buf.push_str(".....");
        buf
    }
}

fn compute_itemize(
    _src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    file_type: char,
) -> ItemizeFlags {
    let dst_meta = fs::symlink_metadata(dst_path).ok();
    let is_new = dst_meta.is_none();

    let (content_changed, size_changed, time_changed, perms_changed) =
        if let Some(ref dm) = dst_meta {
            let sc = src_meta.len() != dm.len() || get_mtime(src_meta) != get_mtime(dm);
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
// rsync configuration
// ============================================================================

struct RsyncConfig {
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
    compress: bool,
    max_size: Option<u64>,
    min_size: Option<u64>,
}

enum RsyncParseResult {
    Run(RsyncConfig),
    Help,
    Version,
}

struct RsyncStats {
    files_transferred: u64,
    files_skipped: u64,
    files_deleted: u64,
    bytes_sent: u64,
    bytes_received: u64,
    bytes_total: u64,
    errors: u64,
    dirs_created: u64,
    symlinks_created: u64,
}

impl RsyncStats {
    fn new() -> Self {
        Self {
            files_transferred: 0,
            files_skipped: 0,
            files_deleted: 0,
            bytes_sent: 0,
            bytes_received: 0,
            bytes_total: 0,
            errors: 0,
            dirs_created: 0,
            symlinks_created: 0,
        }
    }
}

// ============================================================================
// rsync argument parsing
// ============================================================================

fn rsync_parse_args(args: &[String]) -> Result<RsyncParseResult, String> {
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
    let mut compress = false;
    let mut max_size: Option<u64> = None;
    let mut min_size: Option<u64> = None;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" {
            return Ok(RsyncParseResult::Help);
        }
        if arg == "--version" {
            return Ok(RsyncParseResult::Version);
        }
        if arg == "--" {
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
                for ch in arg[1..].chars() {
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
                        'z' => compress = true,
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
        return Err("rsync: not enough arguments -- need SOURCE and DEST".into());
    }

    let dest = positional.pop().unwrap_or_default();
    let sources = positional;

    Ok(RsyncParseResult::Run(RsyncConfig {
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
        compress,
        max_size,
        min_size,
    }))
}

fn rsync_print_help() {
    let help = b"\
rsync 0.1.0 -- OurOS file synchronization

Usage: rsync [OPTION]... SOURCE DEST
       rsync [OPTION]... SOURCE... DEST/

Synchronize files and directories locally.

Options:
  -r, --recursive         Recurse into directories
  -a, --archive           Archive mode (equals -rlpt)
  -v, --verbose           Show files being transferred
  -n, --dry-run           Perform a trial run with no changes
  -z                      Compression flag (reserved)
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
Patterns: * matches any chars, ? matches one char
";
    write_stdout(help);
}

// ============================================================================
// File comparison and transfer
// ============================================================================

fn needs_transfer(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    checksum_mode: bool,
    update_mode: bool,
) -> Result<bool, String> {
    let dst_meta = match fs::symlink_metadata(dst_path) {
        Ok(m) => m,
        Err(_) => return Ok(true),
    };

    if update_mode {
        let src_mtime = get_mtime(src_meta);
        let dst_mtime = get_mtime(&dst_meta);
        if dst_mtime > src_mtime {
            return Ok(false);
        }
    }

    if checksum_mode {
        let src_hash = sha256_file(src_path)?;
        let dst_hash = sha256_file(dst_path)?;
        Ok(src_hash != dst_hash)
    } else {
        let size_match = src_meta.len() == dst_meta.len();
        let mtime_match = get_mtime(src_meta) == get_mtime(&dst_meta);
        Ok(!size_match || !mtime_match)
    }
}

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

/// Build a block-signature map for the destination file: each block's rolling
/// checksum maps to its offset. Used for rsync-style delta detection.
fn build_block_signatures(
    path: &Path,
    block_size: usize,
) -> Result<HashMap<u32, Vec<u64>>, String> {
    let mut file = File::open(path).map_err(|e| format!("open '{}': {e}", path.display()))?;
    let mut map: HashMap<u32, Vec<u64>> = HashMap::new();
    let mut buf = vec![0u8; block_size];
    let mut offset: u64 = 0;

    loop {
        let n = read_full(&mut file, &mut buf)
            .map_err(|e| format!("read '{}': {e}", path.display()))?;
        if n == 0 {
            break;
        }
        let cksum = RollingChecksum::from_block(&buf[..n]);
        map.entry(cksum.digest()).or_default().push(offset);
        offset = offset.wrapping_add(n as u64);
    }
    Ok(map)
}

/// Transfer a file using rolling-checksum delta detection. Compares source
/// blocks against destination block signatures and only writes differing blocks.
fn delta_transfer(
    src_path: &Path,
    dst_path: &Path,
    file_size: u64,
    human_readable: bool,
    show_progress: bool,
    stats: &mut RsyncStats,
) -> Result<u64, String> {
    let dst_sigs = build_block_signatures(dst_path, DELTA_BLOCK_SIZE)?;

    let mut src =
        File::open(src_path).map_err(|e| format!("open '{}': {e}", src_path.display()))?;
    let mut dst = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dst_path)
        .map_err(|e| format!("open '{}' for delta: {e}", dst_path.display()))?;

    let mut src_buf = [0u8; DELTA_BLOCK_SIZE];
    let mut dst_buf = [0u8; DELTA_BLOCK_SIZE];
    let mut offset: u64 = 0;
    let mut bytes_written: u64 = 0;
    let mut last_pct: u64 = u64::MAX;

    loop {
        let src_n = read_full(&mut src, &mut src_buf)
            .map_err(|e| format!("read '{}': {e}", src_path.display()))?;
        if src_n == 0 {
            break;
        }

        let src_cksum = RollingChecksum::from_block(&src_buf[..src_n]).digest();

        // Check if this block matches any destination block by checksum.
        let mut matched = false;
        if let Some(offsets) = dst_sigs.get(&src_cksum) {
            for &dst_off in offsets {
                if dst_off == offset {
                    // Potential match at the same offset -- verify byte-by-byte.
                    use std::io::Seek;
                    let _ = dst.seek(std::io::SeekFrom::Start(offset));
                    let dst_n = read_full(&mut dst, &mut dst_buf[..src_n])
                        .map_err(|e| format!("read '{}': {e}", dst_path.display()))?;
                    if dst_n == src_n && dst_buf[..src_n] == src_buf[..src_n] {
                        matched = true;
                        break;
                    }
                }
            }
        }

        if !matched {
            use std::io::Seek;
            dst.seek(std::io::SeekFrom::Start(offset))
                .map_err(|e| format!("seek '{}': {e}", dst_path.display()))?;
            dst.write_all(&src_buf[..src_n])
                .map_err(|e| format!("write '{}': {e}", dst_path.display()))?;
            bytes_written = bytes_written.wrapping_add(src_n as u64);
        }

        offset = offset.wrapping_add(src_n as u64);

        if show_progress && file_size > 0 {
            let pct = offset.saturating_mul(100) / file_size;
            if pct != last_pct {
                last_pct = pct;
                let msg = format!(
                    "\r  {}/{} ({pct}%) delta",
                    format_size(offset, human_readable),
                    format_size(file_size, human_readable),
                );
                write_stderr(msg.as_bytes());
            }
        }
    }

    if show_progress && file_size > 0 {
        write_stderr(b"\n");
    }

    stats.bytes_sent = stats.bytes_sent.wrapping_add(bytes_written);
    Ok(bytes_written)
}

fn full_copy(
    src_path: &Path,
    dst_path: &Path,
    file_size: u64,
    human_readable: bool,
    show_progress: bool,
    stats: &mut RsyncStats,
) -> Result<u64, String> {
    let mut src =
        File::open(src_path).map_err(|e| format!("open '{}': {e}", src_path.display()))?;
    let mut dst =
        File::create(dst_path).map_err(|e| format!("create '{}': {e}", dst_path.display()))?;

    let mut buf = [0u8; COPY_BUF_SIZE];
    let mut written: u64 = 0;
    let mut last_pct: u64 = u64::MAX;

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

        if show_progress && file_size > 0 {
            let pct = written.saturating_mul(100) / file_size;
            if pct != last_pct {
                last_pct = pct;
                let msg = format!(
                    "\r  {}/{} ({pct}%)",
                    format_size(written, human_readable),
                    format_size(file_size, human_readable),
                );
                write_stderr(msg.as_bytes());
            }
        }
    }

    if show_progress && file_size > 0 {
        write_stderr(b"\n");
    }

    stats.bytes_sent = stats.bytes_sent.wrapping_add(written);
    Ok(written)
}

fn transfer_file(
    src_path: &Path,
    dst_path: &Path,
    cfg: &RsyncConfig,
    stats: &mut RsyncStats,
) -> Result<u64, String> {
    let src_meta =
        fs::metadata(src_path).map_err(|e| format!("stat '{}': {e}", src_path.display()))?;
    let file_size = src_meta.len();
    stats.bytes_total = stats.bytes_total.wrapping_add(file_size);

    if cfg.dry_run {
        return Ok(file_size);
    }

    // Try delta transfer if dest exists and has the same size.
    if dst_path.is_file()
        && let Ok(dst_meta) = fs::metadata(dst_path)
        && dst_meta.len() == file_size
        && file_size > 0
    {
        return delta_transfer(
            src_path,
            dst_path,
            file_size,
            cfg.human_readable,
            cfg.progress,
            stats,
        );
    }

    full_copy(
        src_path,
        dst_path,
        file_size,
        cfg.human_readable,
        cfg.progress,
        stats,
    )
}

// ============================================================================
// Symlink handling
// ============================================================================

#[cfg(target_family = "unix")]
fn copy_symlink(src: &Path, dst: &Path, dry_run: bool) -> Result<(), String> {
    let target = fs::read_link(src).map_err(|e| format!("readlink '{}': {e}", src.display()))?;
    if dry_run {
        return Ok(());
    }
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
    if dry_run {
        return Ok(());
    }
    fs::copy(src, dst).map(|_| ()).map_err(|e| {
        format!(
            "copy symlink '{}' -> '{}': {e}",
            src.display(),
            dst.display()
        )
    })
}

// ============================================================================
// rsync synchronization engine
// ============================================================================

fn sync_file_entry(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &Metadata,
    rel_path: &str,
    cfg: &RsyncConfig,
    stats: &mut RsyncStats,
) -> Result<(), String> {
    let transfer = needs_transfer(src_path, dst_path, src_meta, cfg.checksum, cfg.update)?;
    if !transfer {
        stats.files_skipped += 1;
        return Ok(());
    }

    let display_path = if rel_path.is_empty() {
        src_path.display().to_string()
    } else {
        rel_path.to_string()
    };

    if cfg.itemize {
        let flags = compute_itemize(src_path, dst_path, src_meta, 'f');
        let msg = format!("{} {display_path}\n", flags.format());
        write_stdout(msg.as_bytes());
    } else if cfg.verbose {
        let msg = format!("{display_path}\n");
        write_stdout(msg.as_bytes());
    }

    if !cfg.dry_run
        && let Some(parent) = dst_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create dir '{}': {e}", parent.display()))?;
    }

    transfer_file(src_path, dst_path, cfg, stats)?;
    stats.files_transferred += 1;

    if !cfg.dry_run {
        if cfg.preserve_perms
            && let Err(e) = set_file_permissions(dst_path, get_mode(src_meta))
        {
            let msg = format!("rsync: warning: {e}\n");
            write_stderr(msg.as_bytes());
        }
        if cfg.preserve_times
            && let Err(e) = set_file_mtime(dst_path, get_mtime(src_meta))
        {
            let msg = format!("rsync: warning: {e}\n");
            write_stderr(msg.as_bytes());
        }
    }
    Ok(())
}

fn sync_symlink_entry(
    src_path: &Path,
    dst_path: &Path,
    rel_path: &str,
    cfg: &RsyncConfig,
    stats: &mut RsyncStats,
) -> Result<(), String> {
    let display_path = if rel_path.is_empty() {
        src_path.display().to_string()
    } else {
        rel_path.to_string()
    };

    if cfg.itemize {
        let src_meta = fs::symlink_metadata(src_path)
            .map_err(|e| format!("stat '{}': {e}", src_path.display()))?;
        let flags = compute_itemize(src_path, dst_path, &src_meta, 'L');
        let msg = format!("{} {display_path}\n", flags.format());
        write_stdout(msg.as_bytes());
    } else if cfg.verbose {
        let msg = format!("{display_path} -> symlink\n");
        write_stdout(msg.as_bytes());
    }

    copy_symlink(src_path, dst_path, cfg.dry_run)?;
    stats.symlinks_created += 1;
    Ok(())
}

fn delete_extraneous(
    dst_base: &Path,
    src_rel_set: &HashMap<&str, ()>,
    cfg: &RsyncConfig,
    stats: &mut RsyncStats,
) -> Result<(), String> {
    let dst_entries = match scan_tree(
        dst_base,
        &cfg.excludes,
        &cfg.includes,
        cfg.max_size,
        cfg.min_size,
    ) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in dst_entries.iter().rev() {
        if src_rel_set.contains_key(entry.rel_path.as_str()) {
            continue;
        }
        if cfg.verbose || cfg.itemize {
            let prefix = if cfg.itemize {
                "*deleting "
            } else {
                "deleting "
            };
            let msg = format!("{prefix}{}\n", entry.rel_path);
            write_stdout(msg.as_bytes());
        }
        if !cfg.dry_run {
            if entry.meta.is_dir() {
                let _ = fs::remove_dir(&entry.full_path);
            } else {
                fs::remove_file(&entry.full_path)
                    .map_err(|e| format!("delete '{}': {e}", entry.full_path.display()))?;
            }
        }
        stats.files_deleted += 1;
    }
    Ok(())
}

fn sync_one(
    src_path: &Path,
    dst_base: &Path,
    cfg: &RsyncConfig,
    stats: &mut RsyncStats,
) -> Result<(), String> {
    let src_meta = fs::symlink_metadata(src_path)
        .map_err(|e| format!("stat '{}': {e}", src_path.display()))?;

    if src_meta.is_file() {
        return sync_file_entry(src_path, dst_base, &src_meta, "", cfg, stats);
    }

    if src_meta.is_symlink() && cfg.preserve_links {
        return sync_symlink_entry(src_path, dst_base, "", cfg, stats);
    }

    if !src_meta.is_dir() {
        return Err(format!(
            "'{}' is not a file or directory",
            src_path.display()
        ));
    }

    if !cfg.recursive {
        return Err(format!(
            "skipping directory '{}' (use -r for recursive)",
            src_path.display()
        ));
    }

    if !cfg.dry_run {
        fs::create_dir_all(dst_base)
            .map_err(|e| format!("create dir '{}': {e}", dst_base.display()))?;
    }

    let src_entries = scan_tree(
        src_path,
        &cfg.excludes,
        &cfg.includes,
        cfg.max_size,
        cfg.min_size,
    )?;

    let src_rel_set: HashMap<&str, ()> = src_entries
        .iter()
        .map(|e| (e.rel_path.as_str(), ()))
        .collect();

    for entry in &src_entries {
        let dst_entry_path = dst_base.join(&entry.rel_path);

        if entry.meta.is_dir() {
            if !dst_entry_path.is_dir() {
                if cfg.verbose || cfg.itemize {
                    let prefix = if cfg.itemize {
                        let flags =
                            compute_itemize(&entry.full_path, &dst_entry_path, &entry.meta, 'd');
                        format!("{} ", flags.format())
                    } else {
                        String::new()
                    };
                    let msg = format!("{prefix}created directory {}/\n", entry.rel_path);
                    write_stdout(msg.as_bytes());
                }
                if !cfg.dry_run {
                    fs::create_dir_all(&dst_entry_path)
                        .map_err(|e| format!("create dir '{}': {e}", dst_entry_path.display()))?;
                }
                stats.dirs_created += 1;
            }
            if cfg.preserve_perms
                && !cfg.dry_run
                && dst_entry_path.is_dir()
                && let Err(e) = set_file_permissions(&dst_entry_path, get_mode(&entry.meta))
            {
                let msg = format!("rsync: warning: {e}\n");
                write_stderr(msg.as_bytes());
            }
        } else if entry.meta.is_symlink() && cfg.preserve_links {
            sync_symlink_entry(
                &entry.full_path,
                &dst_entry_path,
                &entry.rel_path,
                cfg,
                stats,
            )?;
        } else if entry.meta.is_file() {
            sync_file_entry(
                &entry.full_path,
                &dst_entry_path,
                &entry.meta,
                &entry.rel_path,
                cfg,
                stats,
            )?;
        }
    }

    if cfg.delete {
        delete_extraneous(dst_base, &src_rel_set, cfg, stats)?;
    }
    Ok(())
}

fn rsync_run(args: &[String]) -> i32 {
    let cfg = match rsync_parse_args(args) {
        Ok(RsyncParseResult::Help) => {
            rsync_print_help();
            return 0;
        }
        Ok(RsyncParseResult::Version) => {
            write_stdout(b"rsync ");
            write_stdout(VERSION.as_bytes());
            write_stdout(b"\n");
            return 0;
        }
        Ok(RsyncParseResult::Run(c)) => c,
        Err(e) => {
            let msg = format!("rsync: {e}\n");
            write_stderr(msg.as_bytes());
            return 2;
        }
    };

    if cfg.dry_run && cfg.verbose {
        write_stderr(b"(dry run)\n");
    }
    if cfg.compress && cfg.verbose {
        write_stderr(b"rsync: -z compression flag noted (not yet effective)\n");
    }

    let mut stats = RsyncStats::new();
    let mut had_error = false;

    for source in &cfg.sources {
        let trailing_slash = source.ends_with('/') || source.ends_with('\\');
        let src_path = PathBuf::from(source);

        let dst_path = if cfg.sources.len() == 1
            && !PathBuf::from(&cfg.dest).is_dir()
            && !cfg.dest.ends_with('/')
            && !cfg.dest.ends_with('\\')
        {
            PathBuf::from(&cfg.dest)
        } else if src_path.is_dir() && !trailing_slash {
            let dir_name = match src_path.file_name() {
                Some(n) => n,
                None => {
                    let msg = format!("rsync: cannot determine name for '{source}'\n");
                    write_stderr(msg.as_bytes());
                    had_error = true;
                    continue;
                }
            };
            PathBuf::from(&cfg.dest).join(dir_name)
        } else {
            PathBuf::from(&cfg.dest)
        };

        if let Err(e) = sync_one(&src_path, &dst_path, &cfg, &mut stats) {
            let msg = format!("rsync: {e}\n");
            write_stderr(msg.as_bytes());
            stats.errors += 1;
            had_error = true;
        }
    }

    // Stats summary.
    if cfg.stats {
        // Simulate bytes_received as the file-list metadata overhead.
        stats.bytes_received = stats
            .files_transferred
            .saturating_mul(64)
            .saturating_add(stats.dirs_created.saturating_mul(32));

        let total = stats.bytes_total;
        let sent = stats.bytes_sent;
        let speedup = if sent > 0 {
            total as f64 / sent as f64
        } else {
            1.0
        };

        write_stderr(b"\n");
        let msg = format!(
            "Number of files: {} (transferred: {})\n\
             Number of created directories: {}\n\
             Number of deleted files: {}\n\
             Total file size: {} bytes\n\
             Total transferred size: {} bytes\n\
             Bytes sent: {}\n\
             Bytes received: {}\n\
             Speedup: {speedup:.2}\n",
            stats.files_transferred + stats.files_skipped,
            stats.files_transferred,
            stats.dirs_created,
            stats.files_deleted,
            format_size(total, cfg.human_readable),
            format_size(sent, cfg.human_readable),
            format_size(sent, cfg.human_readable),
            format_size(stats.bytes_received, cfg.human_readable),
        );
        write_stderr(msg.as_bytes());
        if stats.errors > 0 {
            let msg = format!("Errors: {}\n", stats.errors);
            write_stderr(msg.as_bytes());
        }
    }

    if had_error { 1 } else { 0 }
}

// ============================================================================
// SCP mode
// ============================================================================

#[derive(Clone)]
enum ScpLocation {
    Local(String),
    Remote {
        user: Option<String>,
        host: String,
        path: String,
    },
}

impl ScpLocation {
    fn parse(s: &str) -> Self {
        if let Some(colon_pos) = s.find(':') {
            let is_drive_letter = colon_pos == 1
                && s.as_bytes()
                    .first()
                    .is_some_and(|b| b.is_ascii_alphabetic());
            if !is_drive_letter && colon_pos > 0 {
                let before_colon = &s[..colon_pos];
                if !before_colon.contains('/') && !before_colon.contains('\\') {
                    let host_part = before_colon;
                    let path_part = &s[colon_pos + 1..];
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
                                    ".".into()
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
                                ".".into()
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

    fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

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

struct ScpConfig {
    sources: Vec<ScpLocation>,
    target: ScpLocation,
    recursive: bool,
    preserve: bool,
    verbose: bool,
}

enum ScpParseResult {
    Run(ScpConfig),
    Help,
    Version,
}

fn scp_parse_args(args: &[String]) -> Result<ScpParseResult, String> {
    let mut recursive = false;
    let mut preserve = false;
    let mut verbose = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Ok(ScpParseResult::Help);
        }
        if arg == "--version" {
            return Ok(ScpParseResult::Version);
        }
        if arg == "--" {
            for a in &args[i + 1..] {
                positional.push(a.clone());
            }
            break;
        }
        if arg.starts_with("--") {
            return Err(format!("scp: unknown option: {arg}"));
        }
        if arg.starts_with('-') && arg.len() > 1 {
            for ch in arg[1..].chars() {
                match ch {
                    'r' => recursive = true,
                    'p' => preserve = true,
                    'v' => verbose = true,
                    _ => return Err(format!("scp: unknown option: -{ch}")),
                }
            }
            i += 1;
            continue;
        }
        positional.push(arg.clone());
        i += 1;
    }

    if positional.len() < 2 {
        return Err("scp: need at least a source and a target".into());
    }

    let target_str = positional.pop().unwrap_or_default();
    let target = ScpLocation::parse(&target_str);
    let sources: Vec<ScpLocation> = positional.iter().map(|s| ScpLocation::parse(s)).collect();

    if sources.len() > 1
        && let ScpLocation::Local(ref p) = target
    {
        let path = Path::new(p);
        if !path.is_dir() && !p.ends_with('/') && !p.ends_with('\\') {
            return Err("scp: target must be a directory when copying multiple sources".into());
        }
    }

    Ok(ScpParseResult::Run(ScpConfig {
        sources,
        target,
        recursive,
        preserve,
        verbose,
    }))
}

fn scp_print_help() {
    let help = b"\
scp 0.1.0 -- OurOS secure file copy

Usage: scp [OPTION]... SOURCE... TARGET

Copy files and directories between hosts.

Options:
  -r              Recursively copy directories
  -p              Preserve timestamps and permissions
  -v              Verbose mode
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
    write_stdout(help);
}

struct ScpStats {
    files: u64,
    directories: u64,
    bytes: u64,
    errors: u64,
}

impl ScpStats {
    fn new() -> Self {
        Self {
            files: 0,
            directories: 0,
            bytes: 0,
            errors: 0,
        }
    }
}

/// Progress display for SCP transfers.
struct ScpProgress {
    file_name: String,
    total_bytes: u64,
    transferred: u64,
    start_ns: u64,
    last_print_ns: u64,
}

impl ScpProgress {
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

    fn update(&mut self, additional: u64) {
        self.transferred = self.transferred.saturating_add(additional);
        let now = clock_ns();
        if now.saturating_sub(self.last_print_ns) < 200_000_000
            && self.transferred < self.total_bytes
        {
            return;
        }
        self.last_print_ns = now;
        self.print_line(now);
    }

    fn finish(&mut self) {
        self.transferred = self.total_bytes;
        self.print_line(clock_ns());
        write_stderr(b"\n");
    }

    fn print_line(&self, now: u64) {
        let pct = self
            .transferred
            .saturating_mul(100)
            .checked_div(self.total_bytes)
            .unwrap_or(100);
        let elapsed_sec = now.saturating_sub(self.start_ns) / 1_000_000_000;
        let speed = self
            .transferred
            .checked_div(elapsed_sec)
            .unwrap_or(self.transferred);
        let eta = if speed > 0 && self.transferred < self.total_bytes {
            (self.total_bytes.saturating_sub(self.transferred)) / speed
        } else {
            0
        };
        let display_name = if self.file_name.len() > 24 {
            let start = self.file_name.len() - 21;
            format!("...{}", &self.file_name[start..])
        } else {
            self.file_name.clone()
        };
        let eta_str = if self.transferred >= self.total_bytes {
            "done".into()
        } else {
            format!("ETA {}", format_eta(eta))
        };
        let msg = format!(
            "\r{display_name:<24} {pct:>3}% {:>8} {:>9} {eta_str}   ",
            format_size(self.transferred, true),
            format_speed(speed),
        );
        write_stderr(msg.as_bytes());
    }
}

fn scp_copy_file(
    src: &Path,
    dst: &Path,
    cfg: &ScpConfig,
    stats: &mut ScpStats,
) -> Result<(), String> {
    let src_meta = fs::metadata(src).map_err(|e| format!("scp: {}: {e}", src.display()))?;
    if src_meta.is_dir() {
        return Err(format!("scp: {}: is a directory (use -r)", src.display()));
    }
    let file_size = src_meta.len();

    if cfg.verbose {
        let msg = format!("{} -> {}\n", src.display(), dst.display());
        write_stderr(msg.as_bytes());
    }

    if let Some(parent) = dst.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("scp: cannot create {}: {e}", parent.display()))?;
    }

    let mut reader = File::open(src).map_err(|e| format!("scp: {}: {e}", src.display()))?;
    let mut writer = File::create(dst).map_err(|e| format!("scp: {}: {e}", dst.display()))?;

    let mut buf = [0u8; COPY_BUF_SIZE];
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| src.display().to_string());
    let mut progress = ScpProgress::new(&file_name, file_size);

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("scp: read {}: {e}", src.display()))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("scp: write {}: {e}", dst.display()))?;
        if cfg.verbose {
            progress.update(n as u64);
        }
    }

    if cfg.verbose {
        progress.finish();
    }

    if cfg.preserve {
        // Preserve permissions.
        let readonly = src_meta.permissions().readonly();
        if let Ok(dm) = fs::metadata(dst) {
            let mut perms = dm.permissions();
            perms.set_readonly(readonly);
            let _ = fs::set_permissions(dst, perms);
        }
        // Preserve mtime (best-effort).
        let _ = set_file_mtime(dst, get_mtime(&src_meta));
    }

    stats.files = stats.files.saturating_add(1);
    stats.bytes = stats.bytes.saturating_add(file_size);
    Ok(())
}

fn scp_copy_directory(
    src: &Path,
    dst: &Path,
    cfg: &ScpConfig,
    stats: &mut ScpStats,
) -> Result<(), String> {
    if !cfg.recursive {
        return Err(format!("scp: {}: is a directory (use -r)", src.display()));
    }
    if cfg.verbose {
        let msg = format!("d {}\n", dst.display());
        write_stderr(msg.as_bytes());
    }
    fs::create_dir_all(dst).map_err(|e| format!("scp: cannot create {}: {e}", dst.display()))?;
    stats.directories = stats.directories.saturating_add(1);

    let mut entries: Vec<_> = fs::read_dir(src)
        .map_err(|e| format!("scp: cannot read {}: {e}", src.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let entry_path = entry.path();
        let dst_child = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .map_err(|e| format!("scp: {}: {e}", entry_path.display()))?;

        if ft.is_dir() {
            scp_copy_directory(&entry_path, &dst_child, cfg, stats)?;
        } else if ft.is_file()
            && let Err(e) = scp_copy_file(&entry_path, &dst_child, cfg, stats)
        {
            let msg = format!("{e}\n");
            write_stderr(msg.as_bytes());
            stats.errors = stats.errors.saturating_add(1);
        }
    }
    Ok(())
}

fn scp_run(args: &[String]) -> i32 {
    let cfg = match scp_parse_args(args) {
        Ok(ScpParseResult::Help) => {
            scp_print_help();
            return 0;
        }
        Ok(ScpParseResult::Version) => {
            write_stdout(b"scp ");
            write_stdout(VERSION.as_bytes());
            write_stdout(b" (OurOS)\n");
            return 0;
        }
        Ok(ScpParseResult::Run(c)) => c,
        Err(e) => {
            let msg = format!("{e}\nTry 'scp --help' for more information.\n");
            write_stderr(msg.as_bytes());
            return 2;
        }
    };

    let any_remote = cfg.sources.iter().any(|s| s.is_remote()) || cfg.target.is_remote();
    if any_remote {
        // Find the first remote location for the error message.
        let remote_loc = cfg
            .sources
            .iter()
            .find(|s| s.is_remote())
            .unwrap_or(&cfg.target);
        let msg = format!(
            "scp: remote transfer not yet supported: {}\n",
            remote_loc.display()
        );
        write_stderr(msg.as_bytes());
        return 1;
    }

    let mut stats = ScpStats::new();

    let target_str = match &cfg.target {
        ScpLocation::Local(p) => p.clone(),
        ScpLocation::Remote { .. } => unreachable!(),
    };
    let target_path = PathBuf::from(&target_str);
    let target_is_dir = target_path.is_dir()
        || target_str.ends_with('/')
        || target_str.ends_with('\\')
        || cfg.sources.len() > 1;

    if target_is_dir
        && !target_path.exists()
        && let Err(e) = fs::create_dir_all(&target_path)
    {
        let msg = format!("scp: cannot create {}: {e}\n", target_path.display());
        write_stderr(msg.as_bytes());
        return 1;
    }

    for src in &cfg.sources {
        let src_str = match src {
            ScpLocation::Local(p) => p.clone(),
            ScpLocation::Remote { .. } => unreachable!(),
        };
        let src_path = PathBuf::from(&src_str);

        if !src_path.exists() {
            let msg = format!("scp: {}: No such file or directory\n", src_path.display());
            write_stderr(msg.as_bytes());
            stats.errors = stats.errors.saturating_add(1);
            continue;
        }

        let dst_path = if target_is_dir {
            let name = src_path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_else(|| src_path.as_os_str().to_os_string());
            target_path.join(name)
        } else {
            target_path.clone()
        };

        let src_meta = match fs::symlink_metadata(&src_path) {
            Ok(m) => m,
            Err(e) => {
                let msg = format!("scp: {}: {e}\n", src_path.display());
                write_stderr(msg.as_bytes());
                stats.errors = stats.errors.saturating_add(1);
                continue;
            }
        };

        let result = if src_meta.is_dir() {
            scp_copy_directory(&src_path, &dst_path, &cfg, &mut stats)
        } else {
            scp_copy_file(&src_path, &dst_path, &cfg, &mut stats)
        };

        if let Err(e) = result {
            let msg = format!("{e}\n");
            write_stderr(msg.as_bytes());
            stats.errors = stats.errors.saturating_add(1);
        }
    }

    if cfg.verbose {
        let msg = format!(
            "Transferred {} file(s), {} dir(s), {}\n",
            stats.files,
            stats.directories,
            format_size(stats.bytes, true),
        );
        write_stderr(msg.as_bytes());
        if stats.errors > 0 {
            let msg = format!("{} error(s)\n", stats.errors);
            write_stderr(msg.as_bytes());
        }
    }

    if stats.errors > 0 { 1 } else { 0 }
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    // Detect personality from argv[0].
    let personality = if let Some(arg0) = args.first() {
        detect_personality(arg0.as_bytes())
    } else {
        Personality::Rsync
    };

    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Rsync => rsync_run(&cmd_args),
        Personality::Scp => scp_run(&cmd_args),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // -- Personality detection ------------------------------------------------

    #[test]
    fn personality_rsync_plain() {
        assert_eq!(detect_personality(b"rsync"), Personality::Rsync);
    }

    #[test]
    fn personality_rsync_with_path() {
        assert_eq!(detect_personality(b"/usr/bin/rsync"), Personality::Rsync);
    }

    #[test]
    fn personality_scp_plain() {
        assert_eq!(detect_personality(b"scp"), Personality::Scp);
    }

    #[test]
    fn personality_scp_with_path() {
        assert_eq!(detect_personality(b"/usr/bin/scp"), Personality::Scp);
    }

    #[test]
    fn personality_scp_exe() {
        assert_eq!(detect_personality(b"C:\\bin\\scp.exe"), Personality::Scp);
    }

    #[test]
    fn personality_rsync_exe() {
        assert_eq!(
            detect_personality(b"C:\\bin\\rsync.EXE"),
            Personality::Rsync
        );
    }

    #[test]
    fn personality_unknown_defaults_rsync() {
        assert_eq!(detect_personality(b"foobar"), Personality::Rsync);
    }

    // -- Rolling checksum ----------------------------------------------------

    #[test]
    fn rolling_checksum_empty() {
        let ck = RollingChecksum::new();
        assert_eq!(ck.digest(), (0 << 16) | 1);
    }

    #[test]
    fn rolling_checksum_from_block() {
        let data = b"hello world";
        let ck = RollingChecksum::from_block(data);
        assert_ne!(ck.digest(), 0);
    }

    #[test]
    fn rolling_checksum_deterministic() {
        let data = b"test data for checksum";
        let ck1 = RollingChecksum::from_block(data);
        let ck2 = RollingChecksum::from_block(data);
        assert_eq!(ck1.digest(), ck2.digest());
    }

    #[test]
    fn rolling_checksum_different_data() {
        let ck1 = RollingChecksum::from_block(b"aaaa");
        let ck2 = RollingChecksum::from_block(b"bbbb");
        assert_ne!(ck1.digest(), ck2.digest());
    }

    #[test]
    fn rolling_checksum_roll() {
        let mut ck = RollingChecksum::from_block(b"abcd");
        ck.roll(b'a', b'e');
        // After rolling, the checksum should change.
        let fresh = RollingChecksum::from_block(b"abcd");
        assert_ne!(ck.digest(), fresh.digest());
    }

    #[test]
    fn rolling_checksum_reset_with() {
        let mut ck = RollingChecksum::from_block(b"old data");
        ck.reset_with(b"new data");
        let fresh = RollingChecksum::from_block(b"new data");
        assert_eq!(ck.digest(), fresh.digest());
    }

    // -- SHA-256 -------------------------------------------------------------

    #[test]
    fn sha256_empty() {
        let hash = Sha256::new().finalize();
        assert_eq!(hash[0], 0xe3);
        assert_eq!(hash[1], 0xb0);
        assert_eq!(hash[31], 0x55);
    }

    #[test]
    fn sha256_abc() {
        let mut h = Sha256::new();
        h.update(b"abc");
        let hash = h.finalize();
        assert_eq!(hash[0], 0xba);
        assert_eq!(hash[1], 0x78);
        assert_eq!(hash[31], 0xad);
    }

    #[test]
    fn sha256_longer_than_block() {
        let mut h = Sha256::new();
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        h.update(data);
        let hash = h.finalize();
        assert_eq!(hash[0], 0x24);
        assert_eq!(hash[1], 0x8d);
        assert_eq!(hash[31], 0xc1);
    }

    #[test]
    fn sha256_incremental() {
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

    #[test]
    fn sha256_two_blocks() {
        let mut h = Sha256::new();
        h.update(&[0x41; 128]); // Two full 64-byte blocks.
        let hash = h.finalize();
        // Just verify it produces a 32-byte hash without panic.
        assert_eq!(hash.len(), 32);
    }

    // -- parse_size ----------------------------------------------------------

    #[test]
    fn parse_size_plain() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
    }

    #[test]
    fn parse_size_kilo() {
        assert_eq!(parse_size("4K").unwrap(), 4096);
        assert_eq!(parse_size("4k").unwrap(), 4096);
    }

    #[test]
    fn parse_size_mega() {
        assert_eq!(parse_size("2M").unwrap(), 2 * 1024 * 1024);
    }

    #[test]
    fn parse_size_giga() {
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_tera() {
        assert_eq!(parse_size("1T").unwrap(), 1024u64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_empty() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn parse_size_invalid() {
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn parse_size_zero() {
        assert_eq!(parse_size("0").unwrap(), 0);
    }

    // -- format_size ---------------------------------------------------------

    #[test]
    fn format_size_not_human() {
        assert_eq!(format_size(12345, false), "12345");
    }

    #[test]
    fn format_size_human_bytes() {
        assert_eq!(format_size(500, true), "500B");
    }

    #[test]
    fn format_size_human_kib() {
        let s = format_size(2048, true);
        assert!(s.contains("KiB"), "got: {s}");
    }

    #[test]
    fn format_size_human_mib() {
        let s = format_size(5 * 1024 * 1024, true);
        assert!(s.contains("MiB"), "got: {s}");
    }

    #[test]
    fn format_size_human_zero() {
        assert_eq!(format_size(0, true), "0.00B");
    }

    #[test]
    fn format_speed_zero() {
        assert_eq!(format_speed(0), "0B/s");
    }

    #[test]
    fn format_speed_kib() {
        let s = format_speed(1024);
        assert!(s.contains("/s"), "got: {s}");
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

    #[test]
    fn format_eta_zero() {
        assert_eq!(format_eta(0), "00:00");
    }

    // -- glob_match ----------------------------------------------------------

    #[test]
    fn glob_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn glob_star() {
        assert!(glob_match("*.txt", "foo.txt"));
        assert!(!glob_match("*.txt", "foo.rs"));
        assert!(glob_match("foo*", "foobar"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_question() {
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
    }

    #[test]
    fn glob_combined() {
        assert!(glob_match("test_*.rs", "test_foo.rs"));
        assert!(!glob_match("test_*.rs", "test_foo.txt"));
    }

    #[test]
    fn glob_empty_pattern() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn glob_star_star() {
        assert!(glob_match("**", "anything/at/all"));
    }

    // -- is_excluded ---------------------------------------------------------

    #[test]
    fn excluded_basic() {
        let exc = vec!["*.tmp".to_string()];
        let inc: Vec<String> = vec![];
        assert!(is_excluded("foo.tmp", &exc, &inc));
        assert!(!is_excluded("foo.txt", &exc, &inc));
    }

    #[test]
    fn include_overrides_exclude() {
        let exc = vec!["*.log".to_string()];
        let inc = vec!["important.log".to_string()];
        assert!(!is_excluded("important.log", &exc, &inc));
        assert!(is_excluded("debug.log", &exc, &inc));
    }

    #[test]
    fn no_patterns_not_excluded() {
        let exc: Vec<String> = vec![];
        let inc: Vec<String> = vec![];
        assert!(!is_excluded("anything.txt", &exc, &inc));
    }

    // -- ItemizeFlags --------------------------------------------------------

    #[test]
    fn itemize_new_file() {
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
    fn itemize_unchanged() {
        let flags = ItemizeFlags {
            file_type: 'f',
            content_changed: false,
            size_changed: false,
            time_changed: false,
            perms_changed: false,
            is_new: false,
        };
        let s = flags.format();
        // rsync's itemize string (%i) is exactly 11 chars: update-type, file
        // type, then c s t p o g u a x.  Here: '.' (no change), 'f', then 4
        // change flags (c/s/t/p) and 5 trailing dots = ".f.........".
        assert_eq!(&s, ".f.........");
        assert_eq!(s.len(), 11);
    }

    #[test]
    fn itemize_directory() {
        let flags = ItemizeFlags {
            file_type: 'd',
            content_changed: false,
            size_changed: false,
            time_changed: true,
            perms_changed: false,
            is_new: false,
        };
        let s = flags.format();
        assert!(s.starts_with(">d"));
        assert!(s.contains('t'));
    }

    #[test]
    fn itemize_symlink() {
        let flags = ItemizeFlags {
            file_type: 'L',
            content_changed: true,
            size_changed: false,
            time_changed: false,
            perms_changed: false,
            is_new: true,
        };
        let s = flags.format();
        assert!(s.starts_with(">L"));
    }

    // -- read_full -----------------------------------------------------------

    #[test]
    fn read_full_exact() {
        let data = b"hello world!";
        let mut cursor = Cursor::new(data);
        let mut buf = [0u8; 5];
        let n = read_full(&mut cursor, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn read_full_short() {
        let data = b"hi";
        let mut cursor = Cursor::new(data);
        let mut buf = [0u8; 10];
        let n = read_full(&mut cursor, &mut buf).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], b"hi");
    }

    #[test]
    fn read_full_empty() {
        let data = b"";
        let mut cursor = Cursor::new(data);
        let mut buf = [0u8; 10];
        let n = read_full(&mut cursor, &mut buf).unwrap();
        assert_eq!(n, 0);
    }

    // -- rsync arg parsing ---------------------------------------------------

    #[test]
    fn rsync_args_basic() {
        let args: Vec<String> = vec!["src".into(), "dst".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert_eq!(c.sources, vec!["src"]);
                assert_eq!(c.dest, "dst");
                assert!(!c.recursive);
                assert!(!c.verbose);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_archive() {
        let args: Vec<String> = vec!["-a".into(), "src/".into(), "dst/".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert!(c.recursive);
                assert!(c.preserve_links);
                assert!(c.preserve_perms);
                assert!(c.preserve_times);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_combined_short() {
        let args: Vec<String> = vec!["-rvn".into(), "a".into(), "b".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert!(c.recursive);
                assert!(c.verbose);
                assert!(c.dry_run);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_compress_flag() {
        let args: Vec<String> = vec!["-z".into(), "a".into(), "b".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => assert!(c.compress),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_exclude_include() {
        let args: Vec<String> = vec![
            "--exclude=*.o".into(),
            "--include=keep.o".into(),
            "s".into(),
            "d".into(),
        ];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert_eq!(c.excludes, vec!["*.o"]);
                assert_eq!(c.includes, vec!["keep.o"]);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_help() {
        let args: Vec<String> = vec!["--help".into()];
        assert!(matches!(
            rsync_parse_args(&args).unwrap(),
            RsyncParseResult::Help
        ));
    }

    #[test]
    fn rsync_args_version() {
        let args: Vec<String> = vec!["--version".into()];
        assert!(matches!(
            rsync_parse_args(&args).unwrap(),
            RsyncParseResult::Version
        ));
    }

    #[test]
    fn rsync_args_not_enough() {
        let args: Vec<String> = vec!["only_one".into()];
        assert!(rsync_parse_args(&args).is_err());
    }

    #[test]
    fn rsync_args_unknown_long() {
        let args: Vec<String> = vec!["--bogus".into(), "a".into(), "b".into()];
        assert!(rsync_parse_args(&args).is_err());
    }

    #[test]
    fn rsync_args_unknown_short() {
        let args: Vec<String> = vec!["-x".into(), "a".into(), "b".into()];
        assert!(rsync_parse_args(&args).is_err());
    }

    #[test]
    fn rsync_args_max_min_size() {
        let args: Vec<String> = vec![
            "--max-size=10M".into(),
            "--min-size=1K".into(),
            "a".into(),
            "b".into(),
        ];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert_eq!(c.max_size, Some(10 * 1024 * 1024));
                assert_eq!(c.min_size, Some(1024));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_double_dash() {
        let args: Vec<String> = vec!["--".into(), "-weird".into(), "dst".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => {
                assert_eq!(c.sources, vec!["-weird"]);
                assert_eq!(c.dest, "dst");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_delete_flag() {
        let args: Vec<String> = vec!["--delete".into(), "a".into(), "b".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => assert!(c.delete),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_progress_flag() {
        let args: Vec<String> = vec!["--progress".into(), "a".into(), "b".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => assert!(c.progress),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn rsync_args_stats_flag() {
        let args: Vec<String> = vec!["--stats".into(), "a".into(), "b".into()];
        match rsync_parse_args(&args).unwrap() {
            RsyncParseResult::Run(c) => assert!(c.stats),
            _ => panic!("expected Run"),
        }
    }

    // -- SCP Location parsing ------------------------------------------------

    #[test]
    fn scp_loc_local_simple() {
        let loc = ScpLocation::parse("/tmp/file.txt");
        assert!(matches!(loc, ScpLocation::Local(ref p) if p == "/tmp/file.txt"));
    }

    #[test]
    fn scp_loc_local_relative() {
        let loc = ScpLocation::parse("./foo/bar");
        assert!(matches!(loc, ScpLocation::Local(ref p) if p == "./foo/bar"));
    }

    #[test]
    fn scp_loc_remote_with_user() {
        let loc = ScpLocation::parse("alice@myhost:/data/file");
        match loc {
            ScpLocation::Remote { user, host, path } => {
                assert_eq!(user.as_deref(), Some("alice"));
                assert_eq!(host, "myhost");
                assert_eq!(path, "/data/file");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn scp_loc_remote_without_user() {
        let loc = ScpLocation::parse("myhost:/data/file");
        match loc {
            ScpLocation::Remote { user, host, path } => {
                assert!(user.is_none());
                assert_eq!(host, "myhost");
                assert_eq!(path, "/data/file");
            }
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn scp_loc_remote_empty_path() {
        let loc = ScpLocation::parse("user@host:");
        match loc {
            ScpLocation::Remote { path, .. } => assert_eq!(path, "."),
            _ => panic!("expected Remote"),
        }
    }

    #[test]
    fn scp_loc_slash_before_colon() {
        let loc = ScpLocation::parse("/some/path:with:colons");
        assert!(matches!(loc, ScpLocation::Local(_)));
    }

    #[test]
    fn scp_loc_windows_drive() {
        let loc = ScpLocation::parse("C:\\Users\\file");
        assert!(matches!(loc, ScpLocation::Local(_)));
    }

    #[test]
    fn scp_loc_display_local() {
        let loc = ScpLocation::Local("/tmp/f".into());
        assert_eq!(loc.display(), "/tmp/f");
    }

    #[test]
    fn scp_loc_display_remote_user() {
        let loc = ScpLocation::Remote {
            user: Some("bob".into()),
            host: "srv".into(),
            path: "/data".into(),
        };
        assert_eq!(loc.display(), "bob@srv:/data");
    }

    #[test]
    fn scp_loc_display_remote_no_user() {
        let loc = ScpLocation::Remote {
            user: None,
            host: "srv".into(),
            path: "/data".into(),
        };
        assert_eq!(loc.display(), "srv:/data");
    }

    #[test]
    fn scp_loc_is_remote() {
        let loc = ScpLocation::parse("host:/path");
        assert!(loc.is_remote());
    }

    #[test]
    fn scp_loc_is_not_remote() {
        let loc = ScpLocation::parse("/local/path");
        assert!(!loc.is_remote());
    }

    // -- SCP arg parsing -----------------------------------------------------

    #[test]
    fn scp_args_basic() {
        let args: Vec<String> = vec!["file1".into(), "file2".into()];
        assert!(matches!(scp_parse_args(&args), Ok(ScpParseResult::Run(_))));
    }

    #[test]
    fn scp_args_recursive() {
        let args: Vec<String> = vec!["-r".into(), "dir1".into(), "dir2".into()];
        match scp_parse_args(&args) {
            Ok(ScpParseResult::Run(c)) => assert!(c.recursive),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn scp_args_preserve_verbose() {
        let args: Vec<String> = vec!["-pv".into(), "a".into(), "b".into()];
        match scp_parse_args(&args) {
            Ok(ScpParseResult::Run(c)) => {
                assert!(c.preserve);
                assert!(c.verbose);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn scp_args_help() {
        let args: Vec<String> = vec!["--help".into()];
        assert!(matches!(scp_parse_args(&args), Ok(ScpParseResult::Help)));
    }

    #[test]
    fn scp_args_version() {
        let args: Vec<String> = vec!["--version".into()];
        assert!(matches!(scp_parse_args(&args), Ok(ScpParseResult::Version)));
    }

    #[test]
    fn scp_args_too_few() {
        let args: Vec<String> = vec!["only_one".into()];
        assert!(scp_parse_args(&args).is_err());
    }

    #[test]
    fn scp_args_unknown_flag() {
        let args: Vec<String> = vec!["-z".into(), "a".into(), "b".into()];
        assert!(scp_parse_args(&args).is_err());
    }

    #[test]
    fn scp_args_double_dash() {
        let args: Vec<String> = vec!["--".into(), "-weird".into(), "dst".into()];
        match scp_parse_args(&args) {
            Ok(ScpParseResult::Run(c)) => {
                if let ScpLocation::Local(ref p) = c.sources[0] {
                    assert_eq!(p, "-weird");
                } else {
                    panic!("expected local");
                }
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn scp_args_remote_target() {
        let args: Vec<String> = vec!["file".into(), "user@host:/tmp/".into()];
        match scp_parse_args(&args) {
            Ok(ScpParseResult::Run(c)) => assert!(c.target.is_remote()),
            _ => panic!("expected Run"),
        }
    }
}
