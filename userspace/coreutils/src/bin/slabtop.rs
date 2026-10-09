//! Display the kernel's slab caches, a screen at a time: procps-ng 4.0.4's
//! `slabtop`, ported. Ubuntu ships `src/slabtop.c` unpatched, so this is the
//! program Ubuntu ships too.
//!
//! ```text
//! slabtop [options]
//! ```
//!
//! A transcription of `src/slabtop.c` and the parts of `library/slabinfo.c`
//! it reaches (`coreutils::procps::slabinfo`), drawn through the curses
//! crate (`userspace/curses`), which is ncursesw's screen library ported.
//! What upstream does and this keeps:
//!
//! - The totals come from a second read of `/proc/slabinfo` -- the library's
//!   `select` reads the file again after `reap` has read it for the caches --
//!   so on a live system the two can disagree a little, as upstream's do.
//! - The caches are sorted as `qsort_r` sorts them in glibc 2.39, a stable
//!   merge, numbers largest first and names in `strcoll`'s order, which is
//!   the C library's for the selected locale. A key read at the prompt
//!   chooses the order by `tolower`'s answer, and anything else sorts by the
//!   number of objects again.
//! - The percentages and sizes are `double` arithmetic printed with
//!   `printf`'s `%.1f` and `%.2f`, in the locale's decimal point; a total of
//!   nothing prints glibc's `-nan`, a total of something over nothing `inf`.
//! - The screen is the terminal's size if it has more than ten rows, and 80
//!   by 24 if not; a `SIGWINCH` measures it again, and the next frame resizes
//!   the screen. The caches shown are the rows less eight.
//! - Keys are read one at a time between frames: `q` or `Q` ends the program,
//!   as does the end of standard input or a failed read; any other sets the
//!   order. `SIGINT` ends it after the frame being drawn. `SIGTERM` is left to
//!   curses, which puts the terminal back and exits with status 1.
//! - Standard input's modes are saved before the screen is made and put back
//!   (`TCSAFLUSH`) before it is ended -- zeros, if they could not be read.
//! - A failure to read the totals ends the program on the spot, the screen
//!   left as it is, as upstream's `xerrx` does.
//! - `-o` prints one report on standard output, no screen at all, and
//!   standard output is closed as procps' `close_stdout` closes it.
//!
//! # Deliberately different
//!
//! - `-V`/`--version` names this build.
//! - The argument of a refused `-d` goes through `quoteaf` (`free`'s
//!   divergence 6).
//! - A `/proc/slabinfo` that ends before its version line makes the library
//!   return `-errno` from a read that set none, so upstream's outcome turns
//!   on whatever `errno` was left over. With none -- the usual case -- that is
//!   success with no info, and then `Unable to get slabinfo node data:
//!   Invalid argument`, which is what this says always. A locale that does
//!   not exist leaves `ENOENT` from `setlocale`, and upstream then says
//!   `Unable to create slabinfo structure: No such file or directory`
//!   (`free`'s divergence 3).

use std::ffi::{CString, OsString};
use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicI64, AtomicU16, Ordering};

