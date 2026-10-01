//! blkdiscard -- discard the content of sectors on a device.
//!
//! A port of util-linux 2.39.3's `sys-utils/blkdiscard.c`: a range of a
//! block device discarded (`BLKDISCARD`), securely discarded
//! (`BLKSECDISCARD`) or zero-filled (`BLKZEROOUT`), in steps if asked, after
//! a check with `ulblkid` (the port of libblkid) that it holds no signature
//! -- which refuses only when standard input is a terminal, so as not to
//! break scripts. Measured against `blkdiscard from util-linux 2.39.3` by
//! `scripts/blkdiscard-diff.sh`, as far as an ordinary user can reach it
//! (the options and the refusals; the discarding itself needs a block
//! device and root).
//!
//! This replaces the `blkdiscard` personality of the old hand-written
//! `wipefs`, which no executable was ever made for.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **A sector size of 0**, which upstream divides by, fails the alignment
//!   checks instead.

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::process::ExitCode;
use std::rc::Rc;
use ulclosestream::{Stdout, stderr_write, warn, warnx};

/// Getopt's errors are only sentences here; the referral follows them.
const BLKDISCARD: Program = Program::new("blkdiscard", 1);

/// Upstream's `longopts[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("force", Takes::Nothing),
    ("help", Takes::Nothing),
    ("length", Takes::Required),
    ("offset", Takes::Required),
    ("quiet", Takes::Nothing),
    ("secure", Takes::Nothing),
    ("step", Takes::Required),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
    ("zeroout", Takes::Nothing),
];
const LONG_VALS: [u8; 10] = *b"fhloqspvVz";

/// `EXIT_NOTSUPP` (`include/exitcodes.h`).
#[cfg(unix)]
const EXIT_NOTSUPP: u8 = 2;
/// `EOPNOTSUPP`.
#[cfg(unix)]
const EOPNOTSUPP: i32 = 95;

/// What is done to the range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// `ACT_DISCARD`, the default.
    Discard,
    /// `ACT_ZEROOUT`.
    Zeroout,
    /// `ACT_SECURE`.
    Secure,
}

/// A fatal error: printed, and the status to exit with.
struct Fatal(u8);

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `err(status, msg)` with `errno` as the error.
fn err_errno(short: &[u8], status: u8, msg: &str, errno: i32) -> Fatal {
    if errno == 0 {
        warnx(short, &format!("{msg}: Success"));
    } else {
        warn(short, msg, &std::io::Error::from_raw_os_error(errno));
    }
    Fatal(status)
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] <device>\n", shown(short)));
    t.push('\n');
    t.push_str("Discard the content of sectors on a device.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -f, --force         disable all checking\n");
    t.push_str(" -l, --length <num>  length of bytes to discard from the offset\n");
    t.push_str(" -o, --offset <num>  offset in bytes to discard from\n");
    t.push_str(" -p, --step <num>    size of the discard iterations within the offset\n");
    t.push_str(" -q, --quiet         suppress warning messages\n");
    t.push_str(" -s, --secure        perform secure discard\n");
    t.push_str(" -v, --verbose       print aligned length and offset\n");
    t.push_str(" -z, --zeroout       zero-fill rather than discard\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<21}{}\n{:<21}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nArguments:\n");
    t.push_str(" <num> arguments may be followed by the suffixes for\n   GiB, TiB, PiB, EiB, ZiB, and YiB (the \"iB\" is optional)\n");
    t.push_str("\nFor more details see blkdiscard(8).\n");
    t.into_bytes()
}

/// `print_stats(act, path, stats)`.
#[cfg(unix)]
fn print_stats(out: &mut Stdout, act: Act, path: &[u8], stats: [u64; 2]) {
    let verb = match act {
        Act::Zeroout => "Zero-filled",
        Act::Secure | Act::Discard => "Discarded",
    };
    let mut line = path.to_vec();
    line.extend_from_slice(
        format!(": {verb} {} bytes from the offset {}\n", stats[1], stats[0]).as_bytes(),
    );
    out.write(&line);
}

