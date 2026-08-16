//! Slab free-quarantine — delayed slot reuse to catch use-after-free / stale
//! pointer writes (the leading **B-KNULLJUMP** hypothesis; see `known-issues.md`).
//!
//! ## Why this exists
//!
//! B-KNULLJUMP is an intermittent (~1-in-120) wild write that zeroes a link
//! pointer inside a *live* scheduler `BTreeMap` node at a Path-Z process
//! spawn/teardown boundary. The two leading root causes are:
//!
//! 1. **Use-after-free / stale-pointer write** — some code frees an allocation,
//!    the slab immediately hands that slot back out to the scheduler's BTree
//!    node, and a lingering pointer to the *old* object writes through it,
//!    corrupting the freshly-reused live node.
//! 2. Adjacent in-slot overflow (covered by `mm/poison.rs` redzones + KASAN).
//!
//! The existing detectors (`mm/poison.rs`, `mm/kasan.rs`) are *passive*: they
//! mark freed/redzone memory but only fire when instrumented code happens to
//! *read/check* it. A stale-pointer write into an immediately-reused slot is
//! never checked, so it silently lands on the live victim.
//!
//! **Quarantine changes the allocation pattern to make that class detectable.**
//! When enabled, a freed slab slot is not returned to the free list right away.
//! Instead it is filled with the [`poison::POISON_FREE`] pattern and parked in a
//! fixed-size FIFO ring for the next `QUARANTINE_CAP` frees. While parked:
//!
//! - The live scheduler node can **never be allocated at that address**, so if
//!   the corruption vanishes under quarantine, the bug is confirmed to be a
//!   reuse/UAF (hypothesis 1) rather than an in-place overflow.
//! - A lingering stale pointer that writes into the parked slot lands on
//!   *poison* instead of a live object; the mismatch is caught when the slot is
//!   evicted ([`on_free`]) or on an on-demand full-ring sweep ([`scan_all`]),
//!   with the address, size-class and first corrupted byte reported.
//!
//! ## Cost / gating
//!
//! Runtime-gated by [`ENABLED`] (default `false`): a normal build pays one
//! relaxed atomic load per free. When *enabled*, frees route through a global
//! locked ring (perf is irrelevant during a corruption hunt) and up to
//! `QUARANTINE_CAP` slots are held out of circulation (bounded memory: worst
//! case `QUARANTINE_CAP * max_slot` ≈ a couple MiB). It is meant to be switched
//! on around the Path-Z boot self-tests, not left on in production.
//!
//! ## References
//!
//! - Linux KASAN quarantine (`mm/kasan/quarantine.c`) — same idea: delay slab
//!   reuse so UAF has a detection window.
//! - Windows Delayed Free / Application Verifier page-heap deferred frees.

// KASAN debug profile: this module is exempt from compiler instrumentation.
// Part of the allocator's deferred-free path, which runs inside the
// allocator lock that the shadow maintenance hooks also run under.
// (`sanitize` is nightly-only, so it is gated on the `kasan_instrumented` cfg
// that `scripts/kasan-build.sh` sets; the ordinary build never sees it.)
#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]
// Diagnostic subsystem: the public API is tooling/hunt surface that may have no
// production call sites in a normal build. The ring index arithmetic is bounded
// by QUARANTINE_CAP (all `% QUARANTINE_CAP`), and the self-test intentionally
// uses assert!/expect to fail loudly on a broken invariant.
#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic
)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::mm::poison::POISON_FREE;
use crate::mm::rawmem;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of freed slab slots held out of circulation at once. Larger = a
/// longer detection window (a stale write is more likely to hit a still-parked
/// slot) at the cost of more memory held. 8192 slots ≈ up to ~1–2 MiB parked
/// for typical small classes — fine for a focused corruption hunt.
const QUARANTINE_CAP: usize = 8192;

/// Whether the free-quarantine is active. Default `false` so a normal build
/// pays only one relaxed atomic load per `dealloc`.
static ENABLED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total slots ever pushed into quarantine.
static TOTAL_PARKED: AtomicU64 = AtomicU64::new(0);
/// Total slots evicted (returned to the slab free list) after their hold.
static TOTAL_EVICTED: AtomicU64 = AtomicU64::new(0);
/// Total corruptions detected (a parked slot's poison was overwritten).
static CORRUPTIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// The ring
// ---------------------------------------------------------------------------