use coreutils::cfmt::{self, Spec, Value};
use coreutils::extfloat::ExtF80;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::cvt;
use coreutils::procps::slabinfo::{Node, SlabInfo, Summary};
use coreutils::procps::strutils;
use coreutils::quote::{os_bytes, quoteaf};
use coreutils::stdfd::{self, Stream};
use libcall::termios::{self as tc, Termios};

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`.
const SLABTOP: Program = Program::new("slabtop", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "d:s:ohV";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("delay", Takes::Required),
    ("sort", Takes::Required),
    ("once", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The usage text after the program's name.
const USAGE_REST: &str = concat!(
    " [options]\n",
    "\n",
    "Options:\n",
    " -d, --delay <secs>  delay updates\n",
    " -o, --once          only display once, then exit\n",
    " -s, --sort <char>   specify sort criteria by character (see below)\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "The following are valid sort criteria:\n",
    " a: sort by number of active objects\n",
    " b: sort by objects per slab\n",
    " c: sort by cache size\n",
    " l: sort by number of slabs\n",
    " v: sort by (non display) number of active slabs\n",
    " n: sort by name\n",
    " o: sort by number of objects (the default)\n",
    " p: sort by (non display) pages per slab\n",
    " s: sort by object size\n",
    " u: sort by cache utilization\n",
    "\n",
    "For more details see slabtop(1).\n",
);

/// The heading, `%-78s` of upstream's string.
const HEADING: &[u8] = b"  OBJS ACTIVE  USE OBJ SIZE  SLABS OBJ/SLAB CACHE SIZE NAME";

/// `DEFAULT_DELAY`.
const DEFAULT_DELAY: i64 = 3;

/// `Rows`, `Cols`: the screen's size, which the `SIGWINCH` handler sets.
static ROWS: AtomicU16 = AtomicU16::new(0);
static COLS: AtomicU16 = AtomicU16::new(0);
/// `Delay`: the seconds between frames, which `SIGINT` makes 0.
static DELAY: AtomicI64 = AtomicI64::new(0);

/// What the caches are sorted by: one of the items `slabtop` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Name,
    NumObjs,
    ActiveObjs,
    ObjSize,
    ObjPerSlab,
    PagesPerSlab,
    NumsSlabs,
    ActiveSlabs,
    SizeTotal,
    PercentUsed,
}

/// `Sort_item` and `Sort_Order` (`true` for `SLABINFO_SORT_ASCEND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sort {
    item: Item,
    ascend: bool,
}

/// `DEFAULT_SORT`, descending.
const DEFAULT: Sort = Sort {
    item: Item::NumObjs,
    ascend: false,
};

/// `set_sort_stuff (key)`: the order a key chooses, by its `tolower`.
/// `key` is a `const char`, signed: a byte past 127 reaches `tolower` as a
/// negative number, where glibc's table answers it as it is.
fn sort_for(key: u8) -> Sort {
    let lowered = if key < 0x80 {
        libcall::locale::tolower(i32::from(key))
    } else {
        i32::from(key)
    };
    let item = match u8::try_from(lowered).unwrap_or(0) {
        b'n' => {
            return Sort {
                item: Item::Name,
                ascend: true,
            };
        }
        b'o' => Item::NumObjs,
        b'a' => Item::ActiveObjs,
        b's' => Item::ObjSize,
        b'b' => Item::ObjPerSlab,
        b'p' => Item::PagesPerSlab,
        b'l' => Item::NumsSlabs,
        b'v' => Item::ActiveSlabs,
        b'c' => Item::SizeTotal,
        b'u' => Item::PercentUsed,
        _ => Item::NumObjs,
    };
    Sort {
        item,
        ascend: false,
    }
}

/// `procps_slabinfo_sort`: the comparator for the item, under the order.
fn compare(sort: Sort, a: &Node, b: &Node) -> std::cmp::Ordering {
    let number = |x: u64, y: u64| {
        if sort.ascend { x.cmp(&y) } else { y.cmp(&x) }
    };
    match sort.item {
        Item::Name => {
            // `order * strcoll (a, b)`. A name holds no NUL: the library's
            // `%128s` stops at the first one.
            let c = |n: &[u8]| CString::new(n.to_vec()).unwrap_or_default();
            let ord = libcall::locale::strcoll(&c(&a.name), &c(&b.name));
            if sort.ascend { ord } else { ord.reverse() }
        }
        Item::NumObjs => number(a.nr_objs.into(), b.nr_objs.into()),
        Item::ActiveObjs => number(a.nr_active_objs.into(), b.nr_active_objs.into()),
        Item::ObjSize => number(a.obj_size.into(), b.obj_size.into()),
        Item::ObjPerSlab => number(a.objs_per_slab.into(), b.objs_per_slab.into()),
        Item::PagesPerSlab => number(a.pages_per_slab.into(), b.pages_per_slab.into()),
        Item::NumsSlabs => number(a.nr_slabs.into(), b.nr_slabs.into()),
        Item::ActiveSlabs => number(a.nr_active_slabs.into(), b.nr_active_slabs.into()),
        Item::SizeTotal => number(a.cache_size, b.cache_size),
        Item::PercentUsed => number(a.percent_used.into(), b.percent_used.into()),
    }
}

