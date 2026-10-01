//! prlimit -- get and set process resource limits.
//!
//! A port of util-linux 2.39.3's `sys-utils/prlimit.c`, function by function
//! and with upstream's names, printing its table through `smartcols` (the
//! libsmartcols port) as upstream prints through libsmartcols; measured
//! against `prlimit from util-linux 2.39.3` by `scripts/prlimit-diff.sh`.
//!
//! This replaces a hand-written program that issued raw Linux system-call
//! instructions rather than calling the C library, read argv as UTF-8, laid
//! its table out itself, and carried an unreachable `ulimit` personality.
//!
//! Upstream's parsing quirks are kept, because scripts depend on what a limit
//! string means: a value is `strtoull`'s, so `-1` is unlimited and `10abc` is
//! 10 (nothing checks what follows a lone value); `unlimitedfoo` is
//! unlimited; and `-p 0 -p 1` is not "--pid given twice", since only a
//! non-zero PID counts as given.
//!
//! Output is buffered as glibc buffers it (`ulclosestream`): when prlimit
//! then runs a COMMAND, what it printed and had not flushed is lost with the
//! process image, as upstream's is -- on a pipe, `prlimit --nofile CMD`
//! prints only CMD's output; on a terminal the table comes first.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

mod sys;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{FL_RIGHT, FL_TRUNC, Table};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{IdListError, num_error_message, string_to_idarray, ul_strtos32};

/// `RLIM_INFINITY`.
const RLIM_INFINITY: u64 = u64::MAX;

/// `PRLIMIT_SOFT` and `PRLIMIT_HARD`: which half of a limit an option set.
const PRLIMIT_SOFT: u8 = 1 << 1;
const PRLIMIT_HARD: u8 = 1 << 2;

/// `EX_EXEC_FAILED` and `EX_EXEC_ENOENT`: `errexec`'s statuses.
const EX_EXEC_FAILED: u8 = 126;
const EX_EXEC_ENOENT: u8 = 127;

/// Getopt's errors are only sentences here; the referral follows them.
const PRLIMIT: Program = Program::new("prlimit", 1);

/// Upstream's option string: `+`, so COMMAND ends the options; each
/// resource takes an optional value, attached (`-n5`, `-n=5`); and `v` is
/// there twice, the first -- `--as`'s -- being the one getopt reads.
const SHORTS: &str = "+c::d::e::f::i::l::m::n::q::r::s::t::u::v::x::y::p:o:vVh";

/// `VERBOSE_OPTION`, `RAW_OPTION`, `NOHEADINGS_OPTION`: `CHAR_MAX + 1` on.
const VERBOSE_OPTION: i32 = 128;
const RAW_OPTION: i32 = 129;
const NOHEADINGS_OPTION: i32 = 130;

/// Upstream's `longopts[]`, in its order (the order an ambiguity lists), and
/// each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("pid", Takes::Required),
    ("output", Takes::Required),
    ("as", Takes::Optional),
    ("core", Takes::Optional),
    ("cpu", Takes::Optional),
    ("data", Takes::Optional),
    ("fsize", Takes::Optional),
    ("locks", Takes::Optional),
    ("memlock", Takes::Optional),
    ("msgqueue", Takes::Optional),
    ("nice", Takes::Optional),
    ("nofile", Takes::Optional),
    ("nproc", Takes::Optional),
    ("rss", Takes::Optional),
    ("rtprio", Takes::Optional),
    ("rttime", Takes::Optional),
    ("sigpending", Takes::Optional),
    ("stack", Takes::Optional),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("verbose", Takes::Nothing),
];
const LONG_VALS: [i32; 23] = [
    b'p' as i32,
    b'o' as i32,
    b'v' as i32,
    b'c' as i32,
    b't' as i32,
    b'd' as i32,
    b'f' as i32,
    b'x' as i32,
    b'l' as i32,
    b'q' as i32,
    b'e' as i32,
    b'n' as i32,
    b'u' as i32,
    b'm' as i32,
    b'r' as i32,
    b'y' as i32,
    b'i' as i32,
    b's' as i32,
    b'V' as i32,
    b'h' as i32,
    NOHEADINGS_OPTION,
    RAW_OPTION,
    VERBOSE_OPTION,
];

