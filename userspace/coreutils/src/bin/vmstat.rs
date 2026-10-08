//! `vmstat` -- report virtual memory statistics: procps-ng 4.0.4's, ported.
//!
//! ```text
//! vmstat [options] [delay [count]]
//! ```
//!
//! A transcription of `src/vmstat.c` over the parts of `libproc2` it reads,
//! which live in `coreutils::procps` beside the ones `free`, `ps`, `w` and
//! `uptime` share: [`stat`](coreutils::procps::stat) (`/proc/stat`),
//! [`meminfo`](coreutils::procps::meminfo), [`vmstat`](coreutils::procps::vmstat)
//! (`/proc/vmstat`), [`diskstats`](coreutils::procps::diskstats) and
//! [`slabinfo`](coreutils::procps::slabinfo). It replaces `userspace/vmstat`,
//! which was written from the manual and agreed with upstream on the shape of
//! a line and little else: its own help text, no `-V`, `-S` units off by the
//! difference between `m` and `M`, and no `gu` column.
//!
//! # Upstream's arithmetic, kept
//!
//! - The first line is since boot: paging and interrupts are per second of
//!   uptime, but context switches are divided by the total of CPU ticks, not
//!   by seconds -- upstream's, and visible.
//! - [`unit_convert`] is `(double)size / dataUnit * 1024`, in that order, so a
//!   size that does not divide comes out a unit low, as upstream's does.
//! - A tick count that ran backwards between two readings is no ticks (the
//!   library's clamp), and when a whole sum of them did -- user, system,
//!   idle, busy or total, as when a CPU goes offline -- the interval has no
//!   ticks at all and reads as all idle. Idle ticks that *look* negative once
//!   cut to a C `int` are kept as a `debt` against the next interval.
//! - Every printed percentage is `(100 * part + total / 2) / total` in
//!   `long long`, with `total / 2` taken in `unsigned long long`, as C's
//!   conversions have it.
//! - In `-d`, the header is placed by a count of devices that includes the
//!   partitions it does not print, and the in-progress and I/O-time columns
//!   are divided by 1000 although neither is in milliseconds.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - `-m` sorts the caches by name bytewise, where upstream calls `strcoll`.
//!   The two agree under `C` and `C.UTF-8`; under a locale with a collation
//!   order of its own they need not -- there is no collation table in this
//!   tree to consult (`comm`'s module docs say more).
//! - When `/proc/vmstat` or `/proc/stat` stops reading part way through a
//!   run, upstream dereferences the null that `procps_vmstat_get` or
//!   `procps_stat_get` returns and dies of `SIGSEGV`; this port says
//!   `Unable to select vmstat information` (or upstream's own `Unable to
//!   select stat information`) and exits 1.
//! - A `/proc/uptime` that opens but does not hold two numbers: upstream
//!   appends the reason of whatever call last set `errno`, and this port
//!   appends none. Measured, that is `No such process` for a word where a
//!   number should be -- the library looks every `/proc/meminfo` key up in a
//!   hash table, and a key it does not know leaves `ESRCH` behind -- and
//!   nothing for a file that ends after one number, because glibc's `fscanf`
//!   zeroes `errno` when it meets the end of a file while skipping blanks. So
//!   the second agrees and the first does not, and which reason the first
//!   gets depends on the rest of `/proc`. Likewise for an empty
//!   `/proc/slabinfo`, where this port gives the `ERANGE` the library gives
//!   for a malformed one, and for an empty delay, which `strtol_or_err`
//!   rejects without clearing `errno` first (`free`'s divergence 3). An empty
//!   count agrees: the delay's parse cleared it.
//! - An argument echoed back in a diagnostic cannot rewrite the terminal, as
//!   in `free` (its divergence 6): the one between upstream's apostrophes is
//!   rendered by `quoteaf`, which prints the same `'abc'` for anything
//!   printable without an apostrophe in it, and a partition name by
//!   `escape_unprintable`, which leaves a printable name as it is.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use coreutils::diag;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::diskstats::{self, DiskStats, Kind};
use coreutils::procps::meminfo::{Mem, MemInfo};
use coreutils::procps::scanf::{as_ulong, low_i32, low_u32};
use coreutils::procps::slabinfo::SlabInfo;
use coreutils::procps::stat::{Reading, Stat, sys_delta, tic_delta};
use coreutils::procps::strutils::strtol;
use coreutils::procps::vmstat::{VmStat, now_secs};
use coreutils::procps::{UPTIME_FILE, cvt, uptime_secs};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};

coreutils::guard_std_fds!();

