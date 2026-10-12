//! `/proc/stat`, transcribed from procps-ng 4.0.4's `library/stat.c`: the
//! summary `cpu` line's ticks and the system counters, with the one reading of
//! history the library keeps so that a `DELTA` item is the difference of two
//! reads. That is all of the file `vmstat` asks for; the per-CPU, per-core and
//! NUMA tables `top` uses are not here.
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
}
