//! `misc-utils/hardlink.c` of util-linux 2.39.3, function by function and
//! with upstream's names.
//!
//! Files are gathered by walking each operand ([`crate::walk`], glibc's
//! `nftw`) into two trees ([`crate::tree`], glibc's `tsearch`): one keyed by
//! device and inode, which makes a second name of a file one more link of
//! it, and one keyed by device and size, each node of which holds a list of
//! files ordered so its head is the one to keep (the master). Walking that
//! tree in glibc's order (`twalk`'s post-order), each list's master is
//! compared with every file after it -- attributes first, then contents
//! ([`crate::fileeq`]) -- and each equal one replaced by a link to it,
//! made under a temporary name and renamed over the old one.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{File, OpenOptions, Permissions};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;

use getoptlong::{LongVals, Opt, Program, Takes};
use libcall::xattr::{FIEMAP_EXTENT_LAST, FIEMAP_EXTENT_SHARED, FIEMAP_FLAG_SYNC, fiemap_extent};
use quoting::{escape_unprintable, os_bytes};
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{
    SIZE_DECIMAL_2DIGITS, SIZE_SUFFIX_3LETTER, SIZE_SUFFIX_SPACE, err_exclusive_options,
    size_to_human_string,
};

use crate::fileeq::{Data, FileEq};
use crate::tree::{Tree, Visit};
use crate::walk::{Ftw, Kind, nftw};
use libcall::file::{S_IFMT, S_IFREG, Stat};

/// `SIGTERM`, which `handle_interrupt` honours though nothing catches it.
use libcall::SIGTERM;
use libcall::signal::{SIGINT, SIGUSR1};
/// `EACCES`, which a directory `nftw` could not read failed with, and
/// `ENOTSUP` (`EOPNOTSUPP`): a file system that keeps no extended
/// attributes.
use libcall::{EACCES, EOPNOTSUPP as ENOTSUP};

/// `EMLINK`: a file has as many links as it may.
const EMLINK: i32 = 31;

/// `STATFS_BTRFS_MAGIC` and `STATFS_XFS_MAGIC`: the file systems a clone
/// is tried on.
const STATFS_BTRFS_MAGIC: i64 = 0x9123_683E;
const STATFS_XFS_MAGIC: i64 = 0x5846_5342;

/// glibc's `BUFSIZ`: the size of `is_reflink`'s extent maps.
const BUFSIZ: usize = 8192;

/// `last_signal`: the last signal caught, for the loops to see.
static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// `sighandler (i)`: the signal kept, a `SIGINT` not overwritten -- and,
/// for one, a newline written straight to the descriptor.
extern "C" fn sighandler(i: i32) {
    // One atomic step: a SIGINT that lands inside a SIGUSR1's handler, as
    // this one blocks nothing, is not overwritten when it resumes.
    let _ = LAST_SIGNAL.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        (cur != SIGINT).then_some(i)
    });
    if i == SIGINT {
        // "can't use stdio on signal handler": `ignore_result (write
        // (STDOUT_FILENO, "\n", 1))`.
        let _ = libcall::fd::write(1, b"\n");
    }
}

/// `enum log_level`.
const JLOG_SUMMARY: i32 = 0;
const JLOG_INFO: i32 = 1;
const JLOG_VERBOSE1: i32 = 2;
const JLOG_VERBOSE2: i32 = 3;

/// `REFLINK_NEVER`, `REFLINK_AUTO` and `REFLINK_ALWAYS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reflink {
    Never,
    Auto,
    Always,
}

/// What of a file's `struct stat` is looked at.
#[derive(Clone, Copy, Debug, Default)]
struct St {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u64,
    size: u64,
    mtime: i64,
}

impl St {
    fn of(m: &Stat) -> Self {
        St {
            dev: m.st_dev,
            ino: m.st_ino,
            mode: m.st_mode,
            uid: m.st_uid,
            gid: m.st_gid,
            nlink: m.st_nlink,
            size: u64::try_from(m.st_size).unwrap_or(0),
            mtime: m.st_mtime,
        }
    }
}

/// `struct link`: one name of a file -- its path, where its last component
/// starts, and where the directories below the operand start.
#[derive(Debug)]
struct Link {
    path: Vec<u8>,
    basename: i64,
    dirname: i64,
}

/// `struct file`: a file (an inode, or with `--respect-name` an inode and a
/// name), its names -- upstream's list kept backwards, its head `links`'
/// last element, so that adding a name in front, taking the head away and
/// putting one after it are each O(1) -- and the next file of its size.
#[derive(Debug)]
struct FileNode {
    st: St,
    data: Data,
    next: Option<usize>,
    links: Vec<Link>,
}

/// An `--include` or `--exclude` pattern.
struct Pattern {
    /// As given.
    text: Vec<u8>,
    /// Compiled.
    re: ere::Regex,
    /// Whether the warning that it was too costly to decide has been given:
    /// it is given once, not once for every file.
    warned: std::cell::Cell<bool>,
}

impl Pattern {
    fn new(text: Vec<u8>, re: ere::Regex) -> Self {
        Self {
            text,
            re,
            warned: std::cell::Cell::new(false),
        }
    }
}

/// `struct options`.
struct Options {
    /// `--include` and `--exclude`.
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
    method: Vec<u8>,
    verbosity: i32,
    respect_mode: bool,
    respect_owner: bool,
    respect_name: bool,
    respect_dir: bool,
    respect_time: bool,
    respect_xattrs: bool,
    maximise: bool,
    minimise: bool,
    keep_oldest: bool,
    dry_run: bool,
    min_size: u64,
    max_size: u64,
    io_size: usize,
    cache_size: usize,
}

/// `struct statistics`.
struct Statistics {
    started: bool,
    files: usize,
    linked: usize,
    xattr_comparisons: usize,
    comparisons: usize,
    ignored_reflinks: usize,
    saved: f64,
    start_time: Instant,
}

/// The program's state: upstream's globals.
struct Hardlink {
    /// `program_invocation_short_name`.
    short: Vec<u8>,
    out: Stdout,
    quiet: bool,
    rootbasesz: i64,
    reflink_mode: Reflink,
    reflinks_skip: bool,
    opts: Options,
    stats: Statistics,
    /// Every `struct file`, by index; the trees and lists hold indices.
    files: Vec<FileNode>,
    fileeq: Option<FileEq>,
    /// `is_reflink_compatible`'s `static dev_t last_dev` and `static int
    /// last_status`.
    last_dev: u64,
    last_status: bool,
}

/// `CMP (a, b)`.
fn cmp<T: Ord>(a: T, b: T) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// The C string at `offset` of `path`: its bytes from there, or none past
/// its end.
fn tail(path: &[u8], offset: i64) -> &[u8] {
    usize::try_from(offset)
        .ok()
        .and_then(|o| path.get(o..))
        .unwrap_or_default()
}

/// `strncmp (a, b, n)` of two C strings whose bytes are `a` and `b`.
fn strncmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return i32::from(x).wrapping_sub(i32::from(y));
        }
        if x == 0 {
            return 0;
        }
    }
    0
}

/// `filename_strcmp (a, b)`: the last components of the first names.
fn filename_strcmp(a: &FileNode, b: &FileNode) -> i32 {
    match (a.links.last(), b.links.last()) {
        (Some(x), Some(y)) => cmp(tail(&x.path, x.basename), tail(&y.path, y.basename)),
        _ => 0,
    }
}

