//! lsirq -- the kernel's interrupt counters, as a table.
//!
//! A port of util-linux 2.39.3's `sys-utils/lsirq.c` and the half of
//! `irq-common.c` it uses, function by function and with upstream's names,
//! printing through `smartcols` (the libsmartcols port) as upstream prints
//! through libsmartcols; measured against `lsirq from util-linux 2.39.3` by
//! `scripts/lsirq-diff.sh`, on `/proc/interrupts` and `/proc/softirqs`
//! files of its own.
//!
//! This replaces a hand-written program that laid its table out itself and
//! read argv as UTF-8.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **The counters are fixed-width fields.** After the `:`, each of as many
//!   counters as the header names `CPU`s is `sscanf(" %10lu")` -- blanks,
//!   then at most ten characters of `strtoul` -- and the next is looked for
//!   eleven bytes on, however much the last one used; the first that does
//!   not parse leaves every later one unparsed. What follows the last
//!   counter is the name, its runs of white space made single spaces.
//! * **The order.** The default sort is `qsort` with `a->total < b->total`,
//!   a comparator that never answers "less" -- and glibc 2.39's `qsort` is a
//!   merge sort, under which that is a stable sort by descending total
//!   (measured against glibc's own `qsort`). The others are `strverscmp` of
//!   the IRQ, and `strcoll` of the name, which in C and C.UTF-8 is byte
//!   order; all are stable.
//! * **A total is `%ld`** of an `unsigned long`: `-5` in a counter wraps,
//!   and prints as `-5`.
//! * **The messages name `/proc/interrupts`** even for `--softirq`; and the
//!   one for an empty file reports an `errno` nothing set -- `Success` in
//!   the C locale, `No such file or directory` in any other, which is what
//!   loading a locale leaves behind (`smartcols::tty::setlocale_errno`).
//! * **No `close_stdout`**: lsirq never registers it, so a failed write is
//!   never reported and never changes the status.
//! * **`-o DELTA` is accepted** though the help does not list it: the column
//!   exists for `irqtop`, and here it is always 0.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **`strcoll` in a locale other than C and C.UTF-8** sorts by bytes here;
//!   SlateOS has no other locales.

stdfdguard::guard_std_fds!();

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{FL_RIGHT, FL_TRUNC, JsonType, Table};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{c_isspace, err_exclusive_options, string_add_to_idarray, strverscmp};

/// `_PATH_PROC_INTERRUPTS` and `_PATH_PROC_SOFTIRQS`.
const PATH_PROC_INTERRUPTS: &str = "/proc/interrupts";
const PATH_PROC_SOFTIRQS: &str = "/proc/softirqs";

/// Getopt's errors are only sentences here; the referral follows them.
const LSIRQ: Program = Program::new("lsirq", 1);

/// Upstream's option string: GNU order, operands (which are ignored)
/// anywhere.
const SHORTS: &str = "no:s:ShJPV";

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("sort", Takes::Required),
    ("noheadings", Takes::Nothing),
    ("output", Takes::Required),
    ("softirq", Takes::Nothing),
    ("json", Takes::Nothing),
    ("pairs", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];
const LONG_VALS: [u8; 8] = [b's', b'n', b'o', b'S', b'J', b'P', b'h', b'V'];

/// `excl[]`.
const EXCL: [&[i32]; 1] = [&[b'J' as i32, b'P' as i32]];

/// `COL_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Irq,
    Total,
    Delta,
    Name,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
    json_type: JsonType,
}

/// `infos[]`, in `enum` order.
const INFOS: [ColInfo; 4] = [
    ColInfo {
        id: Col::Irq,
        name: "IRQ",
        whint: 0.10,
        flags: FL_RIGHT,
        help: "interrupts",
        json_type: JsonType::String,
    },
    ColInfo {
        id: Col::Total,
        name: "TOTAL",
        whint: 0.10,
        flags: FL_RIGHT,
        help: "total count",
        json_type: JsonType::Number,
    },
    ColInfo {
        id: Col::Delta,
        name: "DELTA",
        whint: 0.10,
        flags: FL_RIGHT,
        help: "delta count",
        json_type: JsonType::Number,
    },
    ColInfo {
        id: Col::Name,
        name: "NAME",
        whint: 0.70,
        flags: FL_TRUNC,
        help: "name",
        json_type: JsonType::String,
    },
];