/// `struct prlimit_desc`.
struct PrlimitDesc {
    name: &'static str,
    help: &'static str,
    unit: Option<&'static str>,
    /// `RLIMIT_*`, Linux's numbering, which SlateOS's C library shares.
    resource: i32,
    /// The short option that names it.
    flag: u8,
}

/// `prlimit_desc[]`, in `enum { AS, CORE, ... }` order: the order of the
/// default listing.
const PRLIMIT_DESC: [PrlimitDesc; 16] = [
    PrlimitDesc {
        name: "AS",
        help: "address space limit",
        unit: Some("bytes"),
        resource: 9,
        flag: b'v',
    },
    PrlimitDesc {
        name: "CORE",
        help: "max core file size",
        unit: Some("bytes"),
        resource: 4,
        flag: b'c',
    },
    PrlimitDesc {
        name: "CPU",
        help: "CPU time",
        unit: Some("seconds"),
        resource: 0,
        flag: b't',
    },
    PrlimitDesc {
        name: "DATA",
        help: "max data size",
        unit: Some("bytes"),
        resource: 2,
        flag: b'd',
    },
    PrlimitDesc {
        name: "FSIZE",
        help: "max file size",
        unit: Some("bytes"),
        resource: 1,
        flag: b'f',
    },
    PrlimitDesc {
        name: "LOCKS",
        help: "max number of file locks held",
        unit: Some("locks"),
        resource: 10,
        flag: b'x',
    },
    PrlimitDesc {
        name: "MEMLOCK",
        help: "max locked-in-memory address space",
        unit: Some("bytes"),
        resource: 8,
        flag: b'l',
    },
    PrlimitDesc {
        name: "MSGQUEUE",
        help: "max bytes in POSIX mqueues",
        unit: Some("bytes"),
        resource: 12,
        flag: b'q',
    },
    PrlimitDesc {
        name: "NICE",
        help: "max nice prio allowed to raise",
        unit: None,
        resource: 13,
        flag: b'e',
    },
    PrlimitDesc {
        name: "NOFILE",
        help: "max number of open files",
        unit: Some("files"),
        resource: 7,
        flag: b'n',
    },
    PrlimitDesc {
        name: "NPROC",
        help: "max number of processes",
        unit: Some("processes"),
        resource: 6,
        flag: b'u',
    },
    PrlimitDesc {
        name: "RSS",
        help: "max resident set size",
        unit: Some("bytes"),
        resource: 5,
        flag: b'm',
    },
    PrlimitDesc {
        name: "RTPRIO",
        help: "max real-time priority",
        unit: None,
        resource: 14,
        flag: b'r',
    },
    PrlimitDesc {
        name: "RTTIME",
        help: "timeout for real-time tasks",
        unit: Some("microsecs"),
        resource: 15,
        flag: b'y',
    },
    PrlimitDesc {
        name: "SIGPENDING",
        help: "max number of pending signals",
        unit: Some("signals"),
        resource: 11,
        flag: b'i',
    },
    PrlimitDesc {
        name: "STACK",
        help: "max stack size",
        unit: Some("bytes"),
        resource: 3,
        flag: b's',
    },
];

/// `COL_*`, in upstream's enum order -- which is `infos[]`' order, and so
/// the order `--help` lists them: DESCRIPTION before RESOURCE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Help,
    Res,
    Soft,
    Hard,
    Units,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
}

/// `infos[]`.
const INFOS: [ColInfo; 5] = [
    ColInfo {
        id: Col::Help,
        name: "DESCRIPTION",
        whint: 0.1,
        flags: FL_TRUNC,
        help: "resource description",
    },
    ColInfo {
        id: Col::Res,
        name: "RESOURCE",
        whint: 0.25,
        flags: FL_TRUNC,
        help: "resource name",
    },
    ColInfo {
        id: Col::Soft,
        name: "SOFT",
        whint: 0.1,
        flags: FL_RIGHT,
        help: "soft limit",
    },
    ColInfo {
        id: Col::Hard,
        name: "HARD",
        whint: 1.0,
        flags: FL_RIGHT,
        help: "hard limit (ceiling)",
    },
    ColInfo {
        id: Col::Units,
        name: "UNITS",
        whint: 0.1,
        flags: FL_TRUNC,
        help: "units",
    },
];