/// `dirname_strcmp (a, b)`: the directories between the operand and the
/// last component of the first names -- first their lengths, then their
/// bytes. A file named as an operand has a negative length, and then the
/// whole rest of the path is compared, as `strncmp` takes it as a `size_t`.
fn dirname_strcmp(a: &FileNode, b: &FileNode) -> i32 {
    let (Some(x), Some(y)) = (a.links.last(), b.links.last()) else {
        return 0;
    };
    // `int` differences, as upstream's.
    let asz = x.basename.wrapping_sub(x.dirname);
    let bsz = y.basename.wrapping_sub(y.dirname);
    let diff = cmp(asz, bsz);
    if diff != 0 {
        return diff;
    }
    let n = usize::try_from(asz).unwrap_or(usize::MAX);
    strncmp(tail(&x.path, x.dirname), tail(&y.path, y.dirname), n)
}

/// `get_xattr_name_count (names, len)`: how many NUL-ended names the list
/// holds.
fn get_xattr_name_count(names: &[u8]) -> usize {
    let mut count = 0usize;
    let mut at = 0usize;
    while at < names.len() {
        count = count.saturating_add(1);
        let len = names
            .get(at..)
            .unwrap_or_default()
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(names.len().saturating_sub(at));
        at = at.saturating_add(len).saturating_add(1);
    }
    count
}

/// `get_sorted_xattr_name_table (names, n)`: the first `n` names, sorted
/// as `strcmp` orders them.
fn get_sorted_xattr_name_table(names: &[u8], n: usize) -> Vec<&[u8]> {
    let mut table: Vec<&[u8]> = names.split(|&b| b == 0).take(n).collect();
    table.sort_unstable();
    table
}

/// `size_to_human_string (SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE |
/// SIZE_DECIMAL_2DIGITS, bytes)`.
fn human(bytes: u64) -> String {
    size_to_human_string(
        SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE | SIZE_DECIMAL_2DIGITS,
        bytes,
    )
}

/// `stats.saved`, a `double`, as the `uint64_t` `size_to_human_string`
/// takes: C's conversion, which drops the fraction (there is none: only
/// sizes are added).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "upstream's implicit double-to-uint64_t conversion; the sum is of sizes, never negative"
)]
fn saved_bytes(saved: f64) -> u64 {
    saved as u64
}

/// `stats.saved += a->st.st_size`, in a `double`.
#[allow(
    clippy::cast_precision_loss,
    reason = "upstream keeps the sum in a double, exact to 2^53 bytes"
)]
fn add_saved(saved: f64, size: u64) -> f64 {
    saved + size as f64
}

/// A path (or an attribute's name) for a system call: its bytes up to the
/// first NUL, which ends it in C too. None of hardlink's has one -- they
/// come from argv, `realpath`, `readdir` and `llistxattr`, all C strings.
fn c_path(path: &[u8]) -> CString {
    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    let bytes = path.get(..end).unwrap_or_default();
    // No NUL is left in `bytes`, so the empty fallback is never taken.
    CString::new(bytes).unwrap_or_else(|_| CString::default())
}

/// Bytes as a path.
fn as_path(path: &[u8]) -> &std::path::Path {
    std::path::Path::new(OsStr::from_bytes(path))
}

/// The `errno` behind an I/O error.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(libcall::EINVAL)
}

/// The usage text.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options] <directory>|<file> ...\n\
\nConsolidate duplicate files using hardlinks.\n\
\nOptions:\n\
\x20-c, --content              compare only file contents, same as -pot\n\
\x20-b, --io-size <size>       I/O buffer size for file reading\n\
\x20                             (speedup, using more RAM)\n\
\x20-d, --respect-dir          directory names have to be identical\n\
\x20-f, --respect-name         filenames have to be identical\n\
\x20-i, --include <regex>      regular expression to include files/dirs\n\
\x20-m, --maximize             maximize the hardlink count, remove the file with\n\
\x20                             lowest hardlink count\n\
\x20-M, --minimize             reverse the meaning of -m\n\
\x20-n, --dry-run              don't actually link anything\n\
\x20-o, --ignore-owner         ignore owner changes\n\
\x20-O, --keep-oldest          keep the oldest file of multiple equal files\n\
\x20                             (lower precedence than minimize/maximize)\n\
\x20-p, --ignore-mode          ignore changes of file mode\n\
\x20-q, --quiet                quiet mode - don't print anything\n\
\x20-r, --cache-size <size>    memory limit for cached file content data\n\
\x20-s, --minimum-size <size>  minimum size for files.\n\
\x20-S, --maximum-size <size>  maximum size for files.\n\
\x20-t, --ignore-time          ignore timestamps (when testing for equality)\n\
\x20-v, --verbose              verbose output (repeat for more verbosity)\n\
\x20-x, --exclude <regex>      regular expression to exclude files\n\
\x20-X, --respect-xattrs       respect extended attributes\n\
\x20-y, --method <name>        file content comparison method\n\
\x20    --reflink[=<when>]     create clone/CoW copies (auto, always, never)\n\
\x20    --skip-reflinks        skip already cloned files (enabled on --reflink)\n\
\n",
    );
    text.extend_from_slice(format!("{:<28}{}\n", " -h, --help", "display this help").as_bytes());
    text.extend_from_slice(format!("{:<28}{}\n", " -V, --version", "display version").as_bytes());
    text.extend_from_slice(b"\nFor more details see hardlink(1).\n");
    text
}

/// Getopt's errors are only sentences here; the referral follows them.
const HARDLINK: Program = Program::new("hardlink", 1);

/// Upstream's option string.
const SHORTS: &str = "VhvndfpotXcmMOx:y:i:r:S:s:b:q";

/// `OPT_REFLINK` and `OPT_SKIP_RELINKS`: `CHAR_MAX + 1` and on.
const OPT_REFLINK: i32 = 128;
const OPT_SKIP_RELINKS: i32 = 129;

/// Upstream's `long_options[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("dry-run", Takes::Nothing),
    ("respect-name", Takes::Nothing),
    ("respect-dir", Takes::Nothing),
    ("ignore-mode", Takes::Nothing),
    ("ignore-owner", Takes::Nothing),
    ("ignore-time", Takes::Nothing),
    ("respect-xattrs", Takes::Nothing),
    ("maximize", Takes::Nothing),
    ("minimize", Takes::Nothing),
    ("keep-oldest", Takes::Nothing),
    ("exclude", Takes::Required),
    ("include", Takes::Required),
    ("method", Takes::Required),
    ("minimum-size", Takes::Required),
    ("maximum-size", Takes::Required),
    ("reflink", Takes::Optional),
    ("skip-reflinks", Takes::Nothing),
    ("io-size", Takes::Required),
    ("content", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("cache-size", Takes::Required),
];
const LONG_VALS: [i32; 24] = [
    b'V' as i32,
    b'h' as i32,
    b'v' as i32,
    b'n' as i32,
    b'f' as i32,
    b'd' as i32,
    b'p' as i32,
    b'o' as i32,
    b't' as i32,
    b'X' as i32,
    b'm' as i32,
    b'M' as i32,
    b'O' as i32,
    b'x' as i32,
    b'i' as i32,
    b'y' as i32,
    b's' as i32,
    b'S' as i32,
    OPT_REFLINK,
    OPT_SKIP_RELINKS,
    b'b' as i32,
    b'c' as i32,
    b'q' as i32,
    b'r' as i32,
];