/// `columns[__COL_COUNT * 2]`.
const MAX_COLUMNS: usize = 8;

/// `softirq_descs[]`: the softirqs' names for people.
const SOFTIRQ_DESCS: [(&[u8], &[u8]); 10] = [
    (b"HI", b"high priority tasklet softirq"),
    (b"TIMER", b"timer softirq"),
    (b"NET_TX", b"network transmit softirq"),
    (b"NET_RX", b"network receive softirq"),
    (b"BLOCK", b"block device softirq"),
    (b"IRQ_POLL", b"IO poll softirq"),
    (b"TASKLET", b"normal priority tasklet softirq"),
    (b"SCHED", b"schedule softirq"),
    (b"HRTIMER", b"high resolution timer softirq"),
    (b"RCU", b"RCU softirq"),
];

/// `struct irq_info`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct IrqInfo {
    irq: Vec<u8>,
    name: Vec<u8>,
    total: u64,
    delta: u64,
}

/// The comparators `set_sort_func_by_name` chooses between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortBy {
    Interrupts,
    Total,
    Delta,
    Name,
}

/// `struct irq_output`.
struct IrqOutput {
    columns: Vec<Col>,
    sort: SortBy,
    json: bool,
    pairs: bool,
    no_headings: bool,
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("lsirq"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `exit`: lsirq registers no `close_stdout`.
    out.flush_at_exit();
    ExitCode::from(status)
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
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options]\n\
\nUtility to display kernel interrupt information.\n\
\nOptions:\n\
\x20-J, --json           use JSON output format\n\
\x20-P, --pairs          use key=\"value\" output format\n\
\x20-n, --noheadings     don't print headings\n\
\x20-o, --output <list>  define which output columns to use\n\
\x20-s, --sort <column>  specify sort column\n\
\x20-S, --softirq        show softirqs instead of interrupts\n\
\n",
    );
    text.extend_from_slice(format!("{:<22}{}\n", " -h, --help", "display this help").as_bytes());
    text.extend_from_slice(format!("{:<22}{}\n", " -V, --version", "display version").as_bytes());
    text.extend_from_slice(b"\nAvailable output columns:\n");
    // `irq_print_columns(stdout, 1)`: without DELTA, which is irqtop's.
    for info in INFOS.iter().filter(|i| i.id != Col::Delta) {
        text.extend_from_slice(format!("  {:<5}  {}\n", info.name, info.help).as_bytes());
    }
    text.extend_from_slice(b"\nFor more details see lsirq(1).\n");
    text
}

/// `irq_column_name_to_id(name, namesz)`: a column by its name, in any case.
/// Unknown, it is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
fn irq_column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
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

/// `set_sort_func_by_name(out, name)`.
fn sort_func_by_name(name: &[u8]) -> Option<SortBy> {
    [
        (&b"IRQ"[..], SortBy::Interrupts),
        (b"TOTAL", SortBy::Total),
        (b"DELTA", SortBy::Delta),
        (b"NAME", SortBy::Name),
    ]
    .iter()
    .find(|(n, _)| n.eq_ignore_ascii_case(name))
    .map(|&(_, s)| s)
}

/// `ltrim_whitespace`.
fn ltrim_whitespace(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !c_isspace(b)).unwrap_or(s.len());
    s.get(start..).unwrap_or_default()
}

/// `rtrim_whitespace`.
fn rtrim_whitespace(s: &[u8]) -> &[u8] {
    let end = s
        .iter()
        .rposition(|&b| !c_isspace(b))
        .map_or(0, |i| i.saturating_add(1));
    s.get(..end).unwrap_or_default()
}

/// `remove_repeated_spaces`: each run of white space one space.
fn remove_repeated_spaces(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut prev_space = false;
    for &b in s {
        if c_isspace(b) {
            if !prev_space {
                out.push(b' ');
                prev_space = true;
            }
        } else {
            out.push(b);
            prev_space = false;
        }
    }
    out
}

