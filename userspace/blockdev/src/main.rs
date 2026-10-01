//! blockdev -- call block device ioctls from the command line.
//!
//! A port of util-linux 2.39.3's `disk-utils/blockdev.c`. Each of a list of
//! commands -- `--getro`, `--setra 256`, `--getsz`, ... -- is made as an
//! `ioctl` on each of a list of devices in turn; or `--report` prints a line
//! per device (the read-only flag, read-ahead, logical sector and block
//! sizes, where a partition starts, the size), for the devices named or for
//! every one `/proc/partitions` lists. Measured against `blockdev from
//! util-linux 2.39.3` by `scripts/blockdev-diff.sh`, which runs both under an
//! `ioctl` shim that answers for fixture files as a block device would and
//! logs every request each side makes -- so the requests, their arguments
//! and their order are compared too, not only what is printed.
//!
//! This replaces a hand-written `blockdev` that read its answers from sysfs
//! rather than asking the device, printed no partition start in `--report`,
//! and accepted options upstream refuses.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

use quoting::{escape_unprintable, os_bytes, os_from_bytes};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use ulblkid::blkdev::{
    BLKALIGNOFF, BLKBSZGET, BLKBSZSET, BLKDISCARDZEROES, BLKFLSBUF, BLKFRAGET, BLKFRASET,
    BLKGETDISKSEQ, BLKGETSIZE, BLKGETSIZE64, BLKIOMIN, BLKIOOPT, BLKPBSZGET, BLKRAGET, BLKRASET,
    BLKROGET, BLKROSET, BLKRRPART, BLKSECTGET, BLKSSZGET, get_sectors, get_size, ioctl_ptr,
    ioctl_val, rdev,
};
use ulclosestream::{Stdout, stderr_write, warn, warnx};

#[cfg(test)]
mod tests;

/// `_PATH_PROC_PARTITIONS`.
const PATH_PROC_PARTITIONS: &str = "/proc/partitions";

/// `O_NONBLOCK`: Linux's value, which SlateOS's C library shares -- so a
/// FIFO named to `--report` does not hang the open.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

/// `ARG_*`: what a request's third argument is. Upstream's `ARG_LLONG`
/// names no command, so it has no counterpart here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arg {
    /// `ARG_NONE`: a literal 0.
    None,
    /// `ARG_USHRT`: a pointer to an `unsigned short`.
    Ushrt,
    /// `ARG_INT`: a pointer to an `int` -- or, with `FL_NOPTR`, the `int`.
    Int,
    /// `ARG_UINT`: a pointer to an `unsigned int`.
    Uint,
    /// `ARG_LONG`: a pointer to a `long`.
    Long,
    /// `ARG_ULONG`: a pointer to an `unsigned long`.
    Ulong,
    /// `ARG_ULLONG`: a pointer to an `unsigned long long`.
    Ullong,
}

/// One of upstream's `bdcms[]`: a command and the request it makes.
#[derive(Debug)]
struct Bdc {
    /// `ioc`: the request.
    ioc: u64,
    /// `iocname`: its name, for "ioctl error on".
    iocname: &'static str,
    /// `argval`: the argument's value before the call -- what a request
    /// that sets is given, and what one that fails to get leaves.
    argval: i64,
    /// `name`: the command, `--setro`.
    name: &'static str,
    /// `argname`: the command's own argument, as the help shows it.
    argname: Option<&'static str>,
    /// `help`.
    help: &'static str,
    /// `argtype`.
    argtype: Arg,
    /// `FL_NOPTR`: an `ARG_INT` passed by value, not by pointer.
    noptr: bool,
    /// `FL_NORESULT`: nothing comes back to print.
    noresult: bool,
}

impl Bdc {
    /// An entry with the fields upstream's initializers leave out zero:
    /// `ARG_NONE`, argument 0, no flags.
    const fn new(ioc: u64, iocname: &'static str, name: &'static str, help: &'static str) -> Self {
        Bdc {
            ioc,
            iocname,
            argval: 0,
            name,
            argname: None,
            help,
            argtype: Arg::None,
            noptr: false,
            noresult: false,
        }
    }

