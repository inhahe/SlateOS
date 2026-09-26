//! TLB shootdown for SMP systems.
//!
//! When one CPU modifies a page table entry (unmap, permission change),
//! other CPUs may still have the old entry cached in their TLBs.  This
//! module provides a TLB shootdown mechanism that uses a fixed-mode IPI
//! to ask all other CPUs to invalidate their TLB entries for a given
//! address range.
//!
//! ## Protocol
//!
//! 1. Initiator takes the shootdown lock (with preemption disabled), stores
//!    the flush request (address + page count) in shared statics, and gives
//!    it the next sequence number.
//! 2. Initiator sends an IPI to all other CPUs (vector 251).
//! 3. Initiator spins waiting for all other CPUs to acknowledge.
//! 4. Each receiving CPU executes `invlpg` for the range, then bumps
//!    the acknowledgement counter — once per sequence number, however many
//!    times it is asked.
//! 5. Initiator continues once all CPUs have acknowledged.
//!
//! A CPU that is itself waiting for the lock services the pending request on
//! every spin, so it acknowledges even with interrupts disabled — the case
//! that used to deadlock two CPUs shooting down at once. See `broadcast`.
//!
//! For a full address space flush (e.g., process exit, CR3 change),
//! we simply CR3-reload on all CPUs.
//!
//! ## Single-CPU Fast Path
//!
//! If only one CPU is online, the IPI is skipped — we just flush locally.
//!
//! ## References
//!
//! - Linux `arch/x86/mm/tlb.c` — TLB flush IPI mechanism
//! - Intel SDM Vol. 3A §4.10.4 "Invalidation of TLBs and Paging-Structure Caches"

use crate::serial_println;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// TLB shootdown statistics
// ---------------------------------------------------------------------------

/// Total number of flush_range() calls (partial TLB flushes).
static RANGE_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total number of flush_all() calls (full TLB flushes).
static FULL_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total number of 4 KiB pages invalidated across all flush_range() calls.
static TOTAL_PAGES_FLUSHED: AtomicU64 = AtomicU64::new(0);

/// Total number of IPI-based remote flushes (only counted when >1 CPU online).
static IPI_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total number of local-only flushes (skipped IPI because single CPU).
static LOCAL_ONLY_COUNT: AtomicU64 = AtomicU64::new(0);

/// TLB shootdown statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct TlbStats {
    /// Number of partial range flushes.
    pub range_flushes: u64,
    /// Number of full TLB flushes (CR3 reload all CPUs).
    pub full_flushes: u64,
    /// Total 4 KiB pages invalidated via range flushes.
    pub total_pages_flushed: u64,
    /// Number of flushes that required IPI (SMP active).
    pub ipi_flushes: u64,
    /// Number of flushes that were local-only (single CPU / pre-SMP).
    pub local_only: u64,
}

/// Get current TLB statistics.
#[must_use]
pub fn stats() -> TlbStats {
    TlbStats {
        range_flushes: RANGE_FLUSH_COUNT.load(Ordering::Relaxed),
        full_flushes: FULL_FLUSH_COUNT.load(Ordering::Relaxed),
        total_pages_flushed: TOTAL_PAGES_FLUSHED.load(Ordering::Relaxed),
        ipi_flushes: IPI_FLUSH_COUNT.load(Ordering::Relaxed),
        local_only: LOCAL_ONLY_COUNT.load(Ordering::Relaxed),
    }
}

/// IPI vector for TLB shootdown requests.
///
/// We use vector 251, which is in the high range (above device IRQs
/// at 33–56) and below the APIC spurious vector (255).
pub const TLB_SHOOTDOWN_VECTOR: u8 = 251;

/// Sentinel value meaning "flush entire TLB (CR3 reload)".
const FLUSH_ALL: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Shared shootdown request (protected by the initiator holding the lock)
// ---------------------------------------------------------------------------

/// Start address of the TLB range to flush.
///
/// `FLUSH_ALL` means reload CR3 (full flush).
static FLUSH_ADDR: AtomicU64 = AtomicU64::new(0);

/// Number of 4 KiB hardware pages to flush (1 page per invlpg).
/// Ignored if `FLUSH_ADDR == FLUSH_ALL`.
static FLUSH_PAGES: AtomicU32 = AtomicU32::new(0);