/// `columns[ARRAY_SIZE(infos) * 2]`.
const MAX_COLUMNS: usize = 10;

/// `struct prlimit`: one resource to show or to change.
struct Prlimit {
    /// Index into [`PRLIMIT_DESC`].
    desc: usize,
    /// `rlim_cur`, `rlim_max`.
    cur: u64,
    max: u64,
    /// `PRLIMIT_{SOFT,HARD}` mask: 0 to show the limit.
    modify: u8,
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without is closed again, as upstream -- and COMMAND, which
    // inherits it -- would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("prlimit"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `close_stdout`, which upstream registers with `atexit`.
    ExitCode::from(out.close(status, &short))
}

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

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n".to_vec();
    for form in [
        &b" [options] [--<resource>=<limit>] [-p PID]\n"[..],
        b" [options] [--<resource>=<limit>] COMMAND\n",
    ] {
        text.push(b' ');
        text.extend_from_slice(short);
        text.extend_from_slice(form);
    }
    text.extend_from_slice(
        b"\nShow or change the resource limits of a process.\n\
\nOptions:\n\
\x20-p, --pid <pid>        process id\n\
\x20-o, --output <list>    define which output columns to use\n\
\x20    --noheadings       don't print headings\n\
\x20    --raw              use the raw output format\n\
\x20    --verbose          verbose output\n\
\x20-h, --help             display this help\n\
\x20-V, --version          display version\n\
\nResources:\n\
\x20-c, --core             maximum size of core files created\n\
\x20-d, --data             maximum size of a process's data segment\n\
\x20-e, --nice             maximum nice priority allowed to raise\n\
\x20-f, --fsize            maximum size of files written by the process\n\
\x20-i, --sigpending       maximum number of pending signals\n\
\x20-l, --memlock          maximum size a process may lock into memory\n\
\x20-m, --rss              maximum resident set size\n\
\x20-n, --nofile           maximum number of open files\n\
\x20-q, --msgqueue         maximum bytes in POSIX message queues\n\
\x20-r, --rtprio           maximum real-time scheduling priority\n\
\x20-s, --stack            maximum stack size\n\
\x20-t, --cpu              maximum amount of CPU time in seconds\n\
\x20-u, --nproc            maximum number of user processes\n\
\x20-v, --as               size of virtual memory\n\
\x20-x, --locks            maximum number of file locks\n\
\x20-y, --rttime           CPU time in microseconds a process scheduled\n\
\x20                       under real-time scheduling\n\
\nArguments:\n\
\x20<limit> is defined as a range soft:hard, soft:, :hard or a value to\n\
\x20        define both limits (e.g. -e=0:10 -r=:10).\n\
\nAvailable output columns:\n",
    );
    for info in &INFOS {
        text.extend_from_slice(format!(" {:>11}  {}\n", info.name, info.help).as_bytes());
    }
    text.extend_from_slice(b"\nFor more details see prlimit(1).\n");
    text
}

/// `column_name_to_id(name, namesz)`: a column by its name, in any case.
/// Unknown, it is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = INFOS
        .iter()
        .find(|i| i.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|i| i.id);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// A column's `infos[]` entry.
fn info(col: Col) -> &'static ColInfo {
    INFOS.iter().find(|i| i.id == col).unwrap_or(&INFOS[0])
}