    /// `.argtype` and `.argval`.
    const fn arg(mut self, argtype: Arg, argval: i64) -> Self {
        self.argtype = argtype;
        self.argval = argval;
        self
    }

    /// `.argname`.
    const fn argname(mut self, argname: &'static str) -> Self {
        self.argname = Some(argname);
        self
    }

    /// `FL_NOPTR`.
    const fn noptr(mut self) -> Self {
        self.noptr = true;
        self
    }

    /// `FL_NORESULT`.
    const fn noresult(mut self) -> Self {
        self.noresult = true;
        self
    }
}

/// Upstream's `bdcms[]`, in its order -- which is the help's.
const BDCMS: [Bdc; 21] = [
    Bdc::new(BLKROSET, "BLKROSET", "--setro", "set read-only")
        .arg(Arg::Int, 1)
        .noresult(),
    Bdc::new(BLKROSET, "BLKROSET", "--setrw", "set read-write")
        .arg(Arg::Int, 0)
        .noresult(),
    Bdc::new(BLKROGET, "BLKROGET", "--getro", "get read-only").arg(Arg::Int, -1),
    Bdc::new(
        BLKDISCARDZEROES,
        "BLKDISCARDZEROES",
        "--getdiscardzeroes",
        "get discard zeroes support status",
    )
    .arg(Arg::Uint, -1),
    Bdc::new(
        BLKSSZGET,
        "BLKSSZGET",
        "--getss",
        "get logical block (sector) size",
    )
    .arg(Arg::Int, -1),
    Bdc::new(
        BLKPBSZGET,
        "BLKPBSZGET",
        "--getpbsz",
        "get physical block (sector) size",
    )
    .arg(Arg::Uint, -1),
    Bdc::new(BLKIOMIN, "BLKIOMIN", "--getiomin", "get minimum I/O size").arg(Arg::Uint, -1),
    Bdc::new(BLKIOOPT, "BLKIOOPT", "--getioopt", "get optimal I/O size").arg(Arg::Uint, -1),
    Bdc::new(
        BLKALIGNOFF,
        "BLKALIGNOFF",
        "--getalignoff",
        "get alignment offset in bytes",
    )
    .arg(Arg::Int, -1),
    Bdc::new(
        BLKSECTGET,
        "BLKSECTGET",
        "--getmaxsect",
        "get max sectors per request",
    )
    .arg(Arg::Ushrt, -1),
    Bdc::new(BLKBSZGET, "BLKBSZGET", "--getbsz", "get blocksize").arg(Arg::Int, -1),
    Bdc::new(
        BLKBSZSET,
        "BLKBSZSET",
        "--setbsz",
        "set blocksize on file descriptor opening the block device",
    )
    .argname("<bytes>")
    .arg(Arg::Int, 0)
    .noresult(),
    Bdc::new(
        BLKGETSIZE,
        "BLKGETSIZE",
        "--getsize",
        "get 32-bit sector count (deprecated, use --getsz)",
    )
    .arg(Arg::Ulong, -1),
    Bdc::new(
        BLKGETSIZE64,
        "BLKGETSIZE64",
        "--getsize64",
        "get size in bytes",
    )
    .arg(Arg::Ullong, -1),
    Bdc::new(BLKRASET, "BLKRASET", "--setra", "set readahead")
        .argname("<sectors>")
        .arg(Arg::Int, 0)
        .noptr()
        .noresult(),
    Bdc::new(BLKRAGET, "BLKRAGET", "--getra", "get readahead").arg(Arg::Long, -1),
    Bdc::new(
        BLKFRASET,
        "BLKFRASET",
        "--setfra",
        "set filesystem readahead",
    )
    .argname("<sectors>")
    .arg(Arg::Int, 0)
    .noptr()
    .noresult(),
    Bdc::new(
        BLKFRAGET,
        "BLKFRAGET",
        "--getfra",
        "get filesystem readahead",
    )
    .arg(Arg::Long, -1),
    Bdc::new(
        BLKGETDISKSEQ,
        "BLKGETDISKSEQ",
        "--getdiskseq",
        "get disk sequence number",
    )
    .arg(Arg::Ullong, -1),
    Bdc::new(BLKFLSBUF, "BLKFLSBUF", "--flushbufs", "flush buffers"),
    Bdc::new(
        BLKRRPART,
        "BLKRRPART",
        "--rereadpt",
        "reread partition table",
    ),
];

