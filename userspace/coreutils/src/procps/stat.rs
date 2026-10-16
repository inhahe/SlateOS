//! `/proc/stat`, transcribed from procps-ng 4.0.4's `library/stat.c`: the
//! summary `cpu` line's ticks and the system counters, with the one reading of
//! history the library keeps so that a `DELTA` item is the difference of two
//! reads -- all `vmstat` asks for, as [`Stat`] -- and, for `top`, the same for
//! every `cpuN` line, each CPU tied to its core from `/proc/cpuinfo`, as
//! [`StatInfo`] (`procps_stat_reap`).
//!
//! # What the parse keeps from `stat_read_failed`
//!
//! - The summary line is `sscanf (bp, "cpu %llu %llu …")` over ten fields, and
//!   fewer than eight conversions is a failed read (`ERANGE`) -- which is how
//!   `vmstat` comes to say `Unable to select stat information`. A field the
//!   line does not have keeps the value it had in the previous reading,
//!   because the library writes the new reading over the old one in place.
//! - The counters (`intr`, `ctxt`, `btime`, `processes`, `procs_blocked`,
//!   `procs_running`) are found with `strstr` from the first line after the
//!   `cpuN` lines, each 0 when absent or unreadable.
//! - `procs_running` is one less than the file says, when it says anything:
//!   the library does not count the process reading it.
//! - The text is a C string, so a NUL in the file ends it.
//! - When the summary's ticks went backwards between two readings -- any of
//!   its user, system, idle, busy or total sums smaller than before, as when
//!   a CPU goes offline -- the older reading is discarded and the newer one
//!   stands for both, so every tick delta of that reading is 0
//!   (`stat_derive_unique`).
//!
//! The `DELTA` items are computed by the caller from a [`Stat`]'s `new` and
//! `old` readings with [`tic_delta`] and [`sys_delta`], which keep the
//! library's types: a tick delta that ran backwards is 0, a counter's is cut
//! to an `int`.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::scanf::{Scan, c_str};

/// `STAT_FILE`.
pub const STAT_FILE: &str = "/proc/stat";

/// `struct stat_jifs`'s ten fields, from a `cpu` line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Jifs {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub sirq: u64,
    pub stolen: u64,
    pub guest: u64,
    pub gnice: u64,
}

/// `struct stat_data`: the counters after the CPU lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sys {
    pub intr: u64,
    pub ctxt: u64,
    pub btime: u64,
    pub procs_created: u64,
    pub procs_blocked: u64,
    pub procs_running: u64,
}

impl Jifs {
    /// `stat_derive_unique`'s five sums, in `unsigned long long`: user (with
    /// nice), system (with both IRQs), idle (with I/O wait), busy, and the
    /// total (stolen and both guests included).
    #[must_use]
    pub fn sums(&self) -> [u64; 5] {
        let xusr = self.user.wrapping_add(self.nice);
        let xsys = self.system.wrapping_add(self.irq).wrapping_add(self.sirq);
        let xidl = self.idle.wrapping_add(self.iowait);
        let xtot = xusr
            .wrapping_add(xsys)
            .wrapping_add(xidl)
            .wrapping_add(self.stolen)
            .wrapping_add(self.guest)
            .wrapping_add(self.gnice);
        let xbsy = xtot.wrapping_sub(xidl);
        [xusr, xsys, xidl, xbsy, xtot]
    }
}

/// One reading of the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub cpu: Jifs,
    pub sys: Sys,
}