const VMSTAT: Program = Program::new("vmstat", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "afmnsdDp:S:wthVy";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("active", Takes::Nothing),
    ("forks", Takes::Nothing),
    ("slabs", Takes::Nothing),
    ("one-header", Takes::Nothing),
    ("stats", Takes::Nothing),
    ("disk", Takes::Nothing),
    ("disk-sum", Takes::Nothing),
    ("partition", Takes::Required),
    ("unit", Takes::Required),
    ("wide", Takes::Nothing),
    ("timestamp", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("no-first", Takes::Nothing),
];

/// Upstream's `usage()`: the same text on standard output for `--help` and
/// on standard error after a command-line error.
const HELP: &str = concat!(
    "\n",
    "Usage:\n",
    " vmstat [options] [delay [count]]\n",
    "\n",
    "Options:\n",
    " -a, --active           active/inactive memory\n",
    " -f, --forks            number of forks since boot\n",
    " -m, --slabs            slabinfo\n",
    " -n, --one-header       do not redisplay header\n",
    " -s, --stats            event counter statistics\n",
    " -d, --disk             disk statistics\n",
    " -D, --disk-sum         summarize disk statistics\n",
    " -p, --partition <dev>  partition specific statistics\n",
    " -S, --unit <char>      define display unit\n",
    " -w, --wide             wide output\n",
    " -t, --timestamp        show timestamp\n",
    " -y, --no-first         skips first line of output\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see vmstat(8).\n",
);

/// Upstream prints `PROCPS_NG_VERSION`, `vmstat from procps-ng 4.0.4`.
const VERSION: &str = "vmstat from SlateOS coreutils 0.1.0\n";

/// `statMode`'s bits.
const DISKSTAT: u32 = 0x01;
const VMSUMSTAT: u32 = 0x02;
const SLABSTAT: u32 = 0x04;
const PARTITIONSTAT: u32 = 0x08;
const DISKSUMSTAT: u32 = 0x10;

/// The heading of the timestamp column, whose length also cuts `%Z`.
const TIMESTAMP_HEADER: &str = " -----timestamp-----";

/// The size of upstream's `timebuf`, NUL included: a `strftime` result that
/// does not fit is no result.
const TIMEBUF_SIZE: usize = 32;

/// The diagnostics shared by more than one format.
const STAT_NEW: &str = "Unable to create system stat structure";
const STAT_SELECT: &str = "Unable to select stat information";
const VMSTAT_NEW: &str = "Unable to create vmstat structure";
const VMSTAT_SELECT: &str = "Unable to select vmstat information";
const MEM_NEW: &str = "Unable to create meminfo structure";
const MEM_SELECT: &str = "Unable to select memory information";
const DISK_NEW: &str = "Unable to create diskstat structure";
const DISK_REAP: &str = "Unable to retrieve disk statistics";

/// The widths of `new_format`'s eighteen columns, narrow and `-w`.
const WIDTHS: [usize; 18] = [2, 2, 6, 6, 6, 6, 4, 4, 5, 5, 4, 4, 2, 2, 2, 2, 2, 2];
const WIDE_WIDTHS: [usize; 18] = [4, 4, 12, 12, 12, 12, 4, 4, 5, 5, 4, 4, 3, 3, 3, 3, 3, 3];

/// A run's settings: upstream's file-scope variables.
#[derive(Debug)]
struct Run {
    stat_mode: u32,
    a_option: bool,
    w_option: bool,
    y_option: bool,
    t_option: bool,
    sleep_time: u32,
    infinite_updates: bool,
    num_updates: u64,
    height: u32,
    moreheaders: bool,
    /// `dataUnit`.
    data_unit: u64,
    /// `szDataUnit`: the `-S` letter, as `-s` prints it.
    sz_data_unit: u8,
    partition: Vec<u8>,
    zone: localtime::Zone,
}

/// Why the run ended early.
#[derive(Debug)]
enum Stop {
    /// `xerrx`/`xerr`: the diagnostic after `vmstat: `, as bytes -- upstream
    /// prints an argument or a partition name raw.
    Fatal(Vec<u8>),
    /// `usage(stderr)`, after a getopt sentence when there is one.
    Usage(Option<String>),
}

/// `xerrx (EXIT_FAILURE, MESSAGE)`.
fn fatal(message: &str) -> Stop {
    Stop::Fatal(message.as_bytes().to_vec())
}

/// `xerr (EXIT_FAILURE, MESSAGE)`: the message and the reason.
fn fatal_errno(message: &str, e: &std::io::Error) -> Stop {
    Stop::Fatal(format!("{message}: {}", coreutils::errmsg::strerror(e)).into_bytes())
}

/// Write `bytes` to standard output, as upstream's `printf` does: a `Stream`
/// never fails a write but records it, as stdio's error flag does, for
/// `close_stdout` to report at exit -- so there is no result here to act on.
fn emit(out: &mut Stream, bytes: &[u8]) {
    // Never an error; see above.
    let _ = out.write_all(bytes);
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Buffered as stdio buffers it until `run` reaches upstream's
    // `setlinebuf (stdout)`, which comes after the options: `--help`,
    // `--version` and `-f` are written before it.
    let mut out = Stream::stdout();
    let status = match run(&argv, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Stop::Fatal(message)) => {
            // `error (3)` flushes standard output before it speaks, and so
            // does every diagnostic here.
            let mut line = b"vmstat: ".to_vec();
            line.extend_from_slice(&message);
            line.push(b'\n');
            stdfd::diag_bytes(&line);
            ExitCode::FAILURE
        }
        Err(Stop::Usage(sentence)) => {
            if let Some(sentence) = sentence {
                diag!("vmstat: {sentence}");
            }
            stdfd::diag_bytes(HELP.as_bytes());
            ExitCode::FAILURE
        }
    };
    stdfd::close_stdout("vmstat", out, status)
}