/// A fatal error, already printed: the status to exit with.
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

/// `argv[i]` as bytes; past the end, nothing (never reached: every caller
/// indexes below `argc`).
fn arg_at(argv: &[OsString], i: usize) -> Vec<u8> {
    argv.get(i)
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default()
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let name = shown(short);
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(
        " {name} [-v|-q] commands devices\n {name} --report [devices]\n {name} -h|-V\n"
    ));
    t.push('\n');
    t.push_str("Call block device ioctls from the command line.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -q             quiet mode\n");
    t.push_str(" -v             verbose mode\n");
    t.push_str("     --report   print report for specified (or all) devices\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<16}{}\n{:<16}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push('\n');
    t.push_str("Available commands:\n");
    t.push_str(&format!(
        " {:<25} get size in 512-byte sectors\n",
        "--getsz"
    ));
    for c in &BDCMS {
        match c.argname {
            // `" %s %-*s %s\n"` with the width `24 - strlen(name)`: the
            // name and its argument together fill the column.
            Some(argname) => {
                let width = 24usize.saturating_sub(c.name.len());
                t.push_str(&format!(" {} {:<width$} {}\n", c.name, argname, c.help));
            }
            None => t.push_str(&format!(" {:<25} {}\n", c.name, c.help)),
        }
    }
    t.push_str("\nFor more details see blockdev(8).\n");
    t.into_bytes()
}

/// `find_cmd(s)`: the command's index in [`BDCMS`].
fn find_cmd(s: &[u8]) -> Option<usize> {
    BDCMS.iter().position(|c| c.name.as_bytes() == s)
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("blockdev"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(s) | Err(Fatal(s)) => s,
    };
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let argc = argv.len();
    if argc < 2 {
        warnx(short, "not enough arguments");
        return Err(Fatal(errtryhelp(short)));
    }

    // -V not together with commands
    let first = arg_at(argv, 1);
    if first == b"-V" || first == b"--version" {
        out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
        return Ok(0);
    }
    if first == b"-h" || first == b"--help" {
        out.write(&usage(short));
        return Ok(0);
    }

    // --report not together with other commands
    if first == b"--report" {
        report_header(out);
        if argc > 2 {
            for device in argv.iter().skip(2) {
                report_device(out, short, &os_bytes(device), false);
            }
        } else {
            report_all_devices(out, short)?;
        }
        return Ok(0);
    }

    // The devices start after the last command: a command's own argument
    // is skipped, "--" ends the commands, and so does the first word that
    // does not start with '-'.
    let mut d = 1usize;
    while d < argc {
        let a = arg_at(argv, d);
        if let Some(j) = find_cmd(&a) {
            if BDCMS.get(j).is_some_and(|c| c.argname.is_some()) {
                d = d.saturating_add(1);
            }
            d = d.saturating_add(1);
            continue;
        }
        if a == b"--getsz" {
            d = d.saturating_add(1);
            continue;
        }
        if a == b"--" {
            d = d.saturating_add(1);
            break;
        }
        if a.first() != Some(&b'-') {
            break;
        }
        d = d.saturating_add(1);
    }

    if d >= argc {
        warnx(short, "no device specified");
        return Err(Fatal(errtryhelp(short)));
    }

    // Upstream hands `do_commands` all of argv[1..d] -- a "--" that ended
    // the commands included, which it then refuses as an unknown command.
    let commands = argv.get(1..d).unwrap_or_default();
    for device in argv.iter().skip(d) {
        let f = match File::open(device) {
            Ok(f) => f,
            Err(e) => {
                warn(
                    short,
                    &format!("cannot open {}", shown(&os_bytes(device))),
                    &e,
                );
                return Err(Fatal(1));
            }
        };
        do_commands(out, short, &f, commands)?;
    }
    Ok(0)
}