/// `term_resize`: the size of the terminal on standard output when it has
/// more than ten rows, else 80 by 24. Safe in a handler: one `ioctl` and
/// two atomic stores.
fn term_resize() {
    let (rows, cols) = match libcall::pty::window_size(1) {
        Ok(ws) if ws.rows > 10 => (ws.rows, ws.cols),
        _ => (24, 80),
    };
    COLS.store(cols, Ordering::Relaxed);
    ROWS.store(rows, Ordering::Relaxed);
}

/// `term_resize` as the `SIGWINCH` handler.
extern "C" fn on_winch(_sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    term_resize();
}

/// `sigint_handler`: the frame being drawn is the last.
extern "C" fn on_int(_sig: i32) {
    DELAY.store(0, Ordering::Relaxed);
}

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// glibc's `error (status, errnum, ...)`: standard output delivered, then
/// `NAME: MESSAGE[: REASON]`.
fn error(name: &[u8], msg: &[u8], errnum: Option<i32>) {
    let mut m = name.to_vec();
    m.extend_from_slice(b": ");
    m.extend_from_slice(msg);
    if let Some(e) = errnum.filter(|&e| e != 0) {
        m.extend_from_slice(b": ");
        m.extend_from_slice(
            coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(e)).as_bytes(),
        );
    }
    m.push(b'\n');
    stdfd::diag_bytes(&m);
}

/// `usage (out)`, and the status `exit` is then given.
fn usage(name: &[u8], to_stdout: bool, out: &mut Stream) -> u8 {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(name);
    text.extend_from_slice(USAGE_REST.as_bytes());
    if to_stdout {
        // `Stream` never fails a write; a failure is `close_stdout`'s to say.
        let _ = out.write_all(&text);
        0
    } else {
        stdfd::diag_bytes_ahead_of_stdout(&text);
        1
    }
}

/// A `printf` field: `%[-]W[.P]C`.
const fn spec(conv: u8, width: usize, precision: Option<usize>, minus: bool) -> Spec {
    Spec {
        minus,
        plus: false,
        space: false,
        hash: false,
        zero: false,
        width,
        precision,
        conv,
    }
}

/// How the report is written: through `printw`, or `printf` to standard
/// output.
enum Sink<'a> {
    Screen,
    Stdout(&'a mut Stream),
}

/// What the lines are built with: the locale's decimal point.
struct Printer {
    radix: Vec<u8>,
}

impl Printer {
    /// `%W.Pf`, with the locale's decimal point for C's.
    fn f(&self, width: usize, precision: usize, v: f64) -> Vec<u8> {
        let text = cfmt::render(
            &spec(b'f', width, Some(precision), false),
            Value::Float(ExtF80::from_f64(v)),
        );
        if self.radix == b"." {
            return text;
        }
        let mut out = Vec::with_capacity(text.len());
        for &b in &text {
            if b == b'.' {
                out.extend_from_slice(&self.radix);
            } else {
                out.push(b);
            }
        }
        out
    }

    /// `%Wu` of an `unsigned int`.
    fn u(width: usize, v: u32) -> Vec<u8> {
        cfmt::render(&spec(b'u', width, None, false), Value::Unsigned(v.into()))
    }

    /// `%Wlu`.
    fn lu(width: usize, v: u64) -> Vec<u8> {
        cfmt::render(&spec(b'u', width, None, false), Value::Unsigned(v))
    }

    /// `%-Ws`.
    fn s_left(width: usize, text: &[u8]) -> Vec<u8> {
        cfmt::render(&spec(b's', width, None, true), Value::Text(text))
    }

    /// `PRINT_line`: `printw`, or `printf` under `-o`.
    fn emit(sink: &mut Sink<'_>, line: &[u8]) {
        match sink {
            // `printw` reports a line that ran off the screen; upstream does
            // not look.
            Sink::Screen => {
                let _ = curses::printw(line);
            }
            // `Stream` never fails a write; see `usage`.
            Sink::Stdout(out) => {
                let _ = out.write_all(line);
            }
        }
    }

