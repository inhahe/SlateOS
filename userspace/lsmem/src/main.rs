//! lsmem -- list the ranges of available memory with their online status.
//!
//! A port of util-linux 2.39.3's `sys-utils/lsmem.c`, function by function
//! and with upstream's names, printing through `smartcols` (libsmartcols,
//! ported) as upstream prints through libsmartcols; measured against `lsmem
//! from util-linux 2.39.3` by `scripts/lsmem-diff.sh`, on WSL's own `/sys`
//! and on trees built for it under `--sysroot`.
//!
//! This replaces a hand-written program that looked like lsmem and was not:
//! it invented a 128 MiB block size when `block_size_bytes` could not be
//! read, took a block with no `state` file for online, showed only the first
//! of a block's zones, read names lossily, parsed its options by hand -- no
//! abbreviations, no `--opt=value` -- and laid its table out itself.
//!
//! # What is not upstream's
//!
//! * **`errno` in two messages.** When the memory directory lists no blocks,
//!   and when `block_size_bytes` is empty, upstream reports whatever `errno`
//!   holds by then. Most of that is lsmem's own doing, and is followed here
//!   ([`Errno`]): the table's terminal query leaves `ENOTTY` when stdout is
//!   not a terminal, the `memory0/valid_zones` probe leaves `ENOENT` when
//!   there is none, a `node` entry's `strtol` resets it. What glibc's
//!   `setlocale` leaves while looking for locale files in a UTF-8 locale is
//!   not -- SlateOS has none to look for. In the C locale the two agree.
//! * **A name in a diagnostic** has its unprintable bytes escaped, where
//!   upstream pastes it (design-decisions §370, §1033).
//! * **A reader that went away** is not reported: upstream dies of
//!   `SIGPIPE`, which SlateOS does not send (see `ulclosestream`, which is
//!   util-linux's `close_stdout` and the stdio buffering it judges).
//! * **Paths where upstream holds a descriptor** -- see [`path`].
//! * **The decimal point** of a size is always `.`: upstream asks
//!   `localeconv()`, and SlateOS's locales are C and C.UTF-8.

mod path;

use getoptlong::{Opt, Program, Takes};
use path::SysPath;
use quoting::{escape_unprintable, os_bytes};
use smartcols::{ColumnId, FL_RIGHT, JsonType, Table};
use std::ffi::{OsStr, OsString};
use std::io;
use std::process::ExitCode;
use ulclosestream::{Stdout as Out, warn, warnx};
use ulstrutils::{
    IdListError, SIZE_SUFFIX_1LETTER, isdigit_string, size_to_human_string, string_add_to_idarray,
    strverscmp,
};

/// `_PATH_SYS_MEMORY`.
const PATH_SYS_MEMORY: &[u8] = b"/sys/devices/system/memory";

/// Only the sentences of getopt's errors are used; each is printed after
/// argv[0], and the referral after that.
const LSMEM: Program = Program::new("lsmem", 1);

/// Upstream's option string.
const SHORTS: &str = "abhJno:PrS:s:V";

/// Upstream's `longopts[]`, in its order: the order is what the ambiguity
/// message lists (`--s` names `--sysroot`, `--split`, `--summary`).
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("bytes", Takes::Nothing),
    ("help", Takes::Nothing),
    ("json", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("pairs", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("sysroot", Takes::Required),
    ("split", Takes::Required),
    ("version", Takes::Nothing),
    ("summary", Takes::Optional),
];

/// `LSMEM_OPT_SUMARRY` and `OPT_OUTPUT_ALL`: `CHAR_MAX + 1` and `+ 2`.
const OPT_SUMMARY: i32 = 128;
const OPT_OUTPUT_ALL: i32 = 129;

/// Each long option's `val`, in [`LONGS`]' order.
const LONG_VALS: [i32; 13] = [
    b'a' as i32,
    b'b' as i32,
    b'h' as i32,
    b'J' as i32,
    b'n' as i32,
    b'o' as i32,
    OPT_OUTPUT_ALL,
    b'P' as i32,
    b'r' as i32,
    b's' as i32,
    b'S' as i32,
    b'V' as i32,
    OPT_SUMMARY,
];

/// `excl[]`: the options that exclude each other, rows and columns in ASCII
/// order as `err_exclusive_options` requires.
const EXCL: [&[i32]; 2] = [
    &[b'J' as i32, b'P' as i32, b'r' as i32],
    &[b'S' as i32, b'a' as i32],
];

/// `MEMORY_STATE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemState {
    Online,
    Offline,
    GoingOffline,
    Unknown,
}