/// C's conversion of a `long` to an `unsigned short`: modulo 2^16.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "upstream assigns the long argval to an unsigned short"
)]
fn to_ushort(v: i64) -> u16 {
    v as u16
}

/// C's conversion of a `long` to an `int`: modulo 2^32.
#[allow(
    clippy::cast_possible_truncation,
    reason = "upstream assigns the long argval to an int"
)]
fn to_int(v: i64) -> i32 {
    v as i32
}

/// C's conversion of a `long` to an `unsigned int`: modulo 2^32.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "upstream assigns the long argval to an unsigned int"
)]
fn to_uint(v: i64) -> u32 {
    v as u32
}

/// C's conversion of a `long` to an unsigned 64-bit type: modulo 2^64.
fn to_ulong(v: i64) -> u64 {
    u64::from_ne_bytes(v.to_ne_bytes())
}

/// An `int` passed by value through `ioctl`'s `...`: what the register
/// holds, which is what the kernel reads as its `unsigned long` argument.
/// Measured, not assumed: util-linux's blockdev, as Ubuntu's GCC built it,
/// sign-extends it -- `--setra -1` reaches `ioctl` as
/// `0xffff_ffff_ffff_ffff` and `-2147483648` as `0xffff_ffff_8000_0000`
/// (`scripts/blockdev-diff.sh`'s shim logs the argument). A zero-extending
/// guess here was the only thing that harness found.
fn int_by_value(v: i32) -> i64 {
    i64::from(v)
}

/// `do_commands(fd, argv, d)`: `commands` (argv[1..d]) made on one device.
/// Verbosity starts off for every device.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's do_commands, kept in one piece so it can be read against it"
)]
fn do_commands(
    out: &mut Stdout,
    short: &[u8],
    f: &File,
    commands: &[OsString],
) -> Result<(), Fatal> {
    let mut verbose = false;
    let mut it = commands.iter();
    while let Some(word) = it.next() {
        let word = os_bytes(word);
        if *word == *b"-v" {
            verbose = true;
            continue;
        }
        if *word == *b"-q" {
            verbose = false;
            continue;
        }

        if *word == *b"--getsz" {
            match get_sectors(f) {
                Ok(sectors) => out.write(format!("{sectors}\n").as_bytes()),
                Err(_) => {
                    warnx(short, "could not get device size");
                    return Err(Fatal(1));
                }
            }
            continue;
        }

        let Some(c) = find_cmd(&word).and_then(|j| BDCMS.get(j)) else {
            warnx(short, &format!("Unknown command: {}", shown(&word)));
            return Err(Fatal(errtryhelp(short)));
        };

        // The request, and what it left in its argument, printed as
        // upstream's printf prints that type.
        let (res, value) = match c.argtype {
            Arg::None => (ioctl_val(f, c.ioc, 0), String::new()),
            Arg::Ushrt => {
                let mut v = to_ushort(c.argval);
                // SAFETY: the one ARG_USHRT request, BLKSECTGET, writes one
                // unsigned short, which `v` is.
                let r = unsafe { ioctl_ptr(f, c.ioc, &mut v) };
                (r, v.to_string())
            }
            Arg::Int => {
                let mut v = if c.argname.is_some() {
                    let Some(given) = it.next() else {
                        warnx(short, &format!("{} requires an argument", c.name));
                        return Err(Fatal(errtryhelp(short)));
                    };
                    strtos32_or_err(short, given, "failed to parse command argument")?
                } else {
                    to_int(c.argval)
                };
                let r = if c.noptr {
                    ioctl_val(f, c.ioc, int_by_value(v))
                } else {
                    // SAFETY: the ARG_INT requests passed a pointer
                    // (BLKROSET, BLKROGET, BLKSSZGET, BLKALIGNOFF,
                    // BLKBSZGET, BLKBSZSET) read or write one int, which
                    // `v` is.
                    unsafe { ioctl_ptr(f, c.ioc, &mut v) }
                };
                (r, v.to_string())
            }
            Arg::Uint => {
                let mut v = to_uint(c.argval);
                // SAFETY: the ARG_UINT requests (BLKDISCARDZEROES,
                // BLKPBSZGET, BLKIOMIN, BLKIOOPT) write one unsigned int.
                let r = unsafe { ioctl_ptr(f, c.ioc, &mut v) };
                (r, v.to_string())
            }
            Arg::Long => {
                let mut v = c.argval;
                // SAFETY: the ARG_LONG requests (BLKRAGET, BLKFRAGET) write
                // one long, eight bytes as `v` is.
                let r = unsafe { ioctl_ptr(f, c.ioc, &mut v) };
                (r, v.to_string())
            }
            Arg::Ulong | Arg::Ullong => {
                let mut v = to_ulong(c.argval);
                // SAFETY: BLKGETSIZE writes one unsigned long, BLKGETSIZE64
                // and BLKGETDISKSEQ one 64-bit number: eight bytes each.
                let r = unsafe { ioctl_ptr(f, c.ioc, &mut v) };
                (r, v.to_string())
            }
        };

        if let Err(errno) = res {
            warn(
                short,
                &format!("ioctl error on {}", c.iocname),
                &io::Error::from_raw_os_error(errno),
            );
            if verbose {
                out.write(format!("{} failed.\n", c.help).as_bytes());
            }
            return Err(Fatal(1));
        }

        if c.argtype == Arg::None || c.noresult {
            if verbose {
                out.write(format!("{} succeeded.\n", c.help).as_bytes());
            }
            continue;
        }

        if verbose {
            out.write(format!("{}: ", c.help).as_bytes());
        }
        out.write(format!("{value}\n").as_bytes());
    }
    Ok(())
}