/// `strtoull(str, &end, 10)`: the value, where the digits end, and whether
/// it overflowed (`ERANGE`). `None` when nothing was converted. A minus sign
/// negates in two's complement, so `-1` is `ULLONG_MAX` -- unlimited.
fn strtoull(s: &[u8]) -> Option<(u64, usize, bool)> {
    let sc = ulstrutils::scan_integer(s, 10)?;
    match u64::try_from(sc.magnitude) {
        Ok(v) if !sc.saturated => {
            let v = if sc.negative { v.wrapping_neg() } else { v };
            Some((v, sc.end, false))
        }
        _ => Some((u64::MAX, sc.end, true)),
    }
}

/// `strtoull` that must consume all of `s`: upstream's
/// `errno || !end || *end || end == str` refusal.
fn strtoull_whole(s: &[u8]) -> Result<u64, ()> {
    match strtoull(s) {
        Some((v, end, false)) if end == s.len() => Ok(v),
        _ => Err(()),
    }
}

/// `get_range(str, &soft, &hard, &found)`: a limit string's soft and hard
/// values and which of them it set.
fn get_range(s: &[u8]) -> Result<(u64, u64, u8), ()> {
    let mut soft = RLIM_INFINITY;
    let mut hard = RLIM_INFINITY;
    if s == b"unlimited" {
        return Ok((soft, hard, PRLIMIT_SOFT | PRLIMIT_HARD));
    }
    if let Some(rest) = s.strip_prefix(b":") {
        // <:hard>
        if rest != b"unlimited" {
            hard = strtoull_whole(rest)?;
        }
        return Ok((soft, hard, PRLIMIT_HARD));
    }
    let end = if s.starts_with(b"unlimited") {
        // <unlimited> or <unlimited:>, and -- since only a `:` is looked
        // for after it -- `unlimitedfoo` too.
        b"unlimited".len()
    } else {
        // <value> or <soft:>. What follows the digits is not checked unless
        // it starts with `:`: `10abc` is 10.
        match strtoull(s) {
            Some((v, end, false)) => {
                soft = v;
                hard = v;
                end
            }
            _ => return Err(()),
        }
    };
    let tail = s.get(end..).unwrap_or_default();
    let found = if tail == b":" {
        // <soft:>
        PRLIMIT_SOFT
    } else if let Some(h) = tail.strip_prefix(b":") {
        // <soft:hard>
        hard = if h == b"unlimited" {
            RLIM_INFINITY
        } else {
            strtoull_whole(h)?
        };
        PRLIMIT_SOFT | PRLIMIT_HARD
    } else {
        // <value>
        PRLIMIT_SOFT | PRLIMIT_HARD
    };
    Ok((soft, hard, found))
}

/// `parse_prlim(&lim, ops, id)`: an option's value -- a leading `=` dropped,
/// as `-n=5` hands getopt `=5` -- into `lim`. Refused, it is upstream's
/// `failed to parse NAME limit`.
fn parse_prlim(lim: &mut Prlimit, ops: &[u8], short: &[u8]) -> Result<(), u8> {
    let ops = ops.strip_prefix(b"=").unwrap_or(ops);
    match get_range(ops) {
        Ok((soft, hard, found)) => {
            lim.cur = soft;
            lim.max = hard;
            lim.modify = found;
            Ok(())
        }
        Err(()) => {
            let name = PRLIMIT_DESC.get(lim.desc).map_or("", |d| d.name);
            warnx(short, &format!("failed to parse {name} limit"));
            Err(1)
        }
    }
}

/// `add_prlim(ops, lims, id)`.
fn add_prlim(
    ops: Option<&[u8]>,
    lims: &mut Vec<Prlimit>,
    id: usize,
    short: &[u8],
) -> Result<(), u8> {
    let mut lim = Prlimit {
        desc: id,
        cur: 0,
        max: 0,
        modify: 0,
    };
    if let Some(ops) = ops {
        parse_prlim(&mut lim, ops, short)?;
    }
    lims.push(lim);
    Ok(())
}