/// [`LONGS`] and [`LONG_VALS`], upstream's `struct option[]`, as one table.
const OPTIONS: LongVals<'static, i32> = LongVals::new(LONGS, &LONG_VALS);

/// `excl[]`.
const EXCL: [&[i32]; 1] = [&[b'q' as i32, b'v' as i32]];

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Whether the locale `setlocale (LC_ALL, "")` selects reads text as
/// UTF-8, as `regcomp` and `regexec` then do.
fn select_locale() -> bool {
    let mut name = [0u8; 256];
    // A locale that does not exist leaves "C", whose codeset is ASCII.
    let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut name);
    let mut codeset = [0u8; 64];
    let n = libcall::locale::codeset(&mut codeset).unwrap_or(0);
    codeset.get(..n) == Some(b"UTF-8".as_slice())
}

/// `main (argc, argv)`: the status to exit with, everything printed but
/// what stdout still holds, which [`Stdout::flush_at_exit`] then writes.
pub fn main(argv: &[OsString]) -> u8 {
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("hardlink"), OsString::as_os_str),
    );
    // `sigaction (SIGINT, &sa, NULL)` and `SIGUSR1`, `SA_RESTART`. Upstream
    // blocks every signal while the handler runs (`sigfillset`); this one
    // blocks none, and keeps a SIGINT by updating the signal atomically.
    // Upstream ignores a failure, which none of these can have.
    let _ = libcall::signal::set_handler(SIGINT, sighandler, true);
    let _ = libcall::signal::set_handler(SIGUSR1, sighandler, true);
    let utf8 = select_locale();
    let mut h = Hardlink {
        short,
        out: Stdout::new(1),
        quiet: false,
        rootbasesz: 0,
        reflink_mode: Reflink::Never,
        reflinks_skip: false,
        opts: Options {
            include: Vec::new(),
            exclude: Vec::new(),
            method: b"sha256".to_vec(),
            verbosity: 0,
            respect_mode: true,
            respect_owner: true,
            respect_name: false,
            respect_dir: false,
            respect_time: true,
            respect_xattrs: false,
            maximise: false,
            minimise: false,
            keep_oldest: false,
            dry_run: false,
            min_size: 1,
            max_size: 0,
            io_size: 0,
            cache_size: 10 * 1024 * 1024,
        },
        stats: Statistics {
            started: false,
            files: 0,
            linked: 0,
            xattr_comparisons: 0,
            comparisons: 0,
            ignored_reflinks: 0,
            saved: 0.0,
            start_time: Instant::now(),
        },
        files: Vec::new(),
        fileeq: None,
        last_dev: 0,
        last_status: false,
    };
    let status = h.run(argv, utf8);
    // `to_be_called_atexit`, then exit's flush, whose failure nobody hears
    // of: hardlink registers no `close_stdout`.
    if h.stats.started {
        h.print_stats();
    }
    h.out.flush_at_exit();
    status
}

impl Hardlink {
    /// `is_log_enabled (level)`.
    fn is_log_enabled(&self, level: i32) -> bool {
        !self.quiet && level <= self.opts.verbosity
    }

    /// `jlog (level, ...)` for a line that costs something to make: made
    /// only when it will be printed, as upstream's `vfprintf` is reached
    /// only then.
    fn jlog_with(&mut self, level: i32, line: impl FnOnce(&Self) -> Vec<u8>) {
        if self.is_log_enabled(level) {
            let line = line(self);
            self.jlog(level, &line);
        }
    }

    /// `jlog (level, ...)`: a line to stdout, at `level` or more verbose.
    fn jlog(&mut self, level: i32, line: &[u8]) {
        if !self.is_log_enabled(level) {
            return;
        }
        self.out.write(line);
        self.out.write(b"\n");
    }

    /// `warn (msg)` with `errno` `e`.
    fn warn(&self, msg: &str, e: i32) {
        warn(&self.short, msg, &std::io::Error::from_raw_os_error(e));
    }

    /// `errtryhelp (EXIT_FAILURE)`.
    fn errtryhelp(&self) -> u8 {
        stderr_write(
            format!(
                "Try '{} --help' for more information.\n",
                escape_unprintable(&self.short)
            )
            .as_bytes(),
        );
        1
    }

    /// `print_stats ()`.
    fn print_stats(&mut self) {
        let delta = self.stats.start_time.elapsed();
        let mode = if self.opts.dry_run { "dry-run" } else { "real" };
        self.jlog(JLOG_SUMMARY, format!("{:<25} {mode}", "Mode:").as_bytes());
        let mut line = format!("{:<25} ", "Method:").into_bytes();
        line.extend_from_slice(&self.opts.method);
        self.jlog(JLOG_SUMMARY, &line);
        let s = &self.stats;
        let lines = [
            format!("{:<25} {}", "Files:", s.files),
            format!("{:<25} {} files", "Linked:", s.linked),
            format!("{:<25} {} xattrs", "Compared:", s.xattr_comparisons),
            format!("{:<25} {} files", "Compared:", s.comparisons),
        ];
        let skipped = format!("{:<25} {} files", "Skipped reflinks:", s.ignored_reflinks);
        let saved = format!("{:<25} {}", "Saved:", human(saved_bytes(s.saved)));
        let duration = format!(
            "{:<25} {}.{:06} seconds",
            "Duration:",
            delta.as_secs(),
            delta.subsec_micros()
        );
        for l in lines {
            self.jlog(JLOG_SUMMARY, l.as_bytes());
        }
        if self.reflinks_skip {
            self.jlog(JLOG_SUMMARY, skipped.as_bytes());
        }
        self.jlog(JLOG_SUMMARY, saved.as_bytes());
        self.jlog(JLOG_SUMMARY, duration.as_bytes());
    }

    /// `handle_interrupt ()`: whether to stop -- after a `SIGINT`, which
    /// stays; a `SIGUSR1` prints the statistics, and is forgotten.
    fn handle_interrupt(&mut self) -> bool {
        let seen = LAST_SIGNAL.load(Ordering::Relaxed);
        match seen {
            SIGINT | SIGTERM => return true,
            SIGUSR1 => {
                self.print_stats();
                self.out.write(b"\n");
            }
            _ => {}
        }
        // Only the signal handled is forgotten. Upstream stores 0 whatever
        // came in meanwhile, so a SIGINT caught while the statistics were
        // being written (to a slow pipe, say) is lost and the run goes on;
        // here it stays and stops the run at the next check. A failed
        // exchange means exactly that: another signal is waiting.
        let _ = LAST_SIGNAL.compare_exchange(seen, 0, Ordering::Relaxed, Ordering::Relaxed);
        false
    }

    /// `compare_nodes (a, b)`: the device, then the size.
    fn compare_nodes(&self, a: usize, b: usize) -> i32 {
        let (Some(a), Some(b)) = (self.files.get(a), self.files.get(b)) else {
            return 0;
        };
        let mut diff = cmp(a.st.dev, b.st.dev);
        if diff == 0 {
            diff = cmp(a.st.size, b.st.size);
        }
        diff
    }