    /// `" %-35s: %u / %u (%.1f%%)\n"`.
    fn counts(&self, label: &[u8], active: u32, total: u32) -> Vec<u8> {
        let mut l = b" ".to_vec();
        l.extend_from_slice(&Self::s_left(35, label));
        l.extend_from_slice(b": ");
        l.extend_from_slice(&Self::u(0, active));
        l.extend_from_slice(b" / ");
        l.extend_from_slice(&Self::u(0, total));
        l.extend_from_slice(b" (");
        l.extend_from_slice(&self.f(0, 1, 100.0 * f64::from(active) / f64::from(total)));
        l.extend_from_slice(b"%)\n");
        l
    }

    /// `print_summary`, from the totals of a second read. `Err` is the
    /// read's failure, which ends the program.
    fn summary(&self, info: &mut SlabInfo, sink: &mut Sink<'_>) -> Result<(), ()> {
        let s: Summary = info.select().map_err(|_| ())?;
        Self::emit(
            sink,
            &self.counts(
                b"Active / Total Objects (% used)",
                s.nr_active_objs,
                s.nr_objs,
            ),
        );
        Self::emit(
            sink,
            &self.counts(
                b"Active / Total Slabs (% used)",
                s.nr_active_slabs,
                s.nr_slabs,
            ),
        );
        Self::emit(
            sink,
            &self.counts(
                b"Active / Total Caches (% used)",
                s.nr_active_caches,
                s.nr_caches,
            ),
        );
        let mut l = b" ".to_vec();
        l.extend_from_slice(&Self::s_left(35, b"Active / Total Size (% used)"));
        l.extend_from_slice(b": ");
        l.extend_from_slice(&self.f(0, 2, cvt::dbl(s.active_size) / 1024.0));
        l.extend_from_slice(b"K / ");
        l.extend_from_slice(&self.f(0, 2, cvt::dbl(s.total_size) / 1024.0));
        l.extend_from_slice(b"K (");
        l.extend_from_slice(&self.f(
            0,
            1,
            100.0 * cvt::dbl(s.active_size) / cvt::dbl(s.total_size),
        ));
        l.extend_from_slice(b"%)\n");
        Self::emit(sink, &l);
        let mut l = b" ".to_vec();
        l.extend_from_slice(&Self::s_left(35, b"Minimum / Average / Maximum Object"));
        l.extend_from_slice(b": ");
        l.extend_from_slice(&self.f(0, 2, f64::from(s.min_obj_size) / 1024.0));
        l.extend_from_slice(b"K / ");
        l.extend_from_slice(&self.f(0, 2, f64::from(s.avg_obj_size) / 1024.0));
        l.extend_from_slice(b"K / ");
        l.extend_from_slice(&self.f(0, 2, f64::from(s.max_obj_size) / 1024.0));
        l.extend_from_slice(b"K\n\n");
        Self::emit(sink, &l);
        Ok(())
    }

    /// `print_headings`.
    fn headings(sink: &mut Sink<'_>) {
        let mut l = Self::s_left(78, HEADING);
        l.push(b'\n');
        Self::emit(sink, &l);
    }

    /// `print_details`: `"%6u %6u %3u%% %7.2fK %6u %8u %9luK %-23s\n"`.
    fn details(&self, node: &Node, sink: &mut Sink<'_>) {
        let mut l = Self::u(6, node.nr_objs);
        l.push(b' ');
        l.extend_from_slice(&Self::u(6, node.nr_active_objs));
        l.push(b' ');
        l.extend_from_slice(&Self::u(3, node.percent_used));
        l.extend_from_slice(b"% ");
        l.extend_from_slice(&self.f(7, 2, f64::from(node.obj_size) / 1024.0));
        l.extend_from_slice(b"K ");
        l.extend_from_slice(&Self::u(6, node.nr_slabs));
        l.push(b' ');
        l.extend_from_slice(&Self::u(8, node.objs_per_slab));
        l.push(b' ');
        l.extend_from_slice(&Self::lu(9, node.cache_size / 1024));
        l.extend_from_slice(b"K ");
        l.extend_from_slice(&Self::s_left(23, &node.name));
        l.push(b'\n');
        Self::emit(sink, &l);
    }
}