fn run(argv: &[OsString], out: &mut Stream) -> Result<(), Stop> {
    let mut run = Run {
        stat_mode: 0,
        a_option: false,
        w_option: false,
        y_option: false,
        t_option: false,
        sleep_time: 1,
        infinite_updates: false,
        num_updates: 1,
        height: 0,
        moreheaders: true,
        data_unit: 1024,
        sz_data_unit: b'K',
        partition: Vec::new(),
        zone: localtime::Zone::from_env(),
    };
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in VMSTAT.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = item.map_err(|e| Stop::Usage(Some(e.sentence)))?;
        match opt {
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                emit(out, VERSION.as_bytes());
                return Ok(());
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                emit(out, HELP.as_bytes());
                return Ok(());
            }
            Opt::Short(b'd', _) | Opt::Long("disk", _) => run.stat_mode |= DISKSTAT,
            Opt::Short(b'a', _) | Opt::Long("active", _) => run.a_option = true,
            // `fork_format (); exit (0);` -- at once, whatever follows.
            Opt::Short(b'f', _) | Opt::Long("forks", _) => return fork_format(out),
            Opt::Short(b'm', _) | Opt::Long("slabs", _) => run.stat_mode |= SLABSTAT,
            Opt::Short(b'D', _) | Opt::Long("disk-sum", _) => run.stat_mode |= DISKSUMSTAT,
            Opt::Short(b'n', _) | Opt::Long("one-header", _) => run.moreheaders = false,
            Opt::Short(b'p', value) | Opt::Long("partition", value) => {
                run.stat_mode |= PARTITIONSTAT;
                let name = os_bytes(&value.unwrap_or_default()).into_owned();
                run.partition = match name.strip_prefix(b"/dev/") {
                    Some(rest) => rest.to_vec(),
                    None => name,
                };
            }
            Opt::Short(b'S', value) | Opt::Long("unit", value) => {
                let arg = os_bytes(&value.unwrap_or_default()).into_owned();
                let first = arg.first().copied().unwrap_or(0);
                run.data_unit = match first {
                    b'b' | b'B' => 1,
                    b'k' => 1000,
                    b'K' => 1024,
                    b'm' => 1_000_000,
                    b'M' => 1_048_576,
                    _ => return Err(fatal("-S requires k, K, m or M (default is KiB)")),
                };
                run.sz_data_unit = first;
            }
            Opt::Short(b's', _) | Opt::Long("stats", _) => run.stat_mode |= VMSUMSTAT,
            Opt::Short(b'w', _) | Opt::Long("wide", _) => run.w_option = true,
            Opt::Short(b't', _) | Opt::Long("timestamp", _) => run.t_option = true,
            Opt::Short(b'y', _) | Opt::Long("no-first", _) => run.y_option = true,
            Opt::Operand(value) => operands.push(os_bytes(value).into_owned()),
            // Unreachable: every letter and every long option is above.
            Opt::Short(..) | Opt::Long(..) => return Err(Stop::Usage(None)),
        }
    }

    let mut operands = operands.into_iter();
    if let Some(delay) = operands.next() {
        let tmp = strtol_or_err(&delay)?;
        if tmp < 1 {
            return Err(fatal("delay must be positive integer"));
        }
        run.sleep_time = u32::try_from(tmp).map_err(|_| fatal("too large delay value"))?;
        run.infinite_updates = true;
    }
    run.num_updates = 1;
    if let Some(count) = operands.next() {
        // A `long` stored in an `unsigned long`: a negative count wraps.
        run.num_updates = as_ulong(strtol_or_err(&count)?);
        run.infinite_updates = false;
    }
    if operands.next().is_some() {
        return Err(Stop::Usage(None));
    }

    if run.moreheaders {
        let wheight = winhi().saturating_sub(3);
        run.height = u32::try_from(wheight).ok().filter(|&h| h > 0).unwrap_or(22);
    }
    // `setlinebuf (stdout)`: each report line is written as it is finished --
    // and on a full disk fails as it is, which leaves the close no reason to
    // give (`vmstat: write error`).
    out.set_line_buffered();

    match run.stat_mode {
        0 => new_format(&run, out),
        VMSUMSTAT => sum_format(&run, out),
        DISKSTAT => diskformat(&run, out),
        PARTITIONSTAT => diskpartition_format(&run, out),
        SLABSTAT => slabformat(&run, out),
        DISKSUMSTAT => disksum_format(out),
        _ => Err(Stop::Usage(None)),
    }
}

/// `strtol_or_err (str, _("failed to parse argument"))`, the argument quoted
/// as `free` quotes it (see the module docs).
fn strtol_or_err(text: &[u8]) -> Result<i64, Stop> {
    strtol(text).map_err(|fault| {
        Stop::Fatal(
            format!(
                "failed to parse argument: {}{}",
                coreutils::quote::quoteaf(text),
                fault.suffix()
            )
            .into_bytes(),
        )
    })
}

/// `winhi`: the rows of the terminal on standard output, or 24.
fn winhi() -> i32 {
    match libcall::pty::window_size(1) {
        Ok(w) if w.rows > 0 => i32::from(w.rows),
        _ => 24,
    }
}

/// `sysconf (_SC_PAGESIZE) / 1024ul`.
fn kb_per_page() -> u64 {
    const SC_PAGESIZE: i32 = 30;
    let p = libcall::conf::sysconf(SC_PAGESIZE);
    u64::try_from(p).unwrap_or(4096) / 1024
}