/// `zone_names[]`, indexed by `enum zone_id`.
const ZONE_NAMES: [&str; 8] = [
    "DMA", "DMA32", "Normal", "Highmem", "Movable", "Device", "None", "Unknown",
];
/// `ZONE_UNKNOWN`.
const ZONE_UNKNOWN: usize = 7;
/// `MAX_NR_ZONES`: how many of a block's zones are read.
const MAX_NR_ZONES: usize = 8;

/// `struct memory_block`: one block, or a run of blocks merged into it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoryBlock {
    index: u64,
    count: u64,
    state: MemState,
    node: i32,
    /// Indices into [`ZONE_NAMES`], at most [`MAX_NR_ZONES`].
    zones: Vec<usize>,
    removable: bool,
}

/// `COL_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Range,
    Size,
    State,
    Removable,
    Block,
    Node,
    Zones,
}

/// `struct coldesc`.
struct ColDesc {
    id: Col,
    name: &'static str,
    /// Width hint: below 1 a fraction of the terminal, else columns.
    whint: f64,
    flags: u32,
    help: &'static str,
}

/// `coldescs[]`.
const COLDESCS: [ColDesc; 7] = [
    ColDesc {
        id: Col::Range,
        name: "RANGE",
        whint: 0.0,
        flags: 0,
        help: "start and end address of the memory range",
    },
    ColDesc {
        id: Col::Size,
        name: "SIZE",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "size of the memory range",
    },
    ColDesc {
        id: Col::State,
        name: "STATE",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "online status of the memory range",
    },
    ColDesc {
        id: Col::Removable,
        name: "REMOVABLE",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "memory is removable",
    },
    ColDesc {
        id: Col::Block,
        name: "BLOCK",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "memory block number or blocks range",
    },
    ColDesc {
        id: Col::Node,
        name: "NODE",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "numa node of memory",
    },
    ColDesc {
        id: Col::Zones,
        name: "ZONES",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "valid zones for the memory range",
    },
];

/// `columns[ARRAY_SIZE(coldescs) * 2]`: each column may be asked for twice.
const MAX_COLUMNS: usize = 14;

/// A column's description.
fn desc(col: Col) -> &'static ColDesc {
    COLDESCS
        .iter()
        .find(|d| d.id == col)
        .unwrap_or(&COLDESCS[0])
}

/// What `errno` holds, as far as [`read_basic_info`] and [`read_info`] ever
/// report it without a failing call of their own to name.
#[derive(Debug)]
enum Errno {
    /// 0: `Success`.
    Zero,
    /// `ERANGE`, from a `strtol`.
    Range,
    /// A failed call's.
    Os(io::Error),
}

impl Errno {
    /// `strerror(errno)`.
    fn text(&self) -> String {
        match self {
            Errno::Zero => "Success".to_string(),
            Errno::Range => "Numerical result out of range".to_string(),
            Errno::Os(e) => errmsg::strerror(e),
        }
    }
}

/// `struct lsmem`.
#[derive(Default)]
struct Lsmem {
    /// The `memory<N>` names, in `versionsort` order.
    dirs: Vec<Vec<u8>>,
    blocks: Vec<MemoryBlock>,
    block_size: u64,
    mem_online: u64,
    mem_offline: u64,
    have_nodes: bool,
    raw: bool,
    export: bool,
    json: bool,
    noheadings: bool,
    list_all: bool,
    bytes: bool,
    want_summary: bool,
    want_table: bool,
    split_by_node: bool,
    split_by_state: bool,
    split_by_removable: bool,
    split_by_zones: bool,
    have_zones: bool,
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without is closed again, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("lsmem"), OsString::as_os_str),
    );
    // `lsmem.c` leaves `CLOSE_EXIT_CODE` at `EXIT_FAILURE`.
    let mut out = Out::new(1);
    let status = run(&argv, &short, &mut out);
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

/// Text to stderr, as `fprintf(stderr, ...)` writes it: a failure counts
/// against the exit status at `close_stdout`.
fn to_stderr(text: &str) {
    ulclosestream::stderr_write(text.as_bytes());
}