/// Number of CPUs that have acknowledged the current shootdown.
static ACK_COUNT: AtomicU32 = AtomicU32::new(0);

/// Serializes concurrent shootdown requests.
///
/// Only one CPU can initiate a shootdown at a time.  Other CPUs that
/// need to shootdown will spin on this lock.  This is acceptable because
/// shootdowns are infrequent and the critical section is very short.
static SHOOTDOWN_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// Sequence number of the most recent shootdown request; 0 before the first.
///
/// Bumped by the initiator, under [`SHOOTDOWN_LOCK`], after it has published
/// [`FLUSH_ADDR`]/[`FLUSH_PAGES`] and reset [`ACK_COUNT`], so a CPU that sees a
/// new number also sees the request it names.
static SHOOTDOWN_SEQ: AtomicU64 = AtomicU64::new(0);

/// Per CPU: the last request sequence number this CPU has serviced (or, for
/// the initiator, issued — it flushes its own TLB directly).
///
/// What makes servicing idempotent, so a request can be answered from two
/// places — the IPI handler, and a CPU that is spinning for
/// [`SHOOTDOWN_LOCK`] and may have interrupts disabled — without being
/// acknowledged twice. See [`service_pending`].
static HANDLED_SEQ: [AtomicU64; crate::smp::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::smp::MAX_CPUS];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Flush TLB entries for a range of 4 KiB pages on all CPUs.
///
/// This performs `invlpg` for `page_count` pages starting at `vaddr`
/// on the current CPU, and sends a TLB shootdown IPI to all other CPUs.
///
/// Blocks until all online CPUs have flushed.
///
/// For single-CPU systems (or before SMP init), this just flushes locally.
pub fn flush_range(vaddr: u64, page_count: u32) {
    let _prof_t = crate::kprofile::begin(crate::kprofile::Slot::TlbShootdown);

    RANGE_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
    TOTAL_PAGES_FLUSHED.fetch_add(u64::from(page_count), Ordering::Relaxed);

    // Local flush first.
    local_flush_range(vaddr, page_count);

    // If only one CPU is online, no IPI needed.
    if crate::smp::cpu_count() <= 1 {
        LOCAL_ONLY_COUNT.fetch_add(1, Ordering::Relaxed);
        crate::kprofile::end(crate::kprofile::Slot::TlbShootdown, _prof_t);
        return;
    }

    broadcast(vaddr, page_count);
    crate::kprofile::end(crate::kprofile::Slot::TlbShootdown, _prof_t);
}

/// Flush the entire TLB on all CPUs (CR3 reload).
///
/// Use this for large-scale changes (e.g., process exit, address space
/// switch) where individual `invlpg` would be more expensive.
pub fn flush_all() {
    FULL_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);

    // Local full flush.
    local_flush_all();

    if crate::smp::cpu_count() <= 1 {
        LOCAL_ONLY_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }

    broadcast(FLUSH_ALL, 0);
}