/// `sscanf(s, " %10lu", &count)`: blanks, then at most ten characters of
/// an optional sign and digits, as `strtoul` reads them -- a minus sign
/// wraps. `None` for no number.
fn scan_count(s: &[u8]) -> Option<u64> {
    let start = s.iter().position(|&b| !c_isspace(b))?;
    let field = s.get(start..)?;
    let field = field.get(..field.len().min(10)).unwrap_or(field);
    let (negative, digits) = match field.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, field),
    };
    let n = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    // At most ten digits: far inside u64.
    let value = digits.get(..n)?.iter().fold(0u64, |v, &d| {
        v.wrapping_mul(10)
            .wrapping_add(u64::from(d.wrapping_sub(b'0')))
    });
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// Why `get_irqinfo` gave up, reported as upstream reports it.
#[derive(Debug)]
enum ReadError {
    /// `cannot open /proc/interrupts`, and why.
    Open(std::io::Error),
    /// `cannot read /proc/interrupts`: the header's `getline` failed, and
    /// `errno` is what it left -- nothing, at the end of an empty file.
    Read(i32),
}

/// One data line of the file: `get_irqinfo`'s loop body. `None` for a line
/// with no `:`.
fn parse_line(raw: &[u8], nr_active_cpu: usize, softirq: bool) -> Option<IrqInfo> {
    // `line` is a C string; `length` is its `strlen`, newline included.
    let line = smartcols::mbs::c_str(raw);
    let colon = line.iter().position(|&b| b == b':')?;
    let length = line.len();
    let irq = ltrim_whitespace(line.get(..colon).unwrap_or_default()).to_vec();
    let mut total: u64 = 0;
    let mut tmp = colon.saturating_add(1);
    for _ in 0..nr_active_cpu {
        if tmp >= length {
            break;
        }
        // A counter that does not parse is not skipped: upstream looks for
        // every later one in the same place, where each fails the same way.
        let Some(count) = scan_count(line.get(tmp..).unwrap_or_default()) else {
            break;
        };
        total = total.wrapping_add(count);
        tmp = tmp.saturating_add(11);
    }
    let name = if softirq {
        SOFTIRQ_DESCS
            .iter()
            .find(|(i, _)| *i == irq.as_slice())
            .map(|(_, d)| d.to_vec())
            .unwrap_or_default()
    } else if tmp < length {
        let rest = ltrim_whitespace(line.get(tmp..).unwrap_or_default());
        rtrim_whitespace(&remove_repeated_spaces(rest)).to_vec()
    } else {
        Vec::new()
    };
    Some(IrqInfo {
        irq,
        name,
        total,
        delta: 0,
    })
}

/// `get_irqinfo(softirq, 0, NULL)`: every line of the file, parsed.
fn get_irqinfo(softirq: bool, errno: i32) -> Result<Vec<IrqInfo>, ReadError> {
    let path = if softirq {
        PATH_PROC_SOFTIRQS
    } else {
        PATH_PROC_INTERRUPTS
    };
    let file = std::fs::File::open(path).map_err(ReadError::Open)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line) {
        Ok(0) => return Err(ReadError::Read(errno)),
        Ok(_) => {}
        Err(e) => return Err(ReadError::Read(e.raw_os_error().unwrap_or(errno))),
    }
    // The header names one `CPU` for each counter to read.
    let header = smartcols::mbs::c_str(&line);
    let nr_active_cpu = header.windows(3).filter(|w| *w == b"CPU").count();
    let mut irqs = Vec::new();
    loop {
        line.clear();
        // A failing read ends the file, as `getline`'s -1 does.
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if let Some(info) = parse_line(&line, nr_active_cpu, softirq) {
            irqs.push(info);
        }
    }
    Ok(irqs)
}

/// `sort_result`: glibc 2.39's `qsort`, a merge sort, which with each of
/// upstream's comparators is a stable sort -- with `cmp_total`'s
/// `a->total < b->total`, a stable sort by descending total.
fn sort_result(sort: SortBy, result: &mut [IrqInfo]) {
    use std::cmp::Ordering;
    let by_name = |a: &IrqInfo, b: &IrqInfo| a.name.cmp(&b.name);
    match sort {
        SortBy::Total => result.sort_by_key(|i| std::cmp::Reverse(i.total)),
        SortBy::Interrupts => result.sort_by(|a, b| strverscmp(&a.irq, &b.irq)),
        SortBy::Name => result.sort_by(by_name),
        SortBy::Delta => result.sort_by(|a, b| match b.delta.cmp(&a.delta) {
            Ordering::Equal => by_name(a, b),
            other => other,
        }),
    }
}

