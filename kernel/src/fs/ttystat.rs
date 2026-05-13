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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        ttys: alloc::vec![
            TtyStats { name: String::from("tty0"), tty_type: TtyType::Console, read_bytes: 1_000_000, write_bytes: 50_000_000, read_ops: 500_000, write_ops: 2_000_000, signals_sent: 10_000, overruns: 0, buf_size: 4096, buf_used: 256 },
            TtyStats { name: String::from("ttyS0"), tty_type: TtyType::Serial, read_bytes: 10_000_000, write_bytes: 100_000_000, read_ops: 5_000_000, write_ops: 10_000_000, signals_sent: 0, overruns: 50, buf_size: 4096, buf_used: 1024 },
            TtyStats { name: String::from("pts/0"), tty_type: TtyType::Pty, read_bytes: 5_000_000, write_bytes: 20_000_000, read_ops: 1_000_000, write_ops: 3_000_000, signals_sent: 5_000, overruns: 0, buf_size: 4096, buf_used: 512 },
        ],
        total_read_bytes: 16_000_000,
        total_write_bytes: 170_000_000,
        total_signals: 15_000,
        total_overruns: 50,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_tty().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("pts/99", TtyType::Pty, 4096).expect("register");
    assert_eq!(per_tty().len(), 4);
    assert!(register("pts/99", TtyType::Pty, 4096).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read.
    record_read("pts/99", 100).expect("read");
    let t = per_tty().iter().find(|t| t.name == "pts/99").cloned().unwrap();
    assert_eq!(t.read_bytes, 100);
    assert_eq!(t.read_ops, 1);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write.
    record_write("pts/99", 200).expect("write");
    let t = per_tty().iter().find(|t| t.name == "pts/99").cloned().unwrap();
    assert_eq!(t.write_bytes, 200);
    assert_eq!(t.write_ops, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Signal.
    record_signal("pts/99").expect("signal");
    let t = per_tty().iter().find(|t| t.name == "pts/99").cloned().unwrap();
    assert_eq!(t.signals_sent, 1);
    crate::serial_println!("  [5/8] signal: OK");

    // 6: Overrun.
    record_overrun("pts/99").expect("overrun");
    let t = per_tty().iter().find(|t| t.name == "pts/99").cloned().unwrap();
    assert_eq!(t.overruns, 1);
    crate::serial_println!("  [6/8] overrun: OK");

    // 7: Buffer.
    set_buf_used("pts/99", 2048).expect("buf");
    let t = per_tty().iter().find(|t| t.name == "pts/99").cloned().unwrap();
    assert_eq!(t.buf_used, 2048);
    crate::serial_println!("  [7/8] buffer: OK");

    // 8: Stats.
    let (ttys, rb, wb, sigs, overruns, ops) = stats();
    assert!(ttys >= 4);
    assert!(rb > 16_000_000);
    assert!(wb > 170_000_000);
    assert!(sigs > 15_000);
    assert!(overruns > 50);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ttystat::self_test() — all 8 tests passed");
}