/// `stat_read_failed`'s parse of the file's text, over `previous` -- the
/// reading a field the summary line lacks keeps its value from.
///
/// # Errors
///
/// `ERANGE` when the summary line holds fewer than eight numbers.
pub fn parse(text: &[u8], previous: &Reading) -> io::Result<Reading> {
    /// `ERANGE`.
    const ERANGE: i32 = 34;
    // The buffer is NUL-terminated and read with `sscanf` and `strstr`, so a
    // NUL in the file ends it.
    let text = c_str(text);
    let mut cpu = previous.cpu;
    let mut scan = Scan::new(text);
    let mut converted = 0usize;
    if scan.lit(b"cpu ").is_some() {
        for slot in [
            &mut cpu.user,
            &mut cpu.nice,
            &mut cpu.system,
            &mut cpu.idle,
            &mut cpu.iowait,
            &mut cpu.irq,
            &mut cpu.sirq,
            &mut cpu.stolen,
            &mut cpu.guest,
            &mut cpu.gnice,
        ] {
            match scan.ulong() {
                Some(v) => {
                    *slot = v;
                    converted = converted.saturating_add(1);
                }
                None => break,
            }
        }
    }
    if converted < 8 {
        return Err(io::Error::from_raw_os_error(ERANGE));
    }

    // The `cpuN` lines: `bp` moves to the next line, and stays on the first
    // one that is not a CPU line -- fewer than eight conversions, the `%d`
    // counted -- which is where the counters are searched for from.
    let mut at = 0usize;
    loop {
        let Some(nl) = text
            .get(at..)
            .and_then(|t| t.iter().position(|&c| c == b'\n'))
        else {
            // Upstream would step past a missing newline into the void; there
            // is nothing after the last line to search.
            at = text.len();
            break;
        };
        at = at.saturating_add(nl).saturating_add(1);
        if cpu_line_conversions(text.get(at..).unwrap_or_default()) < 8 {
            break;
        }
    }
    let rest = text.get(at..).unwrap_or_default();

    let mut sys = Sys {
        intr: counter(rest, b"intr "),
        ctxt: counter(rest, b"ctxt "),
        btime: counter(rest, b"btime "),
        procs_created: counter(rest, b"processes "),
        procs_blocked: counter(rest, b"procs_blocked "),
        procs_running: counter(rest, b"procs_running "),
    };
    // `if (llnum) llnum--; //exclude itself` -- and a 0 stays 0.
    sys.procs_running = sys.procs_running.saturating_sub(1);
    Ok(Reading { cpu, sys })
}

/// A `STAT_TIC_DELTA_*` item: the difference of two `unsigned long long`
/// readings stored in a `signed long`, and 0 when that is negative -- the
/// library's `TICsetH`, so ticks that ran backwards read as none.
#[must_use]
pub fn tic_delta(new: u64, old: u64) -> i64 {
    i64::from_le_bytes(new.wrapping_sub(old).to_le_bytes()).max(0)
}

/// A `STAT_SYS_DELTA_*` item: the difference of two `unsigned long`
/// readings stored in an `int` -- the library's `SYSsetH`, unclamped, so
/// only the low 32 bits are kept.
#[must_use]
pub fn sys_delta(new: u64, old: u64) -> i32 {
    super::scanf::low_i32(i64::from_le_bytes(new.wrapping_sub(old).to_le_bytes()))
}

/// How many conversions `sscanf (bp, "cpu%d %llu …")` makes of one line.
fn cpu_line_conversions(line: &[u8]) -> usize {
    let mut scan = Scan::new(line);
    if scan.lit(b"cpu").is_none() || scan.int().is_none() {
        return 0;
    }
    let mut n = 1usize;
    for _ in 0..10 {
        if scan.ulong().is_none() {
            break;
        }
        n = n.saturating_add(1);
    }
    n
}

/// `if ((b = strstr(bp, "KEY "))) sscanf(b, "KEY %llu", &llnum);`, 0 when
/// either step finds nothing.
fn counter(text: &[u8], key: &[u8]) -> u64 {
    let Some(at) = text.windows(key.len()).position(|w| w == key) else {
        return 0;
    };
    let mut scan = Scan::new(text.get(at..).unwrap_or_default());
    scan.lit(key).and_then(|()| scan.ulong()).unwrap_or(0)
}

/// procps' `struct stat_info`, cut to what `vmstat` uses: the file, held open
/// as the library holds it, and the reading now and the one before it.
#[derive(Debug)]
pub struct Stat {
    file: File,
    pub new: Reading,
    pub old: Reading,
    /// `sav_secs`: when [`Stat::get`] last read the file, by `time()`.
    sav_secs: i64,
}