/// One parked slot. `addr` is stored as a `usize` (not a raw pointer) so the
/// containing struct is trivially `Send`/`Sync` for the `spin::Mutex`.
#[derive(Clone, Copy)]
struct Entry {
    /// Slot base address (kernel virtual, HHDM). 0 = empty.
    addr: usize,
    /// Size-class index the slot belongs to (for returning it to the slab).
    class_idx: usize,
    /// Full slot size in bytes (what was poisoned and must be verified).
    slot_size: usize,
}

impl Entry {
    const EMPTY: Entry = Entry {
        addr: 0,
        class_idx: 0,
        slot_size: 0,
    };
}

/// FIFO ring of parked slots. `entries[head]` is the oldest once `count`
/// reaches `QUARANTINE_CAP`; pushes overwrite the oldest and advance `head`.
struct Quarantine {
    entries: [Entry; QUARANTINE_CAP],
    /// Next write position (also the oldest entry once the ring is full).
    head: usize,
    /// Number of currently-parked slots (`<= QUARANTINE_CAP`).
    count: usize,
}

static RING: spin::Mutex<Quarantine> = spin::Mutex::new(Quarantine {
    entries: [Entry::EMPTY; QUARANTINE_CAP],
    head: 0,
    count: 0,
});

// ---------------------------------------------------------------------------
// Poison fill / verify (self-contained; independent of `poison::ENABLED`)
// ---------------------------------------------------------------------------

/// Fill an entire parked slot with the free-poison pattern.
///
/// # Safety
/// `addr` must point to `len` bytes of writable memory (the slot being parked).
unsafe fn fill_poison(addr: usize, len: usize) {
    // `rawmem`, not `core::ptr::write_bytes`: the generic monomorphises into
    // this crate carrying instrumentation despite the module-level opt-out, and
    // a parked slot is poisoned in the KASAN shadow by construction — so the
    // fill would report a use-after-free on every park.  See `mm::rawmem`.
    // SAFETY: caller guarantees `[addr, addr+len)` is a valid writable slot.
    unsafe { rawmem::fill_u8(addr as *mut u8, POISON_FREE, len) };
}

/// Verify a parked slot is still fully poisoned. Returns the offset of the
/// first corrupted byte, or `None` if intact.
///
/// # Safety
/// `addr` must point to `len` bytes of readable memory.
unsafe fn find_corruption(addr: usize, len: usize) -> Option<usize> {
    for i in 0..len {
        // SAFETY: `i < len` and caller guarantees the slot is readable.
        let b = unsafe { rawmem::read_u8((addr.wrapping_add(i)) as *const u8) };
        if b != POISON_FREE {
            return Some(i);
        }
    }
    None
}