/// `unitConvert`: `(double)size / dataUnit * ((statMode == SLABSTAT) ? 1 :
/// 1024)`, truncated back to `unsigned long` -- in that order, so a size that
/// does not divide evenly comes out low.
fn unit_convert(run: &Run, size: u64) -> u64 {
    let factor = if run.stat_mode == SLABSTAT {
        1.0
    } else {
        1024.0
    };
    cvt::cvt_u64(cvt::dbl(size) / cvt::dbl(run.data_unit) * factor)
}

/// The local time now through `strftime` into upstream's 32-byte `timebuf`:
/// empty when the result is, or when it would not fit.
fn stamp(run: &Run, format: &[u8]) -> Vec<u8> {
    let tm = run.zone.localtime(now_secs(), 0);
    let text = localtime::strftime(format, &tm);
    if text.len() >= TIMEBUF_SIZE {
        Vec::new()
    } else {
        text
    }
}

/// A data line's `%Y-%m-%d %H:%M:%S`, when `-t` asks for one.
fn data_stamp(run: &Run) -> Vec<u8> {
    if run.t_option {
        stamp(run, b"%Y-%m-%d %H:%M:%S")
    } else {
        Vec::new()
    }
}

/// `%-*s`: `text`, then spaces out to `width`.
fn pad_right(line: &mut Vec<u8>, text: &[u8], width: usize) {
    line.extend_from_slice(text);
    let end = line.len().saturating_add(width.saturating_sub(text.len()));
    line.resize(end, b' ');
}

/// `%*s`: spaces out to `width`, then `text`.
fn pad_left(line: &mut Vec<u8>, text: &[u8], width: usize) {
    let end = line.len().saturating_add(width.saturating_sub(text.len()));
    line.resize(end, b' ');
    line.extend_from_slice(text);
}

/// A header's second line ends, under `-t`, in `" %*s"` of `%Z`, cut to the
/// timestamp heading's width less one.
fn timestamp_heading(run: &Run, line: &mut Vec<u8>) {
    let width = TIMESTAMP_HEADER.len().saturating_sub(1);
    let mut zone = stamp(run, b"%Z");
    zone.truncate(width);
    line.push(b' ');
    pad_left(line, &zone, width);
}

/// The two lines of a header: the banner, and the names right-aligned in
/// `widths`, one space apart.
fn header(run: &Run, out: &mut Stream, banner: &str, names: &[&str], widths: &[usize]) {
    let mut line = banner.as_bytes().to_vec();
    if run.t_option {
        line.extend_from_slice(TIMESTAMP_HEADER.as_bytes());
    }
    line.push(b'\n');
    for (k, (name, &w)) in names.iter().zip(widths).enumerate() {
        if k > 0 {
            line.push(b' ');
        }
        pad_left(&mut line, name.as_bytes(), w);
    }
    if run.t_option {
        timestamp_heading(run, &mut line);
    }
    line.push(b'\n');
    emit(out, &line);
}

fn new_header(run: &Run, out: &mut Stream) {
    let banner = if run.w_option {
        "--procs-- -----------------------memory---------------------- ---swap-- -----io---- -system-- ----------cpu----------"
    } else {
        "procs -----------memory---------- ---swap-- -----io---- -system-- -------cpu-------"
    };
    let (buff, cache) = if run.a_option {
        ("inact", "active")
    } else {
        ("buff", "cache")
    };
    let names = [
        "r", "b", "swpd", "free", buff, cache, "si", "so", "bi", "bo", "in", "cs", "us", "sy",
        "id", "wa", "st", "gu",
    ];
    let widths = if run.w_option { WIDE_WIDTHS } else { WIDTHS };
    header(run, out, banner, &names, &widths);
}

/// One `new_format` line: the eighteen columns right-aligned in upstream's
/// widths, then the timestamp under `-t`.
fn print_row(run: &Run, out: &mut Stream, values: &[u64; 18], timebuf: &[u8]) {
    let widths = if run.w_option { WIDE_WIDTHS } else { WIDTHS };
    let mut line = Vec::new();
    for (k, (v, &w)) in values.iter().zip(&widths).enumerate() {
        if k > 0 {
            line.push(b' ');
        }
        pad_left(&mut line, v.to_string().as_bytes(), w);
    }
    if run.t_option {
        line.push(b' ');
        line.extend_from_slice(timebuf);
    }
    line.push(b'\n');
    emit(out, &line);
}

/// C's `unsigned long long` as `long long`: the same 64 bits.
fn ll(v: u64) -> i64 {
    i64::from_le_bytes(v.to_le_bytes())
}

/// `(unsigned)` of an `unsigned long`: its low 32 bits.
fn unsigned(v: u64) -> u64 {
    u64::from(low_u32(ll(v)))
}

/// `(unsigned)` of a `double`, as gcc compiles it.
fn unsigned_f(v: f64) -> u64 {
    u64::from(cvt::cvt_u32(v))
}

/// `(unsigned)((100 * part + divo2) / div)`, all in `long long`.
fn pct(part: i64, divo2: i64, div: i64) -> u64 {
    let v = part
        .wrapping_mul(100)
        .wrapping_add(divo2)
        .checked_div(div)
        .unwrap_or(0);
    u64::from(low_u32(v))
}

/// `(unsigned)((DELTA + sleep_half) / sleep_time)` of an `unsigned long`
/// delta: in `unsigned long`, then cut to `unsigned int`.
fn per_sec(run: &Run, delta: u64) -> u64 {
    let v = delta
        .wrapping_add(u64::from(run.sleep_time / 2))
        .checked_div(u64::from(run.sleep_time))
        .unwrap_or(0);
    unsigned(v)
}