/// `err`'s message for an `errno` that is not a failing call's own.
fn warn_errno(short: &[u8], msg: &str, errno: &Errno) {
    warnx(short, &format!("{msg}: {}", errno.text()));
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    to_stderr(&format!(
        "Try '{} --help' for more information.\n",
        shown(short)
    ));
    1
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options]\n\
\nList the ranges of available memory with their online status.\n\
\nOptions:\n\
\x20-J, --json           use JSON output format\n\
\x20-P, --pairs          use key=\"value\" output format\n\
\x20-a, --all            list each individual memory block\n\
\x20-b, --bytes          print SIZE in bytes rather than in human readable format\n\
\x20-n, --noheadings     don't print headings\n\
\x20-o, --output <list>  output columns\n\
\x20    --output-all     output all columns\n\
\x20-r, --raw            use raw output format\n\
\x20-S, --split <list>   split ranges by specified columns\n\
\x20-s, --sysroot <dir>  use the specified directory as system root\n\
\x20    --summary[=when] print summary information (never,always or only)\n\
\n\
\x20-h, --help           display this help\n\
\x20-V, --version        display version\n\
\nAvailable output columns:\n",
    );
    for d in &COLDESCS {
        text.extend_from_slice(format!(" {:>10}  {}\n", d.name, d.help).as_bytes());
    }
    text.extend_from_slice(b"\nFor more details see lsmem(1).\n");
    text
}

/// `option_to_longopt(c, longopts)`: the first long option whose `val` is
/// `c`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

/// `err_exclusive_options(c, longopts, excl, status)`: refuse `c` when
/// another option of a group it belongs to came first. The message names
/// the whole group, and no referral follows it.
fn err_exclusive_options(c: i32, status: &mut [i32; 2], short: &[u8]) -> Result<(), u8> {
    for (group, st) in EXCL.iter().zip(status.iter_mut()) {
        if group.first().is_some_and(|&first| first > c) {
            break;
        }
        for &op in group.iter() {
            if op > c {
                break;
            }
            if op != c {
                continue;
            }
            if *st == 0 {
                *st = c;
            } else if *st != c {
                let mut msg = format!("{}: mutually exclusive arguments:", shown(short));
                for &member in group.iter() {
                    if let Some(name) = option_to_longopt(member) {
                        msg.push_str(&format!(" --{name}"));
                    } else if let Ok(b) = u8::try_from(member)
                        && b.is_ascii_graphic()
                    {
                        msg.push_str(&format!(" -{}", char::from(b)));
                    }
                }
                msg.push('\n');
                to_stderr(&msg);
                return Err(1);
            }
            break;
        }
    }
    Ok(())
}

/// `column_name_to_id(name, namesz)`: a column by its name, in any case.
/// Unknown, it is reported with the rest of the list after it -- upstream
/// prints the name as a C string that runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = COLDESCS
        .iter()
        .find(|d| d.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|d| d.id);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// `zone_name_to_id(name)`.
fn zone_name_to_id(name: &[u8]) -> usize {
    ZONE_NAMES
        .iter()
        .position(|z| z.as_bytes().eq_ignore_ascii_case(name))
        .unwrap_or(ZONE_UNKNOWN)
}

/// `reset_split_policy` then `set_split_policy(l, cols)`.
fn set_split_policy(l: &mut Lsmem, cols: &[Col]) {
    l.split_by_state = cols.contains(&Col::State);
    l.split_by_node = cols.contains(&Col::Node);
    l.split_by_removable = cols.contains(&Col::Removable);
    l.split_by_zones = cols.contains(&Col::Zones);
}

/// One cell of [`add_scols_line`]: the text, or nothing to show.
fn cell_text(l: &Lsmem, blk: &MemoryBlock, col: Col) -> Option<String> {
    let size = blk.count.wrapping_mul(l.block_size);
    match col {
        Col::Range => {
            let start = blk.index.wrapping_mul(l.block_size);
            let end = start.wrapping_add(size).wrapping_sub(1);
            Some(format!("0x{start:016x}-0x{end:016x}"))
        }
        // `%PRId64` of an unsigned value: above 2^63 it prints negative.
        Col::Size if l.bytes => Some(format!("{}", size as i64)),
        Col::Size => Some(size_to_human_string(SIZE_SUFFIX_1LETTER, size)),
        Col::State => Some(
            match blk.state {
                MemState::Online => "online",
                MemState::Offline => "offline",
                MemState::GoingOffline => "on->off",
                MemState::Unknown => "?",
            }
            .to_string(),
        ),
        Col::Removable => (blk.state == MemState::Online)
            .then(|| if blk.removable { "yes" } else { "no" }.to_string()),
        Col::Block if blk.count == 1 => Some(format!("{}", blk.index as i64)),
        Col::Block => Some(format!(
            "{}-{}",
            blk.index as i64,
            blk.index.wrapping_add(blk.count).wrapping_sub(1) as i64
        )),
        Col::Node => l.have_nodes.then(|| format!("{}", blk.node)),
        Col::Zones => l.have_zones.then(|| {
            blk.zones
                .iter()
                .map(|&z| ZONE_NAMES.get(z).copied().unwrap_or("Unknown"))
                .collect::<Vec<_>>()
                .join("/")
        }),
    }
}