/// Ask every other online CPU to flush `addr`/`pages` (or everything, for
/// [`FLUSH_ALL`]) and wait until each has.
///
/// # Two ways this used to be able to hang, and what stops them
///
/// **Waiting for the lock with interrupts off.** The lock holder waits for an
/// acknowledgement from every other CPU, and a CPU acknowledges from the IPI
/// handler. A CPU that wanted to shoot down too, and spun on the lock with
/// interrupts disabled, could never take that IPI: the holder waited for it
/// and it waited for the holder, forever, with no message. A CPU spinning for
/// the lock now services the pending request itself on every iteration
/// ([`service_pending`]), so it acknowledges whether or not interrupts are on.
///
/// **Being preempted while holding the lock.** A holder switched out
/// mid-shootdown left every other would-be initiator spinning until it was
/// scheduled again. Preemption is disabled from before the lock is taken until
/// after it is released, as the kernel's other spinlocks do.
///
/// What this does not solve, and cannot: a CPU sitting in some *other* loop
/// with interrupts disabled — waiting on a lock this CPU holds while it
/// shoots down, say — still never acknowledges. The rule for callers stands:
/// do not shoot down while holding a lock that is taken with interrupts off.
fn broadcast(addr: u64, pages: u32) {
    let cpu = crate::smp::fast_cpu_index();
    crate::sched::preempt_disable();
    let guard = loop {
        if let Some(g) = SHOOTDOWN_LOCK.try_lock() {
            break g;
        }
        // Someone else is shooting down, and may be waiting for this CPU.
        service_pending(cpu);
        core::hint::spin_loop();
    };

    // Publish the request, then the sequence number that names it.
    FLUSH_ADDR.store(addr, Ordering::Release);
    FLUSH_PAGES.store(pages, Ordering::Release);
    ACK_COUNT.store(0, Ordering::Release);
    let seq = SHOOTDOWN_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    // This CPU has already flushed its own TLB; mark the request handled here
    // so a later `service_pending` on this CPU never acknowledges its own
    // request and makes the count reach its target one CPU early.
    if let Some(mine) = HANDLED_SEQ.get(cpu) {
        mine.store(seq, Ordering::Release);
    }

    // target_acks = online - 1 (exclude self).
    #[allow(clippy::cast_possible_truncation)]
    let target_acks = crate::smp::cpu_count().saturating_sub(1) as u32;

    IPI_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);

    // SAFETY: APIC is initialized, the vector has a valid ISR.
    unsafe {
        crate::apic::send_ipi_all_excluding_self(TLB_SHOOTDOWN_VECTOR);
    }

    // Spin-wait for all other CPUs to acknowledge.
    while ACK_COUNT.load(Ordering::Acquire) < target_acks {
        core::hint::spin_loop();
    }

    drop(guard);
    crate::sched::preempt_enable();
}

/// Service the in-flight shootdown request on CPU `cpu`, unless it already
/// has: flush what it names, then acknowledge it. Returns whether it did.
///
/// Called from the IPI handler, and from a CPU spinning for
/// [`SHOOTDOWN_LOCK`] (see [`broadcast`]). Both can run on one CPU for one
/// request — the IPI can arrive in the middle of the spinning — so the claim
/// is a compare-and-swap on [`HANDLED_SEQ`]: exactly one of them wins, and
/// only the winner flushes and acknowledges. The flush happens before the
/// acknowledgement, because the acknowledgement is the initiator's licence to
/// reuse what it unmapped.
fn service_pending(cpu: usize) -> bool {
    let Some(handled) = HANDLED_SEQ.get(cpu) else {
        return false;
    };
    let seq = SHOOTDOWN_SEQ.load(Ordering::Acquire);
    let seen = handled.load(Ordering::Acquire);
    if seen >= seq {
        return false;
    }
    if handled
        .compare_exchange(seen, seq, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let addr = FLUSH_ADDR.load(Ordering::Acquire);
    let pages = FLUSH_PAGES.load(Ordering::Acquire);
    if addr == FLUSH_ALL {
        local_flush_all();
    } else {
        local_flush_range(addr, pages);
    }
    ACK_COUNT.fetch_add(1, Ordering::Release);
    true
}

// ---------------------------------------------------------------------------
// IPI handler (called from IDT stub)
// ---------------------------------------------------------------------------

/// ISR handler for the TLB shootdown IPI (vector 251).
///
/// Called from the IDT assembly stub.  Services the pending request (see
/// [`service_pending`], which makes a second delivery harmless) and sends EOI.
///
/// Must be fast — no allocations, no lock contention, no serial output.
#[unsafe(no_mangle)]
pub extern "C" fn handle_tlb_shootdown_irq(_frame: &crate::idt::InterruptStackFrame, _error: u64) {
    service_pending(crate::smp::fast_cpu_index());

    // Send EOI to the local APIC.
    // SAFETY: Always safe to write to the APIC EOI register.
    unsafe {
        crate::apic::eoi();
    }
}

// ---------------------------------------------------------------------------
// Local TLB operations
// ---------------------------------------------------------------------------

/// Flush TLB entries for a range of 4 KiB pages on the local CPU.
fn local_flush_range(vaddr: u64, page_count: u32) {
    for i in 0..page_count {
        let addr = vaddr.wrapping_add(u64::from(i) * 4096);
        // SAFETY: invlpg is always safe in ring 0.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) addr,
                options(nostack, preserves_flags),
            );
        }
    }
}

