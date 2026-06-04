//! OurOS Multi-Personality File Operations Utility
//!
//! A single binary that determines its behavior based on `argv[0]`:
//!
//! - **stat**: display detailed file status information
//! - **touch**: update file access/modification times (create if missing)
//! - **ln**: create hard or symbolic links
//! - **readlink**: print symlink target (optionally canonicalized)
//! - **realpath**: print resolved absolute pathname
//! - **mkfifo**: create named pipes (FIFOs)
//!
//! Symlink or hardlink this binary under the desired name to activate the
//! corresponding personality.
//!
//! # stat usage
//!
//! ```text
//! stat [OPTIONS] FILE...
//!   -f, --file-system         Display filesystem status
//!   -L, --dereference         Follow symlinks
//!   -c FORMAT, --format=FMT   Custom format string
//!   -t, --terse               Terse output
//! ```
//!
//! # touch usage
//!
//! ```text
//! touch [OPTIONS] FILE...
//!   -a                   Change access time only
//!   -m                   Change modification time only
//!   -c, --no-create      Don't create new files
//!   -d DATE, --date=DATE Parse date string
//!   -t STAMP             [[CC]YY]MMDDhhmm[.ss]
//!   -r FILE, --reference=FILE  Use FILE's timestamps
//! ```
//!
//! # ln usage
//!
//! ```text
//! ln [OPTIONS] TARGET LINK_NAME
//!   -s, --symbolic             Create symbolic links
//!   -f, --force                Remove existing destination
//!   -n, --no-dereference       Treat LINK_NAME as normal file
//!   -v, --verbose              Print each link created
//!   -b, --backup               Backup existing destination
//!   -t DIR, --target-directory=DIR
//!   -T, --no-target-directory
//! ```
//!
//! # readlink usage
//!
//! ```text
//! readlink [OPTIONS] FILE...
//!   -f, --canonicalize          Canonicalize (follow all symlinks)
//!   -e, --canonicalize-existing All components must exist
//!   -m, --canonicalize-missing  Components need not exist
//!   -n, --no-newline            No trailing newline
//!   -z, --zero                  NUL delimiter
//! ```
//!
//! # realpath usage
//!
//! ```text
//! realpath [OPTIONS] FILE...
//!   -e, --canonicalize-existing All components must exist
//!   -m, --canonicalize-missing  No component needs to exist
//!   -s, --strip, --no-symlinks  Don't resolve symlinks
//!   --relative-to=DIR           Print relative to DIR
//!   --relative-base=DIR         Print relative only if below DIR
//! ```
//!
//! # mkfifo usage
//!
//! ```text
//! mkfifo [OPTIONS] NAME...
//!   -m MODE, --mode=MODE  Set permission mode (default 0666)
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// libc bindings (OurOS posix layer)
// ============================================================================
//
// std::fs covers stat/lstat/readlink/link/symlink/unlink/rename, all of which
// route through the posix libc layer to native OurOS syscalls.  The three
// operations std does not expose — statvfs, mkfifo, and utimensat — are called
// directly through their C ABI symbols, which the posix crate implements as
// `extern "C"` functions backed by native syscalls.
//
// The libc-backed paths require the unix platform abstractions
// (`std::os::unix`, `extern "C"` symbols from the posix sysroot), so they are
// gated to `#[cfg(unix)]`.  This matches the real target (x86_64-ouros, which
// is `target-family = unix`); host unit tests run on a non-unix toolchain and
// exercise only the portable argument-parsing/formatting logic, so the gated
// functions get inert stubs there.

/// `AT_FDCWD` for the `dirfd` argument of `utimensat` (resolve relative to cwd).
#[cfg(unix)]
const AT_FDCWD: i32 = -100;

/// Filesystem statistics, matching the posix-crate `struct statvfs` layout
/// (11 `u64` fields).  Populated by the [`statvfs`] binding.
#[cfg(unix)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PosixStatvfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: u64,
    f_flag: u64,
    f_namemax: u64,
}

// SAFETY: These symbols are provided by the posix crate (linked via the
// sysroot's libstubs) with exactly these C signatures.  Each returns 0 on
// success and -1 on error with `errno` set, which we surface via
// `std::io::Error::last_os_error()`.
#[cfg(unix)]
unsafe extern "C" {
    /// Query filesystem statistics for `path` (NUL-terminated C string).
    fn statvfs(path: *const u8, buf: *mut PosixStatvfs) -> i32;
    /// Create a FIFO at `pathname` (NUL-terminated C string) with `mode`.
    fn mkfifo(pathname: *const u8, mode: u32) -> i32;
    /// Set access/modification times of `path` with nanosecond precision.
    fn utimensat(dirfd: i32, path: *const u8, times: *const Timespec, flags: i32) -> i32;
}

/// Build a NUL-terminated C string from a path, mapping interior-NUL errors
/// to the same `String` error type the `do_*` wrappers return.
#[cfg(unix)]
fn cstr(path: &str) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(path).map_err(|_| format!("invalid path (contains NUL): {path}"))
}

/// Convert a possibly-negative timestamp component to `u64`, clamping
/// pre-epoch values to 0.  Avoids clippy's `cast_sign_loss`.
#[cfg(unix)]
fn nonneg_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

// ============================================================================
// Kernel data structures
// ============================================================================

/// Matches the OurOS kernel stat buffer layout (128 bytes).
/// All fields are little-endian u64 for simplicity on x86-64.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KernelStat {
    st_dev: u64,
    st_ino: u64,
    st_mode: u64,
    st_nlink: u64,
    st_uid: u64,
    st_gid: u64,
    st_rdev: u64,
    st_size: u64,
    st_blksize: u64,
    st_blocks: u64,
    st_atime_sec: u64,
    st_atime_nsec: u64,
    st_mtime_sec: u64,
    st_mtime_nsec: u64,
    st_ctime_sec: u64,
    st_ctime_nsec: u64,
}

/// Matches the OurOS kernel statfs buffer layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KernelStatFs {
    f_type: u64,
    f_bsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_fsid: u64,
    f_namelen: u64,
    f_frsize: u64,
    f_flags: u64,
    _spare: [u64; 4],
}

/// Timespec for utimensat.
#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

// ============================================================================
// Personality detection
// ============================================================================

/// The six modes this binary can operate in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Stat,
    Touch,
    Ln,
    Readlink,
    Realpath,
    Mkfifo,
}

/// Determine personality from argv[0].
fn detect_personality(argv0: &str) -> Personality {
    let basename = argv0
        .rsplit('/')
        .next()
        .unwrap_or(argv0)
        .rsplit('\\')
        .next()
        .unwrap_or(argv0);
    let lower = basename.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);

    if stem.contains("touch") {
        Personality::Touch
    } else if stem.contains("mkfifo") {
        Personality::Mkfifo
    } else if stem.contains("readlink") {
        Personality::Readlink
    } else if stem.contains("realpath") {
        Personality::Realpath
    } else if stem.contains("ln") && !stem.contains("readlink") {
        // "ln" but not "readlink" (which contains "ln")
        Personality::Ln
    } else {
        Personality::Stat
    }
}

// ============================================================================
// File type helpers
// ============================================================================

const S_IFMT: u64 = 0o170000;
const S_IFBLK: u64 = 0o060000;
const S_IFCHR: u64 = 0o020000;
const S_IFDIR: u64 = 0o040000;
const S_IFIFO: u64 = 0o010000;
const S_IFLNK: u64 = 0o120000;
const S_IFREG: u64 = 0o100000;
const S_IFSOCK: u64 = 0o140000;

fn file_type_name(mode: u64) -> &'static str {
    match mode & S_IFMT {
        S_IFREG => "regular file",
        S_IFDIR => "directory",
        S_IFLNK => "symbolic link",
        S_IFCHR => "character special file",
        S_IFBLK => "block special file",
        S_IFIFO => "fifo",
        S_IFSOCK => "socket",
        _ => "unknown",
    }
}

fn file_type_letter(mode: u64) -> char {
    match mode & S_IFMT {
        S_IFREG => '-',
        S_IFDIR => 'd',
        S_IFLNK => 'l',
        S_IFCHR => 'c',
        S_IFBLK => 'b',
        S_IFIFO => 'p',
        S_IFSOCK => 's',
        _ => '?',
    }
}

/// Format mode bits as rwxrwxrwx string (10 chars with leading type char).
fn format_rwx(mode: u64) -> String {
    let mut s = String::with_capacity(10);
    s.push(file_type_letter(mode));

    let m = mode as u32;
    // User
    s.push(if m & 0o400 != 0 { 'r' } else { '-' });
    s.push(if m & 0o200 != 0 { 'w' } else { '-' });
    s.push(if m & 0o4000 != 0 {
        if m & 0o100 != 0 { 's' } else { 'S' }
    } else if m & 0o100 != 0 {
        'x'
    } else {
        '-'
    });
    // Group
    s.push(if m & 0o040 != 0 { 'r' } else { '-' });
    s.push(if m & 0o020 != 0 { 'w' } else { '-' });
    s.push(if m & 0o2000 != 0 {
        if m & 0o010 != 0 { 's' } else { 'S' }
    } else if m & 0o010 != 0 {
        'x'
    } else {
        '-'
    });
    // Other
    s.push(if m & 0o004 != 0 { 'r' } else { '-' });
    s.push(if m & 0o002 != 0 { 'w' } else { '-' });
    s.push(if m & 0o1000 != 0 {
        if m & 0o001 != 0 { 't' } else { 'T' }
    } else if m & 0o001 != 0 {
        'x'
    } else {
        '-'
    });

    s
}