    /// `compare_nodes_ino (a, b)`: the device and inode -- and the name or
    /// the directories, when they must be the same to link.
    fn compare_nodes_ino(&self, a: usize, b: usize) -> i32 {
        let (Some(a), Some(b)) = (self.files.get(a), self.files.get(b)) else {
            return 0;
        };
        let mut diff = cmp(a.st.dev, b.st.dev);
        if diff == 0 {
            diff = cmp(a.st.ino, b.st.ino);
        }
        // "If opts.respect_name is used, we will restrict a struct file to
        // contain only links with the same basename to keep the rest
        // simple."
        if diff == 0 && self.opts.respect_name {
            diff = filename_strcmp(a, b);
        }
        if diff == 0 && self.opts.respect_dir {
            diff = dirname_strcmp(a, b);
        }
        diff
    }

    /// The first name of file `i`.
    fn path(&self, i: usize) -> &[u8] {
        self.files
            .get(i)
            .and_then(|f| f.links.last())
            .map_or(&[][..], |l| &l.path)
    }

    /// `llistxattr_or_die (path, list, size)`: the length, -1 where the file
    /// system keeps no attributes, or the status to exit with after any
    /// other failure.
    fn llistxattr_or_die(&self, path: &[u8], list: Option<&mut [u8]>) -> Result<i64, u8> {
        let c = c_path(path);
        match libcall::xattr::llistxattr(&c, list) {
            Ok(n) => Ok(i64::try_from(n).unwrap_or(i64::MAX)),
            Err(e) if e == ENOTSUP => Ok(-1),
            Err(e) => {
                self.warn(
                    &format!("cannot get xattr names for {}", escape_unprintable(path)),
                    e,
                );
                Err(1)
            }
        }
    }

    /// `lgetxattr_or_die (path, name, value, size)`: the length, or the
    /// status to exit with after a failure.
    fn lgetxattr_or_die(
        &self,
        path: &[u8],
        name: &[u8],
        value: Option<&mut [u8]>,
    ) -> Result<usize, u8> {
        let c = c_path(path);
        let n = c_path(name);
        libcall::xattr::lgetxattr(&c, &n, value).map_err(|e| {
            self.warn(
                &format!(
                    "cannot get xattr value of {} for {}",
                    escape_unprintable(name),
                    escape_unprintable(path)
                ),
                e,
            );
            1
        })
    }

