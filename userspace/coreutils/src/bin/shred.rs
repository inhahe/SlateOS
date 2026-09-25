//! `shred` — overwrite files so their contents are harder to recover, and
//! optionally delete them.
//!
//! ```text
//! Usage: shred [OPTION]... FILE...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/shred.c`, with gnulib's `randint` and
//! `randread` in [`coreutils::randint`]. It replaces the `shred` personality of
//! `userspace/pv`, which no link reached and which wrote xorshift output over a
//! file three times under a help text that promised `/dev/urandom`
//! (`design-decisions.md` §1005).
//!
//! # What happens to a file
//!
//! 1. It is opened for writing (with `-f`, made writable first if it was not).
//! 2. Its size is settled: `-s`, else a regular file's size rounded up to a
//!    whole block (so the slack after the end is covered too; `-x` does not
//!    round), else a device's size from seeking to its end.
//! 3. `-n` passes (3 by default) are scheduled by upstream's `genpattern`: a
//!    random pass first and last and spread evenly between, and fixed bit
//!    patterns -- chosen group by group from upstream's table of 1- to 4-bit
//!    patterns, then shuffled -- in the rest. `-z` adds a final pass of zeros.
//! 4. Each pass rewinds and writes the whole size, syncing to the device at
//!    the end. A file smaller than a block is done twice over: once at its
//!    exact size (quietly), then at the rounded size, which is upstream's way of
//!    reaching data a file system stores inside the inode.
//! 5. With `-u`, the file is truncated, renamed to ever shorter names of `0`s
//!    (so the directory slot forgets the name's length as well as the name),
//!    and removed; `wipesync`, the default, syncs the directory after each
//!    rename.
//!
//! # Reproducibility
//!
//! With `--random-source=FILE`, the schedule and every random pass come from
//! FILE's bytes, in upstream's order -- so this port and GNU's write the same
//! bytes to the same file, which `scripts/shred-diff.sh` checks, contents and
//! all. Without it, randomness is the kernel's CSPRNG, where upstream seeds
//! ISAAC from `getrandom`; see [`coreutils::randint`].
//!
//! # Deliberate differences from GNU
//!
//! * **No direct I/O.** Upstream switches `O_DIRECT` on for large passes and
//!   off again if the file system refuses. It is a throughput tweak; the bytes
//!   written and the messages are the same without it.
//! * **No tape rewind.** On Linux upstream rewinds a tape device with the
//!   `MTREW` ioctl before seeking; there is no tape driver here, and a character
//!   device is rewound with `lseek` like everything else.
//! * **`--help` omits the GNU project's bug-report block**, as every converted
//!   utility here does.
//!
//! # A warning upstream gives, repeated
//!
//! shred assumes the file system and hardware overwrite data in place.
//! Journaling, copy-on-write and log-structured file systems, and flash with
//! wear levelling, may keep the old blocks: the data then survives however
//! many passes are made. `--help` says so, as upstream's does.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::randint::{RandError, RandInt};
use coreutils::stdfd;
use coreutils::xnum;
use std::ffi::OsString;
use std::process::ExitCode;

coreutils::guard_std_fds!();

const SHRED: Program = Program::new("shred", 1);

/// `DEFAULT_PASSES`.
const DEFAULT_PASSES: u64 = 3;

/// `MIN (ULONG_MAX, SIZE_MAX / sizeof (int))` on a 64-bit host: the largest
/// `-n` upstream will allocate a pass array for.
const MAX_PASSES: u64 = u64::MAX / 4;

/// `OFF_T_MAX`.
const OFF_T_MAX: u64 = i64::MAX.unsigned_abs();

/// `-u`/`--remove`'s methods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Remove {
    /// The default: overwrite only.
    #[default]
    None,
    /// `unlink`: remove, with no renaming.
    Unlink,
    /// `wipe`: rename to shorter and shorter names, then remove.
    Wipe,
    /// `wipesync`: as `wipe`, syncing the directory after every rename.
    WipeSync,
}

/// `remove_args` paired with `remove_methods`, for `XARGMATCH`.
const REMOVE_ARGS: &[(&str, Remove)] = &[
    ("unlink", Remove::Unlink),
    ("wipe", Remove::Wipe),
    ("wipesync", Remove::WipeSync),
];