impl Stat {
    /// `procps_stat_new`, whose priming read makes a `DELTA` item useful
    /// from the first select on.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read, or its summary line was short.
    pub fn new() -> io::Result<Self> {
        let mut stat = Self {
            file: File::open(STAT_FILE)?,
            new: Reading::default(),
            old: Reading::default(),
            sav_secs: 0,
        };
        stat.read()?;
        Ok(stat)
    }

    /// `procps_stat_get`: the newest reading, the file read again first only
    /// if the clock has moved on a second since `get` last read it -- which
    /// the first `get` always finds, the priming read not counting.
    ///
    /// # Errors
    ///
    /// The read that was due failed.
    pub fn get(&mut self) -> io::Result<Reading> {
        let cur_secs = super::vmstat::now_secs();
        if cur_secs.saturating_sub(self.sav_secs) >= 1 {
            self.read()?;
            self.sav_secs = cur_secs;
        }
        Ok(self.new)
    }

    /// `stat_read_failed`: the whole file read again from its start, the
    /// reading so far becoming the old one.
    ///
    /// # Errors
    ///
    /// As [`Stat::new`].
    pub fn read(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut text = Vec::new();
        self.file.read_to_end(&mut text)?;
        let reading = parse(&text, &self.new)?;
        self.old = baseline(self.new, &reading);
        self.new = reading;
        Ok(())
    }
}

/// The reading `new` is measured from: `previous`, unless the summary's
/// ticks went backwards. `stat_derive_unique`'s "don't distort deltas when
/// cpus are taken offline or brought online": if any of the five sums is
/// smaller than before, the old ticks become the new ones, and every tick
/// delta of this reading is 0. The counters after the CPU lines keep theirs.
fn baseline(previous: Reading, new: &Reading) -> Reading {
    let went_back = new
        .cpu
        .sums()
        .iter()
        .zip(previous.cpu.sums())
        .any(|(&now, before)| now < before);
    if went_back {
        Reading {
            cpu: new.cpu,
            sys: previous.sys,
        }
    } else {
        previous
    }
}

// ---------------------------------------------------------------------------
// The per-CPU reading `top` makes: `procps_stat_reap`.
// ---------------------------------------------------------------------------

/// `STAT_SUMMARY_ID`: the summary stack's `STAT_TIC_ID`.
pub const STAT_SUMMARY_ID: i32 = -11111;
/// `STAT_NODE_INVALID`: `STAT_TIC_NUMA_NODE` of a CPU no node was found for.
pub const STAT_NODE_INVALID: i32 = -22222;
/// `STAT_TIC_TYPE_CORE`'s values: a power-saving core, and a full one (0 is
/// "unsure").
pub const E_CORE: i32 = 1;
/// See [`E_CORE`].
pub const P_CORE: i32 = 2;
/// A core's second thread when it has only one.
const VACANT: i32 = -1;
/// `CORE_BUFSIZ`: `fgets` reads `/proc/cpuinfo` in pieces of at most one less
/// than this.
const CORE_BUFSIZ: usize = 1024;

/// `struct stat_core`: one core, and the (at most two) CPUs on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Core {
    id: i32,
    kind: i32,
    thread_1: i32,
    thread_2: i32,
}

/// `stat_core_add`: `cpu` is on core `core`. A core seen with a second thread
/// is a full core; a third replaces the second.
fn core_add(cores: &mut Vec<Core>, core: i32, cpu: i32) {
    if let Some(c) = cores.iter_mut().find(|c| c.id == core) {
        if cpu == c.thread_1 || cpu == c.thread_2 {
            return;
        }
        c.thread_2 = cpu;
        c.kind = P_CORE;
        return;
    }
    cores.push(Core {
        id: core,
        kind: 0,
        thread_1: cpu,
        thread_2: VACANT,
    });
}

/// `stat_cores_check`: once any core has two threads, every core with one is
/// taken for a power-saving core. Without any, all stay "unsure".
fn cores_check(cores: &mut [Core]) {
    if cores.iter().any(|c| c.kind == P_CORE) {
        for c in cores.iter_mut().filter(|c| c.thread_2 == VACANT) {
            c.kind = E_CORE;
        }
    }
}