/// The block-device requests, through the C library.
#[cfg(unix)]
mod ioctls {
    use std::fs::File;
    use std::os::fd::AsRawFd;

    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }

    /// `BLKGETSIZE64`: `_IOR(0x12, 114, size_t)`.
    const BLKGETSIZE64: u64 = 0x8008_1272;
    /// `BLKSSZGET`: `_IO(0x12, 104)`.
    const BLKSSZGET: u64 = 0x1268;
    /// `BLKDISCARD`: `_IO(0x12, 119)`.
    pub const BLKDISCARD: u64 = 0x1277;
    /// `BLKSECDISCARD`: `_IO(0x12, 125)`.
    pub const BLKSECDISCARD: u64 = 0x127d;
    /// `BLKZEROOUT`: `_IO(0x12, 127)`.
    pub const BLKZEROOUT: u64 = 0x127f;

    fn last_errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(5)
    }

    /// `ioctl(fd, BLKGETSIZE64, &size)`.
    pub fn getsize64(f: &File) -> Result<u64, i32> {
        let mut size = 0u64;
        // SAFETY: BLKGETSIZE64 writes one u64 through the pointer, which
        // points at `size`; the descriptor is open.
        let rc = unsafe { ioctl(f.as_raw_fd(), BLKGETSIZE64, &raw mut size) };
        if rc != 0 { Err(last_errno()) } else { Ok(size) }
    }

    /// `ioctl(fd, BLKSSZGET, &secsize)`.
    pub fn sszget(f: &File) -> Result<i32, i32> {
        let mut secsize = 0i32;
        // SAFETY: BLKSSZGET writes one int through the pointer, which
        // points at `secsize`; the descriptor is open.
        let rc = unsafe { ioctl(f.as_raw_fd(), BLKSSZGET, &raw mut secsize) };
        if rc != 0 {
            Err(last_errno())
        } else {
            Ok(secsize)
        }
    }

    /// `ioctl(fd, request, &range)` for the three range requests.
    pub fn range(f: &File, request: u64, range: &mut [u64; 2]) -> Result<(), i32> {
        // SAFETY: the discard and zero-out requests read two u64s (start and
        // length) through the pointer, which points at `range`; the
        // descriptor is open.
        let rc = unsafe { ioctl(f.as_raw_fd(), request, range.as_mut_ptr()) };
        if rc != 0 { Err(last_errno()) } else { Ok(()) }
    }
}

/// `probe_device(fd, path)`: 0 when the device holds a signature (said
/// unless `-q`), 1 when it holds none, negative when it cannot be probed --
/// the `errno` then in the second field.
#[cfg(unix)]
fn probe_device(file: &Rc<File>, path: &[u8], quiet: bool, short: &[u8]) -> (i32, i32) {
    let mut pr = ulblkid::Probe::new();
    if pr.set_device(Some(Rc::clone(file)), 0, 0) != 0 {
        return (-1, pr.errno);
    }
    pr.enable_superblocks(true);
    pr.enable_partitions(true);
    let ret = pr.do_fullprobe();
    if ret != 0 {
        return (ret, pr.errno);
    }
    if !quiet {
        let get = |n: &str| pr.lookup_value(n).map(|v| v.as_c_str().to_vec());
        if let Some(t) = get("TYPE") {
            warnx(
                short,
                &format!(
                    "{} contains existing file system ({}).",
                    shown(path),
                    shown(&t)
                ),
            );
        } else if let Some(t) = get("PTTYPE") {
            warnx(
                short,
                &format!(
                    "{} contains existing partition ({}).",
                    shown(path),
                    shown(&t)
                ),
            );
        } else {
            warnx(
                short,
                &format!("{} contains existing signature.", shown(path)),
            );
        }
    }
    (0, 0)
}