/// What `parse_opts` settled.
struct Opts {
    run_once: bool,
    sort: Sort,
}

/// `parse_opts`: the options, or the status of a refusal.
fn parse_opts(argv0: &[u8], name: &[u8], words: &[OsString], out: &mut Stream) -> Result<Opts, u8> {
    let mut run_once = false;
    let mut sort = DEFAULT;
    // getopt moves the operands past the options, so an option after one is
    // still acted on, and only then does `optind != argc` refuse them.
    let mut operands = false;
    for item in SLABTOP.parse(words, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Ok(Opt::Short(b'd', Some(v)) | Opt::Long("delay", Some(v))) => {
                if run_once {
                    error(name, b"Cannot combine -d and -o options", None);
                    return Err(1);
                }
                let arg = os_bytes(&v);
                match strutils::strtol(&arg) {
                    Ok(d) => DELAY.store(d, Ordering::Relaxed),
                    Err(fault) => {
                        // `errno = 0` first, so an empty argument has no
                        // reason after it, as upstream's has none.
                        let mut m = b"illegal delay: ".to_vec();
                        m.extend_from_slice(quoteaf(&arg).as_bytes());
                        m.extend_from_slice(fault.suffix().as_bytes());
                        error(name, &m, None);
                        return Err(1);
                    }
                }
                if DELAY.load(Ordering::Relaxed) < 1 {
                    error(name, b"delay must be positive integer", None);
                    return Err(1);
                }
            }
            Ok(Opt::Short(b's', Some(v)) | Opt::Long("sort", Some(v))) => {
                // `set_sort_stuff (optarg[0])`: the NUL of an empty argument
                // is no key, and sorts by the default.
                sort = sort_for(os_bytes(&v).first().copied().unwrap_or(0));
            }
            Ok(Opt::Short(b'o', _) | Opt::Long("once", _)) => {
                if DELAY.load(Ordering::Relaxed) != 0 {
                    error(name, b"Cannot combine -d and -o options", None);
                    return Err(1);
                }
                run_once = true;
            }
            Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                let mut v = name.to_vec();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                // `Stream` never fails a write; see `usage`.
                let _ = out.write_all(&v);
                return Err(0);
            }
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return Err(usage(name, true, out)),
            Ok(Opt::Operand(_)) => operands = true,
            Ok(Opt::Short(..) | Opt::Long(..)) => return Err(usage(name, false, out)),
            Err(e) => {
                let mut m = argv0.to_vec();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes_ahead_of_stdout(&m);
                return Err(usage(name, false, out));
            }
        }
    }
    if operands {
        return Err(usage(name, false, out));
    }
    if !run_once && DELAY.load(Ordering::Relaxed) == 0 {
        DELAY.store(DEFAULT_DELAY, Ordering::Relaxed);
    }
    Ok(Opts { run_once, sort })
}

/// The caches, read and sorted; `None` after reporting a failed read.
fn reap(info: &mut SlabInfo, sort: Sort, name: &[u8]) -> Option<Vec<Node>> {
    match info.reap() {
        Ok(nodes) => {
            let mut nodes = nodes.to_vec();
            nodes.sort_by(|a, b| compare(sort, a, b));
            Some(nodes)
        }
        Err(failure) => {
            error(
                name,
                b"Unable to get slabinfo node data",
                Some(failure.errno()),
            );
            None
        }
    }
}

/// What the wait between frames came to.
enum Waited {
    /// No key in time, or the wait interrupted (`select`'s 0 or -1).
    Nothing,
    /// A key typed.
    Key(u8),
    /// The end of the input, or a read that failed.
    End,
}