/// `(unsigned)((DSYSv (E) + sleep_half) / sleep_time)`: an `int` delta plus an
/// `unsigned`, which C does in `unsigned int`.
fn per_sec_int(run: &Run, delta: i32) -> u64 {
    let v = u32::from_le_bytes(delta.to_le_bytes())
        .wrapping_add(run.sleep_time / 2)
        .checked_div(run.sleep_time)
        .unwrap_or(0);
    u64::from(v)
}

/// `new_format`'s CPU tick counts, in its `long long`s.
#[derive(Clone, Copy, Debug)]
struct Cpu {
    /// `cpu_use`: user and nice, guest included.
    user: i64,
    /// `cpu_sys`: system, IRQ and soft IRQ.
    system: i64,
    idle: i64,
    iowait: i64,
    stolen: i64,
    /// `cpu_gue`: guest and guest nice.
    guest: i64,
}

impl Cpu {
    /// `Div`, and the six percentages after upstream's adjustments: a total of
    /// nothing is made 1 with idle 1, `divo2` is half of it, and the guest
    /// ticks are taken out of the user ticks they are counted in (to 0 when
    /// they exceed them).
    fn columns(mut self) -> (i64, [u64; 6]) {
        let mut div = self
            .user
            .wrapping_add(self.system)
            .wrapping_add(self.idle)
            .wrapping_add(self.iowait)
            .wrapping_add(self.stolen);
        if div == 0 {
            div = 1;
            self.idle = 1;
        }
        // `Div / 2UL`: a `long long` over an `unsigned long`, which C does in
        // `unsigned long long`.
        let divo2 = ll(as_ulong(div) / 2);
        self.user = if self.user >= self.guest {
            self.user.wrapping_sub(self.guest)
        } else {
            0
        };
        let pcts = [
            self.user,
            self.system,
            self.idle,
            self.iowait,
            self.stolen,
            self.guest,
        ]
        .map(|part| pct(part, divo2, div));
        (div, pcts)
    }
}

/// The four memory columns, `-a` choosing which two follow `free`.
fn memory_columns(run: &Run, m: &Mem) -> [u64; 4] {
    [
        unit_convert(run, m.swap_used),
        unit_convert(run, m.free),
        unit_convert(run, if run.a_option { m.inactive } else { m.buffers }),
        unit_convert(run, if run.a_option { m.active } else { m.cached_all }),
    ]
}

fn new_format(run: &Run, out: &mut Stream) -> Result<(), Stop> {
    let kbpp = kb_per_page();
    let mut vm = VmStat::new().map_err(|_| fatal(VMSTAT_NEW))?;
    let mut stat = Stat::new().map_err(|_| fatal(STAT_NEW))?;
    let mut mem = MemInfo::new().map_err(|_| fatal(MEM_NEW))?;
    let mut uptime = read_uptime()?;
    if uptime == 0.0 {
        uptime = 1.0;
    }
    new_header(run, out);

    // `pgpgin[tog] = VMSTAT_GET (…)` and its three siblings: the counters the
    // first interval is measured from.
    let mut before = vm.get().map_err(|_| fatal(VMSTAT_SELECT))?;
    let mut m: Mem = mem.select().map_err(|_| fatal(MEM_SELECT))?;

    let mut num_updates = run.num_updates;
    if run.y_option {
        num_updates = num_updates.wrapping_add(1);
    } else {
        let timebuf = data_stamp(run);
        stat.read().map_err(|_| fatal(STAT_SELECT))?;
        let s: Reading = stat.new;
        let cpu = Cpu {
            user: ll(s.cpu.user).wrapping_add(ll(s.cpu.nice)),
            system: ll(s.cpu.system)
                .wrapping_add(ll(s.cpu.irq))
                .wrapping_add(ll(s.cpu.sirq)),
            idle: ll(s.cpu.idle),
            iowait: ll(s.cpu.iowait),
            stolen: ll(s.cpu.stolen),
            guest: ll(s.cpu.guest).wrapping_add(ll(s.cpu.gnice)),
        };
        let (div, [us, sy, id, wa, st, gu]) = cpu.columns();
        let v = vm.get().map_err(|_| fatal(VMSTAT_SELECT))?;
        let per_uptime = |x: u64| unsigned_f(cvt::dbl(x) / uptime);
        let [swpd, free, buff, cache] = memory_columns(run, &m);
        let values = [
            s.sys.procs_running,
            s.sys.procs_blocked,
            swpd,
            free,
            buff,
            cache,
            per_uptime(unit_convert(run, v.pswpin.wrapping_mul(kbpp))),
            per_uptime(unit_convert(run, v.pswpout.wrapping_mul(kbpp))),
            per_uptime(v.pgpgin),
            per_uptime(v.pgpgout),
            per_uptime(s.sys.intr),
            // `SYSv (stat_CTX) / Div`: an `unsigned long` over a `long long`,
            // which C does in `unsigned long long`.
            unsigned(s.sys.ctxt.checked_div(as_ulong(div)).unwrap_or(0)),
            us,
            sy,
            id,
            wa,
            st,
            gu,
        ];
        print_row(run, out, &values, &timebuf);
    }

    // `unsigned int i`, so the header test is in `unsigned int` too.
    let mut debt: i32 = 0;
    let mut i: u32 = 1;
    while run.infinite_updates || u64::from(i) < num_updates {
        std::thread::sleep(Duration::from_secs(u64::from(run.sleep_time)));
        if run.moreheaders && run.height != 0 && i.is_multiple_of(run.height) {
            new_header(run, out);
        }

        stat.read().map_err(|_| fatal(STAT_SELECT))?;
        let (new, old) = (stat.new, stat.old);
        let (n, o) = (new.cpu, old.cpu);
        let mut cpu = Cpu {
            user: tic_delta(n.user, o.user).wrapping_add(tic_delta(n.nice, o.nice)),
            system: tic_delta(n.system, o.system)
                .wrapping_add(tic_delta(n.irq, o.irq))
                .wrapping_add(tic_delta(n.sirq, o.sirq)),
            idle: tic_delta(n.idle, o.idle),
            iowait: tic_delta(n.iowait, o.iowait),
            stolen: tic_delta(n.stolen, o.stolen),
            // Read through `TICv` rather than `DTICv`, but the `DELTA` items'
            // 64 bits as `unsigned long long` are the same number.
            guest: tic_delta(n.guest, o.guest).wrapping_add(tic_delta(n.gnice, o.gnice)),
        };
        let now = vm.get().map_err(|_| fatal(VMSTAT_SELECT))?;
        m = mem.select().map_err(|_| fatal(MEM_SELECT))?;
        let timebuf = data_stamp(run);

        // "idle can run backwards for a moment -- kernel "feature"", through C
        // `int`s: `cpu_idl = (int)cpu_idl + debt`.
        if debt != 0 {
            cpu.idle = i64::from(low_i32(cpu.idle).wrapping_add(debt));
            debt = 0;
        }
        if low_i32(cpu.idle) < 0 {
            debt = low_i32(cpu.idle);
            cpu.idle = 0;
        }
        let (_, [us, sy, id, wa, st, gu]) = cpu.columns();

        let [swpd, free, buff, cache] = memory_columns(run, &m);
        let swapped =
            |a: u64, b: u64| per_sec(run, unit_convert(run, a.wrapping_sub(b).wrapping_mul(kbpp)));
        let values = [
            new.sys.procs_running,
            new.sys.procs_blocked,
            swpd,
            free,
            buff,
            cache,
            swapped(now.pswpin, before.pswpin),
            swapped(now.pswpout, before.pswpout),
            per_sec(run, now.pgpgin.wrapping_sub(before.pgpgin)),
            per_sec(run, now.pgpgout.wrapping_sub(before.pgpgout)),
            per_sec_int(run, sys_delta(new.sys.intr, old.sys.intr)),
            per_sec_int(run, sys_delta(new.sys.ctxt, old.sys.ctxt)),
            us,
            sy,
            id,
            wa,
            st,
            gu,
        ];
        before = now;
        print_row(run, out, &values, &timebuf);
        i = i.wrapping_add(1);
    }
    Ok(())
}