    /// `file_xattrs_equal (a, b)`: whether the two files have the same
    /// extended attributes; the status to exit with when one cannot be
    /// read.
    fn file_xattrs_equal(&mut self, a: usize, b: usize) -> Result<bool, u8> {
        let pa = self.path(a).to_vec();
        let pb = self.path(b).to_vec();
        let mut line = b"Comparing xattrs of ".to_vec();
        line.extend_from_slice(&pa);
        line.extend_from_slice(b" to ");
        line.extend_from_slice(&pb);
        self.jlog(JLOG_VERBOSE1, &line);
        self.stats.xattr_comparisons = self.stats.xattr_comparisons.saturating_add(1);

        let len_a = self.llistxattr_or_die(&pa, None)?;
        let len_b = self.llistxattr_or_die(&pb, None)?;
        if len_a <= 0 && len_b <= 0 {
            // "xattrs not supported or neither file has any"
            return Ok(true);
        }
        if len_a != len_b {
            // "total lengths of xattr names differ"
            return Ok(false);
        }
        let size = usize::try_from(len_a).unwrap_or(0);
        let mut names_a = vec![0u8; size];
        let mut names_b = vec![0u8; size];
        let len_a = self.llistxattr_or_die(&pa, Some(&mut names_a))?;
        let len_b = self.llistxattr_or_die(&pb, Some(&mut names_b))?;
        // Upstream asserts these, which only a change made since the first
        // call can break; such a list is not taken for the same.
        if len_a <= 0 || len_a != len_b {
            return Ok(false);
        }
        let len = usize::try_from(len_a).unwrap_or(0);
        names_a.truncate(len);
        names_b.truncate(len);
        let n_a = get_xattr_name_count(&names_a);
        let n_b = get_xattr_name_count(&names_b);
        if n_a != n_b {
            // "numbers of xattrs differ"
            return Ok(false);
        }
        let table_a = get_sorted_xattr_name_table(&names_a, n_a);
        let table_b = get_sorted_xattr_name_table(&names_b, n_b);
        // "We now have two sorted tables of xattr names."
        for (name_a, name_b) in table_a.iter().zip(&table_b) {
            if self.handle_interrupt() {
                // "user wants to quit"
                return Ok(false);
            }
            if name_a != name_b {
                // "names at same slot differ"
                return Ok(false);
            }
            let len_a = self.lgetxattr_or_die(&pa, name_a, None)?;
            let len_b = self.lgetxattr_or_die(&pb, name_b, None)?;
            if len_a != len_b {
                // "xattrs with same name, different value lengths"
                return Ok(false);
            }
            let mut value_a = vec![0u8; len_a];
            let mut value_b = vec![0u8; len_b];
            let len_a = self.lgetxattr_or_die(&pa, name_a, Some(&mut value_a))?;
            let len_b = self.lgetxattr_or_die(&pb, name_b, Some(&mut value_b))?;
            // Asserted upstream, as the lists above.
            if len_a != len_b || value_a.get(..len_a) != value_b.get(..len_b) {
                // "xattrs with same name, different values"
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `file_may_link_to (a, b)`: whether `b` may be replaced by a link to
    /// `a` -- everything but their contents; the status to exit with when
    /// their attributes cannot be read.
    fn file_may_link_to(&mut self, a: usize, b: usize) -> Result<bool, u8> {
        let (Some(fa), Some(fb)) = (self.files.get(a), self.files.get(b)) else {
            return Ok(false);
        };
        let (sa, sb) = (fa.st, fb.st);
        let o = &self.opts;
        let may = sa.size != 0
            && sa.size == sb.size
            && !fa.links.is_empty()
            && !fb.links.is_empty()
            && sa.dev == sb.dev
            && sa.ino != sb.ino
            && (!o.respect_mode || sa.mode == sb.mode)
            && (!o.respect_owner || sa.uid == sb.uid)
            && (!o.respect_owner || sa.gid == sb.gid)
            && (!o.respect_time || sa.mtime == sb.mtime)
            && (!o.respect_name || filename_strcmp(fa, fb) == 0)
            && (!o.respect_dir || dirname_strcmp(fa, fb) == 0);
        if !may {
            return Ok(false);
        }
        if self.opts.respect_xattrs {
            return self.file_xattrs_equal(a, b);
        }
        Ok(true)
    }

    /// `file_compare (a, b)`: which of two files to keep -- the greater.
    fn file_compare(&self, a: usize, b: usize) -> i32 {
        let (Some(a), Some(b)) = (self.files.get(a), self.files.get(b)) else {
            return 0;
        };
        let (a, b) = (a.st, b.st);
        if a.dev == b.dev && a.ino == b.ino {
            return 0;
        }
        let mut res = 0;
        if res == 0 && self.opts.maximise {
            res = cmp(a.nlink, b.nlink);
        }
        if res == 0 && self.opts.minimise {
            res = cmp(b.nlink, a.nlink);
        }
        if res == 0 {
            res = if self.opts.keep_oldest {
                cmp(b.mtime, a.mtime)
            } else {
                cmp(a.mtime, b.mtime)
            };
        }
        if res == 0 {
            res = cmp(b.ino, a.ino);
        }
        res
    }

    /// The clone half of `do_link`: `new_name` made a copy of `a` sharing
    /// its extents, with `b`'s mode and owner; the `errno` of the step that
    /// failed, after what it had made was removed.
    fn reflink(&self, a: usize, b: usize, new_name: &[u8]) -> Result<(), i32> {
        let sb = self.files.get(b).map(|f| f.st).unwrap_or_default();
        let dest = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(as_path(new_name))
            .map_err(|e| errno_of(&e))?;
        let cloned = (|| {
            dest.set_permissions(Permissions::from_mode(sb.mode))?;
            std::os::unix::fs::fchown(&dest, Some(sb.uid), Some(sb.gid))?;
            let src = File::open(as_path(self.path(a)))?;
            libcall::xattr::ficlone(dest.as_raw_fd(), src.as_raw_fd())
                .map_err(std::io::Error::from_raw_os_error)
        })();
        let Err(e) = cloned else {
            return Ok(());
        };
        let mut errno = errno_of(&e);
        drop(dest);
        // "fallback": what was made goes; `errno` is the unlink's if that
        // fails too.
        if let Err(u) = std::fs::remove_file(as_path(new_name)) {
            errno = errno_of(&u);
        }
        Err(errno)
    }

    /// `do_link (a, b, new_name, reflink)`: `new_name` made another name of
    /// `a` -- or with `reflink` a clone of it, falling back to a link unless
    /// `--reflink=always` -- or the `errno` it failed with.
    fn do_link(&mut self, a: usize, b: usize, new_name: &[u8], reflink: bool) -> Result<(), i32> {
        if reflink {
            match self.reflink(a, b, new_name) {
                Ok(()) => return Ok(()),
                Err(e) if self.reflink_mode == Reflink::Always => return Err(e),
                Err(_) => self.jlog(JLOG_VERBOSE2, b"Reflinking failed, fallback to hardlinking"),
            }
        }
        std::fs::hard_link(as_path(self.path(a)), as_path(new_name)).map_err(|e| errno_of(&e))
    }

    /// `file_link (a, b, reflink)`: every name of `b` replaced by a link to
    /// `a`, one by one -- or the `errno` of the first that could not be,
    /// after a warning.
    fn file_link(&mut self, a: usize, b: usize, reflink: bool) -> Result<(), i32> {
        loop {
            if self.is_log_enabled(JLOG_INFO) {
                let size = self.files.get(a).map_or(0, |f| f.st.size);
                let mut line = Vec::new();
                if self.opts.dry_run {
                    line.extend_from_slice(b"[DryRun] ");
                }
                if reflink {
                    line.extend_from_slice(b"Ref");
                }
                line.extend_from_slice(b"Linking ");
                line.extend_from_slice(self.path(a));
                line.extend_from_slice(b" to ");
                line.extend_from_slice(self.path(b));
                line.extend_from_slice(format!(" (-{})", human(size)).as_bytes());
                self.jlog(JLOG_INFO, &line);
            }

            if !self.opts.dry_run {
                let mut new_path = self.path(b).to_vec();
                new_path.extend_from_slice(b".hardlink-temporary");
                if let Err(e) = self.do_link(a, b, &new_path, reflink) {
                    self.warn(
                        &format!(
                            "cannot link {} to {}",
                            escape_unprintable(self.path(a)),
                            escape_unprintable(&new_path)
                        ),
                        e,
                    );
                    return Err(e);
                }
                if let Err(e) = std::fs::rename(as_path(&new_path), as_path(self.path(b))) {
                    let mut errno = errno_of(&e);
                    // Upstream names the file linked to and the temporary
                    // name here, not the file renamed over.
                    self.warn(
                        &format!(
                            "cannot rename {} to {}",
                            escape_unprintable(self.path(a)),
                            escape_unprintable(&new_path)
                        ),
                        errno,
                    );
                    if let Err(u) = std::fs::remove_file(as_path(&new_path)) {
                        errno = errno_of(&u);
                    }
                    return Err(errno);
                }
            }

            // "Update statistics"
            self.stats.linked = self.stats.linked.saturating_add(1);
            let size = self.files.get(a).map_or(0, |f| f.st.size);
            // "Increase the link count of this file, and set stat() of other
            // file": `nlink_t`, unsigned.
            if let Some(fa) = self.files.get_mut(a) {
                fa.st.nlink = fa.st.nlink.wrapping_add(1);
            }
            let mut emptied = false;
            let mut moved = None;
            if let Some(fb) = self.files.get_mut(b) {
                fb.st.nlink = fb.st.nlink.wrapping_sub(1);
                emptied = fb.st.nlink == 0;
                // "Move the link from file b to a": b's head...
                moved = fb.links.pop();
            }
            if emptied {
                self.stats.saved = add_saved(self.stats.saved, size);
            }
            // ...put just after a's head.
            if let (Some(link), Some(fa)) = (moved, self.files.get_mut(a)) {
                let at = fa.links.len().saturating_sub(1);
                fa.links.insert(at, link);
            }
            // "Do it again"
            if self.files.get(b).is_none_or(|f| f.links.is_empty()) {
                return Ok(());
            }
        }
    }

    /// `match_any_regex (pregs, what)`: whether a pattern of `list` matches
    /// `what`. `ere` gives up a backreference search past its budget, where
    /// glibc's `regexec` goes on however long it takes: such a pattern is
    /// taken to match when `if_undecided` -- `true` for `--exclude`, `false`
    /// for `--include` -- so that a file no pattern could be decided for is
    /// left alone, never linked. A warning says so for the first such file
    /// of each pattern; a pattern too costly for one name usually is for
    /// thousands, and a line for each would bury every other message.
    fn match_any_regex(&self, list: &[Pattern], what: &[u8], if_undecided: bool) -> bool {
        list.iter().any(|pat| match pat.re.is_match(what) {
            Ok(matched) => matched,
            Err(_) => {
                if !pat.warned.replace(true) {
                    warnx(
                        &self.short,
                        &format!(
                            "{}: too costly to match {}; taken as {}, as it will be for every other name it is \
                             too costly to match",
                            escape_unprintable(&pat.text),
                            escape_unprintable(what),
                            if if_undecided { "a match" } else { "no match" }
                        ),
                    );
                }
                if_undecided
            }
        })
    }

    /// `inserter (fpath, sb, typeflag, ftwbuf)`: a file the walk found,
    /// added to the trees. Non-zero stops the walk.
    fn inserter(
        &mut self,
        by_ino: &mut Tree<usize>,
        by_size: &mut Tree<usize>,
        fpath: &[u8],
        sb: Option<&Stat>,
        typeflag: Kind,
        ftw: Ftw,
    ) -> i32 {
        if self.handle_interrupt() {
            return 1;
        }
        match typeflag {
            Kind::DirNotRead => self.warn(
                &format!("cannot read {}", escape_unprintable(fpath)),
                EACCES,
            ),
            Kind::NoStat(e) => self.warn(&format!("cannot read {}", escape_unprintable(fpath)), e),
            _ => {}
        }
        let Some(sb) = sb.filter(|s| typeflag == Kind::File && s.st_mode & S_IFMT == S_IFREG)
        else {
            return 0;
        };

        let included = self.match_any_regex(&self.opts.include, fpath, false);
        let excluded = self.match_any_regex(&self.opts.exclude, fpath, true);
        let (has_exclude, has_include) =
            (!self.opts.exclude.is_empty(), !self.opts.include.is_empty());
        // Upstream's `(exclude && excluded && !included) || (!exclude &&
        // include && !included)`.
        if !included && ((has_exclude && excluded) || (!has_exclude && has_include)) {
            return 0;
        }

        self.stats.files = self.stats.files.saturating_add(1);
        let st = St::of(sb);

        if st.size < self.opts.min_size {
            self.jlog_with(JLOG_VERBOSE1, |_| {
                [&b"Skipped "[..], fpath, b" (smaller than configured size)"].concat()
            });
            return 0;
        }

        self.jlog_with(JLOG_VERBOSE2, |h| {
            let mut line = format!(
                " {:5}: [{}/{}/{}] ",
                h.stats.files, st.dev, st.ino, st.nlink
            )
            .into_bytes();
            line.extend_from_slice(fpath);
            line
        });

        if self.opts.max_size > 0 && st.size > self.opts.max_size {
            self.jlog_with(JLOG_VERBOSE1, |_| {
                [&b"Skipped "[..], fpath, b" (greater than configured size)"].concat()
            });
            return 0;
        }

        let fil = self.files.len();
        self.files.push(FileNode {
            st,
            data: Data::default(),
            next: None,
            links: vec![Link {
                path: fpath.to_vec(),
                basename: i64::try_from(ftw.base).unwrap_or(i64::MAX),
                dirname: self.rootbasesz,
            }],
        });

        let (node, inserted) = by_ino.search(fil, |&x, &y| self.compare_nodes_ino(x, y).cmp(&0));
        if !inserted {
            // "Already known inode, add link to inode information"
            let known = by_ino.key(node).copied().unwrap_or(fil);
            let link = self.files.pop().and_then(|mut f| f.links.pop());
            let seen = self
                .files
                .get(known)
                .is_some_and(|k| k.links.iter().any(|l| l.path == fpath));
            if seen {
                self.jlog_with(JLOG_VERBOSE1, |_| {
                    [&b"Skipped "[..], fpath, b" (specified more than once)"].concat()
                });
            } else if let (Some(link), Some(k)) = (link, self.files.get_mut(known)) {
                // The new head.
                k.links.push(link);
            }
            return 0;
        }

        // "New inode, insert into by-size table"
        let (node, inserted) = by_size.search(fil, |&x, &y| self.compare_nodes(x, y).cmp(&0));
        if !inserted {
            let head = by_size.key(node).copied().unwrap_or(fil);
            if self.file_compare(fil, head) >= 0 {
                if let Some(f) = self.files.get_mut(fil) {
                    f.next = Some(head);
                }
                if let Some(k) = by_size.key_mut(node) {
                    *k = fil;
                }
            } else {
                let mut l = head;
                loop {
                    let next = self.files.get(l).and_then(|f| f.next);
                    if let Some(n) = next
                        && self.file_compare(fil, n) < 0
                    {
                        l = n;
                        continue;
                    }
                    if let Some(f) = self.files.get_mut(fil) {
                        f.next = next;
                    }
                    if let Some(f) = self.files.get_mut(l) {
                        f.next = Some(fil);
                    }
                    break;
                }
            }
        }
        0
    }

    /// `is_reflink_compatible (devno, filename)`: whether the file system
    /// can clone -- btrfs and XFS -- asked once per device in a row.
    fn is_reflink_compatible(&mut self, devno: u64, filename: &[u8]) -> bool {
        if self.last_dev != devno {
            let Ok(f_type) = libcall::xattr::statfs_type(&c_path(filename)) else {
                return false;
            };
            self.last_dev = devno;
            self.last_status = matches!(f_type, STATFS_BTRFS_MAGIC | STATFS_XFS_MAGIC);
        }
        self.last_status
    }

    /// `is_reflink (xa, xb)`: whether the two files already share every
    /// extent, mapped `BUFSIZ` bytes of extents at a time.
    fn is_reflink(&self, xa: usize, xb: usize) -> bool {
        let (Ok(af), Ok(bf)) = (
            File::open(as_path(self.path(xa))),
            File::open(as_path(self.path(xb))),
        ) else {
            return false;
        };
        let mut amap = vec![0u64; BUFSIZ / 8];
        let mut bmap = vec![0u64; BUFSIZ / 8];
        let mut start = 0u64;
        loop {
            let Ok(na) = libcall::xattr::fiemap(af.as_raw_fd(), start, FIEMAP_FLAG_SYNC, &mut amap)
            else {
                return false;
            };
            let Ok(nb) = libcall::xattr::fiemap(bf.as_raw_fd(), start, FIEMAP_FLAG_SYNC, &mut bmap)
            else {
                return false;
            };
            if na != nb {
                return false;
            }
            let mut last = false;
            let mut end = None;
            for i in 0..na {
                let (Some(a), Some(b)) = (fiemap_extent(&amap, i), fiemap_extent(&bmap, i)) else {
                    return false;
                };
                if a.logical != b.logical || a.length != b.length || a.physical != b.physical {
                    return false;
                }
                if a.flags & FIEMAP_EXTENT_SHARED == 0 || b.flags & FIEMAP_EXTENT_SHARED == 0 {
                    return false;
                }
                if a.flags & FIEMAP_EXTENT_LAST != 0 {
                    last = true;
                }
                end = Some(a.logical.wrapping_add(a.length));
            }
            // No extent at all: upstream reads the one before its array and
            // carries on from wherever that says; here it is no clone.
            let Some(end) = end else {
                return false;
            };
            start = end;
            if last {
                return true;
            }
        }
    }

    /// `count_nodes (x)`: the files from `x` to the end of its list.
    fn count_nodes(&self, x: usize) -> usize {
        let mut ct = 0usize;
        let mut at = Some(x);
        while let Some(i) = at {
            ct = ct.saturating_add(1);
            at = self.files.get(i).and_then(|f| f.next);
        }
        ct
    }

    /// The next file of `i`'s size.
    fn next_of(&self, i: usize) -> Option<usize> {
        self.files.get(i).and_then(|f| f.next)
    }

    /// Whether file `i` has no name left: all were made links to another.
    fn unnamed(&self, i: usize) -> bool {
        self.files.get(i).is_none_or(|f| f.links.is_empty())
    }

    /// `ul_fileeq (&fileeq, &a->data, &b->data)`.
    fn contents_equal(&mut self, a: usize, b: usize) -> bool {
        let Some(eq) = self.fileeq.as_mut() else {
            return false;
        };
        let mut da = self
            .files
            .get_mut(a)
            .map(|f| std::mem::take(&mut f.data))
            .unwrap_or_default();
        let mut db = self
            .files
            .get_mut(b)
            .map(|f| std::mem::take(&mut f.data))
            .unwrap_or_default();
        let same = eq.equal(&mut da, &mut db);
        if let Some(f) = self.files.get_mut(a) {
            f.data = da;
        }
        if let Some(f) = self.files.get_mut(b) {
            f.data = db;
        }
        same
    }

    /// `visitor (nodep, which, depth)` for a leaf or a node after its
    /// subtrees: the list of one size, each file compared with those after
    /// it. The status to exit with when it must stop.
    fn visitor(&mut self, begin: usize) -> Result<(), u8> {
        let mut master_at = Some(begin);
        while let Some(start) = master_at {
            let mut master = start;
            if self.handle_interrupt() {
                return Err(1);
            }
            if self.unnamed(master) {
                master_at = self.next_of(master);
                continue;
            }
            // "calculate per file max memory use"
            let nnodes = self.count_nodes(master);
            // "per-file cache size"
            let memsiz = self.opts.cache_size.checked_div(nnodes).unwrap_or(0);
            let (size, dev) = self
                .files
                .get(master)
                .map_or((0, 0), |f| (f.st.size, f.st.dev));
            let io_size = self.opts.io_size;
            if let Some(eq) = self.fileeq.as_mut() {
                eq.set_size(size, io_size, memsiz);
            }
            let mut may_reflink = false;
            if self.reflink_mode != Reflink::Never || self.reflinks_skip {
                may_reflink = self.reflink_mode == Reflink::Always || {
                    let path = self.path(master).to_vec();
                    self.is_reflink_compatible(dev, &path)
                };
            }
            let mut other_at = self.next_of(master);
            while let Some(other) = other_at {
                other_at = self.next_of(other);
                if self.handle_interrupt() {
                    return Err(1);
                }
                if self.unnamed(other) {
                    continue;
                }
                // "check file attributes, etc."
                if !self.file_may_link_to(master, other)? {
                    self.jlog_with(JLOG_VERBOSE2, |h| {
                        [&b"Skipped (attributes mismatch) "[..], h.path(other)].concat()
                    });
                    continue;
                }
                if may_reflink && self.reflinks_skip && self.is_reflink(master, other) {
                    self.jlog_with(JLOG_VERBOSE2, |h| {
                        [&b"Skipped (already reflink) "[..], h.path(other)].concat()
                    });
                    self.stats.ignored_reflinks = self.stats.ignored_reflinks.saturating_add(1);
                    continue;
                }
                // "initialize content comparison"
                for i in [master, other] {
                    if self.files.get(i).is_some_and(|f| !f.data.associated()) {
                        let name = self.path(i).to_vec();
                        if let Some(f) = self.files.get_mut(i) {
                            f.data.set_file(&name);
                        }
                    }
                }
                // "compare files"
                let eq = self.contents_equal(master, other);
                // "reduce number of open files, keep only master open"
                if let Some(f) = self.files.get_mut(other) {
                    f.data.close_file();
                }
                self.stats.comparisons = self.stats.comparisons.saturating_add(1);
                if !eq {
                    self.jlog_with(JLOG_VERBOSE2, |h| {
                        [&b"Skipped (content mismatch) "[..], h.path(other)].concat()
                    });
                    continue;
                }
                // "link files"
                if self.file_link(master, other, may_reflink) == Err(EMLINK) {
                    if let Some(f) = self.files.get_mut(master) {
                        f.data.deinit();
                    }
                    master = other;
                }
            }
            // "don't keep master data in memory"
            if let Some(f) = self.files.get_mut(master) {
                f.data.deinit();
            }
            master_at = self.next_of(master);
        }
        // "final cleanup"
        let mut at = Some(begin);
        while let Some(i) = at {
            if let Some(f) = self.files.get_mut(i)
                && f.data.associated()
            {
                f.data.deinit();
            }
            at = self.next_of(i);
        }
        Ok(())
    }

    /// `strtosize_or_err (arg, errmesg)`.
    fn size_arg(&self, arg: &OsStr, errmesg: &str) -> Result<u64, u8> {
        ulstrutils::parse_size(&os_bytes(arg)).map_err(|e| {
            warnx(
                &self.short,
                &ulstrutils::size_error_message(errmesg, arg, e),
            );
            1
        })
    }

    /// `parse_options (argc, argv)`: the operands, or the status to exit
    /// with.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's switch, kept whole to be read against it"
    )]
    fn parse_options(&mut self, argv: &[OsString], utf8: bool) -> Result<Vec<Vec<u8>>, u8> {
        let arg0 = argv
            .first()
            .map_or(OsStr::new("hardlink"), OsString::as_os_str);
        let mut excl_st = [0i32; 1];
        let mut content_only = false;
        let mut operands = Vec::new();
        let own = argv.get(1..).unwrap_or_default();
        for item in HARDLINK.parse(own, SHORTS, LONGS) {
            let opt = match item {
                Ok(opt) => opt,
                Err(e) => {
                    // glibc names the program by argv[0] as given.
                    stderr_write(
                        format!("{}: {}\n", escape_unprintable(&os_bytes(arg0)), e.sentence)
                            .as_bytes(),
                    );
                    return Err(self.errtryhelp());
                }
            };
            let Some((c, value)) = OPTIONS.code::<i32>(&opt) else {
                if let Opt::Operand(w) = opt {
                    operands.push(os_bytes(w).into_owned());
                }
                continue;
            };
            if let Some(msg) =
                err_exclusive_options(c, &EXCL, &mut excl_st, |c| OPTIONS.name_of(c), &self.short)
            {
                stderr_write(msg.as_bytes());
                return Err(1);
            }
            let arg = value.clone().unwrap_or_default();
            match c {
                OPT_REFLINK => {
                    self.reflink_mode = Reflink::Auto;
                    if let Some(v) = value.as_deref().map(os_bytes) {
                        self.reflink_mode = match &*v {
                            b"auto" => Reflink::Auto,
                            b"always" => Reflink::Always,
                            b"never" => Reflink::Never,
                            _ => {
                                warnx(
                                    &self.short,
                                    &format!(
                                        "unsupported reflink mode; {}",
                                        escape_unprintable(&v)
                                    ),
                                );
                                return Err(1);
                            }
                        };
                    }
                    if self.reflink_mode != Reflink::Never {
                        self.reflinks_skip = true;
                    }
                    continue;
                }
                OPT_SKIP_RELINKS => {
                    self.reflinks_skip = true;
                    continue;
                }
                _ => {}
            }
            match u8::try_from(c).unwrap_or(0) {
                b'p' => self.opts.respect_mode = false,
                b'o' => self.opts.respect_owner = false,
                b't' => self.opts.respect_time = false,
                b'X' => self.opts.respect_xattrs = true,
                b'm' => self.opts.maximise = true,
                b'M' => self.opts.minimise = true,
                b'O' => self.opts.keep_oldest = true,
                b'f' => self.opts.respect_name = true,
                b'd' => self.opts.respect_dir = true,
                b'v' => self.opts.verbosity = self.opts.verbosity.saturating_add(1),
                b'q' => self.quiet = true,
                b'c' => content_only = true,
                b'n' => self.opts.dry_run = true,
                b'x' => {
                    let pattern = os_bytes(&arg).into_owned();
                    let re = register_regex(&self.short, &pattern, utf8)?;
                    self.opts.exclude.push(Pattern::new(pattern, re));
                }
                b'y' => self.opts.method = os_bytes(&arg).into_owned(),
                b'i' => {
                    let pattern = os_bytes(&arg).into_owned();
                    let re = register_regex(&self.short, &pattern, utf8)?;
                    self.opts.include.push(Pattern::new(pattern, re));
                }
                b's' => self.opts.min_size = self.size_arg(&arg, "failed to parse minimum size")?,
                b'S' => self.opts.max_size = self.size_arg(&arg, "failed to parse maximum size")?,
                b'r' => {
                    let n = self.size_arg(&arg, "failed to parse cache size")?;
                    self.opts.cache_size = usize::try_from(n).unwrap_or(usize::MAX);
                }
                b'b' => {
                    let n = self.size_arg(&arg, "failed to parse I/O size")?;
                    self.opts.io_size = usize::try_from(n).unwrap_or(usize::MAX);
                }
                b'h' => {
                    let text = usage(&self.short);
                    self.out.write(&text);
                    return Err(0);
                }
                b'V' => {
                    let mut line = self.short.clone();
                    line.extend_from_slice(
                        b" from util-linux 2.39.3 (features: reflink, cryptoapi)
",
                    );
                    self.out.write(&line);
                    return Err(0);
                }
                _ => return Err(self.errtryhelp()),
            }
        }

        if content_only {
            let o = &mut self.opts;
            o.respect_mode = false;
            o.respect_name = false;
            o.respect_dir = false;
            o.respect_owner = false;
            o.respect_time = false;
            o.respect_xattrs = false;
        }
        Ok(operands)
    }

    /// `main ()` after the signals and the locale: the status to exit with.
    fn run(&mut self, argv: &[OsString], utf8: bool) -> u8 {
        let operands = match self.parse_options(argv, utf8) {
            Ok(o) => o,
            Err(status) => return status,
        };
        if operands.is_empty() {
            warnx(&self.short, "no directory or file specified");
            return 1;
        }

        self.stats.start_time = Instant::now();

        let eq = match FileEq::init(&self.opts.method) {
            Some(eq) => eq,
            None => {
                let mut line = b"cannot initialize ".to_vec();
                line.extend_from_slice(&self.opts.method);
                line.extend_from_slice(b" method, use 'memcmp' fallback");
                self.jlog(JLOG_INFO, &line);
                self.opts.method = b"memcmp".to_vec();
                // Upstream's "failed to initialize files comparior" follows
                // a memcmp that fails to set up, which nothing can.
                FileEq::memcmp()
            }
        };
        self.fileeq = Some(eq);

        // "defautl I/O size"
        if self.opts.io_size == 0 {
            self.opts.io_size = if self.opts.method == b"memcmp" {
                8 * 1024
            } else {
                1024 * 1024
            };
        }

        self.stats.started = true;

        self.jlog(JLOG_VERBOSE2, b"Scanning [device/inode/links]:");
        let mut by_ino: Tree<usize> = Tree::default();
        let mut by_size: Tree<usize> = Tree::default();
        for operand in &operands {
            let path = match std::fs::canonicalize(as_path(operand)) {
                Ok(p) => p.into_os_string().into_vec(),
                Err(e) => {
                    self.warn(
                        &format!("cannot get realpath: {}", escape_unprintable(operand)),
                        errno_of(&e),
                    );
                    continue;
                }
            };
            if self.opts.respect_dir {
                self.rootbasesz = i64::try_from(path.len()).unwrap_or(i64::MAX);
            }
            let walked =
                nftw(&path, &mut |fpath: &[u8],
                                  sb: Option<&Stat>,
                                  kind: Kind,
                                  ftw: Ftw| {
                    self.inserter(&mut by_ino, &mut by_size, fpath, sb, kind, ftw)
                });
            if let Err(e) = walked {
                self.warn(&format!("cannot process {}", escape_unprintable(&path)), e);
            }
            self.rootbasesz = 0;
        }

        let mut status = 0u8;
        by_size.walk(|&head, which, _depth| {
            if which != Visit::Leaf && which != Visit::Endorder {
                return true;
            }
            match self.visitor(head) {
                Ok(()) => true,
                Err(s) => {
                    status = s;
                    false
                }
            }
        });
        status
    }
}

/// `register_regex (pregs, regex)`: `regex` compiled as `regcomp (REG_NOSUB
/// | REG_EXTENDED)` compiles it -- by characters of the locale's, or bytes
/// outside UTF-8 -- or the status to exit with after the message.
fn register_regex(short: &[u8], regex: &[u8], utf8: bool) -> Result<ere::Regex, u8> {
    let syntax = ere::Syntax::POSIX_EXTENDED;
    let compiled = if utf8 {
        ere::Regex::new_syntax(regex, false, syntax)
    } else {
        ere::Regex::new_syntax_bytes(regex, false, syntax)
    };
    compiled.map_err(|e| {
        warnx(
            short,
            &format!(
                "could not compile regular expression {}: {}",
                escape_unprintable(regex),
                e.message()
            ),
        );
        1
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(path: &str, basename: i64, dirname: i64) -> FileNode {
        FileNode {
            st: St::default(),
            data: Data::default(),
            next: None,
            links: vec![Link {
                path: path.as_bytes().to_vec(),
                basename,
                dirname,
            }],
        }
    }

    #[test]
    fn names_and_directories_compare_as_upstream_s() {
        let a = node("/r/x/f", 5, 2);
        let b = node("/r/x/g", 5, 2);
        let c = node("/r/yy/f", 6, 2);
        assert!(filename_strcmp(&a, &b) < 0);
        assert_eq!(filename_strcmp(&a, &c), 0);
        assert_eq!(dirname_strcmp(&a, &b), 0, "the same directory");
        assert!(dirname_strcmp(&a, &c) < 0, "a shorter directory first");
        // An operand that is a file: the length is negative and the rest
        // of the path is compared whole.
        let f1 = node("/r/f", 3, 4);
        let f2 = node("/s/f", 3, 4);
        assert_eq!(dirname_strcmp(&f1, &f2), 0);
    }

    #[test]
    fn xattr_names_are_counted_and_sorted_as_c_strings() {
        let list = b"user.b\0user.a\0security.x\0";
        assert_eq!(get_xattr_name_count(list), 3);
        let t = get_sorted_xattr_name_table(list, 3);
        assert_eq!(t, vec![&b"security.x"[..], b"user.a", b"user.b"]);
        assert_eq!(get_xattr_name_count(b""), 0);
    }

    #[test]
    fn strncmp_stops_at_the_end_of_either_string() {
        assert_eq!(strncmp(b"abc", b"abd", 2), 0);
        assert!(strncmp(b"abc", b"abd", 3) < 0);
        assert!(strncmp(b"ab", b"abc", usize::MAX) < 0);
        assert_eq!(strncmp(b"", b"", usize::MAX), 0);
    }

    #[test]
    fn the_usage_ends_with_the_manual_page() {
        let u = usage(b"hardlink");
        assert!(u.starts_with(b"\nUsage:\n hardlink [options]"));
        assert!(u.ends_with(b"\nFor more details see hardlink(1).\n"));
        let help = b" -h, --help                 display this help\n";
        assert!(u.windows(help.len()).any(|w| w == help));
    }
}