/// `select (1, {0}, …, Delay seconds)`, and the one-byte read after it.
fn wait_key(seconds: i64) -> Waited {
    let mut left_ms = i128::from(seconds).saturating_mul(1000);
    loop {
        let chunk = i32::try_from(left_ms.min(i128::from(i32::MAX))).unwrap_or(0);
        match libcall::fd::wait_readable(0, chunk.max(0)) {
            Ok(true) => break,
            Ok(false) => {
                left_ms = left_ms.saturating_sub(i128::from(chunk));
                if left_ms <= 0 {
                    return Waited::Nothing;
                }
            }
            // `select`'s -1: a signal, or no standard input to wait on.
            Err(_) => return Waited::Nothing,
        }
    }
    let mut c = [0u8; 1];
    match libcall::fd::read(0, &mut c) {
        Ok(1) => Waited::Key(c[0]),
        _ => Waited::End,
    }
}

/// The screen's frames, until a key, the end of input or `SIGINT` ends
/// them. The status; or `Err` with it when the program must end at once,
/// the screen as it is.
fn interactive(
    info: &mut SlabInfo,
    mut sort: Sort,
    printer: &Printer,
    name: &[u8],
) -> Result<u8, u8> {
    let is_tty = libcall::fd::is_terminal(0);
    let mut saved_tty = Termios::default();
    if is_tty {
        match tc::get_attr(0) {
            Ok(t) => saved_tty = t,
            Err(e) => error(name, b"terminal setting retrieval", Some(e)),
        }
    }
    let mut old_rows = ROWS.load(Ordering::Relaxed);
    term_resize();
    curses::initscr();
    curses::resizeterm(
        i32::from(ROWS.load(Ordering::Relaxed)),
        i32::from(COLS.load(Ordering::Relaxed)),
    );
    // `signal ()`: glibc's restarts what the signal interrupts. Neither can
    // be refused for these two numbers.
    let _ = libcall::signal::set_handler(libcall::signal::SIGWINCH, on_winch, true);
    let _ = libcall::signal::set_handler(libcall::signal::SIGINT, on_int, true);

    let mut rc = 0u8;
    loop {
        let Some(nodes) = reap(info, sort, name) else {
            rc = 1;
            break;
        };
        let rows = ROWS.load(Ordering::Relaxed);
        if old_rows != rows {
            curses::resizeterm(i32::from(rows), i32::from(COLS.load(Ordering::Relaxed)));
            old_rows = rows;
        }
        curses::mv(0, 0);
        let mut sink = Sink::Screen;
        if printer.summary(info, &mut sink).is_err() {
            error(name, b"Error getting slab summary results", None);
            return Err(1);
        }
        curses::attron(curses::A_REVERSE);
        Printer::headings(&mut sink);
        curses::attroff(curses::A_REVERSE);
        for (i, node) in nodes.iter().enumerate() {
            let shown = i32::from(ROWS.load(Ordering::Relaxed)).saturating_sub(8);
            if i32::try_from(i).map_or(true, |i| i >= shown) {
                break;
            }
            printer.details(node, &mut sink);
        }
        curses::refresh();
        match wait_key(DELAY.load(Ordering::Relaxed)) {
            Waited::Nothing => {}
            Waited::End | Waited::Key(b'q' | b'Q') => break,
            Waited::Key(c) => sort = sort_for(c),
        }
        if DELAY.load(Ordering::Relaxed) == 0 {
            break;
        }
    }
    if is_tty {
        // Put back as it was; upstream does not look at the result.
        let _ = tc::set_attr(0, tc::TCSAFLUSH, &saved_tty);
    }
    curses::endwin();
    Ok(rc)
}