/// `get_unknown_hardsoft(lim)`: the half of a limit the option left out is
/// the process's current one.
fn get_unknown_hardsoft(pid: i32, lim: &mut Prlimit, short: &[u8]) -> Result<(), u8> {
    let desc = PRLIMIT_DESC.get(lim.desc).ok_or(1u8)?;
    match sys::prlimit(pid, desc.resource, None) {
        Ok((cur, max)) => {
            if lim.modify & PRLIMIT_SOFT == 0 {
                lim.cur = cur;
            } else if lim.modify & PRLIMIT_HARD == 0 {
                lim.max = max;
            }
            Ok(())
        }
        Err(e) => {
            warn(short, &format!("failed to get old {} limit", desc.name), &e);
            Err(1)
        }
    }
}

/// `%ju` of a limit, or `unlimited`.
fn limit_text(v: u64) -> String {
    if v == RLIM_INFINITY {
        "unlimited".to_string()
    } else {
        v.to_string()
    }
}

/// `do_prlimit(lims)`: set each limit an option gave a value, and read the
/// rest. What was set is dropped from the listing.
fn do_prlimit(
    pid: i32,
    verbose: bool,
    lims: &mut Vec<Prlimit>,
    out: &mut Stdout,
    short: &[u8],
) -> Result<(), u8> {
    let mut shown_lims = Vec::new();
    for mut lim in std::mem::take(lims) {
        let desc = PRLIMIT_DESC.get(lim.desc).ok_or(1u8)?;
        if lim.modify != 0 {
            if lim.modify != PRLIMIT_HARD | PRLIMIT_SOFT {
                get_unknown_hardsoft(pid, &mut lim, short)?;
            }
            if lim.cur > lim.max && (lim.cur != RLIM_INFINITY || lim.max != RLIM_INFINITY) {
                warnx(
                    short,
                    &format!("the soft limit {} cannot exceed the hard limit", desc.name),
                );
                return Err(1);
            }
            if verbose {
                let shown_pid = if pid == 0 {
                    i64::from(std::process::id())
                } else {
                    i64::from(pid)
                };
                out.write(
                    format!(
                        "New {} limit for pid {shown_pid}: <{}:{}>\n",
                        desc.name,
                        limit_text(lim.cur),
                        limit_text(lim.max)
                    )
                    .as_bytes(),
                );
            }
            if let Err(e) = sys::prlimit(pid, desc.resource, Some((lim.cur, lim.max))) {
                warn(
                    short,
                    &format!("failed to set the {} resource limit", desc.name),
                    &e,
                );
                return Err(1);
            }
            // Modify only; not shown.
        } else {
            match sys::prlimit(pid, desc.resource, None) {
                Ok((cur, max)) => {
                    lim.cur = cur;
                    lim.max = max;
                }
                Err(e) => {
                    warn(
                        short,
                        &format!("failed to get the {} resource limit", desc.name),
                        &e,
                    );
                    return Err(1);
                }
            }
            shown_lims.push(lim);
        }
    }
    *lims = shown_lims;
    Ok(())
}