/// `add_scols_line(lsmem, blk)`.
fn add_scols_line(l: &Lsmem, tb: &mut Table, cols: &[(Col, ColumnId)], blk: &MemoryBlock) {
    let Ok(line) = tb.new_line(None) else {
        return;
    };
    for &(col, id) in cols {
        if let Some(text) = cell_text(l, blk, col) {
            // The line and column are this table's own, which is the one
            // way setting data can fail.
            let _ = tb.line_set_data(line, id, text.as_bytes());
        }
    }
}

/// `print_summary(lsmem)`.
fn print_summary(l: &Lsmem, out: &mut Out) {
    let rows = [
        ("Memory block size:", l.block_size),
        ("Total online memory:", l.mem_online),
        ("Total offline memory:", l.mem_offline),
    ];
    for (label, value) in rows {
        let line = if l.bytes {
            format!("{label:<23} {:>15}\n", value as i64)
        } else {
            format!(
                "{label:<23} {:>5}\n",
                size_to_human_string(SIZE_SUFFIX_1LETTER, value)
            )
        };
        out.write(line.as_bytes());
    }
}

/// `strtol(str, NULL, 10)` of a digit string, and whether it set `ERANGE`:
/// saturated at `LONG_MAX`.
fn strtol_digits(digits: &[u8]) -> (i64, bool) {
    match ulstrutils::scan_integer(digits, 10) {
        Some(sc) if !sc.saturated => match i64::try_from(sc.magnitude) {
            Ok(v) => (v, false),
            Err(_) => (i64::MAX, true),
        },
        Some(_) => (i64::MAX, true),
        None => (0, false),
    }
}

/// `strtoumax(str, NULL, base)` and whether it set `ERANGE`: a minus sign
/// negates in two's complement, as C's unsigned conversion does, and a value
/// beyond 64 bits saturates.
fn strtoumax(s: &[u8], base: u32) -> (u64, bool) {
    let Some(sc) = ulstrutils::scan_integer(s, base) else {
        return (0, false);
    };
    match u64::try_from(sc.magnitude) {
        Ok(v) if !sc.saturated => (if sc.negative { v.wrapping_neg() } else { v }, false),
        _ => (u64::MAX, true),
    }
}

/// `memory_block_get_node(lsmem, name)`: the number of the first
/// `node<N>` entry in the block's directory, or -1.
///
/// Each candidate's `strtol` is preceded by `errno = 0`, which is how this
/// touches [`Errno`]. A number too big for `long` saturates, and the
/// saturated value is kept -- truncated to `int`, -1 -- before the next
/// entry is tried.
fn memory_block_get_node(sys: &mut SysPath, name: &[u8], errno: &mut Errno) -> io::Result<i32> {
    let dir = sys.opendir(name)?;
    let mut node = -1;
    // `readdir` ends the walk on an error as it does at the end.
    for entry in dir.map_while(Result::ok) {
        let entry_name = entry.file_name();
        let entry_name = os_bytes(&entry_name);
        let Some(digits) = entry_name.strip_prefix(b"node") else {
            continue;
        };
        if !isdigit_string(digits) {
            continue;
        }
        let (value, overflowed) = strtol_digits(digits);
        // The store into `int`: the low 32 bits.
        node = value as i32;
        if overflowed {
            *errno = Errno::Range;
            continue;
        }
        *errno = Errno::Zero;
        break;
    }
    Ok(node)
}

