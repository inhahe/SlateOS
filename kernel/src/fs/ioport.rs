//! I/O Port Statistics — x86 I/O port access monitoring.
//!
//! Tracks in/out instructions, port access patterns, and
//! per-device port region usage. Essential for understanding
//! legacy device I/O and security auditing.
//!
//! ## Architecture
//!
//! ```text
//! I/O port monitoring
//!   → ioport::record_in(port, size) → port read
//!   → ioport::record_out(port, size) → port write
//!   → ioport::register_region(name, base, len) → register range
//!   → ioport::per_region() → per-region stats
//!
//! Integration:
//!   → irqstat (interrupt stats)
//!   → dmastat (DMA stats)
//!   → acpistat (ACPI events)
//!   → secmod (security module)
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

/// I/O port region.
#[derive(Debug, Clone)]
pub struct PortRegion {
    pub name: String,
    pub base: u16,
    pub length: u16,
    pub reads: u64,
    pub writes: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_REGIONS: usize = 128;

struct State {
    regions: Vec<PortRegion>,
    total_reads: u64,
    total_writes: u64,
    untracked_reads: u64,
    untracked_writes: u64,
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

/// Initialise the I/O-port statistics state.
///
/// Starts with no registered regions and zero read/write totals (tracked and
/// untracked). A port region is added through [`register_region`] when the
/// kernel actually claims a legacy port range (PIC, PIT, keyboard controller,
/// RTC, a UART, …), and its access counters advance only through real
/// [`record_in`] / [`record_out`] calls on the `in`/`out` instruction path.
/// The `/proc/ioport` generator and the `ioport` kshell command surface the
/// region list (and [`per_region`] / [`stats`]) as if it reflects the real
/// port-I/O layout and access activity, so seeding it with phantom regions and
/// access counts would be fabricated procfs data — it would claim millions of
/// port accesses that never happened. (The well-known x86 port assignments
/// below are real hardware, but nothing in the kernel registers them or
/// instruments their accesses yet, so pre-seeding them with invented traffic
/// is exactly the iomem-style fabrication this sweep removes.)
///
/// (Previously this seeded five fictional regions — PIC (0x20, 1M reads / 500K
/// writes), PIT (0x40, 100K / 50K), KBD (0x60, 5M / 1M), RTC (0x70, 500K /
/// 200K) and COM1 (0x3F8, 10M / 8M) — plus global totals of 16,600,000 reads /
/// 9,750,000 writes and 50,000 / 20,000 untracked accesses.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        regions: Vec::new(),
        total_reads: 0,
        total_writes: 0,
        untracked_reads: 0,
        untracked_writes: 0,
        ops: 0,
    });
}

/// Register a port region.
pub fn register_region(name: &str, base: u16, length: u16) -> KernelResult<()> {
    with_state(|state| {
        if state.regions.len() >= MAX_REGIONS { return Err(KernelError::ResourceExhausted); }
        if state.regions.iter().any(|r| r.base == base) { return Err(KernelError::AlreadyExists); }
        state.regions.push(PortRegion {
            name: String::from(name), base, length,
            reads: 0, writes: 0, read_bytes: 0, write_bytes: 0,
        });
        Ok(())
    })
}

/// Record a port read.
pub fn record_in(port: u16, size: u32) -> KernelResult<()> {
    with_state(|state| {
        let found = state.regions.iter_mut().find(|r| port >= r.base && port < r.base + r.length);
        if let Some(r) = found {
            r.reads += 1;
            r.read_bytes += size as u64;
        } else {
            state.untracked_reads += 1;
        }
        state.total_reads += 1;
        Ok(())
    })
}

/// Record a port write.
pub fn record_out(port: u16, size: u32) -> KernelResult<()> {
    with_state(|state| {
        let found = state.regions.iter_mut().find(|r| port >= r.base && port < r.base + r.length);
        if let Some(r) = found {
            r.writes += 1;
            r.write_bytes += size as u64;
        } else {
            state.untracked_writes += 1;
        }
        state.total_writes += 1;
        Ok(())
    })
}

/// Per-region stats.
pub fn per_region() -> Vec<PortRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.regions.clone())
}

/// Statistics: (region_count, total_reads, total_writes, untracked_reads, untracked_writes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.regions.len(), s.total_reads, s.total_writes, s.untracked_reads, s.untracked_writes, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ioport::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live region table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom regions, zero totals.
    assert_eq!(per_region().len(), 0);
    let (c0, r0, w0, ur0, uw0, _) = stats();
    assert_eq!((c0, r0, w0, ur0, uw0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — region TEST (ports 0x100..0x104) appears zeroed; a second
    //    region at the same base is AlreadyExists.
    register_region("TEST", 0x100, 4).expect("register");
    assert_eq!(per_region().len(), 1);
    let r = per_region().into_iter().find(|r| r.base == 0x100).expect("find");
    assert_eq!((r.reads, r.writes, r.read_bytes, r.write_bytes), (0, 0, 0, 0));
    assert!(register_region("DUP", 0x100, 4).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: In (tracked) — per-region reads and the global total advance.
    record_in(0x100, 1).expect("in");
    let r = per_region().into_iter().find(|r| r.base == 0x100).expect("p3");
    assert_eq!((r.reads, r.read_bytes), (1, 1));
    assert_eq!(stats().1, 1); // total_reads
    crate::serial_println!("  [3/8] in tracked: OK");

    // 4: Out (tracked) — per-region writes and the global total advance.
    record_out(0x101, 2).expect("out");
    let r = per_region().into_iter().find(|r| r.base == 0x100).expect("p4");
    assert_eq!((r.writes, r.write_bytes), (1, 2));
    assert_eq!(stats().2, 1); // total_writes
    crate::serial_println!("  [4/8] out tracked: OK");

    // 5: Untracked — a read outside any region bumps untracked_reads and the
    //    global total, but no per-region counter.
    record_in(0xFFFF, 1).expect("untracked");
    assert_eq!(stats().3, 1); // untracked_reads
    assert_eq!(stats().1, 2); // total_reads (1 tracked + 1 untracked)
    crate::serial_println!("  [5/8] untracked: OK");

    // 6: Multiple accesses — 100 reads on the TEST region accrue exactly the
    //    reads they receive (no fabricated baseline).
    for _ in 0..100 { record_in(0x100, 1).expect("loop"); }
    let r = per_region().into_iter().find(|r| r.base == 0x100).expect("p6");
    assert_eq!((r.reads, r.read_bytes), (101, 101)); // 1 + 100
    crate::serial_println!("  [6/8] multi access: OK");

    // 7: Region boundaries — 0x103 is the last port in TEST (in range, counted);
    //    0x104 is just past the end (untracked).
    record_in(0x103, 1).expect("boundary_in");
    record_in(0x104, 1).expect("boundary_out");
    let r = per_region().into_iter().find(|r| r.base == 0x100).expect("p7");
    assert_eq!(r.reads, 102); // 0x103 counted, 0x104 not
    assert_eq!(stats().3, 2); // untracked_reads now 2 (0xFFFF + 0x104)
    crate::serial_println!("  [7/8] boundaries: OK");

    // 8: Final stats reflect only the real activity above: 1 region, 104 reads
    //    (102 tracked + 2 untracked), 1 write, 2 untracked reads, 0 untracked
    //    writes.
    let (regions, reads, writes, ur, uw, ops) = stats();
    assert_eq!((regions, reads, writes, ur, uw), (1, 104, 1, 2, 0));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("ioport::self_test() — all 8 tests passed");
}