/// `procps_uptime (&uptime, NULL)`, failing as `xerr (…, "Unable to get
/// uptime")` does: with the open's or the read's reason, or with none for a
/// file that did not hold two numbers (see the module docs).
fn read_uptime() -> Result<f64, Stop> {
    let text = std::fs::read(UPTIME_FILE).map_err(|e| fatal_errno("Unable to get uptime", &e))?;
    uptime_secs(&text).ok_or_else(|| fatal("Unable to get uptime"))
}

/// Sleep `sleep_time` between iterations, but not after the last.
fn pause(run: &Run, i: u64) {
    if run.infinite_updates || i.wrapping_add(1) < run.num_updates {
        std::thread::sleep(Duration::from_secs(u64::from(run.sleep_time)));
    }
}

fn diskpartition_format(run: &Run, out: &mut Stream) -> Result<(), Stop> {
    let mut stats = DiskStats::new().map_err(|_| fatal(DISK_NEW))?;
    let not_found = || {
        Stop::Fatal(
            format!(
                "Disk/Partition {} not found",
                coreutils::quote::escape_unprintable(&run.partition)
            )
            .into_bytes(),
        )
    };
    // A read that fails is a node not found, as the library's `NULL` is.
    if stats.get(&run.partition).ok().flatten().is_none() {
        return Err(not_found());
    }
    let mut line = Vec::new();
    pad_right(&mut line, &run.partition, 10);
    line.extend_from_slice(
        format!(
            " {:>10}  {:>16}  {:>10}  {:>16}\n",
            "reads", "read sectors", "writes", "requested writes"
        )
        .as_bytes(),
    );
    emit(out, &line);
    let mut i: u64 = 0;
    while run.infinite_updates || i < run.num_updates {
        let Some(node) = stats.select(&run.partition).ok().flatten() else {
            return Err(not_found());
        };
        let row = format!(
            "{:>21}  {:>16}  {:>10}  {:>16}\n",
            node.reads, node.read_sectors, node.writes, node.write_sectors
        );
        emit(out, row.as_bytes());
        pause(run, i);
        i = i.wrapping_add(1);
    }
    Ok(())
}

