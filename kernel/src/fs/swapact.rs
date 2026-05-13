//! Swap Activity Statistics — swap in/out monitoring.
//!
//! Tracks swap-in and swap-out page counts, latencies, per-device
//! swap usage, and swap pressure. Essential for memory pressure
//! analysis and swap performance tuning.
//!
//! ## Architecture
//!
//! ```text
//! Swap activity monitoring
//!   → swapact::register(name, pages) → register swap area
//!   → swapact::record_in(name, pages, ns) → swap in
//!   → swapact::record_out(name, pages, ns) → swap out
//!   → swapact::per_area() → per-area stats
//!
//! Integration:
//!   → mempress (memory pressure)
//!   → pagestat (page allocator)
//!   → zramstat (ZRAM swap)
//!   → oomkiller (OOM killer)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Swap area type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapType {
    Partition,
    File,
    Zram,
}

impl SwapType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Partition => "partition",
            Self::File => "file",
            Self::Zram => "zram",
        }
    }
}

/// Per-area swap stats.
#[derive(Debug, Clone)]
pub struct SwapAreaStats {
    pub name: String,
    pub swap_type: SwapType,
    pub total_pages: u64,
    pub used_pages: u64,
    pub swap_in_count: u64,
    pub swap_in_pages: u64,
    pub swap_in_ns: u64,
    pub swap_out_count: u64,
    pub swap_out_pages: u64,
    pub swap_out_ns: u64,
    pub priority: i32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_AREAS: usize = 32;

struct State {
    areas: Vec<SwapAreaStats>,
    total_swap_in: u64,
    total_swap_out: u64,
    total_swap_in_pages: u64,
    total_swap_out_pages: u64,
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
        areas: alloc::vec![
            SwapAreaStats { name: String::from("/dev/sda2"), swap_type: SwapType::Partition, total_pages: 2_000_000, used_pages: 500_000, swap_in_count: 1_000_000, swap_in_pages: 1_500_000, swap_in_ns: 150_000_000_000, swap_out_count: 800_000, swap_out_pages: 1_200_000, swap_out_ns: 240_000_000_000, priority: -1 },
            SwapAreaStats { name: String::from("/dev/zram0"), swap_type: SwapType::Zram, total_pages: 1_000_000, used_pages: 300_000, swap_in_count: 500_000, swap_in_pages: 600_000, swap_in_ns: 10_000_000_000, swap_out_count: 400_000, swap_out_pages: 500_000, swap_out_ns: 15_000_000_000, priority: 100 },
        ],
        total_swap_in: 1_500_000,
        total_swap_out: 1_200_000,
        total_swap_in_pages: 2_100_000,
        total_swap_out_pages: 1_700_000,
        ops: 0,
    });
}

/// Register a swap area.
pub fn register(name: &str, swap_type: SwapType, total_pages: u64, priority: i32) -> KernelResult<()> {
    with_state(|state| {
        if state.areas.len() >= MAX_AREAS { return Err(KernelError::ResourceExhausted); }
        if state.areas.iter().any(|a| a.name == name) { return Err(KernelError::AlreadyExists); }
        state.areas.push(SwapAreaStats {
            name: String::from(name), swap_type, total_pages, used_pages: 0,
            swap_in_count: 0, swap_in_pages: 0, swap_in_ns: 0,
            swap_out_count: 0, swap_out_pages: 0, swap_out_ns: 0, priority,
        });
        Ok(())
    })
}

/// Record swap-in.
pub fn record_in(name: &str, pages: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let a = state.areas.iter_mut().find(|a| a.name == name)
            .ok_or(KernelError::NotFound)?;
        a.swap_in_count += 1;
        a.swap_in_pages += pages;
        a.swap_in_ns += ns;
        a.used_pages = a.used_pages.saturating_sub(pages);
        state.total_swap_in += 1;
        state.total_swap_in_pages += pages;
        Ok(())
    })
}

/// Record swap-out.
pub fn record_out(name: &str, pages: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let a = state.areas.iter_mut().find(|a| a.name == name)
            .ok_or(KernelError::NotFound)?;
        a.swap_out_count += 1;
        a.swap_out_pages += pages;
        a.swap_out_ns += ns;
        a.used_pages += pages;
        if a.used_pages > a.total_pages { a.used_pages = a.total_pages; }
        state.total_swap_out += 1;
        state.total_swap_out_pages += pages;
        Ok(())
    })
}

/// Per-area stats.
pub fn per_area() -> Vec<SwapAreaStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.areas.clone())
}

/// Statistics: (area_count, total_swap_in, total_swap_out, total_in_pages, total_out_pages, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.areas.len(), s.total_swap_in, s.total_swap_out, s.total_swap_in_pages, s.total_swap_out_pages, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("swapact::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_area().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("/swapfile", SwapType::File, 500_000, -2).expect("register");
    assert_eq!(per_area().len(), 3);
    assert!(register("/swapfile", SwapType::File, 500_000, -2).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Swap out.
    record_out("/swapfile", 100, 5000).expect("out");
    let a = per_area().iter().find(|a| a.name == "/swapfile").cloned().unwrap();
    assert_eq!(a.swap_out_count, 1);
    assert_eq!(a.swap_out_pages, 100);
    assert_eq!(a.used_pages, 100);
    crate::serial_println!("  [3/8] swap out: OK");

    // 4: Swap in.
    record_in("/swapfile", 50, 3000).expect("in");
    let a = per_area().iter().find(|a| a.name == "/swapfile").cloned().unwrap();
    assert_eq!(a.swap_in_count, 1);
    assert_eq!(a.swap_in_pages, 50);
    assert_eq!(a.used_pages, 50); // 100 - 50
    crate::serial_println!("  [4/8] swap in: OK");

    // 5: Latency.
    let a = per_area().iter().find(|a| a.name == "/swapfile").cloned().unwrap();
    assert_eq!(a.swap_out_ns, 5000);
    assert_eq!(a.swap_in_ns, 3000);
    crate::serial_println!("  [5/8] latency: OK");

    // 6: Saturation (used can't exceed total).
    for _ in 0..10 { record_out("/swapfile", 100_000, 100).expect("out_many"); }
    let a = per_area().iter().find(|a| a.name == "/swapfile").cloned().unwrap();
    assert!(a.used_pages <= a.total_pages);
    crate::serial_println!("  [6/8] saturation: OK");

    // 7: Not found.
    assert!(record_in("nonexist", 1, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (areas, si, so, sip, sop, ops) = stats();
    assert!(areas >= 3);
    assert!(si > 1_500_000);
    assert!(so > 1_200_000);
    assert!(sip > 2_100_000);
    assert!(sop > 1_700_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("swapact::self_test() — all 8 tests passed");
}