/// Everything `main` does before `close_stdout`, as the status it reaches.
fn run(argv: &[OsString], out: &mut Stream) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    let mut locale = [0u8; 256];
    // `setlocale (LC_ALL, "")`: a locale the environment names that does
    // not exist leaves the program in `C`, as it leaves a C program.
    let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut locale);
    let words = argv.get(1..).unwrap_or(&[]);
    let opts = match parse_opts(&argv0, &name, words, out) {
        Ok(o) => o,
        Err(status) => return status,
    };
    let mut info = match SlabInfo::new() {
        Ok(i) => i,
        Err(e) => {
            error(&name, b"Unable to create slabinfo structure", Some(e));
            return 1;
        }
    };
    let mut point = [0u8; 16];
    let radix = libcall::locale::radix(&mut point)
        .and_then(|n| point.get(..n))
        .unwrap_or(b".")
        .to_vec();
    let printer = Printer { radix };
    if !opts.run_once {
        return match interactive(&mut info, opts.sort, &printer, &name) {
            Ok(rc) | Err(rc) => rc,
        };
    }
    let Some(nodes) = reap(&mut info, opts.sort, &name) else {
        return 1;
    };
    let mut sink = Sink::Stdout(out);
    if printer.summary(&mut info, &mut sink).is_err() {
        error(&name, b"Error getting slab summary results", None);
        return 1;
    }
    Printer::headings(&mut sink);
    for node in &nodes {
        printer.details(node, &mut sink);
    }
    0
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = Stream::stdout();
    let status = run(&argv, &mut out);
    let name = argv
        .first()
        .map_or_else(Vec::new, |a| short_name(&os_bytes(a)).to_vec());
    stdfd::close_stdout_procps(&name, out, ExitCode::from(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &[u8], objs: u32, size: u64) -> Node {
        Node {
            name: name.to_vec(),
            nr_objs: objs,
            cache_size: size,
            ..Node::default()
        }
    }

    #[test]
    fn keys_choose_the_order_and_anything_else_is_the_default() {
        assert_eq!(
            sort_for(b'n'),
            Sort {
                item: Item::Name,
                ascend: true
            }
        );
        assert_eq!(sort_for(b'N').item, Item::Name);
        assert_eq!(sort_for(b'c').item, Item::SizeTotal);
        assert!(!sort_for(b'c').ascend);
        assert_eq!(sort_for(b'u').item, Item::PercentUsed);
        assert_eq!(sort_for(b'x'), DEFAULT);
        assert_eq!(sort_for(0), DEFAULT);
        assert_eq!(sort_for(0xce), DEFAULT);
    }

    #[test]
    fn numbers_sort_largest_first_and_ties_keep_the_files_order() {
        let mut nodes = [
            node(b"a", 5, 0),
            node(b"b", 9, 0),
            node(b"c", 5, 0),
            node(b"d", 9, 0),
        ];
        nodes.sort_by(|x, y| compare(DEFAULT, x, y));
        let names: Vec<&[u8]> = nodes.iter().map(|n| n.name.as_slice()).collect();
        assert_eq!(names, [&b"b"[..], b"d", b"a", b"c"]);
    }

    #[test]
    fn names_sort_as_strcoll_orders_them() {
        let mut nodes = [
            node(b"kmalloc-8", 1, 0),
            node(b"dentry", 1, 0),
            node(b"Acpi", 1, 0),
        ];
        nodes.sort_by(|x, y| compare(sort_for(b'n'), x, y));
        let names: Vec<&[u8]> = nodes.iter().map(|n| n.name.as_slice()).collect();
        // The C locale's order is the bytes'.
        assert_eq!(names, [&b"Acpi"[..], b"dentry", b"kmalloc-8"]);
    }

    #[test]
    fn a_line_is_printfs() {
        let p = Printer {
            radix: b".".to_vec(),
        };
        assert_eq!(
            p.counts(b"Active / Total Objects (% used)", 1, 3),
            b" Active / Total Objects (% used)    : 1 / 3 (33.3%)\n".to_vec()
        );
        // Nothing over nothing: glibc's `-nan`, the sign of x86's default NaN.
        assert_eq!(
            p.counts(b"x", 0, 0),
            b" x                                  : 0 / 0 (-nan%)\n".to_vec()
        );
        assert_eq!(
            p.counts(b"x", 2, 0),
            b" x                                  : 2 / 0 (inf%)\n".to_vec()
        );
        let comma = Printer {
            radix: b",".to_vec(),
        };
        assert_eq!(comma.f(7, 2, 0.5), b"   0,50".to_vec());
    }
}