/// Upstream's `long_opts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("exact", Takes::Nothing),
    ("force", Takes::Nothing),
    ("iterations", Takes::Required),
    ("size", Takes::Required),
    ("random-source", Takes::Required),
    ("remove", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `struct Options`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Flags {
    force: bool,
    n_iterations: u64,
    /// `-s`; `None` is upstream's `-1`, "use the file's own size".
    size: Option<u64>,
    remove: Remove,
    verbose: bool,
    exact: bool,
    zero_fill: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run {
        flags: Flags,
        random_source: Option<Vec<u8>>,
        files: Vec<OsString>,
    },
}

fn help_text() -> String {
    format!(
        "\
Usage: shred [OPTION]... FILE...
Overwrite the specified FILE(s) repeatedly, in order to make it harder
for even very expensive hardware probing to recover the data.

If FILE is -, shred standard output.

Mandatory arguments to long options are mandatory for short options too.
  -f, --force    change permissions to allow writing if necessary
  -n, --iterations=N  overwrite N times instead of the default ({DEFAULT_PASSES})
      --random-source=FILE  get random bytes from FILE
  -s, --size=N   shred this many bytes (suffixes like K, M, G accepted)
  -u             deallocate and remove file after overwriting
      --remove[=HOW]  like -u but give control on HOW to delete;  See below
  -v, --verbose  show progress
  -x, --exact    do not round file sizes up to the next full block;
                   this is the default for non-regular files
  -z, --zero     add a final overwrite with zeros to hide shredding
      --help        display this help and exit
      --version     output version information and exit

Delete FILE(s) if --remove (-u) is specified.  The default is not to remove
the files because it is common to operate on device files like /dev/hda,
and those files usually should not be removed.
The optional HOW parameter indicates how to remove a directory entry:
'unlink' => use a standard unlink call.
'wipe' => also first obfuscate bytes in the name.
'wipesync' => also sync each obfuscated byte to the device.
The default mode is 'wipesync', but note it can be expensive.

CAUTION: shred assumes the file system and hardware overwrite data in place.
Although this is common, many platforms operate otherwise.  Also, backups
and mirrors may contain unremovable copies that will let a shredded file
be recovered later.  See the GNU coreutils manual for details.
"
    )
}

/// Upstream's option loop, `short_opts = "fn:s:uvxz"`. `-n`, `-s`,
/// `--remove=` and a second `--random-source` exit where they are met, as
/// upstream's do.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = Flags {
        force: false,
        n_iterations: DEFAULT_PASSES,
        size: None,
        remove: Remove::None,
        verbose: false,
        exact: false,
        zero_fill: false,
    };
    let mut random_source: Option<Vec<u8>> = None;
    let mut files = Vec::new();
    for item in SHRED.parse(args, "fn:s:uvxz", LONG_OPTIONS) {
        match item? {
            Opt::Short(b'f', _) | Opt::Long("force", _) => flags.force = true,
            Opt::Short(b'n', Some(v)) | Opt::Long("iterations", Some(v)) => {
                flags.n_iterations = xnum::xdectoumax(
                    &os_bytes(&v),
                    0,
                    MAX_PASSES,
                    Some(b""),
                    "invalid number of passes",
                )
                .map_err(|m| SHRED.usage(m))?;
            }
            Opt::Long("random-source", Some(v)) => {
                let name = os_bytes(&v).into_owned();
                if random_source.as_ref().is_some_and(|prev| *prev != name) {
                    return Err(SHRED.usage("multiple random sources specified".to_string()));
                }
                random_source = Some(name);
            }
            Opt::Short(b'u', _) | Opt::Long("remove", None) => flags.remove = Remove::WipeSync,
            Opt::Long("remove", Some(how)) => {
                flags.remove = SHRED.argmatch(&os_bytes(&how), "--remove", REMOVE_ARGS)?;
            }
            Opt::Short(b's', Some(v)) | Opt::Long("size", Some(v)) => {
                flags.size = Some(
                    xnum::xnumtoumax(
                        &os_bytes(&v),
                        0,
                        0,
                        OFF_T_MAX,
                        Some(b"cbBkKMGTPEZYRQ0"),
                        "invalid file size",
                    )
                    .map_err(|m| SHRED.usage(m))?,
                );
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'x', _) | Opt::Long("exact", _) => flags.exact = true,
            Opt::Short(b'z', _) | Opt::Long("zero", _) => flags.zero_fill = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => files.push(word.clone()),
            // Every table entry is handled above, and a required value is
            // always present; an unknown option arrives as an `Err`.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    if files.is_empty() {
        return Err(SHRED.usage_referring("missing file operand".to_string()));
    }
    Ok(Request::Run {
        flags,
        random_source,
        files,
    })
}

// ------------------------------------------------------------- the patterns ---

/// Upstream's `patterns[]`: groups of fixed patterns, a negative count of
/// random passes between them, and 0 for "start again".
const PATTERNS: &[i32] = &[
    -2, // 2 random passes
    2, 0x000, 0xFFF, // 1-bit
    2, 0x555, 0xAAA, // 2-bit
    -1,    // 1 random pass
    6, 0x249, 0x492, 0x6DB, 0x924, 0xB6D, 0xDB6, // 3-bit
    12, 0x111, 0x222, 0x333, 0x444, 0x666, 0x777, 0x888, 0x999, 0xBBB, 0xCCC, 0xDDD,
    0xEEE, // 4-bit
    -1,    // 1 random pass
    // The following patterns have the first bit per block flipped.
    8, 0x1000, 0x1249, 0x1492, 0x16DB, 0x1924, 0x1B6D, 0x1DB6, 0x1FFF, 14, 0x1111, 0x1222, 0x1333,
    0x1444, 0x1555, 0x1666, 0x1777, 0x1888, 0x1999, 0x1AAA, 0x1BBB, 0x1CCC, 0x1DDD, 0x1EEE,
    -1, // 1 random pass
    0,  // End
];

/// A pass: a fixed pattern (`0..=0x1fff`), or random (upstream's `-1`).
type Pass = i32;