fn diskheader(run: &Run, out: &mut Stream) {
    let banner = if run.w_option {
        "disk- -------------------reads------------------- -------------------writes------------------ ------IO-------"
    } else {
        "disk- ------------reads------------ ------------writes----------- -----IO------"
    };
    let names = [
        " ", "total", "merged", "sectors", "ms", "total", "merged", "sectors", "ms", "cur", "sec",
    ];
    let widths: [usize; 11] = if run.w_option {
        [5, 9, 9, 11, 11, 9, 9, 11, 11, 7, 7]
    } else {
        [5, 6, 6, 7, 7, 6, 6, 7, 7, 6, 6]
    };
    header(run, out, banner, &names, &widths);
}

/// `j % height == 0`, `j` an `int` made `unsigned` by the comparison.
fn header_due(run: &Run, j: usize) -> bool {
    run.moreheaders
        && run.height != 0
        && u32::try_from(j).is_ok_and(|j| j.is_multiple_of(run.height))
}

fn diskformat(run: &Run, out: &mut Stream) -> Result<(), Stop> {
    let mut stats = DiskStats::new().map_err(|_| fatal(DISK_NEW))?;
    if !run.moreheaders {
        diskheader(run, out);
    }
    let mut i: u64 = 0;
    while run.infinite_updates || i < run.num_updates {
        let nodes = stats.reap().map_err(|_| fatal(DISK_REAP))?.to_vec();
        let timebuf = data_stamp(run);
        // `j` counts every device, partitions included, though only disks are
        // printed: the header lands where a multiple of `height` falls.
        for (j, node) in nodes.iter().enumerate() {
            if node.kind != Kind::Disk {
                continue;
            }
            if header_due(run, j) {
                diskheader(run, out);
            }
            let mut line = Vec::new();
            pad_right(&mut line, &node.name, 5);
            let (w1, w2, w3, w4) = if run.w_option {
                (9, 11, 7, 7)
            } else {
                (6, 7, 6, 6)
            };
            line.extend_from_slice(
                format!(
                    " {:>w1$} {:>w1$} {:>w2$} {:>w2$} {:>w1$} {:>w1$} {:>w2$} {:>w2$} {:>w3$} {:>w4$}",
                    node.reads,
                    node.reads_merged,
                    node.read_sectors,
                    node.read_time,
                    node.writes,
                    node.writes_merged,
                    node.write_sectors,
                    node.write_time,
                    diskstats::io_inprogress_int(node) / 1000,
                    node.io_time / 1000,
                )
                .as_bytes(),
            );
            if run.t_option {
                line.push(b' ');
                line.extend_from_slice(&timebuf);
            }
            line.push(b'\n');
            emit(out, &line);
        }
        pause(run, i);
        i = i.wrapping_add(1);
    }
    Ok(())
}

fn slabheader(out: &mut Stream) {
    let row = format!(
        "{:<24} {:>6} {:>6} {:>6} {:>6}\n",
        "Cache", "Num", "Total", "Size", "Pages"
    );
    emit(out, row.as_bytes());
}

fn slabformat(run: &Run, out: &mut Stream) -> Result<(), Stop> {
    let mut info =
        SlabInfo::new().map_err(|e| fatal_errno("Unable to create slabinfo structure", &e))?;
    if !run.moreheaders {
        slabheader(out);
    }
    let mut i: u64 = 0;
    while run.infinite_updates || i < run.num_updates {
        let mut nodes = info
            .reap()
            .map_err(|_| fatal("Unable to get slabinfo node data"))?
            .to_vec();
        // `procps_slabinfo_sort (…, SLAB_NAME, SLABINFO_SORT_ASCEND)`, by
        // bytes rather than `strcoll` -- see the module docs.
        nodes.sort_by(|x, y| x.name.cmp(&y.name));
        for (j, node) in nodes.iter().enumerate() {
            if header_due(run, j) {
                slabheader(out);
            }
            // `%-24.24s`: cut to 24 bytes and padded to them.
            let mut line = Vec::new();
            let name = node.name.get(..node.name.len().min(24)).unwrap_or_default();
            pad_right(&mut line, name, 24);
            line.extend_from_slice(
                format!(
                    " {:>6} {:>6} {:>6} {:>6}\n",
                    node.nr_active_objs, node.nr_objs, node.obj_size, node.objs_per_slab
                )
                .as_bytes(),
            );
            emit(out, &line);
        }
        pause(run, i);
        i = i.wrapping_add(1);
    }
    Ok(())
}

fn disksum_format(out: &mut Stream) -> Result<(), Stop> {
    let mut stats = DiskStats::new().map_err(|_| fatal(DISK_NEW))?;
    let nodes = stats.reap().map_err(|_| fatal(DISK_REAP))?.to_vec();
    let (mut disks, mut parts) = (0i32, 0i32);
    let mut sums = [0u64; 11];
    for node in &nodes {
        if node.kind != Kind::Disk {
            parts = parts.wrapping_add(1);
            continue;
        }
        disks = disks.wrapping_add(1);
        // `inprogress_IO += diskVAL(disk_IO, s_int) / 1000`: an `int` added to
        // an `unsigned long`, sign-extended first.
        let io = as_ulong(i64::from(diskstats::io_inprogress_int(node) / 1000));
        let add = [
            node.reads,
            node.reads_merged,
            node.read_sectors,
            node.read_time,
            node.writes,
            node.writes_merged,
            node.write_sectors,
            node.write_time,
            io,
            node.io_time / 1000,
            node.io_wtime / 1000,
        ];
        for (sum, v) in sums.iter_mut().zip(add) {
            *sum = sum.wrapping_add(v);
        }
    }
    let labels = [
        "total reads",
        "merged reads",
        "read sectors",
        "milli reading",
        "writes",
        "merged writes",
        "written sectors",
        "milli writing",
        "inprogress IO",
        "milli spent IO",
        "milli weighted IO",
    ];
    let mut text = format!("{disks:>13} disks\n{parts:>13} partitions\n");
    for (sum, label) in sums.iter().zip(labels) {
        text.push_str(&format!("{sum:>13} {label}\n"));
    }
    emit(out, text.as_bytes());
    Ok(())
}