/// `fgets (buf, CORE_BUFSIZ, fp)` over a whole file: each line, or each piece
/// of at most `CORE_BUFSIZ - 1` bytes of a longer one.
fn fgets_pieces(text: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let line_end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |nl| nl.saturating_add(1));
        let take = line_end.min(CORE_BUFSIZ - 1);
        let (piece, tail) = rest.split_at(take);
        out.push(piece);
        rest = tail;
    }
    out
}

/// `stat_cores_verify`'s walk of `/proc/cpuinfo`: each `processor` line, then
/// its `core id` -- or, at a blank line before one, the CPU's own number. A
/// processor whose entry the file ends inside is not added. A line that does
/// not convert leaves the number from before it.
fn cores_verify(text: &[u8], cores: &mut Vec<Core>) {
    let pieces = fgets_pieces(c_str(text));
    let mut it = pieces.into_iter();
    let (mut a_cpu, mut a_core) = (0i32, 0i32);
    'processors: while let Some(buf) = it.next() {
        if buf.first() != Some(&b'p') || !contains(buf, b"processor") {
            continue;
        }
        let mut sc = Scan::new(buf);
        if let Some(v) = sc.lit(b"processor : ").and_then(|()| sc.int()) {
            a_cpu = v;
        }
        loop {
            let Some(buf) = it.next() else {
                break 'processors;
            };
            if buf.first() == Some(&b'\n') {
                a_core = a_cpu;
                break;
            }
            if buf.first() != Some(&b'c') || !contains(buf, b"core id") {
                continue;
            }
            let mut sc = Scan::new(buf);
            if let Some(v) = sc.lit(b"core id : ").and_then(|()| sc.int()) {
                a_core = v;
            }
            break;
        }
        core_add(cores, a_core, a_cpu);
    }
    cores_check(cores);
}

/// `strstr` as a test.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `struct hist_tic`: one CPU's ticks now and at the reading before.
#[derive(Clone, Copy, Debug, Default)]
struct HistTic {
    id: i32,
    numa_node: i32,
    new: Jifs,
    old: Jifs,
    /// The core this CPU is on, as an index into [`StatInfo::cores`].
    core: Option<usize>,
    saved_id: i32,
}

impl HistTic {
    /// `stat_derive_unique`'s guard: when any of the five sums ran backwards
    /// -- a CPU taken offline or brought back -- the old reading is replaced by
    /// the new, so every delta is 0.
    fn derive_unique(&mut self) {
        let went_back = self
            .new
            .sums()
            .iter()
            .zip(self.old.sums())
            .any(|(&now, before)| now < before);
        if went_back {
            self.old = self.new;
        }
    }
}

/// One stack of `top`'s `Stat_items`: a CPU's, a node's, or the summary's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TicStack {
    /// `STAT_TIC_ID`: the CPU's number, a node's, or [`STAT_SUMMARY_ID`].
    pub id: i32,
    /// `STAT_TIC_NUMA_NODE`.
    pub numa_node: i32,
    /// `STAT_TIC_TYPE_CORE`: [`E_CORE`], [`P_CORE`], or 0.
    pub type_core: i32,
    /// `STAT_TIC_DELTA_USER` and the nine after it, in `/proc/stat`'s order:
    /// user, nice, system, idle, iowait, irq, softirq, steal, guest,
    /// guest_nice.
    pub delta: [i64; 10],
    /// `STAT_TIC_SUM_DELTA_USER`, `_SYSTEM`, `_IDLE`, `_BUSY` and `_TOTAL`.
    pub sum_delta: [i64; 5],
}

impl TicStack {
    /// `stat_assign_results` for `top`'s items, from one history entry and
    /// the core it is on.
    fn of(t: &HistTic, cores: &[Core]) -> Self {
        let field = |j: &Jifs| {
            [
                j.user, j.nice, j.system, j.idle, j.iowait, j.irq, j.sirq, j.stolen, j.guest,
                j.gnice,
            ]
        };
        let (new, old) = (field(&t.new), field(&t.old));
        let mut delta = [0i64; 10];
        for (d, (n, o)) in delta.iter_mut().zip(new.iter().zip(old.iter())) {
            *d = tic_delta(*n, *o);
        }
        let (ns, os) = (t.new.sums(), t.old.sums());
        let mut sum_delta = [0i64; 5];
        for (d, (n, o)) in sum_delta.iter_mut().zip(ns.iter().zip(os.iter())) {
            *d = tic_delta(*n, *o);
        }
        Self {
            id: t.id,
            numa_node: t.numa_node,
            type_core: t.core.and_then(|c| cores.get(c)).map_or(0, |c| c.kind),
            delta,
            sum_delta,
        }
    }
}