/// The random pass marker.
const RANDOM: Pass = -1;

/// Upstream's `genpattern`: fill `dest` with its length's worth of passes,
/// then shuffle them with the random ones spread evenly -- first, last, and a
/// Bresenham line between. The caller allocates `dest`, as upstream's
/// `do_wipefd` does, because that allocation is where an absurd `-n` fails.
fn genpattern(dest: &mut [Pass], s: &mut RandInt) -> Result<(), RandError> {
    let num = dest.len();
    if num == 0 {
        return Ok(());
    }

    // Stage 1: choose the passes to use.
    let mut p = 0usize;
    let mut randpasses = 0usize;
    let mut d = 0usize;
    let mut n = num;
    let at = |i: usize| PATTERNS.get(i).copied().unwrap_or(0);
    loop {
        let word = at(p);
        p = p.saturating_add(1);
        match word.cmp(&0) {
            // The end of the table: start again from the beginning.
            std::cmp::Ordering::Equal => p = 0,
            // `-k` random passes.
            std::cmp::Ordering::Less => {
                let k = usize::try_from(word.unsigned_abs()).unwrap_or(usize::MAX);
                if k >= n {
                    randpasses = randpasses.saturating_add(n);
                    break;
                }
                randpasses = randpasses.saturating_add(k);
                n = n.saturating_sub(k);
            }
            // A group of `k` fixed patterns.
            std::cmp::Ordering::Greater => {
                let k = usize::try_from(word).unwrap_or(0);
                if k <= n {
                    // A full block of patterns.
                    for j in 0..k {
                        if let Some(slot) = dest.get_mut(d.saturating_add(j)) {
                            *slot = at(p.saturating_add(j));
                        }
                    }
                    p = p.saturating_add(k);
                    d = d.saturating_add(k);
                    n = n.saturating_sub(k);
                } else if n < 2 || n.saturating_mul(3) < k {
                    // Finish with random.
                    randpasses = randpasses.saturating_add(n);
                    break;
                } else {
                    // Pad out with n of the k available.
                    let mut k = k;
                    loop {
                        let take = n == k || s.choose(k as u64)? < n as u64;
                        if take {
                            if let Some(slot) = dest.get_mut(d) {
                                *slot = at(p);
                            }
                            d = d.saturating_add(1);
                            n = n.saturating_sub(1);
                        }
                        p = p.saturating_add(1);
                        k = k.saturating_sub(1);
                        if n == 0 {
                            break;
                        }
                    }
                    break;
                }
            }
        }
    }
    let mut top = num.saturating_sub(randpasses);

    // Stage 2: scramble the fixed passes, with the random ones spread by
    // Bresenham's line from the first pass to the last.
    let randpasses = randpasses.saturating_sub(1);
    let mut accum = randpasses;
    for n in 0..num {
        if accum <= randpasses {
            accum = accum.saturating_add(num.saturating_sub(1));
            let here = dest.get(n).copied().unwrap_or(RANDOM);
            if let Some(slot) = dest.get_mut(top) {
                *slot = here;
            }
            top = top.saturating_add(1);
            if let Some(slot) = dest.get_mut(n) {
                *slot = RANDOM;
            }
        } else {
            let swap = n.saturating_add(
                usize::try_from(s.choose(top.saturating_sub(n) as u64)?).unwrap_or(0),
            );
            dest.swap(n, swap.min(num.saturating_sub(1)));
        }
        accum = accum.wrapping_sub(randpasses);
    }
    Ok(())
}

/// Upstream's `periodic_pattern`: does the 12-bit pattern repeat every byte,
/// or every three?
fn periodic_pattern(pass: Pass) -> bool {
    if pass <= 0 {
        return false;
    }
    let [r0, r1, r2] = pattern_bytes(pass);
    r0 != r1 || r0 != r2
}

/// The three bytes a 12-bit pattern repeats as: `bits |= bits << 12`, then
/// bits 4..12, 8..16 and 0..8.
fn pattern_bytes(pass: Pass) -> [u8; 3] {
    let low = u32::try_from(pass).unwrap_or(0) & 0xfff;
    let bits = low | (low << 12);
    let byte = |shift: u32| u8::try_from((bits >> shift) & 0xff).unwrap_or(0);
    [byte(4), byte(8), byte(0)]
}

/// Upstream's `fillpattern`: `size` bytes of the pattern (the buffer always has
/// at least three), and with bit 12 set, the first bit of every sector flipped.
fn fillpattern(pass: Pass, buf: &mut [u8], size: usize) {
    let [r0, r1, r2] = pattern_bytes(pass);
    for (slot, byte) in buf.iter_mut().zip([r0, r1, r2]) {
        *slot = byte;
    }
    // The doubling copy, which is the same as repeating the three bytes.
    for (i, slot) in buf.iter_mut().enumerate().take(size).skip(3) {
        *slot = [r0, r1, r2].get(i % 3).copied().unwrap_or(0);
    }
    if pass & 0x1000 != 0 {
        for i in (0..size).step_by(SECTOR_SIZE) {
            if let Some(b) = buf.get_mut(i) {
                *b ^= 0x80;
            }
        }
    }
}