/// `memory_block_read_attrs(lsmem, name, &blk)`. Fails only where upstream
/// exits: a block directory that cannot be opened to look for its node.
fn memory_block_read_attrs(
    l: &Lsmem,
    sys: &mut SysPath,
    name: &[u8],
    errno: &mut Errno,
) -> io::Result<MemoryBlock> {
    let mut blk = MemoryBlock {
        index: 0,
        count: 1,
        state: MemState::Unknown,
        node: 0,
        zones: Vec::new(),
        removable: false,
    };
    // `strtoumax(name + 6, NULL, 10)`: past `memory`. Upstream records an
    // overflow in a return value nobody reads.
    blk.index = strtoumax(name.get(6..).unwrap_or_default(), 10).0;

    if let Some(x) = sys.read_s32(&[name, b"/removable"].concat()) {
        blk.removable = x == 1;
    }
    if let Some(line) = sys.read_string(&[name, b"/state"].concat()) {
        match line.as_slice() {
            b"offline" => blk.state = MemState::Offline,
            b"online" => blk.state = MemState::Online,
            b"going-offline" => blk.state = MemState::GoingOffline,
            _ => {}
        }
    }
    if l.have_nodes {
        blk.node = memory_block_get_node(sys, name, errno)?;
    }
    if l.have_zones
        && let Some(line) = sys.read_string(&[name, b"/valid_zones"].concat())
    {
        // `strtok(line, " ")`: runs of spaces separate, and only spaces.
        blk.zones = line
            .split(|&b| b == b' ')
            .filter(|t| !t.is_empty())
            .take(MAX_NR_ZONES)
            .map(zone_name_to_id)
            .collect();
    }
    Ok(blk)
}

/// `is_mergeable(lsmem, blk)`: whether `blk` continues the last range.
fn is_mergeable(l: &Lsmem, blk: &MemoryBlock) -> bool {
    let Some(curr) = l.blocks.last() else {
        return false;
    };
    if l.list_all {
        return false;
    }
    if curr.index.wrapping_add(curr.count) != blk.index {
        return false;
    }
    if l.split_by_state && curr.state != blk.state {
        return false;
    }
    if l.split_by_removable && curr.removable != blk.removable {
        return false;
    }
    if l.split_by_node && l.have_nodes && curr.node != blk.node {
        return false;
    }
    if l.split_by_zones && l.have_zones {
        if curr.zones.len() != blk.zones.len() {
            return false;
        }
        for (&a, &b) in curr.zones.iter().zip(&blk.zones) {
            if a == ZONE_UNKNOWN || a != b {
                return false;
            }
        }
    }
    true
}

/// `read_info(lsmem)`. On failure, the status to exit with, the message
/// printed.
fn read_info(l: &mut Lsmem, sys: &mut SysPath, errno: &mut Errno, short: &[u8]) -> Result<(), u8> {
    const MSG: &str = "failed to read memory block size";
    let buf = match sys.read_buffer(b"block_size_bytes", 128) {
        Ok((0, _)) => {
            warn_errno(short, MSG, errno);
            return Err(1);
        }
        Ok((_, buf)) => buf,
        Err(e) => {
            warn(short, MSG, &e);
            return Err(1);
        }
    };
    let (block_size, overflowed) = strtoumax(&buf, 16);
    if overflowed {
        warn_errno(short, MSG, &Errno::Range);
        return Err(1);
    }
    *errno = Errno::Zero;
    l.block_size = block_size;

    let dirs = std::mem::take(&mut l.dirs);
    for name in &dirs {
        let blk = match memory_block_read_attrs(l, sys, name, errno) {
            Ok(blk) => blk,
            Err(e) => {
                warn(short, &format!("Failed to open {}", shown(name)), &e);
                return Err(1);
            }
        };
        if blk.state == MemState::Online {
            l.mem_online = l.mem_online.wrapping_add(l.block_size);
        } else {
            l.mem_offline = l.mem_offline.wrapping_add(l.block_size);
        }
        if is_mergeable(l, &blk) {
            if let Some(last) = l.blocks.last_mut() {
                last.count = last.count.wrapping_add(1);
            }
            continue;
        }
        l.blocks.push(blk);
    }
    l.dirs = dirs;
    Ok(())
}

/// `memory_block_filter`: `memory` and one or more digits.
fn memory_block_filter(name: &[u8]) -> bool {
    name.strip_prefix(b"memory").is_some_and(isdigit_string)
}