/// Format a timestamp (seconds since epoch) as an ISO-like date string.
fn format_timestamp(secs: u64, nsec: u64) -> String {
    // Simple conversion: compute year/month/day/hour/min/sec from epoch seconds.
    // This is a simplified Gregorian calendar calculation sufficient for display.
    let total_secs = secs;
    let sec = total_secs % 60;
    let total_min = total_secs / 60;
    let min = total_min % 60;
    let total_hours = total_min / 60;
    let hour = total_hours % 24;
    let mut days = total_hours / 24;

    // Compute year and day-of-year from days since 1970-01-01.
    let mut year: u64 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Compute month and day from day-of-year.
    let leap = is_leap_year(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month: u64 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;

    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}.{nsec:09} +0000"
    )
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Populate a [`KernelStat`] from a `std::fs::Metadata`, which routes through
/// the posix libc layer to the native OurOS `stat`/`lstat` syscalls.
#[cfg(unix)]
fn metadata_to_kernel_stat(meta: &fs::Metadata) -> KernelStat {
    use std::os::unix::fs::MetadataExt;
    KernelStat {
        st_dev: meta.dev(),
        st_ino: meta.ino(),
        st_mode: u64::from(meta.mode()),
        st_nlink: meta.nlink(),
        st_uid: u64::from(meta.uid()),
        st_gid: u64::from(meta.gid()),
        st_rdev: meta.rdev(),
        st_size: meta.size(),
        st_blksize: meta.blksize(),
        st_blocks: meta.blocks(),
        st_atime_sec: nonneg_u64(meta.atime()),
        st_atime_nsec: nonneg_u64(meta.atime_nsec()),
        st_mtime_sec: nonneg_u64(meta.mtime()),
        st_mtime_nsec: nonneg_u64(meta.mtime_nsec()),
        st_ctime_sec: nonneg_u64(meta.ctime()),
        st_ctime_nsec: nonneg_u64(meta.ctime_nsec()),
    }
}

#[cfg(unix)]
fn do_stat(path: &str, follow_symlinks: bool) -> Result<KernelStat, String> {
    let meta = if follow_symlinks {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|e| e.to_string())?;
    Ok(metadata_to_kernel_stat(&meta))
}

#[cfg(unix)]
fn do_statfs(path: &str) -> Result<KernelStatFs, String> {
    let cpath = cstr(path)?;
    let mut vfs = PosixStatvfs::default();
    // SAFETY: `cpath` is a valid NUL-terminated C string and `vfs` is a valid,
    // writable buffer of the correct layout for the duration of the call.
    let ret = unsafe { statvfs(cpath.as_ptr().cast::<u8>(), &mut vfs) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // statvfs has no filesystem-type field, so report 0 for f_type.  The
    // remaining fields map directly from the POSIX statvfs structure.
    Ok(KernelStatFs {
        f_type: 0,
        f_bsize: vfs.f_bsize,
        f_blocks: vfs.f_blocks,
        f_bfree: vfs.f_bfree,
        f_bavail: vfs.f_bavail,
        f_files: vfs.f_files,
        f_ffree: vfs.f_ffree,
        f_fsid: vfs.f_fsid,
        f_namelen: vfs.f_namemax,
        f_frsize: vfs.f_frsize,
        f_flags: vfs.f_flag,
        _spare: [0; 4],
    })
}

fn do_readlink(path: &str) -> Result<String, String> {
    let target = fs::read_link(path).map_err(|e| e.to_string())?;
    target
        .into_os_string()
        .into_string()
        .map_err(|_| "readlink: target contains invalid UTF-8".into())
}

fn do_link(target: &str, linkpath: &str) -> Result<(), String> {
    fs::hard_link(target, linkpath).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn do_symlink(target: &str, linkpath: &str) -> Result<(), String> {
    std::os::unix::fs::symlink(target, linkpath).map_err(|e| e.to_string())
}

fn do_unlink(path: &str) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| e.to_string())
}