/// `struct stat_reaped`: the summary, every CPU, and every NUMA node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reaped {
    pub summary: TicStack,
    pub cpus: Vec<TicStack>,
    /// Always empty: the nodes come from `libnuma`, which SlateOS does not
    /// have -- `numa_max_node()` is -1 without it, so there are none.
    pub numa: Vec<TicStack>,
}

/// procps' `struct stat_info` as `top` uses it: `/proc/stat` held open, the
/// summary's and every CPU's history, and the cores.
#[derive(Debug)]
pub struct StatInfo {
    file: File,
    sys_new: Sys,
    cpu_hist: HistTic,
    cpus: Vec<HistTic>,
    n_cpus: usize,
    cores: Vec<Core>,
    cpuinfo: PathBuf,
    cpu_count_hwm: usize,
    /// `stat_read_failed`'s `static int once_sw`: the first CPU line ever
    /// read is linked to its core whatever its number.
    once_sw: bool,
}

impl StatInfo {
    /// `procps_stat_new`: the cores found, then the priming read, so that the
    /// first reap's deltas already mean something.
    ///
    /// # Errors
    ///
    /// `/proc/stat` could not be opened or read, or its summary line was
    /// short. A missing `/proc/cpuinfo` is not an error: there are no cores.
    pub fn new(root: &Path) -> io::Result<Self> {
        let mut info = Self {
            file: File::open(root.join("stat"))?,
            sys_new: Sys::default(),
            cpu_hist: HistTic {
                id: STAT_SUMMARY_ID,
                numa_node: STAT_NODE_INVALID,
                ..HistTic::default()
            },
            cpus: Vec::new(),
            n_cpus: 0,
            cores: Vec::new(),
            cpuinfo: root.join("cpuinfo"),
            cpu_count_hwm: 0,
            once_sw: false,
        };
        info.verify_cores();
        info.read()?;
        Ok(info)
    }

    /// `stat_cores_verify`, tolerant of a missing file.
    fn verify_cores(&mut self) {
        if let Ok(text) = std::fs::read(&self.cpuinfo) {
            cores_verify(&text, &mut self.cores);
        }
    }

    /// `stat_read_failed`: the whole file again, every reading moved to the
    /// old one first.
    fn read(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut text = Vec::new();
        self.file.read_to_end(&mut text)?;
        let text = c_str(&text);

        // The summary, over the reading before it.
        self.cpu_hist.old = self.cpu_hist.new;
        let reading = parse(
            text,
            &Reading {
                cpu: self.cpu_hist.new,
                sys: self.sys_new,
            },
        )?;
        self.cpu_hist.new = reading.cpu;
        self.cpu_hist.derive_unique();
        self.sys_new = reading.sys;

        // The CPU lines: each into the slot of its position, until the first
        // line that is not one.
        let mut at = text
            .iter()
            .position(|&c| c == b'\n')
            .map_or(text.len(), |nl| nl.saturating_add(1));
        let mut i = 0usize;
        loop {
            if self.cpus.len() <= i {
                self.cpus.push(HistTic::default());
            }
            let line = text.get(at..).unwrap_or_default();
            let Some(slot) = self.cpus.get_mut(i) else {
                break;
            };
            slot.old = slot.new;
            slot.numa_node = STAT_NODE_INVALID;
            if !scan_cpu_line(line, slot) {
                break;
            }
            slot.derive_unique();
            if !self.once_sw {
                self.once_sw = true;
                slot.saved_id = -1;
            }
            if slot.saved_id != slot.id {
                slot.saved_id = slot.id;
                let id = slot.id;
                slot.core = self
                    .cores
                    .iter()
                    .position(|c| id == c.thread_1 || id == c.thread_2);
            }
            i = i.saturating_add(1);
            match line.iter().position(|&c| c == b'\n') {
                Some(nl) => at = at.saturating_add(nl).saturating_add(1),
                None => break,
            }
        }
        self.n_cpus = i;
        if self.cpu_count_hwm < self.n_cpus {
            if self.cpu_count_hwm != 0 {
                self.verify_cores();
            }
            self.cpu_count_hwm = self.n_cpus;
        }
        Ok(())
    }