/// `scandir(dir, &dirs, memory_block_filter, versionsort)`.
fn scandir(dir: &[u8]) -> io::Result<Vec<Vec<u8>>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(quoting::os_from_bytes(dir))? {
        let name = entry?.file_name();
        let name = os_bytes(&name);
        if memory_block_filter(&name) {
            names.push(name.into_owned());
        }
    }
    names.sort_by(|a, b| strverscmp(a, b));
    Ok(names)
}

/// `read_basic_info(lsmem)`. On failure, the status to exit with, the
/// message printed.
fn read_basic_info(
    l: &mut Lsmem,
    sys: &mut SysPath,
    errno: &mut Errno,
    short: &[u8],
) -> Result<(), u8> {
    if sys.access(b"block_size_bytes").is_err() {
        warnx(short, "This system does not support memory blocks");
        return Err(1);
    }
    // `ul_path_get_abspath(sysmem, dir, sizeof(dir), NULL)`: the directory
    // has just been opened through this very path, so it fits.
    let dir = sys.absdir().unwrap_or_default();
    let what = format!("Failed to read {}", shown(&dir));
    match scandir(&dir) {
        // `scandir` gives `errno` back as it found it.
        Ok(dirs) if dirs.is_empty() => {
            warn_errno(short, &what, errno);
            return Err(1);
        }
        Ok(dirs) => l.dirs = dirs,
        Err(e) => {
            warn(short, &what, &e);
            return Err(1);
        }
    }
    let first = l.dirs.first().cloned().unwrap_or_default();
    match memory_block_get_node(sys, &first, errno) {
        Ok(-1) => {}
        Ok(_) => l.have_nodes = true,
        Err(e) => {
            warn(short, &format!("Failed to open {}", shown(&first)), &e);
            return Err(1);
        }
    }
    // The `valid_zones` attribute came with Linux 3.18.
    match sys.access(b"memory0/valid_zones") {
        Ok(()) => l.have_zones = true,
        Err(e) => *errno = Errno::Os(e),
    }
    Ok(())
}

/// What `scols_new_table` leaves in `errno`: its `get_terminal_dimension`
/// asks stdout for the terminal size, which fails -- `ENOTTY`, `EBADF` --
/// unless stdout is a terminal; a size that is not there is then looked for
/// in `COLUMNS` and `LINES`, each read by a `strtol` after `errno = 0`.
fn errno_after_new_table(errno: Errno) -> Errno {
    let mut errno = errno;
    let (cols, rows) = match smartcols::tty::winsize() {
        Ok(size) => size,
        Err(e) => {
            errno = Errno::Os(e);
            (0, 0)
        }
    };
    for (size, var) in [(cols, "COLUMNS"), (rows, "LINES")] {
        if size == 0
            && let Some(value) = std::env::var_os(var)
        {
            errno = if strtol_overflows(&os_bytes(&value)) {
                Errno::Range
            } else {
                Errno::Zero
            };
        }
    }
    errno
}