fn sum_format(run: &Run, out: &mut Stream) -> Result<(), Stop> {
    let mut stat = Stat::new().map_err(|_| fatal(STAT_NEW))?;
    stat.read().map_err(|_| fatal(STAT_SELECT))?;
    let s = stat.new;
    let mut vm = VmStat::new().map_err(|_| fatal(VMSTAT_NEW))?;
    let mut mem = MemInfo::new().map_err(|_| fatal(MEM_NEW))?;
    let m = mem.select().map_err(|_| fatal(MEM_SELECT))?;
    let unit = char::from(run.sz_data_unit);
    let mem_rows = [
        (m.total, "total memory"),
        (m.used, "used memory"),
        (m.active, "active memory"),
        (m.inactive, "inactive memory"),
        (m.free, "free memory"),
        (m.buffers, "buffer memory"),
        (m.cached_all, "swap cache"),
        (m.swap_total, "total swap"),
        (m.swap_used, "used swap"),
        (m.swap_free, "free swap"),
    ];
    let mut text = String::new();
    for (value, label) in mem_rows {
        text.push_str(&format!(
            "{:>13} {unit} {label}\n",
            unit_convert(run, value)
        ));
    }
    let tick_rows = [
        (s.cpu.user, "non-nice user cpu ticks"),
        (s.cpu.nice, "nice user cpu ticks"),
        (s.cpu.system, "system cpu ticks"),
        (s.cpu.idle, "idle cpu ticks"),
        (s.cpu.iowait, "IO-wait cpu ticks"),
        (s.cpu.irq, "IRQ cpu ticks"),
        (s.cpu.sirq, "softirq cpu ticks"),
        (s.cpu.stolen, "stolen cpu ticks"),
        (s.cpu.guest, "non-nice guest cpu ticks"),
        (s.cpu.gnice, "nice guest cpu ticks"),
    ];
    for (value, label) in tick_rows {
        // `%13lld` of an `unsigned long long`.
        text.push_str(&format!("{:>13} {label}\n", ll(value)));
    }
    let v = vm.get().map_err(|_| fatal(VMSTAT_SELECT))?;
    let counter_rows = [
        (v.pgpgin, "K paged in"),
        (v.pgpgout, "K paged out"),
        (v.pswpin, "pages swapped in"),
        (v.pswpout, "pages swapped out"),
        (s.sys.intr, "interrupts"),
        (s.sys.ctxt, "CPU context switches"),
        (s.sys.btime, "boot time"),
        (s.sys.procs_created, "forks"),
    ];
    for (value, label) in counter_rows {
        text.push_str(&format!("{value:>13} {label}\n"));
    }
    emit(out, text.as_bytes());
    Ok(())
}

fn fork_format(out: &mut Stream) -> Result<(), Stop> {
    let mut stat = Stat::new().map_err(|_| fatal(STAT_NEW))?;
    let s = stat.get().map_err(|_| fatal(STAT_SELECT))?;
    emit(
        out,
        format!("{:>13} forks\n", s.sys.procs_created).as_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(user: i64, system: i64, idle: i64, iowait: i64, stolen: i64, guest: i64) -> Cpu {
        Cpu {
            user,
            system,
            idle,
            iowait,
            stolen,
            guest,
        }
    }

    #[test]
    fn percentages_round_half_up_and_guest_leaves_user() {
        // 30 + 10 + 60 = 100 ticks; 10 of the 30 user ticks were a guest's.
        let (div, p) = cpu(30, 10, 60, 0, 0, 10).columns();
        assert_eq!(div, 100);
        assert_eq!(p, [20, 10, 60, 0, 0, 10]);
        // 1 of 3 is 33.3%, rounded by adding half the total first.
        assert_eq!(cpu(1, 1, 1, 0, 0, 0).columns().1, [33, 33, 33, 0, 0, 0]);
        // Guest beyond user leaves user at 0, not negative.
        assert_eq!(cpu(2, 0, 2, 0, 0, 5).columns().1[0], 0);
    }

    #[test]
    fn no_ticks_at_all_reads_as_all_idle() {
        assert_eq!(cpu(0, 0, 0, 0, 0, 0).columns(), (1, [0, 0, 100, 0, 0, 0]));
    }

    #[test]
    fn a_negative_total_halves_as_unsigned() {
        // `Div / 2UL` of -2 is 2^63 - 1, so every column is skewed by it.
        let (div, p) = cpu(-2, 0, 0, 0, 0, 0).columns();
        assert_eq!(div, -2);
        let divo2 = i64::MAX;
        assert_eq!(p[1], pct(0, divo2, -2));
    }

    #[test]
    fn padding() {
        let mut line = Vec::new();
        pad_left(&mut line, b"ab", 4);
        pad_right(&mut line, b"cd", 3);
        pad_right(&mut line, b"toolong", 3);
        assert_eq!(line, b"  abcd toolong");
    }
}