/// `add_scols_line` and `show_limits`: the table.
fn show_limits(lims: &[Prlimit], columns: &[Col], raw: bool, no_headings: bool) -> Vec<u8> {
    let mut tb = Table::new();
    tb.enable_raw(raw);
    tb.enable_noheadings(no_headings);
    let ids: Vec<_> = columns
        .iter()
        .map(|&c| {
            let i = info(c);
            (c, tb.new_column(i.name.as_bytes(), i.whint, i.flags))
        })
        .collect();
    for lim in lims {
        let Some(desc) = PRLIMIT_DESC.get(lim.desc) else {
            continue;
        };
        let Ok(line) = tb.new_line(None) else {
            continue;
        };
        for &(col, id) in &ids {
            let text = match col {
                Col::Res => Some(desc.name.to_string()),
                Col::Help => Some(desc.help.to_string()),
                Col::Soft => Some(limit_text(lim.cur)),
                Col::Hard => Some(limit_text(lim.max)),
                Col::Units => desc.unit.map(str::to_string),
            };
            if let Some(text) = text {
                // The line and column are this table's own.
                let _ = tb.line_set_data(line, id, text.as_bytes());
            }
        }
    }
    // `scols_print_table`'s status is not looked at, as upstream does not
    // look at it: what it printed before any failure is the output.
    let mut text = Vec::new();
    let _ = tb.print_into(&mut text);
    text
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(i32, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((i32::from(*c), value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("prlimit"), OsString::as_os_str);
    let mut lims: Vec<Prlimit> = Vec::new();
    let mut pid: i32 = 0;
    let mut verbose = false;
    let mut raw = false;
    let mut no_headings = false;
    let mut columns: Vec<Col> = Vec::new();

    let own = argv.get(1..).unwrap_or_default();
    let mut parser = PRLIMIT.parse(own, SHORTS, LONGS);
    let mut command: Option<usize> = None;
    while let Some(item) = parser.next() {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            // `+`: the first operand is COMMAND, and the rest its arguments.
            command = Some(parser.optind().saturating_sub(1));
            break;
        };
        let value_bytes = value.as_deref().map(os_bytes);
        // A resource's option, `-c` to `-y`: a value sets it, none shows it.
        let resource = u8::try_from(c)
            .ok()
            .and_then(|f| PRLIMIT_DESC.iter().position(|d| d.flag == f));
        if let Some(id) = resource {
            if let Err(status) = add_prlim(value_bytes.as_deref(), &mut lims, id, short) {
                return status;
            }
            continue;
        }
        match c {
            NOHEADINGS_OPTION => no_headings = true,
            VERBOSE_OPTION => verbose = true,
            RAW_OPTION => raw = true,
            _ => match u8::try_from(c).unwrap_or(0) {
                b'p' => {
                    if pid != 0 {
                        warnx(short, "option --pid may be specified only once");
                        return 1;
                    }
                    let value = value.unwrap_or_default();
                    match ul_strtos32(&os_bytes(&value), 10) {
                        Ok(p) => pid = p,
                        Err(e) => {
                            warnx(short, &num_error_message("invalid PID argument", &value, e));
                            return 1;
                        }
                    }
                }
                b'o' => {
                    let list = value.as_deref().map(os_bytes).unwrap_or_default();
                    // `string_to_idarray` from the start each time: a second
                    // `-o` replaces the first.
                    columns.clear();
                    let parsed: Result<usize, IdListError> =
                        string_to_idarray(&list, &mut columns, MAX_COLUMNS, |name, rest| {
                            column_name_to_id(name, rest, short)
                        });
                    if parsed.is_err() {
                        return 1;
                    }
                }
                b'h' => {
                    out.write(&usage(short));
                    return 0;
                }
                b'V' => {
                    let mut line = short.to_vec();
                    line.extend_from_slice(b" from util-linux 2.39.3\n");
                    out.write(&line);
                    return 0;
                }
                _ => return errtryhelp(short),
            },
        }
    }
    let command = command.and_then(|i| own.get(i..)).filter(|c| !c.is_empty());
    if command.is_some() && pid != 0 {
        warnx(short, "options --pid and COMMAND are mutually exclusive");
        return 1;
    }
    if columns.is_empty() {
        columns = vec![Col::Res, Col::Help, Col::Soft, Col::Hard, Col::Units];
    }
    if lims.is_empty() {
        // Default: every resource.
        for id in 0..PRLIMIT_DESC.len() {
            lims.push(Prlimit {
                desc: id,
                cur: 0,
                max: 0,
                modify: 0,
            });
        }
    }
    if let Err(status) = do_prlimit(pid, verbose, &mut lims, out, short) {
        return status;
    }
    if !lims.is_empty() {
        out.write(&show_limits(&lims, &columns, raw, no_headings));
    }
    if let Some(command) = command {
        // `execvp`; what stdout holds is lost with the process image if it
        // runs, and printed at exit if it does not -- `errexec` is `err`.
        let e = sys::exec(command);
        let name = command
            .first()
            .map(|n| os_bytes(n).into_owned())
            .unwrap_or_default();
        warn(short, &format!("failed to execute {}", shown(&name)), &e);
        return if e.kind() == std::io::ErrorKind::NotFound {
            EX_EXEC_ENOENT
        } else {
            EX_EXEC_FAILED
        };
    }
    0
}

#[cfg(test)]
mod tests;