/// `strtos32_or_err(str, errmesg)`, base 10.
fn strtos32_or_err(short: &[u8], s: &OsStr, errmesg: &str) -> Result<i32, Fatal> {
    ulstrutils::ul_strtos32(&os_bytes(s), 10).map_err(|e| {
        warnx(short, &ulstrutils::num_error_message(errmesg, s, e));
        Fatal(1)
    })
}

/// `report_header()`.
fn report_header(out: &mut Stdout) {
    out.write(b"RO    RA   SSZ   BSZ        StartSec            Size   Device\n");
}

/// `open(device, O_RDONLY | O_NONBLOCK)`.
fn open_nonblock(device: &[u8]) -> io::Result<File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(O_NONBLOCK);
    }
    opts.open(os_from_bytes(device))
}

/// `snprintf(buf, 16, ...)`: at most 15 bytes of `s`.
fn cut15(s: &str) -> String {
    s.chars().take(15).collect()
}

/// `report_device(device, quiet)`: one line of `--report`. Nothing is
/// fatal: a device that will not open or answer is warned about (unless
/// `quiet`) and passed over.
fn report_device(out: &mut Stdout, short: &[u8], device: &[u8], quiet: bool) {
    let f = match open_nonblock(device) {
        Ok(f) => f,
        Err(e) => {
            if !quiet {
                warn(short, &format!("cannot open {}", shown(device)), &e);
            }
            return;
        }
    };

    // Where a partition starts, from sysfs: "N/A" for a partition whose
    // `start` cannot be read, 0 for a whole disk (or no block device).
    let mut start = 0u64;
    let mut start_str = None;
    if let Ok(meta) = f.metadata() {
        let st_rdev = rdev(&meta);
        if let Some(pc) = ulsysfs::new_sysfs_path(st_rdev, None, None)
            && let Some((_, disk)) = pc.blkdev_wholedisk(0)
            && disk != st_rdev
        {
            match pc.read_u64(b"start") {
                Some(s) => start = s,
                None => start_str = Some(format!("{:>15}", "N/A")),
            }
        }
    }
    let start_str = start_str.unwrap_or_else(|| cut15(&format!("{start:>15}")));

    let mut ro: i32 = 0;
    let mut ra: i64 = 0;
    let mut ssz: i32 = 0;
    let mut bsz: i32 = 0;
    // SAFETY (all four): BLKROGET, BLKSSZGET and BLKBSZGET write one int,
    // BLKRAGET one long -- the types of `ro`, `ssz`, `bsz` and `ra`.
    let answered = unsafe { ioctl_ptr(&f, BLKROGET, &mut ro) }.is_ok()
        && unsafe { ioctl_ptr(&f, BLKRAGET, &mut ra) }.is_ok()
        && unsafe { ioctl_ptr(&f, BLKSSZGET, &mut ssz) }.is_ok()
        && unsafe { ioctl_ptr(&f, BLKBSZGET, &mut bsz) }.is_ok();
    let bytes = if answered { get_size(&f).ok() } else { None };
    match bytes {
        Some(bytes) => {
            // `%15lld` of an unsigned long long: a size past 2^63 prints
            // negative, as upstream's does.
            let signed = i64::from_ne_bytes(bytes.to_ne_bytes());
            let mut line = format!(
                "{} {ra:>5} {ssz:>5} {bsz:>5} {start_str} {signed:>15}   ",
                if ro != 0 { "ro" } else { "rw" }
            )
            .into_bytes();
            line.extend_from_slice(device);
            line.push(b'\n');
            out.write(&line);
        }
        None => {
            if !quiet {
                warnx(short, &format!("ioctl error on {}", shown(device)));
            }
        }
    }
}