/// `get_scols_table` with `new_scols_table` and `add_scols_line`.
fn get_scols_table(out: &IrqOutput, irqs: &[IrqInfo]) -> Table {
    let mut table = Table::new();
    table.enable_json(out.json);
    table.enable_noheadings(out.no_headings);
    table.enable_export(out.pairs);
    if out.json {
        table.set_name(b"interrupts");
    }
    let mut ids = Vec::with_capacity(out.columns.len());
    for &col in &out.columns {
        let i = info(col);
        let cl = table.new_column(i.name.as_bytes(), i.whint, i.flags);
        if out.json {
            // The column is the table's own.
            let _ = table.column_set_json_type(cl, i.json_type);
        }
        ids.push((col, cl));
    }
    for irq in irqs {
        let Ok(line) = table.new_line(None) else {
            continue;
        };
        for &(col, cl) in &ids {
            let data = match col {
                Col::Irq => irq.irq.clone(),
                // `%ld` of an unsigned long.
                Col::Total => (irq.total as i64).to_string().into_bytes(),
                Col::Delta => (irq.delta as i64).to_string().into_bytes(),
                Col::Name => irq.name.clone(),
            };
            // The line and column are the table's own.
            let _ = table.line_set_data(line, cl, &data);
        }
    }
    table
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

/// `option_to_longopt(c, longopts)`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| i32::from(v) == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// `main()`.
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("lsirq"), OsString::as_os_str);
    // `setlocale(LC_ALL, "")`, and what it leaves in `errno`.
    let errno = smartcols::tty::setlocale_errno();
    let mut output = IrqOutput {
        columns: Vec::new(),
        sort: SortBy::Total,
        json: false,
        pairs: false,
        no_headings: false,
    };
    let mut outarg: Option<Vec<u8>> = None;
    let mut softirq = false;
    let mut excl_st = [0i32; 1];

    let own = argv.get(1..).unwrap_or_default();
    for item in LSIRQ.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
            }
        };
        // Operands are allowed, and ignored.
        let Some((c, value)) = option_code(&opt) else {
            continue;
        };
        if let Some(msg) =
            err_exclusive_options(i32::from(c), &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return 1;
        }
        let arg = value
            .as_deref()
            .map(os_bytes)
            .unwrap_or_default()
            .into_owned();
        match c {
            b'J' => output.json = true,
            b'P' => output.pairs = true,
            b'n' => output.no_headings = true,
            b'o' => outarg = Some(arg),
            b's' => match sort_func_by_name(&arg) {
                Some(s) => output.sort = s,
                None => {
                    warnx(short, "unsupported column name to sort output");
                    return 1;
                }
            },
            b'S' => softirq = true,
            b'V' => {
                let mut line = short.to_vec();
                line.extend_from_slice(b" from util-linux 2.39.3\n");
                out.write(&line);
                return 0;
            }
            b'h' => {
                out.write(&usage(short));
                return 0;
            }
            _ => return errtryhelp(short),
        }
    }

    output.columns = vec![Col::Irq, Col::Total, Col::Name];
    if let Some(list) = outarg
        && string_add_to_idarray(&list, &mut output.columns, MAX_COLUMNS, |name, rest| {
            irq_column_name_to_id(name, rest, short)
        })
        .is_err()
    {
        return 1;
    }

    let mut irqs = match get_irqinfo(softirq, errno) {
        Ok(irqs) => irqs,
        Err(ReadError::Open(e)) => {
            warn(short, &format!("cannot open {PATH_PROC_INTERRUPTS}"), &e);
            return 1;
        }
        Err(ReadError::Read(errno)) => {
            warn(
                short,
                &format!("cannot read {PATH_PROC_INTERRUPTS}"),
                &std::io::Error::from_raw_os_error(errno),
            );
            return 1;
        }
    };
    sort_result(output.sort, &mut irqs);
    let mut table = get_scols_table(&output, &irqs);
    // Its status is not looked at, as upstream does not look at it.
    let mut text = Vec::new();
    let _ = table.print_into(&mut text);
    out.write(&text);
    0
}

#[cfg(test)]
mod tests;