/// `SECTOR_SIZE`: the unit a write error skips over.
const SECTOR_SIZE: usize = 512;

/// Upstream's `passname`: the first three bytes in hex, or `random`.
fn passname(buf: Option<&[u8]>) -> String {
    use std::fmt::Write as _;
    match buf {
        Some(b) => b.iter().take(3).fold(String::new(), |mut out, x| {
            // Formatting into a `String` cannot fail.
            let _ = write!(out, "{x:02x}");
            out
        }),
        None => "random".to_string(),
    }
}

// ------------------------------------------------------------------ main -----

fn main() -> ExitCode {
    stdfd::close_stderr(run(), 1)
}

fn run() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            SHRED.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let (flags, random_source, files) = match request {
        Request::Help => return say(&help_text()),
        Request::Version => return say("shred (SlateOS coreutils) 0.1.0\n"),
        Request::Run {
            flags,
            random_source,
            files,
        } => (flags, random_source, files),
    };
    imp::shred_all(&flags, random_source.as_deref(), &files)
}

fn say(text: &str) -> ExitCode {
    use std::io::Write;
    let mut out = stdfd::Stream::stdout_line_buffered();
    // Deliberately unread: `Stream` latches a failed write and `close_stdout`
    // reports it.
    let _ = out.write_all(text.as_bytes());
    stdfd::close_stdout("shred", out, ExitCode::SUCCESS)
}

/// A random-source failure, reported as gnulib's `randread_error` reports it,
/// and fatal as that is.
fn rand_error(e: &RandError) -> ExitCode {
    let message = match e {
        RandError::EndOfFile(name) => format!("{}: end of file", quote(name)),
        RandError::Read(name, err) => {
            format!(
                "{}: read error: {}",
                quote(name),
                coreutils::errmsg::strerror(err)
            )
        }
        RandError::System(what) => format!("getrandom: {what}"),
    };
    coreutils::diag!("shred: {message}");
    ExitCode::from(1)
}

#[cfg(not(unix))]
mod imp {
    use super::{Flags, OsString};
    use std::process::ExitCode;

    pub fn shred_all(_flags: &Flags, _random: Option<&[u8]>, _files: &[OsString]) -> ExitCode {
        coreutils::diag!("shred: unix-only utility; not supported on this platform");
        ExitCode::from(1)
    }
}

#[cfg(unix)]
mod imp {
    use super::{
        Flags, OFF_T_MAX, Pass, RandError, RandInt, Remove, SECTOR_SIZE, fillpattern, genpattern,
        passname, periodic_pattern, rand_error,
    };
    use coreutils::errmsg::strerror;
    use coreutils::human::{Opts, human_readable};
    use coreutils::pathname::{base_len, dir_name, last_component_offset};
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use coreutils::randint::RandRead;
    use std::ffi::OsString;
    use std::fs::{File, OpenOptions, Permissions};
    use std::io::{self, IsTerminal, Seek, SeekFrom, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Path;
    use std::process::ExitCode;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// `O_NOCTTY`, so opening a terminal never makes it ours.
    const O_NOCTTY: i32 = 0o400;
    /// `O_DIRECTORY` and `O_NONBLOCK`, for the directory `wipesync` syncs.
    const O_DIRECTORY: i32 = 0o200_000;
    const O_NONBLOCK: i32 = 0o4000;
    /// `F_GETFL` and `O_APPEND`, for the `-` operand.
    const F_GETFL: i32 = 3;
    const O_APPEND: i32 = 0o2000;
    /// The `errno` values the sync and write paths compare against.
    const EINVAL: i32 = 22;
    const EBADF: i32 = 9;
    const EISDIR: i32 = 21;
    const EIO: i32 = 5;
    const ENOSPC: i32 = 28;
    const EEXIST: i32 = 17;
    const EACCES: i32 = 13;
    /// `VERBOSE_UPDATE`: seconds between progress lines.
    const VERBOSE_UPDATE: u64 = 5;
    /// `PERIODIC_OUTPUT_SIZE` and `NONPERIODIC_OUTPUT_SIZE`: bytes per write.
    const PERIODIC_OUTPUT_SIZE: u64 = 60 * 1024;
    const NONPERIODIC_OUTPUT_SIZE: u64 = 64 * 1024;
    /// `DEV_BSIZE`, `ST_BLKSIZE`'s fallback.
    const DEV_BSIZE: u64 = 512;

    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn sync();
        fn close(fd: i32) -> i32;
    }