/// Full TLB flush on the local CPU via CR3 reload.
fn local_flush_all() {
    // SAFETY: Reading and reloading CR3 is always safe in ring 0.
    // This flushes all non-global TLB entries.
    unsafe {
        let cr3: u64;
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) cr3,
            options(nostack, preserves_flags),
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify TLB shootdown infrastructure.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[tlb] Running self-test...");

    let online = crate::smp::cpu_count();
    serial_println!("[tlb]   Online CPUs: {}", online);

    // Test 1: Local flush range (should not panic).
    local_flush_range(0x1000_0000, 4);
    serial_println!("[tlb]   Local flush_range: OK");

    // Test 2: Local flush all (should not panic).
    local_flush_all();
    serial_println!("[tlb]   Local flush_all: OK");

    // Test 3: Full shootdown (exercises IPI if SMP).
    flush_range(0x2000_0000, 1);
    serial_println!("[tlb]   Shootdown flush_range: OK ({} CPUs)", online);

    // Test 4: Full flush all (exercises IPI if SMP).
    flush_all();
    serial_println!("[tlb]   Shootdown flush_all: OK ({} CPUs)", online);

    // Test 5: the servicing path a waiting CPU and the IPI share.
    let mut skipped = 0usize;
    if online <= 1 {
        test_service_pending()?;
        serial_println!("[tlb]   service_pending acknowledges once, never its own request: OK");
    } else {
        // The request it stages is visible to every CPU; another one could
        // service it and leave an acknowledgement for the next real request.
        serial_println!(
            "[tlb]   SKIP: service_pending unit test (other CPUs online could answer its staged request)"
        );
        skipped = skipped.saturating_add(1);
    }

    if skipped == 0 {
        serial_println!("[tlb] Self-test PASSED");
    } else {
        serial_println!("[tlb] Self-test passed with {} section(s) SKIPPED", skipped);
    }
    Ok(())
}

/// [`service_pending`] acknowledges a request exactly once however often it
/// is asked — the IPI and a spinning waiter can both ask on one CPU — and
/// never answers a request this CPU issued.
///
/// A single-CPU system never broadcasts, so without this the path runs only on
/// multi-core hardware, which the boot test does not use. The requests are
/// staged by hand under the lock, the way [`broadcast`] stages real ones, and
/// name a harmless address. Only called with one CPU online.
fn test_service_pending() -> crate::error::KernelResult<()> {
    let cpu = crate::smp::fast_cpu_index();
    let fail = |what: &str| {
        serial_println!("[tlb]   FAIL: service_pending: {}", what);
        crate::error::KernelError::InternalError
    };
    let guard = SHOOTDOWN_LOCK.lock();
    let result = (|| -> crate::error::KernelResult<()> {
        // A request from "another CPU": published and numbered, not yet
        // handled here.
        FLUSH_ADDR.store(0x3000_0000, Ordering::Release);
        FLUSH_PAGES.store(1, Ordering::Release);
        ACK_COUNT.store(0, Ordering::Release);
        SHOOTDOWN_SEQ.fetch_add(1, Ordering::AcqRel);
        if !service_pending(cpu) || ACK_COUNT.load(Ordering::Acquire) != 1 {
            return Err(fail(
                "a pending request was not serviced and acknowledged once",
            ));
        }
        if service_pending(cpu) || ACK_COUNT.load(Ordering::Acquire) != 1 {
            return Err(fail(
                "servicing the same request again acknowledged it twice",
            ));
        }

        // A request this CPU issued is marked handled at issue, as
        // `broadcast` does, and must never be self-acknowledged.
        ACK_COUNT.store(0, Ordering::Release);
        let own = SHOOTDOWN_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if let Some(mine) = HANDLED_SEQ.get(cpu) {
            mine.store(own, Ordering::Release);
        }
        if service_pending(cpu) || ACK_COUNT.load(Ordering::Acquire) != 0 {
            return Err(fail("this CPU acknowledged a request it issued itself"));
        }
        Ok(())
    })();
    // The next real request resets the count before it numbers itself.
    ACK_COUNT.store(0, Ordering::Release);
    drop(guard);
    result
}