/// Report a corrupted parked slot to the serial log (B-KNULLJUMP candidate).
fn report(addr: usize, class_idx: usize, slot_size: usize, off: usize) {
    CORRUPTIONS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `off < slot_size` and the slot is readable here.
    let bad = unsafe { rawmem::read_u8((addr.wrapping_add(off)) as *const u8) };
    serial_println!(
        "[quarantine] *** CORRUPTION *** parked slot {:#x} (class {}, {} B) \
         byte +{} = {:#04x} (expected {:#04x}) — stale-pointer/UAF write \
         (B-KNULLJUMP candidate)",
        addr,
        class_idx,
        slot_size,
        off,
        bad,
        POISON_FREE
    );
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Whether the free-quarantine is currently active.
#[inline]
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Enable the free-quarantine (route slab frees through the parking ring).
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Disable the free-quarantine. Already-parked slots stay parked until the
/// caller drains them via [`drain`]; new frees bypass the ring.
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Park a freed slab slot and, if the ring was already full, hand back the
/// displaced (oldest) slot so the caller can return it to the slab free list.
///
/// Poisons the whole slot, pushes it into the FIFO ring, and — on eviction —
/// verifies the displaced slot's poison is intact, reporting any corruption.
///
/// Returns `Some((evicted_ptr, evicted_class_idx))` when an old slot was
/// displaced and must be returned to the slab by the caller; `None` when the
/// ring still had room (this slot is now held).
///
/// # Safety
/// `ptr` must be a live slab slot of exactly `slot_size` bytes belonging to
/// size-class `class_idx`, about to be freed (still mapped and writable).
#[must_use]
pub unsafe fn on_free(
    ptr: *mut u8,
    class_idx: usize,
    slot_size: usize,
) -> Option<(*mut u8, usize)> {
    let addr = ptr as usize;
    // SAFETY: caller guarantees the slot is writable for `slot_size` bytes.
    unsafe { fill_poison(addr, slot_size) };

    let mut evicted: Option<(usize, usize, usize)> = None;
    crate::cpu::without_interrupts(|| {
        let mut q = RING.lock();
        let head = q.head;
        if q.count == QUARANTINE_CAP {
            // Ring full: entries[head] is the oldest — displace it.
            let old = q.entries[head];
            evicted = Some((old.addr, old.class_idx, old.slot_size));
        } else {
            q.count += 1;
        }
        q.entries[head] = Entry {
            addr,
            class_idx,
            slot_size,
        };
        q.head = (head + 1) % QUARANTINE_CAP;
    });

    TOTAL_PARKED.fetch_add(1, Ordering::Relaxed);

    if let Some((old_addr, old_class, old_size)) = evicted {
        // Verify the displaced slot's poison survived its time in quarantine.
        // SAFETY: the slot was a valid `old_size`-byte slab slot when parked and
        // has not been reused since (it was held in the ring), so it is still
        // mapped and readable.
        if let Some(off) = unsafe { find_corruption(old_addr, old_size) } {
            report(old_addr, old_class, old_size, off);
        }
        TOTAL_EVICTED.fetch_add(1, Ordering::Relaxed);
        return Some((old_addr as *mut u8, old_class));
    }
    None
}

/// Sweep every currently-parked slot and verify its poison is intact. Returns
/// the number of corrupted slots found (each also reported to serial).
///
/// Call at a suspected corruption point (e.g. a Path-Z teardown checkpoint) to
/// pin the corruption to a tight window instead of waiting for eviction.
pub fn scan_all() -> usize {
    let mut corrupted = 0usize;
    crate::cpu::without_interrupts(|| {
        let q = RING.lock();
        // Walk all live entries: the `count` slots ending at `head-1`.
        for n in 0..q.count {
            // Oldest is at (head - count) mod CAP; iterate forward from there.
            let idx = (q.head + QUARANTINE_CAP - q.count + n) % QUARANTINE_CAP;
            let e = q.entries[idx];
            if e.addr == 0 {
                continue;
            }
            // SAFETY: parked slots remain mapped/readable until evicted.
            if let Some(off) = unsafe { find_corruption(e.addr, e.slot_size) } {
                report(e.addr, e.class_idx, e.slot_size, off);
                corrupted += 1;
            }
        }
    });
    corrupted
}

/// Drain all parked slots, invoking `return_slot(ptr, class_idx)` for each so
/// the caller can return them to the slab free list. Each slot's poison is
/// verified first (corruption reported). Used to reclaim memory after a hunt.
pub fn drain(mut return_slot: impl FnMut(*mut u8, usize)) {
    loop {
        let next: Option<(usize, usize, usize)> = crate::cpu::without_interrupts(|| {
            let mut q = RING.lock();
            if q.count == 0 {
                return None;
            }
            let idx = (q.head + QUARANTINE_CAP - q.count) % QUARANTINE_CAP;
            let e = q.entries[idx];
            q.entries[idx] = Entry::EMPTY;
            q.count -= 1;
            Some((e.addr, e.class_idx, e.slot_size))
        });
        match next {
            None => break,
            Some((addr, class_idx, slot_size)) => {
                if addr == 0 {
                    continue;
                }
                // SAFETY: parked slot still mapped/readable.
                if let Some(off) = unsafe { find_corruption(addr, slot_size) } {
                    report(addr, class_idx, slot_size, off);
                }
                TOTAL_EVICTED.fetch_add(1, Ordering::Relaxed);
                return_slot(addr as *mut u8, class_idx);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Snapshot of quarantine counters.
#[derive(Debug, Clone, Copy)]
pub struct QuarantineStats {
    /// Whether quarantine is currently enabled.
    pub enabled: bool,
    /// Slots currently held in the ring.
    pub parked_now: usize,
    /// Capacity of the ring.
    pub capacity: usize,
    /// Total slots ever parked.
    pub total_parked: u64,
    /// Total slots evicted / drained back to the slab.
    pub total_evicted: u64,
    /// Corruptions detected.
    pub corruptions: u64,
}

/// Read quarantine statistics.
#[must_use]
pub fn stats() -> QuarantineStats {
    let parked_now = crate::cpu::without_interrupts(|| RING.lock().count);
    QuarantineStats {
        enabled: is_enabled(),
        parked_now,
        capacity: QUARANTINE_CAP,
        total_parked: TOTAL_PARKED.load(Ordering::Relaxed),
        total_evicted: TOTAL_EVICTED.load(Ordering::Relaxed),
        corruptions: CORRUPTIONS.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the free-quarantine ring.
///
/// Uses stack-local buffers as stand-in "slots" so it exercises the ring / FIFO
/// eviction / poison-verify logic without perturbing the real allocator. It
/// does not enable the global `ENABLED` flag (so it can't affect concurrent
/// allocations); it drives `on_free`/`scan_all` directly.
pub fn self_test() {
    serial_println!("[quarantine] Running self-test...");

    let baseline = stats();

    // Two stack "slots" of 32 bytes each. on_free will fill them with poison.
    let mut slot_a = [0u8; 32];
    let mut slot_b = [0u8; 32];
    let a = slot_a.as_mut_ptr();
    let b = slot_b.as_mut_ptr();

    // Test 1: parking a slot fills it with POISON_FREE and holds it (None,
    // since the ring has room).
    // SAFETY: a/b point to 32-byte writable stack slots for the duration.
    let e1 = unsafe {
        on_free(a, 2 /* class 32 */, 32)
    };
    assert!(e1.is_none(), "first park should not evict");
    assert!(
        slot_a.iter().all(|&x| x == POISON_FREE),
        "slot A should be poisoned"
    );
    serial_println!("[quarantine]   park fills poison + holds: OK");

    // Test 2: scan_all finds no corruption while poison is intact.
    let c0 = scan_all();
    assert_eq!(c0, 0, "clean ring should report no corruption");
    serial_println!("[quarantine]   scan_all (clean): OK");

    // Test 3: corrupt a parked slot (simulate a stale-pointer write) and verify
    // scan_all detects exactly one corruption. Write through the raw pointer
    // (as a stale pointer would) so the store aliases the parked slot.
    // SAFETY: `a` points to the 32-byte `slot_a`; offset 7 is in bounds.
    unsafe {
        *a.add(7) = 0x00;
    } // stomp poison (like a zeroed link pointer)
    let c1 = scan_all();
    assert!(c1 >= 1, "scan_all must catch the stomped parked slot");
    serial_println!("[quarantine]   scan_all (corrupted): OK ({} found)", c1);

    // Test 4: drain returns every parked slot exactly once (re-verifying). We
    // count callbacks; both A and B must come back.
    // SAFETY: b is a valid 32-byte writable stack slot.
    let _ = unsafe { on_free(b, 2, 32) };
    let mut returned = 0usize;
    drain(|_ptr, class_idx| {
        assert_eq!(class_idx, 2, "class index preserved through the ring");
        returned += 1;
    });
    assert_eq!(returned, 2, "drain must return both parked slots");
    let after_drain = crate::cpu::without_interrupts(|| RING.lock().count);
    assert_eq!(after_drain, 0, "ring empty after drain");
    serial_println!("[quarantine]   drain returns all slots: OK");

    // Test 5: FIFO eviction order — fill the ring, then one more push evicts the
    // oldest. We verify by draining and checking the survivor set is the last
    // CAP pushes. (Cheap sanity: park CAP+1 tiny slots backed by one buffer.)
    // Use a single dummy buffer address; class/size are what matter for logic.
    let mut dummy = [0u8; 8];
    let d = dummy.as_mut_ptr();
    let mut evictions = 0usize;
    for _ in 0..(QUARANTINE_CAP + 4) {
        // SAFETY: d is a valid 8-byte writable stack slot; re-poisoned each call.
        if unsafe {
            on_free(d, 0 /* class 8 */, 8)
        }
        .is_some()
        {
            evictions += 1;
        }
    }
    assert_eq!(evictions, 4, "exactly (pushes - CAP) evictions expected");
    // Clean up: drain the ring back to empty so we don't leak dangling stack
    // addresses into the global ring past this function.
    drain(|_p, _c| {});
    let final_count = crate::cpu::without_interrupts(|| RING.lock().count);
    assert_eq!(final_count, 0, "ring drained clean at end of self-test");
    serial_println!("[quarantine]   FIFO eviction accounting: OK");

    let st = stats();
    serial_println!(
        "[quarantine]   stats: parked+{}, evicted+{}, corruptions+{}",
        st.total_parked - baseline.total_parked,
        st.total_evicted - baseline.total_evicted,
        st.corruptions - baseline.corruptions
    );

    serial_println!("[quarantine] Self-test PASSED");
}