    /// `procps_stat_reap`: read again, and the stacks. `numa_too` is
    /// `STAT_REAP_NUMA_NODES_TOO`, which reaps no node here (see
    /// [`Reaped::numa`]).
    ///
    /// # Errors
    ///
    /// The read failed, as [`StatInfo::new`] says.
    pub fn reap(&mut self, numa_too: bool) -> io::Result<Reaped> {
        let _ = numa_too; // no nodes without libnuma: both kinds reap alike
        self.read()?;
        let cpus = self
            .cpus
            .get(..self.n_cpus)
            .unwrap_or_default()
            .iter()
            .map(|t| TicStack::of(t, &self.cores))
            .collect();
        Ok(Reaped {
            summary: TicStack::of(&self.cpu_hist, &self.cores),
            cpus,
            numa: Vec::new(),
        })
    }
}

/// `sscanf (bp, "cpu%d %llu %llu …", &id, …)` into `slot`, writing each field
/// as it converts -- so a line that stops short leaves what it did convert --
/// and true when it made the eight conversions a CPU line needs.
fn scan_cpu_line(line: &[u8], slot: &mut HistTic) -> bool {
    let mut sc = Scan::new(line);
    if sc.lit(b"cpu").is_none() {
        return false;
    }
    let Some(id) = sc.int() else {
        return false;
    };
    slot.id = id;
    let mut n = 1usize;
    let j = &mut slot.new;
    for field in [
        &mut j.user,
        &mut j.nice,
        &mut j.system,
        &mut j.idle,
        &mut j.iowait,
        &mut j.irq,
        &mut j.sirq,
        &mut j.stolen,
        &mut j.guest,
        &mut j.gnice,
    ] {
        match sc.ulong() {
            Some(v) => {
                *field = v;
                n = n.saturating_add(1);
            }
            None => break,
        }
    }
    n >= 8
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const TEXT: &[u8] = b"cpu  100 2 30 400 5 6 7 8 9 10\n\
cpu0 50 1 15 200 2 3 3 4 4 5\n\
cpu1 50 1 15 200 3 3 4 4 5 5\n\
intr 12345 0 1 2\n\
ctxt 678\n\
btime 1700000000\n\
processes 4321\n\
procs_running 3\n\
procs_blocked 1\n";

    #[test]
    fn the_summary_and_the_counters() {
        let r = parse(TEXT, &Reading::default()).unwrap();
        assert_eq!(
            r.cpu,
            Jifs {
                user: 100,
                nice: 2,
                system: 30,
                idle: 400,
                iowait: 5,
                irq: 6,
                sirq: 7,
                stolen: 8,
                guest: 9,
                gnice: 10
            }
        );
        assert_eq!(
            r.sys,
            Sys {
                intr: 12345,
                ctxt: 678,
                btime: 1_700_000_000,
                procs_created: 4321,
                procs_blocked: 1,
                procs_running: 2,
            }
        );
    }

    #[test]
    fn a_short_summary_line_is_erange_and_eight_fields_suffice() {
        let short = parse(b"cpu 1 2 3 4 5 6 7\n", &Reading::default()).unwrap_err();
        assert_eq!(short.raw_os_error(), Some(34));
        let mut before = Reading::default();
        before.cpu.guest = 77;
        let eight = parse(b"cpu 1 2 3 4 5 6 7 8\n", &before).unwrap();
        assert_eq!(eight.cpu.stolen, 8);
        assert_eq!(eight.cpu.guest, 77, "a missing field keeps its last value");
    }

    #[test]
    fn counters_absent_are_zero_and_procs_running_zero_stays_zero() {
        let r = parse(
            b"cpu 1 2 3 4 5 6 7 8 9 10\nprocs_running 0\n",
            &Reading::default(),
        )
        .unwrap();
        assert_eq!(r.sys, Sys::default());
    }

    #[test]
    fn counters_are_searched_for_from_the_first_line_that_is_not_a_cpu() {
        // Wherever that line is: here it is the counter's own line, and the
        // per-CPU line after it is never reached.
        let text = b"cpu 1 2 3 4 5 6 7 8 9 10\nctxt 5\ncpu0 1 2 3 4 5 6 7 8 9 10\n";
        assert_eq!(parse(text, &Reading::default()).unwrap().sys.ctxt, 5);
        let text = b"cpu 1 2 3 4 5 6 7 8 9 10\ncpu0 1 2 3 4 5 6 7 8 9 10\nctxt 9\n";
        assert_eq!(parse(text, &Reading::default()).unwrap().sys.ctxt, 9);
        // A per-CPU line with too few fields ends the walk where it stands.
        let text = b"cpu 1 2 3 4 5 6 7 8 9 10\ncpu0 1 2\nctxt 4\n";
        assert_eq!(parse(text, &Reading::default()).unwrap().sys.ctxt, 4);
    }

    #[test]
    fn ticks_that_went_backwards_measure_from_themselves() {
        let before = parse(TEXT, &Reading::default()).unwrap();
        // Idle down by one: the idle sum went backwards, so this reading is
        // its own baseline -- but the counters keep the old reading's.
        let back = parse(
            b"cpu  200 2 30 399 5 6 7 8 9 10\nctxt 900\n",
            &Reading::default(),
        )
        .unwrap();
        let base = baseline(before, &back);
        assert_eq!(base.cpu, back.cpu);
        assert_eq!(base.sys, before.sys);
        // User down by one but nice up by two: no sum went backwards, so the
        // old reading stands, and the user delta is clamped by `tic_delta`.
        let mixed = parse(b"cpu  99 4 30 400 5 6 7 8 9 10\n", &Reading::default()).unwrap();
        assert_eq!(baseline(before, &mixed), before);
        assert_eq!(tic_delta(mixed.cpu.user, before.cpu.user), 0);
        // The sums, by the library's formulas.
        assert_eq!(before.cpu.sums(), [102, 43, 405, 172, 577]);
    }

    #[test]
    fn deltas_as_the_library_stores_them() {
        assert_eq!(tic_delta(10, 4), 6);
        // Backwards is none, not negative.
        assert_eq!(tic_delta(4, 10), 0);
        // A difference past `LONG_MAX` is negative in a `signed long`.
        assert_eq!(tic_delta(u64::MAX, 0), 0);
        assert_eq!(sys_delta(10, 4), 6);
        assert_eq!(sys_delta(4, 10), -6);
        assert_eq!(sys_delta(0x1_0000_0005, 0), 5);
    }

    #[test]
    fn a_nul_ends_the_text() {
        let text = b"cpu 1 2 3 4 5 6 7 8 9 10\nintr 7\n\0ctxt 4\n";
        let r = parse(text, &Reading::default()).unwrap();
        assert_eq!((r.sys.intr, r.sys.ctxt), (7, 0));
    }

    /// Four CPUs on three cores: 0 and 1 share core 0, 2 and 3 have one each.
    const CPUINFO: &[u8] = b"processor\t: 0\nvendor_id\t: X\ncore id\t\t: 0\n\n\
processor\t: 1\ncore id\t\t: 0\n\n\
processor\t: 2\ncore id\t\t: 4\n\n\
processor\t: 3\nmodel name\t: no core id here\n\n";

    #[test]
    fn a_core_with_two_threads_is_full_and_then_lone_ones_save_power() {
        let mut cores = Vec::new();
        cores_verify(CPUINFO, &mut cores);
        let kinds: Vec<(i32, i32)> = cores.iter().map(|c| (c.id, c.kind)).collect();
        // CPU 3 has no `core id`, so its blank line makes it core 3.
        assert_eq!(kinds, [(0, P_CORE), (4, E_CORE), (3, E_CORE)]);
        // Without any core of two threads, every core is "unsure".
        let mut alone = Vec::new();
        cores_verify(
            b"processor : 0\ncore id : 0\n\nprocessor : 1\ncore id : 1\n\n",
            &mut alone,
        );
        assert!(alone.iter().all(|c| c.kind == 0));
    }

    #[test]
    fn a_processor_the_file_ends_inside_is_not_added() {
        let mut cores = Vec::new();
        cores_verify(
            b"processor : 0\ncore id : 0\n\nprocessor : 1\nvendor : X\n",
            &mut cores,
        );
        assert_eq!(cores.len(), 1);
    }

    #[test]
    fn a_cpu_line_converts_what_it_can_and_needs_eight() {
        let mut slot = HistTic::default();
        assert!(scan_cpu_line(b"cpu3 1 2 3 4 5 6 7 8 9 10\n", &mut slot));
        assert_eq!((slot.id, slot.new.user, slot.new.gnice), (3, 1, 10));
        let mut short = HistTic::default();
        assert!(!scan_cpu_line(b"cpu7 1 2\n", &mut short));
        assert_eq!((short.id, short.new.user, short.new.nice), (7, 1, 2));
        assert!(!scan_cpu_line(b"intr 1 2 3 4 5 6 7 8 9\n", &mut short));
    }

    fn world(stat: &[u8]) -> scratchdir::ScratchDir {
        let dir = scratchdir::ScratchDir::new("procps-stat");
        std::fs::write(dir.path("stat"), stat).unwrap();
        std::fs::write(dir.path("cpuinfo"), CPUINFO).unwrap();
        dir
    }

    #[test]
    fn a_reap_gives_each_cpu_its_change_and_its_core() {
        let dir = world(
            b"cpu  40 0 40 400 0 0 0 0 0 0\n\
cpu0 10 0 10 100 0 0 0 0 0 0\ncpu1 10 0 10 100 0 0 0 0 0 0\n\
cpu2 10 0 10 100 0 0 0 0 0 0\ncpu3 10 0 10 100 0 0 0 0 0 0\nintr 5\n",
        );
        let mut info = StatInfo::new(dir.dir()).unwrap();
        std::fs::write(
            dir.path("stat"),
            b"cpu  50 0 44 440 0 0 0 0 0 0\n\
cpu0 15 0 11 110 0 0 0 0 0 0\ncpu1 10 0 10 100 0 0 0 0 0 0\n\
cpu2 10 0 13 115 0 0 0 0 0 0\ncpu3 15 0 10 115 0 0 0 0 0 0\nintr 9\n",
        )
        .unwrap();
        let r = info.reap(false).unwrap();
        assert_eq!(r.summary.id, STAT_SUMMARY_ID);
        assert_eq!(r.summary.numa_node, STAT_NODE_INVALID);
        assert_eq!(r.summary.type_core, 0);
        assert_eq!(
            (r.summary.delta[0], r.summary.delta[2], r.summary.delta[3]),
            (10, 4, 40)
        );
        assert_eq!(r.summary.sum_delta[4], 54);
        let ids: Vec<i32> = r.cpus.iter().map(|c| c.id).collect();
        assert_eq!(ids, [0, 1, 2, 3]);
        let types: Vec<i32> = r.cpus.iter().map(|c| c.type_core).collect();
        assert_eq!(types, [P_CORE, P_CORE, E_CORE, E_CORE]);
        assert_eq!(r.cpus[0].delta[0], 5);
        assert_eq!(r.cpus[1].sum_delta[4], 0);
        assert!(r.numa.is_empty());
    }

    #[test]
    fn a_cpu_whose_ticks_ran_back_measures_from_itself() {
        let dir = world(b"cpu  10 0 10 100 0 0 0 0 0 0\ncpu0 10 0 10 100 0 0 0 0 0 0\n");
        let mut info = StatInfo::new(dir.dir()).unwrap();
        std::fs::write(
            dir.path("stat"),
            b"cpu  10 0 10 100 0 0 0 0 0 0\ncpu0 50 0 10 90 0 0 0 0 0 0\n",
        )
        .unwrap();
        let r = info.reap(false).unwrap();
        assert_eq!(r.cpus[0].delta, [0; 10], "idle ran back: every delta is 0");
    }
}