/// Whether the file is a block device.
fn is_block(file: &File) -> Result<bool, i32> {
    let meta = file.metadata().map_err(|e| ulblkid::errno_of(&e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        Ok(meta.file_type().is_block_device())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Ok(false)
    }
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(u8, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((*c, value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// A size option: `strtosize_or_err(arg, msg)`.
fn size_arg(value: Option<&OsString>, msg: &str, short: &[u8]) -> Result<u64, Fatal> {
    let arg = value.cloned().unwrap_or_default();
    ulstrutils::parse_size(&os_bytes(&arg)).map_err(|e| {
        warnx(short, &ulstrutils::size_error_message(msg, &arg, e));
        Fatal(1)
    })
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("blkdiscard"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(s) | Err(Fatal(s)) => s,
    };
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("blkdiscard"), OsString::as_os_str);
    let mut range: [u64; 2] = [0, u64::MAX];
    let mut step = 0u64;
    let (mut force, mut quiet, mut verbose) = (false, false, false);
    let mut act = Act::Discard;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let own = argv.get(1..).unwrap_or_default();
    for item in BLKDISCARD.parse(own, "hfVsvo:l:p:qz", LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return Ok(errtryhelp(short));
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(os_bytes(o).into_owned());
            }
            continue;
        };
        match c {
            b'f' => force = true,
            b'l' => range[1] = size_arg(value.as_ref(), "failed to parse length", short)?,
            b'o' => range[0] = size_arg(value.as_ref(), "failed to parse offset", short)?,
            b'p' => step = size_arg(value.as_ref(), "failed to parse step", short)?,
            b'q' => quiet = true,
            b's' => act = Act::Secure,
            b'v' => verbose = true,
            b'z' => act = Act::Zeroout,
            b'h' => {
                out.write(&usage(short));
                return Ok(0);
            }
            b'V' => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return Ok(0);
            }
            _ => return Ok(errtryhelp(short)),
        }
    }

    let Some(path) = operands.first().cloned() else {
        warnx(short, "no device specified");
        return Ok(1);
    };
    if operands.len() != 1 {
        warnx(short, "unexpected number of arguments");
        return Ok(errtryhelp(short));
    }

    // O_RDWR, and O_EXCL unless forced: Linux's values, which SlateOS's C
    // library shares.
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(if force { 0 } else { 0o200 });
    }
    let file = match opts.open(quoting::os_from_bytes(&path)) {
        Ok(f) => Rc::new(f),
        Err(e) => {
            return Err(err_errno(
                short,
                1,
                &format!("cannot open {}", shown(&path)),
                ulblkid::errno_of(&e),
            ));
        }
    };
    match is_block(&file) {
        Err(e) => {
            return Err(err_errno(
                short,
                1,
                &format!("stat of {} failed", shown(&path)),
                e,
            ));
        }
        Ok(false) => {
            warnx(short, &format!("{}: not a block device", shown(&path)));
            return Err(Fatal(1));
        }
        Ok(true) => {}
    }
    discard(
        &file, &path, act, range, step, force, quiet, verbose, out, short,
    )
}