fn do_rename(old: &str, new: &str) -> Result<(), String> {
    fs::rename(old, new).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn do_mkfifo(path: &str, mode: u32) -> Result<(), String> {
    let cpath = cstr(path)?;
    // SAFETY: `cpath` is a valid NUL-terminated C string for the call duration.
    let ret = unsafe { mkfifo(cpath.as_ptr().cast::<u8>(), mode) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn do_utimensat(path: &str, times: &[Timespec; 2]) -> Result<(), String> {
    let cpath = cstr(path)?;
    // SAFETY: `cpath` is a valid NUL-terminated C string and `times` points to
    // two valid Timespecs (layout-compatible with the posix Timespec) for the
    // duration of the call.  `AT_FDCWD` resolves `path` relative to the cwd.
    let ret = unsafe { utimensat(AT_FDCWD, cpath.as_ptr().cast::<u8>(), times.as_ptr(), 0) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Non-unix stubs (host unit-test toolchain only)
// ---------------------------------------------------------------------------
//
// The real target is unix; these stubs exist solely so the binary's portable
// logic (argument parsing, formatting, timestamp math) stays compilable and
// testable on the host test toolchain, which is not unix.  They never run on
// the OS itself.

#[cfg(not(unix))]
const UNSUPPORTED_ON_HOST: &str = "operation unavailable on host test toolchain";

#[cfg(not(unix))]
fn do_stat(_path: &str, _follow_symlinks: bool) -> Result<KernelStat, String> {
    Err(UNSUPPORTED_ON_HOST.into())
}

#[cfg(not(unix))]
fn do_statfs(_path: &str) -> Result<KernelStatFs, String> {
    Err(UNSUPPORTED_ON_HOST.into())
}

#[cfg(not(unix))]
fn do_symlink(_target: &str, _linkpath: &str) -> Result<(), String> {
    Err(UNSUPPORTED_ON_HOST.into())
}

#[cfg(not(unix))]
fn do_mkfifo(_path: &str, _mode: u32) -> Result<(), String> {
    Err(UNSUPPORTED_ON_HOST.into())
}

#[cfg(not(unix))]
fn do_utimensat(_path: &str, _times: &[Timespec; 2]) -> Result<(), String> {
    Err(UNSUPPORTED_ON_HOST.into())
}

// ============================================================================
// stat mode
// ============================================================================

/// Parsed stat options.
struct StatOpts {
    filesystem: bool,
    dereference: bool,
    format: Option<String>,
    terse: bool,
    files: Vec<String>,
}

fn parse_stat_args(args: &[String]) -> Result<StatOpts, String> {
    let mut opts = StatOpts {
        filesystem: false,
        dereference: false,
        format: None,
        terse: false,
        files: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-f" || arg == "--file-system" {
            opts.filesystem = true;
        } else if arg == "-L" || arg == "--dereference" {
            opts.dereference = true;
        } else if arg == "-t" || arg == "--terse" {
            opts.terse = true;
        } else if arg == "-c" {
            i += 1;
            if i >= args.len() {
                return Err("option '-c' requires a format argument".into());
            }
            opts.format = Some(args[i].clone());
        } else if let Some(fmt) = arg.strip_prefix("--format=") {
            opts.format = Some(fmt.to_string());
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unrecognized option: '{arg}'"));
        } else {
            opts.files.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("missing file operand".into());
    }
    Ok(opts)
}

/// Apply a stat format string, substituting % sequences.
fn apply_stat_format(fmt: &str, st: &KernelStat, name: &str, link_target: &str) -> String {
    let mut out = String::with_capacity(fmt.len() * 2);
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'a' => {
                    // Access rights in octal
                    let perm = st.st_mode & 0o7777;
                    out.push_str(&format!("{perm:04o}"));
                }
                'A' => {
                    // Access rights in rwx
                    out.push_str(&format_rwx(st.st_mode));
                }
                'b' => {
                    // Number of blocks allocated
                    out.push_str(&st.st_blocks.to_string());
                }
                'B' => {
                    // Size of each block (reported in stat)
                    // Standard: always 512 for block count units
                    out.push_str("512");
                }
                'd' => {
                    // Device number in decimal
                    out.push_str(&st.st_dev.to_string());
                }
                'f' => {
                    // Raw mode in hex
                    out.push_str(&format!("{:x}", st.st_mode));
                }
                'F' => {
                    // File type string
                    out.push_str(file_type_name(st.st_mode));
                }
                'g' => {
                    // Group ID
                    out.push_str(&st.st_gid.to_string());
                }
                'G' => {
                    // Group name (fallback to numeric)
                    out.push_str(&st.st_gid.to_string());
                }
                'h' => {
                    // Number of hard links
                    out.push_str(&st.st_nlink.to_string());
                }
                'i' => {
                    // Inode number
                    out.push_str(&st.st_ino.to_string());
                }
                'n' => {
                    // File name
                    out.push_str(name);
                }
                'N' => {
                    // Quoted file name, with -> target for symlinks
                    if !link_target.is_empty() {
                        out.push_str(&format!("'{name}' -> '{link_target}'"));
                    } else {
                        out.push_str(&format!("'{name}'"));
                    }
                }
                'o' => {
                    // Optimal I/O transfer size
                    out.push_str(&st.st_blksize.to_string());
                }
                's' => {
                    // Total size in bytes
                    out.push_str(&st.st_size.to_string());
                }
                't' => {
                    // Major device type in hex (for char/block special)
                    let major = (st.st_rdev >> 8) & 0xff;
                    out.push_str(&format!("{major:x}"));
                }
                'T' => {
                    // Minor device type in hex
                    let minor = st.st_rdev & 0xff;
                    out.push_str(&format!("{minor:x}"));
                }
                'u' => {
                    // User ID
                    out.push_str(&st.st_uid.to_string());
                }
                'U' => {
                    // Username (fallback to numeric)
                    out.push_str(&st.st_uid.to_string());
                }
                'x' => {
                    // Access time
                    out.push_str(&format_timestamp(st.st_atime_sec, st.st_atime_nsec));
                }
                'y' => {
                    // Modify time
                    out.push_str(&format_timestamp(st.st_mtime_sec, st.st_mtime_nsec));
                }
                'z' => {
                    // Change time
                    out.push_str(&format_timestamp(st.st_ctime_sec, st.st_ctime_nsec));
                }
                'w' => {
                    // Birth time (not always available, show '-' if zero)
                    out.push('-');
                }
                '%' => {
                    out.push('%');
                }
                other => {
                    // Unknown format specifier: pass through
                    out.push('%');
                    out.push(other);
                }

            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// Apply a statfs format string.
fn apply_statfs_format(fmt: &str, sf: &KernelStatFs, name: &str) -> String {
    let mut out = String::with_capacity(fmt.len() * 2);
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'a' => out.push_str(&sf.f_bavail.to_string()),
                'b' => out.push_str(&sf.f_blocks.to_string()),
                'c' => out.push_str(&sf.f_files.to_string()),
                'd' => out.push_str(&sf.f_ffree.to_string()),
                'f' => out.push_str(&sf.f_bfree.to_string()),
                'i' => out.push_str(&format!("{:x}", sf.f_fsid)),
                'l' => out.push_str(&sf.f_namelen.to_string()),
                'n' => out.push_str(name),
                's' => out.push_str(&sf.f_bsize.to_string()),
                'S' => out.push_str(&sf.f_frsize.to_string()),
                't' => out.push_str(&format!("{:x}", sf.f_type)),
                'T' => out.push_str(&fstype_name(sf.f_type)),
                '%' => out.push('%'),
                other => {
                    out.push('%');
                    out.push(other);
                }
            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

fn fstype_name(ftype: u64) -> String {
    match ftype {
        0xEF53 => "ext4".into(),
        0x4d44 => "vfat".into(),
        0x01021994 => "tmpfs".into(),
        0x9fa0 => "proc".into(),
        0x62656572 => "sysfs".into(),
        0x64626720 => "debugfs".into(),
        0x858458f6 => "ramfs".into(),
        _ => format!("UNKNOWN (0x{ftype:x})"),
    }
}

fn run_stat(opts: &StatOpts) -> bool {
    let mut any_error = false;

    for file in &opts.files {
        if opts.filesystem {
            match do_statfs(file) {
                Ok(sf) => {
                    if let Some(ref fmt) = opts.format {
                        println!("{}", apply_statfs_format(fmt, &sf, file));
                    } else if opts.terse {
                        println!(
                            "{} {} {} {} {} {} {} {} {} {} {} {}",
                            file, sf.f_fsid, sf.f_namelen, sf.f_type,
                            sf.f_bsize, sf.f_frsize, sf.f_blocks, sf.f_bfree,
                            sf.f_bavail, sf.f_files, sf.f_ffree, sf.f_flags,
                        );
                    } else {
                        println!("  File: \"{}\"", file);
                        println!(
                            "    ID: {:x} Namelen: {} Type: {}",
                            sf.f_fsid, sf.f_namelen, fstype_name(sf.f_type),
                        );
                        println!(
                            "Block size: {}   Fundamental block size: {}",
                            sf.f_bsize, sf.f_frsize,
                        );
                        println!(
                            "Blocks: Total: {}   Free: {}   Available: {}",
                            sf.f_blocks, sf.f_bfree, sf.f_bavail,
                        );
                        println!(
                            "Inodes: Total: {}   Free: {}",
                            sf.f_files, sf.f_ffree,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("stat: cannot statfs '{file}': {e}");
                    any_error = true;
                }
            }
        } else {
            let follow = opts.dereference;
            match do_stat(file, follow) {
                Ok(st) => {
                    // Get link target if it's a symlink.
                    let link_target = if st.st_mode & S_IFMT == S_IFLNK {
                        do_readlink(file).unwrap_or_default()
                    } else {
                        String::new()
                    };

                    if let Some(ref fmt) = opts.format {
                        println!("{}", apply_stat_format(fmt, &st, file, &link_target));
                    } else if opts.terse {
                        println!(
                            "{} {} {} {:x} {} {} {:x} {} {} {:x} {:x} {} {} {} {}",
                            file, st.st_size, st.st_blocks, st.st_mode,
                            st.st_uid, st.st_gid, st.st_dev, st.st_ino,
                            st.st_nlink, (st.st_rdev >> 8) & 0xff,
                            st.st_rdev & 0xff, st.st_atime_sec,
                            st.st_mtime_sec, st.st_ctime_sec, 0u64,
                        );
                    } else {
                        print_default_stat(&st, file, &link_target);
                    }
                }
                Err(e) => {
                    eprintln!("stat: cannot stat '{file}': {e}");
                    any_error = true;
                }
            }
        }
    }

    !any_error
}

fn print_default_stat(st: &KernelStat, file: &str, link_target: &str) {
    // File line
    if !link_target.is_empty() {
        println!("  File: '{file}' -> '{link_target}'");
    } else {
        println!("  File: '{file}'");
    }

    println!(
        "  Size: {}\tBlocks: {}\tIO Block: {}\t{}",
        st.st_size,
        st.st_blocks,
        st.st_blksize,
        file_type_name(st.st_mode),
    );

    let major = (st.st_dev >> 8) & 0xff;
    let minor = st.st_dev & 0xff;
    println!(
        "Device: {:x}h/{}d\tInode: {}\tLinks: {}",
        st.st_dev, st.st_dev, st.st_ino, st.st_nlink,
    );

    // Device number for block/char devices
    if st.st_mode & S_IFMT == S_IFBLK || st.st_mode & S_IFMT == S_IFCHR {
        println!("Device type: {major},{minor}");
    }

    let perm = st.st_mode & 0o7777;
    println!(
        "Access: ({:04o}/{})\tUid: ({})\tGid: ({})",
        perm,
        format_rwx(st.st_mode),
        st.st_uid,
        st.st_gid,
    );
    println!(
        "Access: {}",
        format_timestamp(st.st_atime_sec, st.st_atime_nsec),
    );
    println!(
        "Modify: {}",
        format_timestamp(st.st_mtime_sec, st.st_mtime_nsec),
    );
    println!(
        "Change: {}",
        format_timestamp(st.st_ctime_sec, st.st_ctime_nsec),
    );
    println!(" Birth: -");
}

// ============================================================================
// touch mode
// ============================================================================

/// Parsed touch options.
struct TouchOpts {
    access_only: bool,
    modify_only: bool,
    no_create: bool,
    date: Option<String>,
    stamp: Option<String>,
    reference: Option<String>,
    files: Vec<String>,
}

fn parse_touch_args(args: &[String]) -> Result<TouchOpts, String> {
    let mut opts = TouchOpts {
        access_only: false,
        modify_only: false,
        no_create: false,
        date: None,
        stamp: None,
        reference: None,
        files: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-a" {
            opts.access_only = true;
        } else if arg == "-m" {
            opts.modify_only = true;
        } else if arg == "-c" || arg == "--no-create" {
            opts.no_create = true;
        } else if arg == "-d" {
            i += 1;
            if i >= args.len() {
                return Err("option '-d' requires a date argument".into());
            }
            opts.date = Some(args[i].clone());
        } else if let Some(val) = arg.strip_prefix("--date=") {
            opts.date = Some(val.to_string());
        } else if arg == "-t" {
            i += 1;
            if i >= args.len() {
                return Err("option '-t' requires a timestamp argument".into());
            }
            opts.stamp = Some(args[i].clone());
        } else if arg == "-r" {
            i += 1;
            if i >= args.len() {
                return Err("option '-r' requires a file argument".into());
            }
            opts.reference = Some(args[i].clone());
        } else if let Some(val) = arg.strip_prefix("--reference=") {
            opts.reference = Some(val.to_string());
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Handle combined short flags like -am, -mc, etc.
            let flags = &arg[1..];
            for ch in flags.chars() {
                match ch {
                    'a' => opts.access_only = true,
                    'm' => opts.modify_only = true,
                    'c' => opts.no_create = true,
                    _ => return Err(format!("unrecognized option: '-{ch}'")),
                }
            }
        } else {
            opts.files.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("missing file operand".into());
    }
    Ok(opts)
}

/// Parse a [[CC]YY]MMDDhhmm[.ss] timestamp string into epoch seconds.
///
/// Supported forms:
///   MMDDhhmm        (current century assumed)
///   YYMMDDhhmm      (00-68 = 20xx, 69-99 = 19xx)
///   CCYYMMDDhhmm
///   Any of the above with .ss suffix
fn parse_touch_stamp(s: &str) -> Result<i64, String> {
    let (main_part, secs) = if let Some(dot_pos) = s.rfind('.') {
        let sec_str = &s[dot_pos + 1..];
        let sec: u32 = sec_str
            .parse()
            .map_err(|_| format!("invalid seconds in timestamp: '{sec_str}'"))?;
        if sec > 59 {
            return Err(format!("seconds out of range: {sec}"));
        }
        (&s[..dot_pos], sec)
    } else {
        (s, 0u32)
    };

    if main_part.len() < 8
        || !main_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("invalid timestamp format: '{s}'"));
    }

    let (year, rest) = match main_part.len() {
        8 => {
            // MMDDhhmm - assume current year (2026 as default)
            (2026u32, main_part)
        }
        10 => {
            // YYMMDDhhmm
            let yy: u32 = main_part[..2]
                .parse()
                .map_err(|_| "invalid year".to_string())?;
            let year = if yy <= 68 { 2000 + yy } else { 1900 + yy };
            (year, &main_part[2..])
        }
        12 => {
            // CCYYMMDDhhmm
            let ccyy: u32 = main_part[..4]
                .parse()
                .map_err(|_| "invalid year".to_string())?;
            (ccyy, &main_part[4..])
        }
        _ => {
            return Err(format!("invalid timestamp length: '{s}'"));
        }
    };

    let month: u32 = rest[..2]
        .parse()
        .map_err(|_| "invalid month".to_string())?;
    let day: u32 = rest[2..4]
        .parse()
        .map_err(|_| "invalid day".to_string())?;
    let hour: u32 = rest[4..6]
        .parse()
        .map_err(|_| "invalid hour".to_string())?;
    let minute: u32 = rest[6..8]
        .parse()
        .map_err(|_| "invalid minute".to_string())?;

    if !(1..=12).contains(&month) {
        return Err(format!("month out of range: {month}"));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day out of range: {day}"));
    }
    if hour > 23 {
        return Err(format!("hour out of range: {hour}"));
    }
    if minute > 59 {
        return Err(format!("minute out of range: {minute}"));
    }

    Ok(date_to_epoch(year, month, day, hour, minute, secs))
}

/// Parse a date string (subset of ISO 8601 and common formats).
///
/// Supported: "YYYY-MM-DD HH:MM:SS", "YYYY-MM-DD", "YYYY-MM-DDTHH:MM:SS"
fn parse_date_string(s: &str) -> Result<i64, String> {
    let s = s.trim();

    // Try "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DDTHH:MM:SS"
    let normalized = s.replace('T', " ");
    let parts: Vec<&str> = normalized.splitn(2, ' ').collect();

    let date_part = parts.first().copied().unwrap_or("");
    let date_fields: Vec<&str> = date_part.split('-').collect();
    if date_fields.len() != 3 {
        return Err(format!("invalid date format: '{s}'"));
    }

    let year: u32 = date_fields[0]
        .parse()
        .map_err(|_| format!("invalid year in '{s}'"))?;
    let month: u32 = date_fields[1]
        .parse()
        .map_err(|_| format!("invalid month in '{s}'"))?;
    let day: u32 = date_fields[2]
        .parse()
        .map_err(|_| format!("invalid day in '{s}'"))?;

    let (hour, minute, second) = if parts.len() > 1 {
        let time_part = parts[1];
        let time_fields: Vec<&str> = time_part.split(':').collect();
        let h: u32 = time_fields
            .first()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let m: u32 = time_fields
            .get(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let sec: u32 = time_fields
            .get(2)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        (h, m, sec)
    } else {
        (0, 0, 0)
    };

    if !(1..=12).contains(&month) {
        return Err(format!("month out of range: {month}"));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day out of range: {day}"));
    }
    if hour > 23 {
        return Err(format!("hour out of range: {hour}"));
    }
    if minute > 59 {
        return Err(format!("minute out of range: {minute}"));
    }
    if second > 59 {
        return Err(format!("second out of range: {second}"));
    }

    Ok(date_to_epoch(year, month, day, hour, minute, second))
}

/// Convert a calendar date to Unix epoch seconds (UTC).
fn date_to_epoch(year: u32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    // Days from 1970-01-01 to the start of the given year.
    let mut days: i64 = 0;
    if year >= 1970 {
        for y in 1970..year {
            days += if is_leap_year(y as u64) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if is_leap_year(y as u64) { 366 } else { 365 };
        }
    }

    // Days within the year up to the start of the given month.
    let leap = is_leap_year(year as u64);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let months_elapsed = (month.saturating_sub(1) as usize).min(12);
    for d in month_days.iter().take(months_elapsed) {
        days += *d;
    }
    days += (day as i64) - 1;

    days * 86400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64)
}

/// Sentinel: use the current time for this timespec field.
const UTIME_NOW: i64 = (1 << 30) - 1;
/// Sentinel: leave this timespec field unchanged.
const UTIME_OMIT: i64 = (1 << 30) - 2;

fn run_touch(opts: &TouchOpts) -> bool {
    // Determine the target time.
    let explicit_time: Option<Timespec> = if let Some(ref stamp) = opts.stamp {
        match parse_touch_stamp(stamp) {
            Ok(epoch) => Some(Timespec { tv_sec: epoch, tv_nsec: 0 }),
            Err(e) => {
                eprintln!("touch: invalid timestamp: {e}");
                return false;
            }
        }
    } else if let Some(ref date) = opts.date {
        match parse_date_string(date) {
            Ok(epoch) => Some(Timespec { tv_sec: epoch, tv_nsec: 0 }),
            Err(e) => {
                eprintln!("touch: invalid date: {e}");
                return false;
            }
        }
    } else if let Some(ref refpath) = opts.reference {
        match do_stat(refpath, true) {
            Ok(st) => Some(Timespec {
                tv_sec: st.st_mtime_sec as i64,
                tv_nsec: st.st_mtime_nsec as i64,
            }),
            Err(e) => {
                eprintln!("touch: cannot stat reference '{refpath}': {e}");
                return false;
            }
        }
    } else {
        None // Use current time
    };

    let mut any_error = false;

    for file in &opts.files {
        // Check if file exists; create if needed.
        let exists = fs::metadata(file).is_ok();
        if !exists {
            if opts.no_create {
                continue;
            }
            // Create the file (empty).
            if let Err(e) = fs::write(file, b"") {
                eprintln!("touch: cannot create '{file}': {e}");
                any_error = true;
                continue;
            }
        }

        // Build timespec pair [atime, mtime].
        let now_ts = explicit_time.unwrap_or({
            Timespec { tv_sec: 0, tv_nsec: UTIME_NOW }
        });

        let atime = if opts.modify_only && !opts.access_only {
            Timespec { tv_sec: 0, tv_nsec: UTIME_OMIT }
        } else {
            now_ts
        };

        let mtime = if opts.access_only && !opts.modify_only {
            Timespec { tv_sec: 0, tv_nsec: UTIME_OMIT }
        } else {
            now_ts
        };

        let times = [atime, mtime];
        if let Err(e) = do_utimensat(file, &times) {
            eprintln!("touch: cannot update times for '{file}': {e}");
            any_error = true;
        }
    }

    !any_error
}

// ============================================================================
// ln mode
// ============================================================================

/// Parsed ln options.
struct LnOpts {
    symbolic: bool,
    force: bool,
    no_deref: bool,
    verbose: bool,
    backup: bool,
    target_dir: Option<String>,
    no_target_dir: bool,
    targets: Vec<String>,
    link_name: Option<String>,
}

fn parse_ln_args(args: &[String]) -> Result<LnOpts, String> {
    let mut opts = LnOpts {
        symbolic: false,
        force: false,
        no_deref: false,
        verbose: false,
        backup: false,
        target_dir: None,
        no_target_dir: false,
        targets: Vec::new(),
        link_name: None,
    };

    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-s" || arg == "--symbolic" {
            opts.symbolic = true;
        } else if arg == "-f" || arg == "--force" {
            opts.force = true;
        } else if arg == "-n" || arg == "--no-dereference" {
            opts.no_deref = true;
        } else if arg == "-v" || arg == "--verbose" {
            opts.verbose = true;
        } else if arg == "-b" || arg == "--backup" {
            opts.backup = true;
        } else if arg == "-T" || arg == "--no-target-directory" {
            opts.no_target_dir = true;
        } else if arg == "-t" {
            i += 1;
            if i >= args.len() {
                return Err("option '-t' requires a directory argument".into());
            }
            opts.target_dir = Some(args[i].clone());
        } else if let Some(val) = arg.strip_prefix("--target-directory=") {
            opts.target_dir = Some(val.to_string());
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Handle combined short flags like -sf, -sfn, etc.
            let flags = &arg[1..];
            for ch in flags.chars() {
                match ch {
                    's' => opts.symbolic = true,
                    'f' => opts.force = true,
                    'n' => opts.no_deref = true,
                    'v' => opts.verbose = true,
                    'b' => opts.backup = true,
                    'T' => opts.no_target_dir = true,
                    _ => return Err(format!("unrecognized option: '-{ch}'")),
                }
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        positional.push(args[i].clone());
        i += 1;
    }

    if positional.is_empty() {
        return Err("missing file operand".into());
    }

    if opts.target_dir.is_some() {
        // All positional args are targets.
        opts.targets = positional;
    } else if opts.no_target_dir || positional.len() == 2 {
        // Last arg is the link name.
        opts.link_name = positional.pop();
        opts.targets = positional;
    } else if positional.len() == 1 {
        // ln TARGET: create link in current directory with same basename
        opts.targets = positional;
    } else {
        // Multiple targets: last must be a directory
        opts.link_name = positional.pop();
        opts.targets = positional;
    }

    if opts.targets.is_empty() {
        return Err("missing file operand".into());
    }

    Ok(opts)
}

fn run_ln(opts: &LnOpts) -> bool {
    let mut any_error = false;

    for target in &opts.targets {
        // Determine the link path.
        let link_path = if let Some(ref dir) = opts.target_dir {
            // Place link in target directory with same basename as target.
            let basename = Path::new(target)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| target.clone());
            format!("{dir}/{basename}")
        } else if let Some(ref name) = opts.link_name {
            // Check if link_name is a directory (and not -T).
            if !opts.no_target_dir && Path::new(name).is_dir() {
                let basename = Path::new(target)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| target.clone());
                format!("{name}/{basename}")
            } else {
                name.clone()
            }
        } else {
            // Default: use basename in current directory.
            Path::new(target)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| target.clone())
        };

        // Handle existing destination.
        let dest_exists = if opts.no_deref {
            // Don't dereference: check if the link path itself exists
            // (including as a symlink that may be dangling).
            fs::symlink_metadata(&link_path).is_ok()
        } else {
            fs::metadata(&link_path).is_ok()
        };

        if dest_exists {
            if opts.backup {
                let backup_path = format!("{link_path}~");
                if let Err(e) = do_rename(&link_path, &backup_path) {
                    eprintln!("ln: cannot create backup of '{link_path}': {e}");
                    any_error = true;
                    continue;
                }
            } else if opts.force {
                if let Err(e) = do_unlink(&link_path) {
                    eprintln!("ln: cannot remove '{link_path}': {e}");
                    any_error = true;
                    continue;
                }
            } else {
                eprintln!("ln: '{link_path}': file exists");
                any_error = true;
                continue;
            }
        }

        let result = if opts.symbolic {
            do_symlink(target, &link_path)
        } else {
            do_link(target, &link_path)
        };

        match result {
            Ok(()) => {
                if opts.verbose {
                    let kind = if opts.symbolic { "symbolic" } else { "hard" };
                    eprintln!("'{link_path}' -> '{target}' ({kind} link)");
                }
            }
            Err(e) => {
                let kind = if opts.symbolic { "symbolic" } else { "hard" };
                eprintln!("ln: failed to create {kind} link '{link_path}' -> '{target}': {e}");
                any_error = true;
            }
        }
    }

    !any_error
}

// ============================================================================
// readlink mode
// ============================================================================

/// Canonicalization mode for readlink/realpath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonMode {
    /// Just print the symlink target (no canonicalization).
    None,
    /// Canonicalize: follow all symlinks, resolve ..
    /// Missing final component is OK; intermediate must exist.
    Canonicalize,
    /// Canonicalize: all components must exist.
    CanonicalizeExisting,
    /// Canonicalize: components need not exist.
    CanonicalizeMissing,
}

/// Parsed readlink options.
struct ReadlinkOpts {
    canon: CanonMode,
    no_newline: bool,
    zero_delim: bool,
    files: Vec<String>,
}

fn parse_readlink_args(args: &[String]) -> Result<ReadlinkOpts, String> {
    let mut opts = ReadlinkOpts {
        canon: CanonMode::None,
        no_newline: false,
        zero_delim: false,
        files: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-f" || arg == "--canonicalize" {
            opts.canon = CanonMode::Canonicalize;
        } else if arg == "-e" || arg == "--canonicalize-existing" {
            opts.canon = CanonMode::CanonicalizeExisting;
        } else if arg == "-m" || arg == "--canonicalize-missing" {
            opts.canon = CanonMode::CanonicalizeMissing;
        } else if arg == "-n" || arg == "--no-newline" {
            opts.no_newline = true;
        } else if arg == "-z" || arg == "--zero" {
            opts.zero_delim = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Combined short flags
            let flags = &arg[1..];
            for ch in flags.chars() {
                match ch {
                    'f' => opts.canon = CanonMode::Canonicalize,
                    'e' => opts.canon = CanonMode::CanonicalizeExisting,
                    'm' => opts.canon = CanonMode::CanonicalizeMissing,
                    'n' => opts.no_newline = true,
                    'z' => opts.zero_delim = true,
                    _ => return Err(format!("unrecognized option: '-{ch}'")),
                }
            }
        } else {
            opts.files.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("missing file operand".into());
    }
    Ok(opts)
}

/// Canonicalize a path: resolve all symlinks, `.`, and `..` components.
///
/// `existing_mode`:
/// - `CanonicalizeExisting`: every component must exist
/// - `CanonicalizeMissing`: nothing needs to exist (purely textual after CWD)
/// - `Canonicalize`: intermediate components must exist, final need not
fn canonicalize_path(path: &str, mode: CanonMode) -> Result<String, String> {
    let p = Path::new(path);

    // Start with an absolute base.
    let abs = if p.is_absolute() {
        PathBuf::from(path)
    } else {
        let cwd = env::current_dir()
            .map_err(|e| format!("cannot determine current directory: {e}"))?;
        cwd.join(path)
    };

    let mut resolved = PathBuf::new();
    let mut symlink_count = 0u32;
    let max_symlinks = 40;

    for (idx, component) in abs.components().enumerate() {
        use std::path::Component;
        match component {
            Component::RootDir => {
                resolved.push("/");
            }
            Component::Prefix(p) => {
                resolved.push(p.as_os_str());
            }
            Component::CurDir => {
                // Skip `.`
            }
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);

                // Check if this component is a symlink.
                let path_str = resolved.to_string_lossy().to_string();
                match do_stat(&path_str, false) {
                    Ok(st) => {
                        if st.st_mode & S_IFMT == S_IFLNK {
                            symlink_count += 1;
                            if symlink_count > max_symlinks {
                                return Err("too many levels of symbolic links".into());
                            }
                            match do_readlink(&path_str) {
                                Ok(target) => {
                                    resolved.pop();
                                    let target_path = Path::new(&target);
                                    if target_path.is_absolute() {
                                        resolved = PathBuf::from(&target);
                                    } else {
                                        resolved.push(&target);
                                    }
                                    // Re-canonicalize the resulting path.
                                    // For simplicity, normalize .. and . in the result.
                                    resolved = normalize_path(&resolved);
                                }
                                Err(e) => {
                                    return Err(format!(
                                        "cannot read symlink '{path_str}': {e}"
                                    ));
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Component doesn't exist.
                        let is_last = idx == abs.components().count() - 1;
                        match mode {
                            CanonMode::CanonicalizeExisting => {
                                return Err(format!(
                                    "'{path_str}': no such file or directory"
                                ));
                            }
                            CanonMode::Canonicalize if !is_last => {
                                return Err(format!(
                                    "'{path_str}': no such file or directory"
                                ));
                            }
                            _ => {
                                // Missing or CanonicalizeMissing: keep going textually.
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(resolved.to_string_lossy().into_owned())
}

/// Normalize a path by resolving `.` and `..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

fn run_readlink(opts: &ReadlinkOpts) -> bool {
    let mut any_error = false;

    for (idx, file) in opts.files.iter().enumerate() {
        let result = if opts.canon != CanonMode::None {
            canonicalize_path(file, opts.canon)
        } else {
            do_readlink(file)
        };

        match result {
            Ok(target) => {
                if opts.zero_delim {
                    print!("{target}\0");
                } else if opts.no_newline && idx == opts.files.len() - 1 {
                    print!("{target}");
                } else {
                    println!("{target}");
                }
            }
            Err(e) => {
                eprintln!("readlink: {file}: {e}");
                any_error = true;
            }
        }
    }

    !any_error
}

// ============================================================================
// realpath mode
// ============================================================================

/// Parsed realpath options.
struct RealpathOpts {
    canon: CanonMode,
    no_symlinks: bool,
    relative_to: Option<String>,
    relative_base: Option<String>,
    files: Vec<String>,
}

fn parse_realpath_args(args: &[String]) -> Result<RealpathOpts, String> {
    let mut opts = RealpathOpts {
        canon: CanonMode::Canonicalize,
        no_symlinks: false,
        relative_to: None,
        relative_base: None,
        files: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-e" || arg == "--canonicalize-existing" {
            opts.canon = CanonMode::CanonicalizeExisting;
        } else if arg == "-m" || arg == "--canonicalize-missing" {
            opts.canon = CanonMode::CanonicalizeMissing;
        } else if arg == "-s" || arg == "--strip" || arg == "--no-symlinks" {
            opts.no_symlinks = true;
        } else if let Some(val) = arg.strip_prefix("--relative-to=") {
            opts.relative_to = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("--relative-base=") {
            opts.relative_base = Some(val.to_string());
        } else if arg.starts_with('-') && arg.len() > 1 {
            let flags = &arg[1..];
            for ch in flags.chars() {
                match ch {
                    'e' => opts.canon = CanonMode::CanonicalizeExisting,
                    'm' => opts.canon = CanonMode::CanonicalizeMissing,
                    's' => opts.no_symlinks = true,
                    _ => return Err(format!("unrecognized option: '-{ch}'")),
                }
            }
        } else {
            opts.files.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("missing file operand".into());
    }
    Ok(opts)
}

/// Make `path` relative to `base`.
///
/// Both must be absolute paths. Returns the relative path from `base` to `path`.
fn make_relative(path: &str, base: &str) -> String {
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let base_parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();

    // Find the common prefix length.
    let common = path_parts
        .iter()
        .zip(base_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Go up from base to common ancestor, then down to path.
    let ups = base_parts.len() - common;
    let mut result = String::new();
    for _ in 0..ups {
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str("..");
    }
    for part in &path_parts[common..] {
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(part);
    }

    if result.is_empty() {
        ".".into()
    } else {
        result
    }
}

/// Resolve a path to absolute without following symlinks (textual only).
fn resolve_no_symlinks(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    let abs = if p.is_absolute() {
        PathBuf::from(path)
    } else {
        let cwd = env::current_dir()
            .map_err(|e| format!("cannot determine current directory: {e}"))?;
        cwd.join(path)
    };
    Ok(normalize_path(&abs).to_string_lossy().into_owned())
}

fn run_realpath(opts: &RealpathOpts) -> bool {
    let mut any_error = false;

    for file in &opts.files {
        let result = if opts.no_symlinks {
            resolve_no_symlinks(file)
        } else {
            canonicalize_path(file, opts.canon)
        };

        match result {
            Ok(mut resolved) => {
                // Handle --relative-to and --relative-base.
                if let Some(ref base) = opts.relative_base {
                    let abs_base = if opts.no_symlinks {
                        resolve_no_symlinks(base)
                    } else {
                        canonicalize_path(base, opts.canon)
                    };
                    match abs_base {
                        Ok(ab) => {
                            if resolved.starts_with(&ab) {
                                let rel_to = opts
                                    .relative_to
                                    .as_deref()
                                    .unwrap_or(base.as_str());
                                let abs_rel = if opts.no_symlinks {
                                    resolve_no_symlinks(rel_to)
                                } else {
                                    canonicalize_path(rel_to, opts.canon)
                                };
                                if let Ok(ar) = abs_rel {
                                    resolved = make_relative(&resolved, &ar);
                                }
                            }
                            // If not below base, print absolute (per GNU coreutils behavior).
                        }
                        Err(e) => {
                            eprintln!("realpath: {base}: {e}");
                            any_error = true;
                            continue;
                        }
                    }
                } else if let Some(ref rel_to) = opts.relative_to {
                    let abs_rel = if opts.no_symlinks {
                        resolve_no_symlinks(rel_to)
                    } else {
                        canonicalize_path(rel_to, opts.canon)
                    };
                    match abs_rel {
                        Ok(ar) => {
                            resolved = make_relative(&resolved, &ar);
                        }
                        Err(e) => {
                            eprintln!("realpath: {rel_to}: {e}");
                            any_error = true;
                            continue;
                        }
                    }
                }

                println!("{resolved}");
            }
            Err(e) => {
                eprintln!("realpath: {file}: {e}");
                any_error = true;
            }
        }
    }

    !any_error
}

// ============================================================================
// mkfifo mode
// ============================================================================

/// Parsed mkfifo options.
struct MkfifoOpts {
    mode: u32,
    files: Vec<String>,
}

fn parse_mkfifo_args(args: &[String]) -> Result<MkfifoOpts, String> {
    let mut opts = MkfifoOpts {
        mode: 0o666,
        files: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" {
            return Err(String::new());
        }
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-m" {
            i += 1;
            if i >= args.len() {
                return Err("option '-m' requires a mode argument".into());
            }
            opts.mode = parse_octal_mode(&args[i])?;
        } else if let Some(val) = arg.strip_prefix("--mode=") {
            opts.mode = parse_octal_mode(val)?;
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unrecognized option: '{arg}'"));
        } else {
            opts.files.push(arg.clone());
        }
        i += 1;
    }
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("missing file operand".into());
    }
    Ok(opts)
}

/// Parse an octal mode string (e.g., "644", "0755").
fn parse_octal_mode(s: &str) -> Result<u32, String> {
    let trimmed = s.strip_prefix('0').unwrap_or(s);
    if trimmed.is_empty() {
        return Ok(0);
    }
    if !trimmed.bytes().all(|b| b.is_ascii_digit() && b <= b'7') {
        return Err(format!("invalid mode: '{s}'"));
    }
    u32::from_str_radix(trimmed, 8).map_err(|e| format!("invalid mode '{s}': {e}"))
}

fn run_mkfifo(opts: &MkfifoOpts) -> bool {
    let mut any_error = false;

    for file in &opts.files {
        if let Err(e) = do_mkfifo(file, opts.mode) {
            eprintln!("mkfifo: cannot create fifo '{file}': {e}");
            any_error = true;
        }
    }

    !any_error
}

// ============================================================================
// Help texts
// ============================================================================

fn print_stat_help() {
    println!("OurOS stat v0.1.0 -- Display file or filesystem status");
    println!();
    println!("USAGE:");
    println!("  stat [OPTIONS] FILE...");
    println!();
    println!("OPTIONS:");
    println!("  -f, --file-system      Display filesystem status instead of file status");
    println!("  -L, --dereference      Follow symlinks");
    println!("  -c FORMAT              Use FORMAT instead of default output");
    println!("  --format=FORMAT        Same as -c FORMAT");
    println!("  -t, --terse            Print in terse form");
    println!("  --help                 Show this help");
    println!();
    println!("FORMAT SEQUENCES (file):");
    println!("  %a  access rights (octal)  %A  access rights (rwx)");
    println!("  %b  blocks allocated       %B  block size (512)");
    println!("  %d  device number          %f  raw mode (hex)");
    println!("  %F  file type              %g  group ID");
    println!("  %G  group name             %h  hard links");
    println!("  %i  inode                  %n  file name");
    println!("  %N  quoted name (->target) %o  optimal I/O size");
    println!("  %s  total size (bytes)     %t  major device type (hex)");
    println!("  %T  minor device type (hex)");
    println!("  %u  user ID               %U  user name");
    println!("  %x  access time           %y  modify time");
    println!("  %z  change time           %w  birth time");
}

fn print_touch_help() {
    println!("OurOS touch v0.1.0 -- Update file access and modification times");
    println!();
    println!("USAGE:");
    println!("  touch [OPTIONS] FILE...");
    println!();
    println!("OPTIONS:");
    println!("  -a                     Change only access time");
    println!("  -m                     Change only modification time");
    println!("  -c, --no-create        Don't create files that don't exist");
    println!("  -d DATE, --date=DATE   Parse DATE string (YYYY-MM-DD [HH:MM:SS])");
    println!("  -t STAMP               Use [[CC]YY]MMDDhhmm[.ss]");
    println!("  -r FILE, --reference=FILE  Use FILE's timestamps");
    println!("  --help                 Show this help");
    println!();
    println!("If no -d, -t, or -r is given, the current time is used.");
    println!("Files are created (empty) if they don't exist, unless -c is specified.");
}

fn print_ln_help() {
    println!("OurOS ln v0.1.0 -- Create links between files");
    println!();
    println!("USAGE:");
    println!("  ln [OPTIONS] TARGET LINK_NAME");
    println!("  ln [OPTIONS] TARGET... DIRECTORY");
    println!("  ln [OPTIONS] -t DIRECTORY TARGET...");
    println!();
    println!("OPTIONS:");
    println!("  -s, --symbolic             Create symbolic links");
    println!("  -f, --force                Remove existing destination files");
    println!("  -n, --no-dereference       Treat LINK_NAME as normal file if symlink");
    println!("  -v, --verbose              Print name of each linked file");
    println!("  -b, --backup               Make backup of existing destination");
    println!("  -t DIR, --target-directory=DIR  Specify target directory");
    println!("  -T, --no-target-directory   Treat LINK_NAME as normal file");
    println!("  --help                     Show this help");
    println!();
    println!("By default, creates hard links. Use -s for symbolic links.");
}

fn print_readlink_help() {
    println!("OurOS readlink v0.1.0 -- Print symlink target or canonical path");
    println!();
    println!("USAGE:");
    println!("  readlink [OPTIONS] FILE...");
    println!();
    println!("OPTIONS:");
    println!("  -f, --canonicalize           Follow all symlinks, resolve ..");
    println!("  -e, --canonicalize-existing  Like -f, but all components must exist");
    println!("  -m, --canonicalize-missing   Like -f, but components need not exist");
    println!("  -n, --no-newline             Don't output trailing newline");
    println!("  -z, --zero                   Use NUL as delimiter instead of newline");
    println!("  --help                       Show this help");
}

fn print_realpath_help() {
    println!("OurOS realpath v0.1.0 -- Print resolved absolute path");
    println!();
    println!("USAGE:");
    println!("  realpath [OPTIONS] FILE...");
    println!();
    println!("OPTIONS:");
    println!("  -e, --canonicalize-existing  All components must exist");
    println!("  -m, --canonicalize-missing   No component needs to exist");
    println!("  -s, --strip, --no-symlinks   Don't resolve symlinks");
    println!("  --relative-to=DIR            Print path relative to DIR");
    println!("  --relative-base=DIR          Print relative only if below DIR");
    println!("  --help                       Show this help");
}

fn print_mkfifo_help() {
    println!("OurOS mkfifo v0.1.0 -- Create named pipes (FIFOs)");
    println!();
    println!("USAGE:");
    println!("  mkfifo [OPTIONS] NAME...");
    println!();
    println!("OPTIONS:");
    println!("  -m MODE, --mode=MODE   Set file permission bits (default: 0666)");
    println!("  --help                 Show this help");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Stat);

    let name = match personality {
        Personality::Stat => "stat",
        Personality::Touch => "touch",
        Personality::Ln => "ln",
        Personality::Readlink => "readlink",
        Personality::Realpath => "realpath",
        Personality::Mkfifo => "mkfifo",
    };

    let success = match personality {
        Personality::Stat => {
            match parse_stat_args(&args) {
                Ok(opts) => run_stat(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_stat_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
        Personality::Touch => {
            match parse_touch_args(&args) {
                Ok(opts) => run_touch(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_touch_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
        Personality::Ln => {
            match parse_ln_args(&args) {
                Ok(opts) => run_ln(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_ln_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
        Personality::Readlink => {
            match parse_readlink_args(&args) {
                Ok(opts) => run_readlink(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_readlink_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
        Personality::Realpath => {
            match parse_realpath_args(&args) {
                Ok(opts) => run_realpath(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_realpath_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
        Personality::Mkfifo => {
            match parse_mkfifo_args(&args) {
                Ok(opts) => run_mkfifo(&opts),
                Err(msg) => {
                    if msg.is_empty() {
                        print_mkfifo_help();
                        process::exit(0);
                    }
                    eprintln!("{name}: {msg}");
                    eprintln!("Try '{name} --help' for usage information.");
                    process::exit(1);
                }
            }
        }
    };

    if !success {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Personality detection ----

    #[test]
    fn test_personality_stat() {
        assert_eq!(detect_personality("stat"), Personality::Stat);
        assert_eq!(detect_personality("/usr/bin/stat"), Personality::Stat);
        assert_eq!(detect_personality("stat.exe"), Personality::Stat);
    }

    #[test]
    fn test_personality_touch() {
        assert_eq!(detect_personality("touch"), Personality::Touch);
        assert_eq!(detect_personality("/bin/touch"), Personality::Touch);
        assert_eq!(detect_personality("C:\\bin\\touch.exe"), Personality::Touch);
    }

    #[test]
    fn test_personality_ln() {
        assert_eq!(detect_personality("ln"), Personality::Ln);
        assert_eq!(detect_personality("/usr/bin/ln"), Personality::Ln);
    }

    #[test]
    fn test_personality_readlink() {
        assert_eq!(detect_personality("readlink"), Personality::Readlink);
        assert_eq!(detect_personality("/usr/bin/readlink"), Personality::Readlink);
    }

    #[test]
    fn test_personality_realpath() {
        assert_eq!(detect_personality("realpath"), Personality::Realpath);
        assert_eq!(detect_personality("/usr/bin/realpath.exe"), Personality::Realpath);
    }

    #[test]
    fn test_personality_mkfifo() {
        assert_eq!(detect_personality("mkfifo"), Personality::Mkfifo);
        assert_eq!(detect_personality("/usr/bin/mkfifo"), Personality::Mkfifo);
    }

    #[test]
    fn test_personality_unknown_defaults_to_stat() {
        assert_eq!(detect_personality("foobar"), Personality::Stat);
        assert_eq!(detect_personality(""), Personality::Stat);
    }

    // ---- File type helpers ----

    #[test]
    fn test_file_type_name() {
        assert_eq!(file_type_name(S_IFREG), "regular file");
        assert_eq!(file_type_name(S_IFDIR), "directory");
        assert_eq!(file_type_name(S_IFLNK), "symbolic link");
        assert_eq!(file_type_name(S_IFIFO), "fifo");
        assert_eq!(file_type_name(S_IFSOCK), "socket");
        assert_eq!(file_type_name(S_IFCHR), "character special file");
        assert_eq!(file_type_name(S_IFBLK), "block special file");
    }

    #[test]
    fn test_file_type_letter() {
        assert_eq!(file_type_letter(S_IFREG), '-');
        assert_eq!(file_type_letter(S_IFDIR), 'd');
        assert_eq!(file_type_letter(S_IFLNK), 'l');
        assert_eq!(file_type_letter(S_IFIFO), 'p');
    }

    #[test]
    fn test_format_rwx_regular_755() {
        let mode = S_IFREG | 0o755;
        assert_eq!(format_rwx(mode), "-rwxr-xr-x");
    }

    #[test]
    fn test_format_rwx_directory_700() {
        let mode = S_IFDIR | 0o700;
        assert_eq!(format_rwx(mode), "drwx------");
    }

    #[test]
    fn test_format_rwx_setuid() {
        let mode = S_IFREG | 0o4755;
        assert_eq!(format_rwx(mode), "-rwsr-xr-x");
    }

    #[test]
    fn test_format_rwx_setgid_no_exec() {
        let mode = S_IFREG | 0o2644;
        assert_eq!(format_rwx(mode), "-rw-r-Sr--");
    }

    #[test]
    fn test_format_rwx_sticky() {
        let mode = S_IFDIR | 0o1777;
        assert_eq!(format_rwx(mode), "drwxrwxrwt");
    }

    #[test]
    fn test_format_rwx_sticky_no_other_exec() {
        let mode = S_IFDIR | 0o1776;
        assert_eq!(format_rwx(mode), "drwxrwxrwT");
    }

    // ---- Stat format parsing ----

    #[test]
    fn test_stat_format_name() {
        let st = KernelStat::default();
        let result = apply_stat_format("%n", &st, "testfile", "");
        assert_eq!(result, "testfile");
    }

    #[test]
    fn test_stat_format_quoted_name_no_link() {
        let st = KernelStat::default();
        let result = apply_stat_format("%N", &st, "testfile", "");
        assert_eq!(result, "'testfile'");
    }

    #[test]
    fn test_stat_format_quoted_name_with_link() {
        let st = KernelStat::default();
        let result = apply_stat_format("%N", &st, "mylink", "/target");
        assert_eq!(result, "'mylink' -> '/target'");
    }

    #[test]
    fn test_stat_format_size() {
        let st = KernelStat {
            st_size: 12345,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%s", &st, "f", "");
        assert_eq!(result, "12345");
    }

    #[test]
    fn test_stat_format_octal_perms() {
        let st = KernelStat {
            st_mode: S_IFREG | 0o755,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%a", &st, "f", "");
        assert_eq!(result, "0755");
    }

    #[test]
    fn test_stat_format_rwx_perms() {
        let st = KernelStat {
            st_mode: S_IFREG | 0o644,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%A", &st, "f", "");
        assert_eq!(result, "-rw-r--r--");
    }

    #[test]
    fn test_stat_format_file_type() {
        let st = KernelStat {
            st_mode: S_IFDIR | 0o755,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%F", &st, "d", "");
        assert_eq!(result, "directory");
    }

    #[test]
    fn test_stat_format_inode_and_links() {
        let st = KernelStat {
            st_ino: 42,
            st_nlink: 3,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%i %h", &st, "f", "");
        assert_eq!(result, "42 3");
    }

    #[test]
    fn test_stat_format_escape_percent() {
        let st = KernelStat::default();
        let result = apply_stat_format("%%", &st, "f", "");
        assert_eq!(result, "%");
    }

    #[test]
    fn test_stat_format_backslash_n() {
        let st = KernelStat::default();
        let result = apply_stat_format("a\\nb", &st, "f", "");
        assert_eq!(result, "a\nb");
    }

    #[test]
    fn test_stat_format_uid_gid() {
        let st = KernelStat {
            st_uid: 1000,
            st_gid: 100,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%u:%g", &st, "f", "");
        assert_eq!(result, "1000:100");
    }

    #[test]
    fn test_stat_format_raw_mode_hex() {
        let st = KernelStat {
            st_mode: S_IFREG | 0o755,
            ..KernelStat::default()
        };
        let result = apply_stat_format("%f", &st, "f", "");
        assert_eq!(result, format!("{:x}", S_IFREG | 0o755));
    }

    #[test]
    fn test_stat_format_device_major_minor() {
        let st = KernelStat {
            st_rdev: (8 << 8) | 1, // major=8, minor=1
            ..KernelStat::default()
        };
        let result = apply_stat_format("%t:%T", &st, "f", "");
        assert_eq!(result, "8:1");
    }

    // ---- Touch date/timestamp parsing ----

    #[test]
    fn test_touch_stamp_mmddhhmm() {
        // 01150830 = Jan 15, 08:30 (assumes year 2026)
        let epoch = parse_touch_stamp("01150830").unwrap();
        let expected = date_to_epoch(2026, 1, 15, 8, 30, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_touch_stamp_yymmddhhmm() {
        // 2501150830 = 2025-01-15 08:30
        let epoch = parse_touch_stamp("2501150830").unwrap();
        let expected = date_to_epoch(2025, 1, 15, 8, 30, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_touch_stamp_ccyymmddhhmm() {
        // 202301150830 = 2023-01-15 08:30
        let epoch = parse_touch_stamp("202301150830").unwrap();
        let expected = date_to_epoch(2023, 1, 15, 8, 30, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_touch_stamp_with_seconds() {
        // 01150830.45 = Jan 15, 08:30:45
        let epoch = parse_touch_stamp("01150830.45").unwrap();
        let expected = date_to_epoch(2026, 1, 15, 8, 30, 45);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_touch_stamp_invalid_month() {
        assert!(parse_touch_stamp("13010830").is_err());
    }

    #[test]
    fn test_touch_stamp_invalid_format() {
        assert!(parse_touch_stamp("abc").is_err());
        assert!(parse_touch_stamp("12").is_err());
    }

    #[test]
    fn test_date_string_full() {
        let epoch = parse_date_string("2024-06-15 14:30:00").unwrap();
        let expected = date_to_epoch(2024, 6, 15, 14, 30, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_date_string_date_only() {
        let epoch = parse_date_string("2024-06-15").unwrap();
        let expected = date_to_epoch(2024, 6, 15, 0, 0, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_date_string_with_t_separator() {
        let epoch = parse_date_string("2024-06-15T14:30:00").unwrap();
        let expected = date_to_epoch(2024, 6, 15, 14, 30, 0);
        assert_eq!(epoch, expected);
    }

    #[test]
    fn test_date_string_invalid() {
        assert!(parse_date_string("not-a-date").is_err());
    }

    // ---- Touch arg parsing ----

    #[test]
    fn test_touch_args_basic() {
        let args = vec!["touch".into(), "file.txt".into()];
        let opts = parse_touch_args(&args).unwrap();
        assert_eq!(opts.files, vec!["file.txt"]);
        assert!(!opts.no_create);
        assert!(!opts.access_only);
        assert!(!opts.modify_only);
    }

    #[test]
    fn test_touch_args_flags() {
        let args = vec!["touch".into(), "-amc".into(), "file.txt".into()];
        let opts = parse_touch_args(&args).unwrap();
        assert!(opts.access_only);
        assert!(opts.modify_only);
        assert!(opts.no_create);
    }

    #[test]
    fn test_touch_args_missing_file() {
        let args = vec!["touch".into()];
        assert!(parse_touch_args(&args).is_err());
    }

    // ---- ln arg parsing ----

    #[test]
    fn test_ln_args_basic() {
        let args = vec!["ln".into(), "target".into(), "link".into()];
        let opts = parse_ln_args(&args).unwrap();
        assert_eq!(opts.targets, vec!["target"]);
        assert_eq!(opts.link_name.as_deref(), Some("link"));
        assert!(!opts.symbolic);
    }

    #[test]
    fn test_ln_args_symbolic() {
        let args = vec!["ln".into(), "-s".into(), "target".into(), "link".into()];
        let opts = parse_ln_args(&args).unwrap();
        assert!(opts.symbolic);
    }

    #[test]
    fn test_ln_args_force_verbose() {
        let args = vec![
            "ln".into(), "-sfv".into(), "target".into(), "link".into(),
        ];
        let opts = parse_ln_args(&args).unwrap();
        assert!(opts.symbolic);
        assert!(opts.force);
        assert!(opts.verbose);
    }

    #[test]
    fn test_ln_args_target_directory() {
        let args = vec![
            "ln".into(), "-t".into(), "/tmp".into(), "file1".into(), "file2".into(),
        ];
        let opts = parse_ln_args(&args).unwrap();
        assert_eq!(opts.target_dir.as_deref(), Some("/tmp"));
        assert_eq!(opts.targets, vec!["file1", "file2"]);
    }

    #[test]
    fn test_ln_args_missing_target() {
        let args = vec!["ln".into()];
        assert!(parse_ln_args(&args).is_err());
    }

    #[test]
    fn test_ln_args_backup() {
        let args = vec!["ln".into(), "-b".into(), "target".into(), "link".into()];
        let opts = parse_ln_args(&args).unwrap();
        assert!(opts.backup);
    }

    #[test]
    fn test_ln_args_no_target_dir() {
        let args = vec![
            "ln".into(), "-T".into(), "target".into(), "link".into(),
        ];
        let opts = parse_ln_args(&args).unwrap();
        assert!(opts.no_target_dir);
    }

    // ---- readlink arg parsing ----

    #[test]
    fn test_readlink_args_basic() {
        let args = vec!["readlink".into(), "mylink".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert_eq!(opts.canon, CanonMode::None);
        assert_eq!(opts.files, vec!["mylink"]);
    }

    #[test]
    fn test_readlink_args_canonicalize() {
        let args = vec!["readlink".into(), "-f".into(), "path".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert_eq!(opts.canon, CanonMode::Canonicalize);
    }

    #[test]
    fn test_readlink_args_canon_existing() {
        let args = vec!["readlink".into(), "-e".into(), "path".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert_eq!(opts.canon, CanonMode::CanonicalizeExisting);
    }

    #[test]
    fn test_readlink_args_canon_missing() {
        let args = vec!["readlink".into(), "-m".into(), "path".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert_eq!(opts.canon, CanonMode::CanonicalizeMissing);
    }

    #[test]
    fn test_readlink_args_no_newline() {
        let args = vec!["readlink".into(), "-n".into(), "link".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert!(opts.no_newline);
    }

    #[test]
    fn test_readlink_args_zero() {
        let args = vec!["readlink".into(), "-z".into(), "link".into()];
        let opts = parse_readlink_args(&args).unwrap();
        assert!(opts.zero_delim);
    }

    // ---- realpath arg parsing ----

    #[test]
    fn test_realpath_args_basic() {
        let args = vec!["realpath".into(), "somefile".into()];
        let opts = parse_realpath_args(&args).unwrap();
        assert_eq!(opts.canon, CanonMode::Canonicalize);
        assert!(!opts.no_symlinks);
    }

    #[test]
    fn test_realpath_args_no_symlinks() {
        let args = vec!["realpath".into(), "-s".into(), "somefile".into()];
        let opts = parse_realpath_args(&args).unwrap();
        assert!(opts.no_symlinks);
    }

    #[test]
    fn test_realpath_args_relative_to() {
        let args = vec![
            "realpath".into(),
            "--relative-to=/home".into(),
            "/home/user/file".into(),
        ];
        let opts = parse_realpath_args(&args).unwrap();
        assert_eq!(opts.relative_to.as_deref(), Some("/home"));
    }

    #[test]
    fn test_realpath_args_relative_base() {
        let args = vec![
            "realpath".into(),
            "--relative-base=/home".into(),
            "/home/user/file".into(),
        ];
        let opts = parse_realpath_args(&args).unwrap();
        assert_eq!(opts.relative_base.as_deref(), Some("/home"));
    }

    // ---- mkfifo arg parsing ----

    #[test]
    fn test_mkfifo_args_basic() {
        let args = vec!["mkfifo".into(), "mypipe".into()];
        let opts = parse_mkfifo_args(&args).unwrap();
        assert_eq!(opts.mode, 0o666);
        assert_eq!(opts.files, vec!["mypipe"]);
    }

    #[test]
    fn test_mkfifo_args_with_mode() {
        let args = vec!["mkfifo".into(), "-m".into(), "644".into(), "pipe".into()];
        let opts = parse_mkfifo_args(&args).unwrap();
        assert_eq!(opts.mode, 0o644);
    }

    #[test]
    fn test_mkfifo_args_mode_long() {
        let args = vec!["mkfifo".into(), "--mode=755".into(), "pipe".into()];
        let opts = parse_mkfifo_args(&args).unwrap();
        assert_eq!(opts.mode, 0o755);
    }

    #[test]
    fn test_mkfifo_missing_operand() {
        let args = vec!["mkfifo".into()];
        assert!(parse_mkfifo_args(&args).is_err());
    }

    // ---- Octal mode parsing ----

    #[test]
    fn test_parse_octal_mode_basic() {
        assert_eq!(parse_octal_mode("644").unwrap(), 0o644);
        assert_eq!(parse_octal_mode("755").unwrap(), 0o755);
        assert_eq!(parse_octal_mode("0777").unwrap(), 0o777);
    }

    #[test]
    fn test_parse_octal_mode_invalid() {
        assert!(parse_octal_mode("999").is_err());
        assert!(parse_octal_mode("abc").is_err());
    }

    // ---- Canonicalization / path helpers ----

    #[test]
    fn test_normalize_path_dots() {
        let p = PathBuf::from("/a/b/../c/./d");
        let result = normalize_path(&p);
        assert_eq!(result, PathBuf::from("/a/c/d"));
    }

    #[test]
    fn test_normalize_path_root() {
        let p = PathBuf::from("/");
        let result = normalize_path(&p);
        assert_eq!(result, PathBuf::from("/"));
    }

    #[test]
    fn test_make_relative_same_dir() {
        assert_eq!(make_relative("/a/b/c", "/a/b/c"), ".");
    }

    #[test]
    fn test_make_relative_child() {
        assert_eq!(make_relative("/a/b/c/d", "/a/b"), "c/d");
    }

    #[test]
    fn test_make_relative_sibling() {
        assert_eq!(make_relative("/a/b/c", "/a/b/d"), "../c");
    }

    #[test]
    fn test_make_relative_divergent() {
        assert_eq!(make_relative("/x/y/z", "/a/b"), "../../x/y/z");
    }

    // ---- Timestamp formatting ----

    #[test]
    fn test_format_timestamp_epoch() {
        let s = format_timestamp(0, 0);
        assert!(s.starts_with("1970-01-01 00:00:00"));
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let s = format_timestamp(1704067200, 0);
        assert!(s.starts_with("2024-01-01 00:00:00"));
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    // ---- date_to_epoch ----

    #[test]
    fn test_date_to_epoch_unix_epoch() {
        assert_eq!(date_to_epoch(1970, 1, 1, 0, 0, 0), 0);
    }

    #[test]
    fn test_date_to_epoch_known() {
        // 2024-01-01 00:00:00 UTC
        let epoch = date_to_epoch(2024, 1, 1, 0, 0, 0);
        assert_eq!(epoch, 1704067200);
    }

    // ---- stat arg parsing ----

    #[test]
    fn test_stat_args_basic() {
        let args = vec!["stat".into(), "file.txt".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.files, vec!["file.txt"]);
        assert!(!opts.filesystem);
        assert!(!opts.dereference);
        assert!(!opts.terse);
    }

    #[test]
    fn test_stat_args_filesystem() {
        let args = vec!["stat".into(), "-f".into(), "/mnt".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert!(opts.filesystem);
    }

    #[test]
    fn test_stat_args_format() {
        let args = vec!["stat".into(), "-c".into(), "%n %s".into(), "f".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.format.as_deref(), Some("%n %s"));
    }

    #[test]
    fn test_stat_args_format_long() {
        let args = vec!["stat".into(), "--format=%i".into(), "f".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.format.as_deref(), Some("%i"));
    }

    #[test]
    fn test_stat_args_terse() {
        let args = vec!["stat".into(), "-t".into(), "file".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert!(opts.terse);
    }

    #[test]
    fn test_stat_args_dereference() {
        let args = vec!["stat".into(), "-L".into(), "link".into()];
        let opts = parse_stat_args(&args).unwrap();
        assert!(opts.dereference);
    }

    #[test]
    fn test_stat_args_missing_file() {
        let args = vec!["stat".into()];
        assert!(parse_stat_args(&args).is_err());
    }

    // ---- fstype_name ----

    #[test]
    fn test_fstype_name_known() {
        assert_eq!(fstype_name(0xEF53), "ext4");
        assert_eq!(fstype_name(0x4d44), "vfat");
        assert_eq!(fstype_name(0x01021994), "tmpfs");
    }

    #[test]
    fn test_fstype_name_unknown() {
        let name = fstype_name(0xDEAD);
        assert!(name.contains("UNKNOWN"));
    }
}