    /// Everything a run needs to report through.
    struct Run<'a> {
        flags: &'a Flags,
        s: RandInt,
    }

    /// A failure that ends the whole run, not one file -- both are an `exit`
    /// from deep inside upstream.
    enum Fatal {
        /// Running out of `--random-source`: gnulib's `randread_error`.
        Rand(RandError),
        /// The pass array could not be allocated: gnulib's `xalloc_die`, for
        /// an `-n` too large to hold, which `-n`'s own bound (`SIZE_MAX /
        /// sizeof (int)`) still admits.
        NoMemory,
    }

    impl From<RandError> for Fatal {
        fn from(e: RandError) -> Self {
            Fatal::Rand(e)
        }
    }

    pub fn shred_all(flags: &Flags, random_source: Option<&[u8]>, files: &[OsString]) -> ExitCode {
        let source = match RandRead::open(random_source) {
            Ok(s) => s,
            Err(e) => {
                let name = random_source.unwrap_or(b"getrandom");
                coreutils::diag!("shred: {}: {}", quotef(name), strerror(&e));
                return ExitCode::from(1);
            }
        };
        let mut run = Run {
            flags,
            s: RandInt::new(source),
        };
        let mut ok = true;
        for operand in files {
            let name = os_bytes(operand).into_owned();
            let qname = quotef(&name);
            let result = if name == b"-" {
                wipefd_stdout(&mut run, &qname)
            } else {
                wipefile(&mut run, &name, &qname)
            };
            match result {
                Ok(good) => ok &= good,
                Err(Fatal::Rand(e)) => return rand_error(&e),
                Err(Fatal::NoMemory) => {
                    diag("memory exhausted");
                    return ExitCode::from(1);
                }
            }
        }
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    }

    fn diag(text: &str) {
        coreutils::diag!("shred: {text}");
    }

    /// `wipefd` for `-`: standard output, which must not be append-only.
    fn wipefd_stdout(run: &mut Run<'_>, qname: &str) -> Result<bool, Fatal> {
        // SAFETY: `fcntl` with `F_GETFL` reads the descriptor's flags and
        // touches no memory; a closed descriptor is reported as -1/`EBADF`.
        let fd_flags = unsafe { fcntl(1, F_GETFL) };
        if fd_flags < 0 {
            let e = io::Error::last_os_error();
            diag(&format!("{qname}: fcntl failed: {}", strerror(&e)));
            return Ok(false);
        }
        if fd_flags & O_APPEND != 0 {
            diag(&format!(
                "{qname}: cannot shred append-only file descriptor"
            ));
            return Ok(false);
        }
        // SAFETY: descriptor 1 is open (the `fcntl` above succeeded), and the
        // `ManuallyDrop` keeps this `File` from closing a descriptor it does
        // not own.
        let mut file = ManuallyDrop::new(unsafe { File::from_raw_fd(1) });
        do_wipefd(run, &mut file, qname)
    }

    /// `wipefile`: open, wipe, close, and with `-u` remove.
    fn wipefile(run: &mut Run<'_>, name: &[u8], qname: &str) -> Result<bool, Fatal> {
        let path = os_from_bytes(name);
        let open = || {
            OpenOptions::new()
                .write(true)
                .custom_flags(O_NOCTTY)
                .open(Path::new(&path))
        };
        let mut opened = open();
        if let Err(e) = &opened
            // `errno == EACCES` exactly: Rust's `PermissionDenied` also
            // covers `EPERM`, and an `EPERM` (an immutable file, say) is not
            // one a `chmod` could cure -- upstream does not try it.
            && e.raw_os_error() == Some(EACCES)
            && run.flags.force
            && std::fs::set_permissions(Path::new(&path), Permissions::from_mode(0o200)).is_ok()
        {
            opened = open();
        }
        let mut file = match opened {
            Ok(f) => f,
            Err(e) => {
                diag(&format!(
                    "{qname}: failed to open for writing: {}",
                    strerror(&e)
                ));
                return Ok(false);
            }
        };
        let mut ok = do_wipefd(run, &mut file, qname)?;
        let fd = file.into_raw_fd();
        // SAFETY: `fd` came from `into_raw_fd`, so this is its only owner, and
        // it is closed exactly once.
        if unsafe { close(fd) } != 0 {
            let e = io::Error::last_os_error();
            diag(&format!("{qname}: failed to close: {}", strerror(&e)));
            ok = false;
        }
        if ok && run.flags.remove != Remove::None {
            ok = wipename(run.flags, name, qname);
        }
        Ok(ok)
    }

    /// `ST_BLKSIZE`: the file's preferred I/O size, or 512 when that is absent
    /// or absurd.
    fn st_blksize(meta: &std::fs::Metadata) -> u64 {
        let b = meta.blksize();
        if b > 0 && b <= u64::MAX / 8 + 1 {
            b
        } else {
            DEV_BSIZE
        }
    }

    /// `do_wipefd`: settle the size, schedule the passes, run them, and with
    /// `-u` truncate.
    fn do_wipefd(run: &mut Run<'_>, file: &mut File, qname: &str) -> Result<bool, Fatal> {
        let flags = run.flags;
        let n: u64 = if flags.verbose {
            flags
                .n_iterations
                .saturating_add(u64::from(flags.zero_fill))
        } else {
            0
        };

        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => {
                diag(&format!("{qname}: fstat failed: {}", strerror(&e)));
                return Ok(false);
            }
        };
        let ft = meta.file_type();
        if (ft.is_char_device() && file.is_terminal()) || ft.is_fifo() || ft.is_socket() {
            diag(&format!("{qname}: invalid file type"));
            return Ok(false);
        }

        // `size`: `None` is upstream's -1, "not known yet".
        let mut i_size: u64 = 0;
        let mut size: Option<u64> = flags.size;
        match flags.size {
            None => {
                if ft.is_file() {
                    let mut sz = meta.len();
                    if !flags.exact {
                        let blk = st_blksize(&meta);
                        let remainder = sz % blk;
                        if sz != 0 && sz < blk {
                            i_size = sz;
                        }
                        if remainder != 0 {
                            let incr = blk.saturating_sub(remainder);
                            sz = sz.saturating_add(incr.min(OFF_T_MAX.saturating_sub(sz)));
                        }
                    }
                    size = Some(sz);
                } else {
                    // Seeking to the end is how a device's size is learned.
                    size = match file.seek(SeekFrom::End(0)) {
                        Ok(end) if end > 0 => Some(end),
                        _ => None,
                    };
                }
            }
            Some(want) => {
                if ft.is_file() && meta.len() < st_blksize(&meta).min(want) {
                    i_size = meta.len();
                }
            }
        }

        // `xnmalloc (flags->n_iterations, sizeof *passarray)`: reserved
        // fallibly, so a huge `-n` is upstream's `memory exhausted` rather
        // than an abort.
        let num = usize::try_from(flags.n_iterations).unwrap_or(usize::MAX);
        let mut passes: Vec<Pass> = Vec::new();
        if passes.try_reserve_exact(num).is_err() {
            return Err(Fatal::NoMemory);
        }
        passes.resize(num, 0);
        genpattern(&mut passes, &mut run.s)?;
        let total = passes.len().saturating_add(usize::from(flags.zero_fill));

        let mut ok = true;
        // The rounds: the exact small size first (quietly), then the whole.
        // `Some(0)` marks "done", as upstream's `size = 0`.
        let mut remaining: Option<u64> = size;
        let mut pending_small = i_size;
        loop {
            let (mut pass_size, pn) = if pending_small != 0 {
                let this = Some(pending_small);
                pending_small = 0;
                (this, 0)
            } else if remaining != Some(0) {
                let this = remaining;
                remaining = Some(0);
                (this, n)
            } else {
                break;
            };

            for i in 0..total {
                let pass = passes.get(i).copied().unwrap_or(0);
                let err = dopass(
                    run,
                    file,
                    &meta,
                    qname,
                    &mut pass_size,
                    pass,
                    i as u64 + 1,
                    pn,
                )?;
                if err != 0 {
                    ok = false;
                    if err < 0 {
                        return Ok(false);
                    }
                }
            }
        }

        if flags.remove != Remove::None
            && let Err(e) = file.set_len(0)
            && ft.is_file()
        {
            diag(&format!("{qname}: error truncating: {}", strerror(&e)));
            return Ok(false);
        }
        Ok(ok)
    }

    /// Seconds since the epoch: upstream's `time (nullptr)`.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// `dopass`: one pass of `pass` over `*sizep` bytes. Returns 1 for a write
    /// error it carried on past, -1 for one that ended the pass, 0 otherwise.
    #[allow(clippy::too_many_arguments)]
    fn dopass(
        run: &mut Run<'_>,
        file: &mut File,
        meta: &std::fs::Metadata,
        qname: &str,
        sizep: &mut Option<u64>,
        pass: Pass,
        k: u64,
        n: u64,
    ) -> Result<i32, Fatal> {
        let size = *sizep;
        let output_size = if periodic_pattern(pass) {
            PERIODIC_OUTPUT_SIZE
        } else {
            NONPERIODIC_OUTPUT_SIZE
        };
        // `FILLPATTERN_SIZE`: a multiple of three at least `output_size`.
        let fill_size = output_size.saturating_add(2) / 3 * 3;
        let mut pbuf = vec![0u8; usize::try_from(fill_size).unwrap_or(0)];

        let mut write_error = false;

        if let Err(e) = dorewind(file, meta) {
            diag(&format!("{qname}: cannot rewind: {}", strerror(&e)));
            return Ok(-1);
        }

        let pass_string = if pass >= 0 {
            let lim = match size {
                Some(sz) if sz < fill_size => sz,
                _ => fill_size,
            };
            fillpattern(pass, &mut pbuf, usize::try_from(lim).unwrap_or(0));
            passname(Some(&pbuf))
        } else {
            passname(None)
        };

        let mut thresh = 0u64;
        let mut previous_human_offset = String::new();
        if n != 0 {
            diag(&format!("{qname}: pass {k}/{n} ({pass_string})..."));
            thresh = now().saturating_add(VERBOSE_UPDATE);
        }

        let progress = Opts::AUTOSCALE | Opts::SI | Opts::BASE_1024 | Opts::B;
        let mut offset: u64 = 0;
        let mut size = size;
        // Upstream's `now`: assigned only when the five-second test is reached,
        // and kept across iterations, so the threshold set after a progress
        // line counts from the last time the clock was actually read.
        let mut stamp = 0u64;
        loop {
            // `size - offset < output_size` in upstream's signed arithmetic:
            // an offset past the size (possible after a size discovered short)
            // is negative there, which here is the explicit first test.
            let mut lim = output_size;
            if let Some(sz) = size {
                if sz < offset {
                    break;
                }
                if sz - offset < output_size {
                    lim = sz - offset;
                    if lim == 0 {
                        break;
                    }
                }
            }
            let lim_usize = usize::try_from(lim).unwrap_or(0);
            if pass < 0 {
                run.s
                    .source()
                    .read(pbuf.get_mut(..lim_usize).unwrap_or(&mut []))?;
            }

            // Retry partial writes.
            let mut soff = 0usize;
            while soff < lim_usize {
                match file.write(pbuf.get(soff..lim_usize).unwrap_or(&[])) {
                    Ok(w) if w > 0 => soff = soff.saturating_add(w),
                    result => {
                        let err = match result {
                            Ok(_) => io::Error::from(io::ErrorKind::WriteZero),
                            Err(e) => e,
                        };
                        let errno = err.raw_os_error();
                        if size.is_none() && (errno.is_none() || errno == Some(ENOSPC)) {
                            // The end of the device.
                            size = Some(offset.saturating_add(soff as u64));
                            *sizep = size;
                            break;
                        }
                        if errno == Some(EINTR_RAW) {
                            continue;
                        }
                        diag(&format!(
                            "{qname}: error writing at offset {}: {}",
                            offset.saturating_add(soff as u64),
                            strerror(&err)
                        ));
                        // Keep going past a bad sector, as a tool run on
                        // failing media must.
                        if errno == Some(EIO)
                            && size.is_some()
                            && (soff | (SECTOR_SIZE - 1)) < lim_usize
                        {
                            let soff1 = (soff | (SECTOR_SIZE - 1)).saturating_add(1);
                            match file.seek(SeekFrom::Start(offset.saturating_add(soff1 as u64))) {
                                Ok(_) => {
                                    soff = soff1;
                                    write_error = true;
                                    continue;
                                }
                                Err(e) => {
                                    diag(&format!("{qname}: lseek failed: {}", strerror(&e)));
                                }
                            }
                        }
                        return Ok(-1);
                    }
                }
            }

            if OFF_T_MAX.saturating_sub(offset) < soff as u64 {
                diag(&format!("{qname}: file too large"));
                return Ok(-1);
            }
            offset = offset.saturating_add(soff as u64);

            let done = Some(offset) == size;

            // Progress, when five seconds have passed or the pass is done after
            // having reported some.
            let due = n != 0
                && ((done && !previous_human_offset.is_empty()) || {
                    stamp = now();
                    thresh <= stamp
                });
            if due {
                let mut human_offset = human_readable(offset, Opts::FLOOR | progress, 1, 1);
                if done || previous_human_offset != human_offset {
                    match size {
                        None => diag(&format!(
                            "{qname}: pass {k}/{n} ({pass_string})...{human_offset}"
                        )),
                        Some(sz) => {
                            let percent = if sz == 0 {
                                100
                            } else if offset <= u64::MAX / 100 {
                                offset.saturating_mul(100) / sz
                            } else {
                                offset / (sz / 100).max(1)
                            };
                            let human_size = human_readable(sz, Opts::CEILING | progress, 1, 1);
                            if done {
                                human_offset.clone_from(&human_size);
                            }
                            diag(&format!(
                                "{qname}: pass {k}/{n} ({pass_string})...{human_offset}/{human_size} {percent}%"
                            ));
                        }
                    }
                    previous_human_offset = human_offset;
                    thresh = stamp.saturating_add(VERBOSE_UPDATE);
                    if let Err(e) = dosync(file, qname) {
                        if e.raw_os_error() != Some(EIO) {
                            return Ok(-1);
                        }
                        write_error = true;
                    }
                }
            }
            // `offset == size` ends the loop at the size test above.
        }

        if let Err(e) = dosync(file, qname) {
            if e.raw_os_error() != Some(EIO) {
                return Ok(-1);
            }
            write_error = true;
        }
        Ok(i32::from(write_error))
    }

    /// `EINTR`: a write interrupted before it wrote anything, retried.
    const EINTR_RAW: i32 = 4;

    /// `dorewind`: back to the start.
    fn dorewind(file: &mut File, _meta: &std::fs::Metadata) -> io::Result<()> {
        match file.seek(SeekFrom::Start(0)) {
            Ok(0) => Ok(()),
            Ok(_) => Err(io::Error::from_raw_os_error(EINVAL)),
            Err(e) => Err(e),
        }
    }

    /// `ignorable_sync_errno`: a descriptor that cannot be synced is not an
    /// error worth stopping for.
    fn ignorable(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(EINVAL | EBADF | EISDIR))
    }

    /// `dosync`: `fdatasync`, else `fsync`, else the whole system's `sync`.
    fn dosync(file: &File, qname: &str) -> io::Result<()> {
        match file.sync_data() {
            Ok(()) => return Ok(()),
            Err(e) if !ignorable(&e) => {
                diag(&format!("{qname}: fdatasync failed: {}", strerror(&e)));
                return Err(e);
            }
            Err(_) => {}
        }
        match file.sync_all() {
            Ok(()) => return Ok(()),
            Err(e) if !ignorable(&e) => {
                diag(&format!("{qname}: fsync failed: {}", strerror(&e)));
                return Err(e);
            }
            Err(_) => {}
        }
        // SAFETY: `sync` takes no arguments and cannot fail.
        unsafe { sync() };
        Ok(())
    }

    /// Upstream's `nameset`: the digits a wiping name is spelled in.
    const NAMESET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_.";

    /// `incname`: the next name of the same length, counting in `NAMESET`.
    /// `false` once every name of this length has been tried.
    fn incname(name: &mut [u8]) -> bool {
        for slot in name.iter_mut().rev() {
            let at = NAMESET.iter().position(|&c| c == *slot).unwrap_or(0);
            if let Some(&next) = NAMESET.get(at.saturating_add(1)) {
                *slot = next;
                return true;
            }
            *slot = NAMESET.first().copied().unwrap_or(b'0');
        }
        false
    }

    /// `wipename`: rename to shorter and shorter names, then remove.
    fn wipename(flags: &Flags, oldname: &[u8], qoldname: &str) -> bool {
        let mut oldname = oldname.to_vec();
        let base_at = last_component_offset(&oldname);
        let dir = dir_name(&oldname).to_vec();
        let qdir = quotef(&dir);
        let mut ok = true;
        let mut first = true;

        let dir_file = if flags.remove == Remove::WipeSync {
            OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOCTTY | O_NONBLOCK)
                .open(os_from_bytes(&dir))
                .ok()
        } else {
            None
        };

        if flags.verbose {
            diag(&format!("{qoldname}: removing"));
        }

        if flags.remove != Remove::Unlink {
            let len = base_len(oldname.get(base_at..).unwrap_or(&[]));
            for len in (1..=len).rev() {
                let mut base = vec![NAMESET.first().copied().unwrap_or(b'0'); len];
                let rename_ok = loop {
                    let mut newname = oldname.get(..base_at).unwrap_or(&[]).to_vec();
                    newname.extend_from_slice(&base);
                    match coreutils::rename::noreplace(
                        Path::new(&os_from_bytes(&oldname)),
                        Path::new(&os_from_bytes(&newname)),
                    ) {
                        Ok(()) => break Some(newname),
                        Err(e)
                            if (e.kind() == io::ErrorKind::AlreadyExists
                                || e.raw_os_error() == Some(EEXIST))
                                && incname(&mut base) => {}
                        Err(_) => break None,
                    }
                };
                if let Some(newname) = rename_ok {
                    if let Some(d) = &dir_file
                        && dosync(d, &qdir).is_err()
                    {
                        ok = false;
                    }
                    if flags.verbose {
                        // The new name was picked here and needs no quoting;
                        // the old one is quoted only the first time. Both go
                        // out as bytes, as upstream's `%s` sends them.
                        let mut line = b"shred: ".to_vec();
                        if first {
                            line.extend_from_slice(qoldname.as_bytes());
                        } else {
                            line.extend_from_slice(&oldname);
                        }
                        line.extend_from_slice(b": renamed to ");
                        line.extend_from_slice(&newname);
                        line.push(b'\n');
                        coreutils::stdfd::diag_bytes(&line);
                        first = false;
                    }
                    oldname = newname;
                }
            }
        }

        match std::fs::remove_file(os_from_bytes(&oldname)) {
            Ok(()) => {
                if flags.verbose {
                    diag(&format!("{qoldname}: removed"));
                }
            }
            Err(e) => {
                diag(&format!("{qoldname}: failed to remove: {}", strerror(&e)));
                ok = false;
            }
        }
        if let Some(d) = dir_file {
            if dosync(&d, &qdir).is_err() {
                ok = false;
            }
            let fd = d.into_raw_fd();
            // SAFETY: `fd` came from `into_raw_fd`; closed exactly once.
            if unsafe { close(fd) } != 0 {
                let e = io::Error::last_os_error();
                diag(&format!("{qdir}: failed to close: {}", strerror(&e)));
                ok = false;
            }
        }
        ok
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn pattern_bytes_are_upstreams() {
        assert_eq!(pattern_bytes(0x000), [0, 0, 0]);
        assert_eq!(pattern_bytes(0xFFF), [0xff, 0xff, 0xff]);
        assert_eq!(pattern_bytes(0x555), [0x55, 0x55, 0x55]);
        // `bits = 0x249249`: bits 4..12, 8..16, 0..8.
        assert_eq!(pattern_bytes(0x249), [0x24, 0x92, 0x49]);
        assert_eq!(passname(Some(&pattern_bytes(0x249))), "249249");
        assert!(!periodic_pattern(0x555));
        assert!(periodic_pattern(0x249));
        assert!(!periodic_pattern(RANDOM));
    }

    #[test]
    fn a_flipped_pattern_flips_the_first_bit_of_each_sector() {
        let mut buf = vec![0u8; 1100];
        fillpattern(0x1000, &mut buf, 1100);
        assert_eq!(buf[0], 0x80);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[512], 0x80);
        assert_eq!(buf[1024], 0x80);
        assert_eq!(passname(Some(&buf)), "800000");
    }

    #[test]
    fn passnames() {
        assert_eq!(passname(None), "random");
        assert_eq!(passname(Some(&[0xab, 0xcd, 0xef, 0x01])), "abcdef");
    }
}