/// The part of `main()` that works on the block device.
#[allow(
    clippy::too_many_arguments,
    reason = "upstream's main's locals, carried into the part only a block device reaches"
)]
#[cfg(unix)]
fn discard(
    file: &Rc<File>,
    path: &[u8],
    act: Act,
    mut range: [u64; 2],
    step: u64,
    force: bool,
    quiet: bool,
    verbose: bool,
    out: &mut Stdout,
    short: &[u8],
) -> Result<u8, Fatal> {
    use std::io::IsTerminal;
    use std::time::Instant;
    let blksize = ioctls::getsize64(file).map_err(|e| {
        err_errno(
            short,
            1,
            &format!("{}: BLKGETSIZE64 ioctl failed", shown(path)),
            e,
        )
    })?;
    let secsize = ioctls::sszget(file).map_err(|e| {
        err_errno(
            short,
            1,
            &format!("{}: BLKSSZGET ioctl failed", shown(path)),
            e,
        )
    })?;
    // `range % secsize`: the int converted to the unsigned operand's type.
    let ss = u64::from(secsize.cast_unsigned());
    let aligned = |v: u64| ss != 0 && v.is_multiple_of(ss);
    if !aligned(range[0]) {
        warnx(
            short,
            &format!(
                "{}: offset {} is not aligned to sector size {secsize}",
                shown(path),
                range[0]
            ),
        );
        return Err(Fatal(1));
    }
    // Is the range's end past the device's?
    if range[0] > blksize {
        warnx(
            short,
            &format!("{}: offset is greater than device size", shown(path)),
        );
        return Err(Fatal(1));
    }
    let mut end = range[0].wrapping_add(range[1]);
    if end < range[0] || end > blksize {
        end = blksize;
    }
    range[1] = if step > 0 {
        step
    } else {
        end.wrapping_sub(range[0])
    };
    if !aligned(range[1]) {
        warnx(
            short,
            &format!(
                "{}: length {} is not aligned to sector size {secsize}",
                shown(path),
                range[1]
            ),
        );
        return Err(Fatal(1));
    }
    if force {
        if !quiet {
            warnx(short, "Operation forced, data will be lost!");
        }
    } else {
        // Signatures already on the device.
        match probe_device(file, path, quiet, short) {
            (0, _) => {
                // Refused only interactively, not to break scripts.
                if std::io::stdin().is_terminal() {
                    warnx(
                        short,
                        "This is destructive operation, data will be lost! Use the -f option to override.",
                    );
                    return Err(Fatal(1));
                }
            }
            (1, _) => {}
            (_, errno) => {
                return Err(err_errno(short, 1, "failed to probe the device", errno));
            }
        }
    }
    let mut stats: [u64; 2] = [range[0], 0];
    let mut last = Instant::now();
    while range[0] < end {
        if range[0].wrapping_add(range[1]) > end {
            range[1] = end.wrapping_sub(range[0]);
        }
        let (request, name) = match act {
            Act::Zeroout => (ioctls::BLKZEROOUT, "BLKZEROOUT"),
            Act::Secure => (ioctls::BLKSECDISCARD, "BLKSECDISCARD"),
            Act::Discard => (ioctls::BLKDISCARD, "BLKDISCARD"),
        };
        if let Err(e) = ioctls::range(file, request, &mut range) {
            let status = if e == EOPNOTSUPP { EXIT_NOTSUPP } else { 1 };
            return Err(err_errno(
                short,
                status,
                &format!("{name}: {} ioctl failed", shown(path)),
                e,
            ));
        }
        stats[1] = stats[1].wrapping_add(range[1]);
        // Progress at most once a second.
        if verbose && step != 0 && last.elapsed().as_secs() >= 1 {
            print_stats(out, act, path, stats);
            stats[0] = stats[0].wrapping_add(stats[1]);
            stats[1] = 0;
            last = Instant::now();
        }
        range[0] = range[0].wrapping_add(range[1]);
    }
    if verbose && stats[1] != 0 {
        print_stats(out, act, path, stats);
    }
    Ok(0)
}

/// Without the block-device requests, nothing can be discarded.
#[cfg(not(unix))]
#[allow(
    clippy::too_many_arguments,
    clippy::unnecessary_wraps,
    reason = "the unix twin's signature"
)]
fn discard(
    file: &Rc<File>,
    path: &[u8],
    act: Act,
    range: [u64; 2],
    step: u64,
    force: bool,
    quiet: bool,
    verbose: bool,
    out: &mut Stdout,
    short: &[u8],
) -> Result<u8, Fatal> {
    let _ = (
        file, path, act, range, step, force, quiet, verbose, out, short,
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_names_the_program() {
        let u = String::from_utf8(usage(b"blkdiscard")).unwrap_or_default();
        assert!(u.starts_with("\nUsage:\n blkdiscard [options] <device>\n"));
        assert!(u.ends_with("\nFor more details see blkdiscard(8).\n"));
    }

    #[test]
    fn options_map_to_their_letters() {
        assert_eq!(LONGS.len(), LONG_VALS.len());
        let opt = Opt::Long("zeroout", None);
        assert_eq!(option_code(&opt).map(|(c, _)| c), Some(b'z'));
    }
}