/// `fgets(line, size, f)` over text already read: each piece ends after a
/// newline or after `size - 1` bytes, whichever comes first.
fn fgets_pieces(text: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    let max = size.saturating_sub(1).max(1);
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let limit = rest.len().min(max);
        let end = rest
            .get(..limit)
            .and_then(|head| head.iter().position(|&b| b == b'\n'))
            .map_or(limit, |nl| nl.saturating_add(1));
        let (piece, tail) = rest.split_at(end);
        rest = tail;
        Some(piece)
    })
}

/// C's `isspace` in the C locale.
fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `sscanf(line, " %d %d %d %200[^\n ]", ...) == 4`: the name the line
/// ends with, or `None` when any of the four conversions fails.
fn scan_partition_line(line: &[u8]) -> Option<&[u8]> {
    // sscanf reads a C string.
    let line = line.split(|&b| b == 0).next().unwrap_or_default();
    let mut pos = 0usize;
    let skip_space = |pos: &mut usize| {
        while line.get(*pos).is_some_and(|&b| c_isspace(b)) {
            *pos = pos.saturating_add(1);
        }
    };
    for _ in 0..3 {
        // `%d`: blanks, a sign, then at least one digit.
        skip_space(&mut pos);
        if matches!(line.get(pos), Some(b'+' | b'-')) {
            pos = pos.saturating_add(1);
        }
        let digits = line
            .get(pos..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        pos = pos.saturating_add(digits);
    }
    skip_space(&mut pos);
    let tail = line.get(pos..).unwrap_or_default();
    let len = tail
        .iter()
        .take(200)
        .take_while(|&&b| b != b'\n' && b != b' ')
        .count();
    if len == 0 {
        return None;
    }
    tail.get(..len)
}

/// `report_all_devices()`: every device `/proc/partitions` lists, quietly.
fn report_all_devices(out: &mut Stdout, short: &[u8]) -> Result<(), Fatal> {
    let mut f = match File::open(PATH_PROC_PARTITIONS) {
        Ok(f) => f,
        Err(e) => {
            warn(short, &format!("cannot open {PATH_PROC_PARTITIONS}"), &e);
            return Err(Fatal(1));
        }
    };
    let mut text = Vec::new();
    // fgets stops at a read error as at the end of the file, and what was
    // read before the error is still reported: `read_to_end` keeps those
    // bytes in `text` whether or not it fails, so its result has nothing
    // left to say.
    let _ = f.read_to_end(&mut text);
    for line in fgets_pieces(&text, 200) {
        if let Some(name) = scan_partition_line(line) {
            let mut device = b"/dev/".to_vec();
            device.extend_from_slice(name);
            report_device(out, short, &device, true);
        }
    }
    Ok(())
}