/// Whether `strtol(s, &end, 10)` sets `ERANGE`: the number it reads is
/// below `LONG_MIN` or above `LONG_MAX`.
fn strtol_overflows(s: &[u8]) -> bool {
    let Some(sc) = ulstrutils::scan_integer(s, 10) else {
        return false;
    };
    let limit = u128::from(i64::MIN.unsigned_abs());
    sc.saturated || sc.magnitude > limit || (!sc.negative && sc.magnitude == limit)
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
fn run(argv: &[OsString], short: &[u8], out: &mut Out) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("lsmem"), OsString::as_os_str);
    let mut l = Lsmem {
        want_table: true,
        want_summary: true,
        ..Lsmem::default()
    };
    let mut outarg: Option<OsString> = None;
    let mut splitarg: Option<OsString> = None;
    let mut prefix: Option<OsString> = None;
    let mut columns: Vec<Col> = Vec::new();
    let mut excl_st = [0i32; 2];
    let mut operands = 0usize;

    let own = argv.get(1..).unwrap_or_default();
    for item in LSMEM.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                to_stderr(&format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence));
                return errtryhelp(short);
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            operands = operands.saturating_add(1);
            continue;
        };
        if let Err(status) = err_exclusive_options(c, &mut excl_st, short) {
            return status;
        }
        match c {
            OPT_OUTPUT_ALL => columns = COLDESCS.iter().map(|d| d.id).collect(),
            OPT_SUMMARY => match value.as_deref().map(os_bytes).as_deref() {
                None | Some(b"only") => l.want_table = false,
                Some(b"never") => l.want_summary = false,
                Some(b"always") => l.want_summary = true,
                Some(_) => {
                    warnx(short, "unsupported --summary argument");
                    return 1;
                }
            },
            _ => match u8::try_from(c).unwrap_or(0) {
                b'a' => l.list_all = true,
                b'b' => l.bytes = true,
                b'J' => {
                    l.json = true;
                    l.want_summary = false;
                }
                b'n' => l.noheadings = true,
                b'o' => outarg = value,
                b'P' => {
                    l.export = true;
                    l.want_summary = false;
                }
                b'r' => {
                    l.raw = true;
                    l.want_summary = false;
                }
                b's' => prefix = value,
                b'S' => splitarg = value,
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

    if operands > 0 {
        warnx(short, "bad usage");
        return errtryhelp(short);
    }

    if !l.want_table && !l.want_summary {
        warnx(
            short,
            "options --{raw,json,pairs} and --summary=only are mutually exclusive",
        );
        return 1;
    }

    let mut sys = SysPath::new(PATH_SYS_MEMORY);
    if let Some(prefix) = &prefix {
        sys.set_prefix(&os_bytes(prefix));
    }
    if let Err(e) = sys.is_accessible() {
        warn(short, "cannot open /sys/devices/system/memory", &e);
        return 1;
    }

    let mut errno = Errno::Zero;

    // Shortcut to avoid the table machinery on --summary=only.
    if !l.want_table && l.want_summary {
        if let Err(status) = read_basic_info(&mut l, &mut sys, &mut errno, short)
            .and_then(|()| read_info(&mut l, &mut sys, &mut errno, short))
        {
            return status;
        }
        print_summary(&l, out);
        return 0;
    }

    // Default columns.
    if columns.is_empty() {
        columns = vec![
            Col::Range,
            Col::Size,
            Col::State,
            Col::Removable,
            Col::Block,
        ];
    }
    if let Some(list) = &outarg {
        let added =
            string_add_to_idarray(&os_bytes(list), &mut columns, MAX_COLUMNS, |name, rest| {
                column_name_to_id(name, rest, short)
            });
        if added.is_err() {
            return 1;
        }
    }

    // Initialize output.
    let mut tb = Table::new();
    errno = errno_after_new_table(errno);
    tb.enable_raw(l.raw);
    tb.enable_export(l.export);
    tb.enable_json(l.json);
    tb.enable_noheadings(l.noheadings);
    if l.json {
        tb.set_name(b"memory");
    }
    let mut cols: Vec<(Col, ColumnId)> = Vec::new();
    for &col in &columns {
        let d = desc(col);
        let id = tb.new_column(d.name.as_bytes(), d.whint, d.flags);
        if l.json {
            let ty = match col {
                Col::Size if l.bytes => Some(JsonType::Number),
                Col::Node => Some(JsonType::Number),
                Col::Removable => Some(JsonType::Boolean),
                _ => None,
            };
            if let Some(ty) = ty {
                // The column was made by this table a line ago.
                let _ = tb.column_set_json_type(id, ty);
            }
        }
        cols.push((col, id));
    }

    if let Some(list) = &splitarg {
        let list = os_bytes(list);
        let mut split: Vec<Col> = Vec::new();
        if !list.eq_ignore_ascii_case(b"none") {
            let added: Result<usize, IdListError> =
                string_add_to_idarray(&list, &mut split, COLDESCS.len(), |name, rest| {
                    column_name_to_id(name, rest, short)
                });
            if added.is_err() {
                return 1;
            }
        }
        set_split_policy(&mut l, &split);
    } else {
        // Follow the output columns.
        set_split_policy(&mut l, &columns);
    }

    // Read data and print output.
    if let Err(status) = read_basic_info(&mut l, &mut sys, &mut errno, short)
        .and_then(|()| read_info(&mut l, &mut sys, &mut errno, short))
    {
        return status;
    }

    if l.want_table {
        for blk in &l.blocks {
            add_scols_line(&l, &mut tb, &cols, blk);
        }
        // `scols_print_table`'s status is not looked at, as upstream does
        // not look at it: what it printed before any failure is written.
        let mut text = Vec::new();
        let _ = tb.print_into(&mut text);
        out.write(&text);
        if l.want_summary {
            out.write(b"\n");
        }
    }
    if l.want_summary {
        print_summary(&l, out);
    }
    0
}

#[cfg(test)]
mod tests;
