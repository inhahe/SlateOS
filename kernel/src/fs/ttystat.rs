//! TTY Statistics — terminal/serial device monitoring.
//!
//! Tracks per-TTY read/write bytes, line discipline events,
//! signal delivery, and buffer usage. Essential for terminal
//! performance and debugging serial I/O issues.
//!
//! ## Architecture
//!
//! ```text
//! TTY monitoring
//!   → ttystat::register(name) → register TTY device
//!   → ttystat::record_read(name, bytes) → read from TTY
//!   → ttystat::record_write(name, bytes) → write to TTY
//!   → ttystat::record_signal(name) → signal through TTY
//!   → ttystat::per_tty() → per-TTY stats
//!
//! Integration:
//!   → ioport (I/O ports — serial)
//!   → fdtable (file descriptors)
//!   → procstat (process stats)
//!   → signalq (signal queue)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// TTY type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtyType {
    Console,
    Serial,
    Pty,
    Virtual,
}

impl TtyType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Serial => "serial",
            Self::Pty => "pty",
            Self::Virtual => "vt",
        }
    }
}

/// Per-TTY stats.
#[derive(Debug, Clone)]
pub struct TtyStats {
    pub name: String,
    pub tty_type: TtyType,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub signals_sent: u64,
    pub overruns: u64,
    pub buf_size: u32,
    pub buf_used: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TTYS: usize = 128;

struct State {
    ttys: Vec<TtyStats>,
    total_read_bytes: u64,
    total_write_bytes: u64,
    total_signals: u64,
    total_overruns: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** per-TTY statistics table.
///
/// Seeds NO TTY devices and zero counters.  Real per-TTY accounting is wired
/// through [`register`] (one row per TTY the console/serial/pty layer creates)
/// and the `record_read`/`record_write`/`record_signal`/`record_overrun`/
/// `set_buf_used` functions; until those are called the table is genuinely empty,
/// so `/proc/ttystat` and the `ttystat` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded three fictional TTY rows ("tty0" console: 1MB
/// read / 50MB write / 500k read ops / 2M write ops / 10k signals; "ttyS0"
/// serial: 10MB read / 100MB write / 50 overruns; "pts/0" pty: 5MB read / 20MB
/// write / 5k signals) plus invented aggregate totals (total_read_bytes 16MB,
/// total_write_bytes 170MB, total_signals 15k, total_overruns 50), which
/// `/proc/ttystat` (and the `per_tty` view) then displayed as if they were real
/// measured terminal I/O.  That demo data was removed; the self-test now builds
/// its own fixtures explicitly via the real API (see [`self_test`]).  The TTY
/// layer (console, serial driver, pty subsystem) is expected to call [`register`]
/// when a device is created and the record functions on every read/write/signal/
/// overrun.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ttys: Vec::new(),
        total_read_bytes: 0,
        total_write_bytes: 0,
        total_signals: 0,
        total_overruns: 0,
        ops: 0,
    });
}

/// Register a TTY device.
pub fn register(name: &str, tty_type: TtyType, buf_size: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.ttys.len() >= MAX_TTYS { return Err(KernelError::ResourceExhausted); }
        if state.ttys.iter().any(|t| t.name == name) { return Err(KernelError::AlreadyExists); }
        state.ttys.push(TtyStats {
            name: String::from(name), tty_type, read_bytes: 0, write_bytes: 0,
            read_ops: 0, write_ops: 0, signals_sent: 0, overruns: 0,
            buf_size, buf_used: 0,
        });
        Ok(())
    })
}

/// Record a read.
pub fn record_read(name: &str, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.ttys.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        t.read_bytes += bytes;
        t.read_ops += 1;
        state.total_read_bytes += bytes;
        Ok(())
    })
}

/// Record a write.
pub fn record_write(name: &str, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.ttys.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        t.write_bytes += bytes;
        t.write_ops += 1;
        state.total_write_bytes += bytes;
        Ok(())
    })
}

/// Record a signal sent through TTY.
pub fn record_signal(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let t = state.ttys.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        t.signals_sent += 1;
        state.total_signals += 1;
        Ok(())
    })
}

/// Record a buffer overrun.
pub fn record_overrun(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let t = state.ttys.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        t.overruns += 1;
        state.total_overruns += 1;
        Ok(())
    })
}

/// Update buffer usage.
pub fn set_buf_used(name: &str, used: u32) -> KernelResult<()> {
    with_state(|state| {
        let t = state.ttys.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        t.buf_used = used;
        Ok(())
    })
}

/// Per-TTY stats.
pub fn per_tty() -> Vec<TtyStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.ttys.clone())
}

/// Statistics: (tty_count, total_read_bytes, total_write_bytes, total_signals, total_overruns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.ttys.len(), s.total_read_bytes, s.total_write_bytes, s.total_signals, s.total_overruns, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ttystat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/ttystat must never surface).  Resetting
    // first clears any residue from a prior `ttystat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated TTYs or counters.
    assert_eq!(per_tty().len(), 0);
    let (c0, rb0, wb0, s0, o0, _op0) = stats();
    assert_eq!((c0, rb0, wb0, s0, o0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters, type/buf preserved; dup name fails; record
    // before register fails (no phantom TTY is created).
    assert!(record_read("pts/0", 1).is_err());
    register("pts/0", TtyType::Pty, 4096).expect("register");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.tty_type, TtyType::Pty);
    assert_eq!(t.buf_size, 4096);
    assert_eq!((t.read_bytes, t.write_bytes, t.signals_sent, t.overruns), (0, 0, 0, 0));
    assert!(register("pts/0", TtyType::Pty, 4096).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read — bytes accumulate, ops increment.
    record_read("pts/0", 100).expect("read");
    record_read("pts/0", 50).expect("read2");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.read_bytes, 150);
    assert_eq!(t.read_ops, 2);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write — bytes accumulate, ops increment.
    record_write("pts/0", 200).expect("write");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.write_bytes, 200);
    assert_eq!(t.write_ops, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Signal.
    record_signal("pts/0").expect("signal");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.signals_sent, 1);
    crate::serial_println!("  [5/8] signal: OK");

    // 6: Overrun.
    record_overrun("pts/0").expect("overrun");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.overruns, 1);
    crate::serial_println!("  [6/8] overrun: OK");

    // 7: Buffer set; unknown TTY → NotFound on every record path.
    set_buf_used("pts/0", 2048).expect("buf");
    let t = per_tty().into_iter().find(|t| t.name == "pts/0").expect("find");
    assert_eq!(t.buf_used, 2048);
    assert!(record_write("missing", 1).is_err());
    assert!(record_signal("missing").is_err());
    assert!(record_overrun("missing").is_err());
    assert!(set_buf_used("missing", 1).is_err());
    crate::serial_println!("  [7/8] buffer + not found: OK");

    // 8: Aggregate stats are exact: 150 read / 200 write / 1 signal / 1 overrun.
    let (ttys, rb, wb, sigs, overruns, ops) = stats();
    assert_eq!(ttys, 1);
    assert_eq!(rb, 150);
    assert_eq!(wb, 200);
    assert_eq!(sigs, 1);
    assert_eq!(overruns, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/ttystat table.
    *STATE.lock() = None;

    crate::serial_println!("ttystat::self_test() — all 8 tests passed");
}
