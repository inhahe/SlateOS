//! Deferred interrupt processing (softirq).
//!
//! Softirqs are lightweight deferred work items that run immediately
//! after hardware ISR handlers complete, with interrupts **re-enabled**.
//! This is the standard mechanism for deferring work out of hard-IRQ
//! context so that other hardware interrupts are not blocked while the
//! kernel processes timer expirations, deferred wakes, or scheduler
//! rebalancing.
//!
//! ## Architecture
//!
//! Each CPU has an atomic pending bitmask.  ISR handlers raise softirqs
//! by setting bits via [`raise`].  After sending LAPIC EOI, the ISR
//! calls [`process_pending`] which:
//!
//! 1. Checks the per-CPU pending bitmap — returns immediately if empty
//!    (fast path: two atomic loads, ~2 ns).
//! 2. Sets a per-CPU `IN_SOFTIRQ` flag to prevent re-entry from nested
//!    interrupts.
//! 3. Re-enables interrupts (`STI`) so device interrupts can preempt
//!    softirq processing.
//! 4. Atomically swaps-and-clears the pending bits, runs the
//!    corresponding handlers.
//! 5. Loops up to [`MAX_SOFTIRQ_LOOPS`] times (new bits may have been
//!    raised during processing).  If bits remain after the limit, they
//!    are serviced on the next ISR exit — prevents livelock under heavy
//!    interrupt load.
//! 6. Disables interrupts (`CLI`) and clears `IN_SOFTIRQ` before
//!    returning to the assembly stub.
//!
//! ## Softirq types
//!
//! | Bit | Name | Handler |
//! |-----|------|---------|
//! | 0 | `TIMER` | Sleep-queue wakeups + IPC timer expirations |
//! | 1 | `SCHED` | Scheduler load balancing (future) |
//! | 2 | `IRQ_POLL` | Retry deferred IRQ wakes for userspace drivers |
//!
//! ## Why not Linux-style ksoftirqd?
//!
//! Linux falls back to a kernel thread (`ksoftirqd`) when softirqs
//! can't be fully drained in the IRQ exit path.  We skip this for now
//! because:
//! - Our microkernel defers most work to userspace driver tasks anyway.
//! - The softirq handlers are lightweight (scan a few arrays, `try_wake`).
//! - The `MAX_SOFTIRQ_LOOPS` limit prevents livelock.
//! - If we later need ksoftirqd, it's an additive change.
//!
//! ## Safety
//!
//! Softirq handlers run with interrupts enabled on the interrupted
//! task's kernel stack.  They must not:
//! - Hold a lock that the interrupted code might also hold (use
//!   `try_lock` in handlers, same as before).
//! - Assume they won't be preempted by a hardware interrupt.
//! - Block or sleep — this is still interrupt-adjacent context.
//!
//! ## References
//!
//! - Linux `kernel/softirq.c` — the `__do_softirq()` loop and
//!   `MAX_SOFTIRQ_RESTART` (10) limit.
//! - LWN article: "A new softirq mechanism" (context on design tradeoffs).

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};

use crate::smp::MAX_CPUS;

// ---------------------------------------------------------------------------
// Softirq type bits
// ---------------------------------------------------------------------------

/// Timer softirq — process sleep-queue wakeups and IPC timer expirations.
///
/// Raised by the LAPIC timer ISR.  Replaces the previous inline calls
/// to `process_sleep_wakeups()` and `process_timer_expirations()` that
/// ran with interrupts disabled.
pub const TIMER_SOFTIRQ: u32 = 1 << 0;

/// Scheduler softirq — load balancing and task migration.
///
/// Reserved for future use.  Currently a no-op handler.
pub const SCHED_SOFTIRQ: u32 = 1 << 1;

/// IRQ poll softirq — retry deferred wakes for userspace driver tasks.
///
/// Raised by the LAPIC timer ISR and by device ISRs when `try_wake`
/// fails (scheduler lock contention).  Replaces the previous inline
/// call to `process_deferred_wakes()` in the timer ISR.
pub const IRQ_POLL_SOFTIRQ: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum iterations of the process-pending loop before giving up.
///
/// Matches Linux `MAX_SOFTIRQ_RESTART` (10).  If new softirqs keep
/// getting raised during processing (e.g., heavy network traffic),
/// we stop after this many loops and let the next ISR exit handle the
/// rest.  This bounds worst-case softirq latency and prevents the
/// interrupted task from being starved.
const MAX_SOFTIRQ_LOOPS: u32 = 10;

// ---------------------------------------------------------------------------
// Per-CPU state
// ---------------------------------------------------------------------------

/// Per-CPU pending softirq bitmask.
///
/// ISR handlers set bits via `fetch_or`.  `process_pending` atomically
/// swaps-and-clears bits before processing.  Using `AtomicU32` gives
/// us room for 32 softirq types (we use 3 currently).
static PENDING: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];

/// Per-CPU re-entry guard.
///
/// Set while `process_pending` is executing on a CPU.  If a nested
/// interrupt also calls `process_pending`, the flag prevents re-entry
/// (the nested call returns immediately, and the outer call picks up
/// any new bits on its next loop iteration).
static IN_SOFTIRQ: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

// ---------------------------------------------------------------------------
// Counters for diagnostics / self-test
// ---------------------------------------------------------------------------

/// Total softirq processing invocations (across all CPUs).
static TOTAL_RUNS: AtomicU32 = AtomicU32::new(0);

/// Total individual softirq handler invocations.
static TOTAL_HANDLERS: AtomicU32 = AtomicU32::new(0);

/// Number of times re-entry was prevented.
static REENTRY_PREVENTED: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Public API — raise softirqs
// ---------------------------------------------------------------------------

/// Raise one or more softirq types on the current CPU.
///
/// Safe to call from ISR context (no locks, just an atomic OR).
/// The softirqs will be processed when the current (or next) ISR
/// calls [`process_pending`].
///
/// # Example
///
/// ```ignore
/// // In a timer ISR:
/// softirq::raise(softirq::TIMER_SOFTIRQ | softirq::IRQ_POLL_SOFTIRQ);
/// ```
#[inline]
pub fn raise(bits: u32) {
    let cpu = crate::smp::current_cpu_index();
    if let Some(slot) = PENDING.get(cpu) {
        slot.fetch_or(bits, Ordering::Release);
    }
}

/// Check whether this CPU is currently processing softirqs.
///
/// Used by the timer ISR to avoid preempting during softirq
/// processing — the interrupted task shouldn't lose its time slice
/// because kernel deferred work took too long.
#[inline]
#[must_use]
pub fn is_processing() -> bool {
    let cpu = crate::smp::current_cpu_index();
    IN_SOFTIRQ
        .get(cpu)
        .is_some_and(|f| f.load(Ordering::Acquire))
}

/// Check if any softirqs are pending on the current CPU (diagnostic).
#[must_use]
#[allow(dead_code)]
pub fn any_pending() -> bool {
    let cpu = crate::smp::current_cpu_index();
    PENDING
        .get(cpu)
        .is_some_and(|p| p.load(Ordering::Acquire) != 0)
}

/// Softirq subsystem statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct SoftirqStats {
    /// Total softirq processing invocations.
    pub total_runs: u32,
    /// Total individual handler invocations.
    pub total_handlers: u32,
    /// Times re-entry was prevented.
    pub reentry_prevented: u32,
}

/// Get a snapshot of softirq subsystem statistics.
#[must_use]
pub fn stats() -> SoftirqStats {
    SoftirqStats {
        total_runs: TOTAL_RUNS.load(Ordering::Relaxed),
        total_handlers: TOTAL_HANDLERS.load(Ordering::Relaxed),
        reentry_prevented: REENTRY_PREVENTED.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Public API — process softirqs
// ---------------------------------------------------------------------------

/// Process all pending softirqs on the current CPU.
///
/// Called at the end of ISR handlers, after LAPIC EOI.  Re-enables
/// interrupts during processing so hardware interrupts can preempt
/// the deferred work.  Disables interrupts before returning.
///
/// This is the main softirq dispatch loop — equivalent to Linux's
/// `__do_softirq()`.
///
/// # Fast path
///
/// If no softirqs are pending, this function checks one atomic load
/// and returns (~2 ns).  ISRs that don't raise softirqs pay almost
/// nothing.
///
/// # Safety
///
/// Must be called from ISR context, after LAPIC EOI has been sent.
/// The caller's assembly stub must expect interrupts to have been
/// re-enabled and re-disabled during this call (which is transparent
/// since the stub does `IRETQ` which restores the original `RFLAGS`).
pub unsafe fn process_pending() {
    let cpu = crate::smp::current_cpu_index();

    // Fast path: nothing pending.
    let Some(pending) = PENDING.get(cpu) else {
        return;
    };
    if pending.load(Ordering::Acquire) == 0 {
        return;
    }

    // Re-entry guard: if we're already processing softirqs on this
    // CPU (a nested interrupt called process_pending), skip.  The
    // outer invocation will pick up the new bits on its next loop.
    let Some(in_softirq) = IN_SOFTIRQ.get(cpu) else {
        return;
    };
    if in_softirq.swap(true, Ordering::AcqRel) {
        REENTRY_PREVENTED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    TOTAL_RUNS.fetch_add(1, Ordering::Relaxed);

    // --- CPU time accounting: entering softirq context ---
    crate::cputime::enter_softirq();

    // Enable interrupts so hardware IRQs can preempt softirq work.
    //
    // SAFETY: We've already sent EOI, so the LAPIC won't re-deliver
    // the interrupt that brought us here.  The per-CPU `IN_SOFTIRQ`
    // flag prevents re-entry if a nested interrupt also raises softirqs.
    // The kernel stack is 32 KiB per task — ample for a few nested
    // interrupt frames (~200 bytes each).
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }

    let mut loops: u32 = 0;
    loop {
        // Atomically read-and-clear the pending bits.
        let bits = pending.swap(0, Ordering::AcqRel);
        if bits == 0 {
            break;
        }

        // Dispatch to handlers.
        if bits & TIMER_SOFTIRQ != 0 {
            handle_timer();
            TOTAL_HANDLERS.fetch_add(1, Ordering::Relaxed);
        }
        if bits & SCHED_SOFTIRQ != 0 {
            handle_sched();
            TOTAL_HANDLERS.fetch_add(1, Ordering::Relaxed);
        }
        if bits & IRQ_POLL_SOFTIRQ != 0 {
            handle_irq_poll();
            TOTAL_HANDLERS.fetch_add(1, Ordering::Relaxed);
        }

        loops = loops.saturating_add(1);
        if loops >= MAX_SOFTIRQ_LOOPS {
            // Still have pending work but we've hit the limit.
            // Leave remaining bits for the next ISR exit.
            break;
        }
    }

    // --- CPU time accounting: leaving softirq context ---
    crate::cputime::exit_softirq();

    // Disable interrupts before returning to the assembly stub.
    //
    // SAFETY: Restoring the interrupt state to what the assembly stub
    // expects (CLI).  The stub's IRETQ will restore the original
    // RFLAGS (with IF set if the interrupted code had interrupts on).
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    in_softirq.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Softirq handlers
// ---------------------------------------------------------------------------

/// Tick counter for periodic cache writeback (global across all CPUs).
///
/// Wraps to 0 — that's fine since we just check divisibility.
/// Only CPU 0 runs the flush to avoid redundant work.
static CACHE_FLUSH_TICKS: AtomicU16 = AtomicU16::new(0);

/// Number of timer ticks between cache writeback attempts.
///
/// At 100 Hz tick rate, 500 ticks ≈ 5 seconds.  This matches
/// `DIRTY_EXPIRE_NS` (5 seconds) — entries dirty longer than the
/// expiry window are written back to their backing store, bounding
/// the data-loss window on crash.
const CACHE_FLUSH_INTERVAL: u16 = 500;

/// Timer softirq handler.
///
/// Processes sleep-queue wakeups (tasks that called `SYS_SLEEP` or
/// similar and whose deadline has passed), IPC timer expirations
/// (completion-port notifications for expired kernel timers), and
/// periodic cache writeback (every ~5 seconds on CPU 0).
///
/// Previously this work ran inline in `handle_timer_irq` with
/// interrupts disabled.  Running it here with interrupts enabled
/// means device interrupts are not blocked during the scan.
fn handle_timer() {
    crate::sched::process_sleep_wakeups();
    crate::ipc::timer::process_timer_expirations();
    crate::ktimer::process_expirations();

    // Periodic background writeback of dirty cache entries.
    //
    // Only CPU 0 runs the flush to avoid multiple CPUs doing the same work.
    // We use try_flush_expired() which returns None if the cache lock is
    // already held (avoids deadlock in softirq context).
    let ticks = CACHE_FLUSH_TICKS.fetch_add(1, Ordering::Relaxed);
    if ticks % CACHE_FLUSH_INTERVAL == 0 && crate::smp::current_cpu_index() == 0 {
        // Ignore result — if the lock is contended, we'll retry in
        // ~5 seconds.  No log spam; this is background housekeeping.
        let _ = crate::fs::cache::try_flush_expired();
    }

    // Soft lockup detection: BSP checks all CPUs' heartbeats.
    // Runs every CHECK_INTERVAL_TICKS ticks (1 second) on CPU 0 only.
    if u64::from(ticks) % crate::watchdog::CHECK_INTERVAL_TICKS == 0
        && crate::smp::current_cpu_index() == 0
    {
        crate::watchdog::check();
    }

    // Periodic system metrics sampling (1 sample/sec on BSP).
    if u64::from(ticks) % crate::kstat::SAMPLE_INTERVAL_TICKS == 0
        && crate::smp::current_cpu_index() == 0
    {
        crate::kstat::sample();
    }

    // Load average update (every 5 seconds on BSP, like Linux's LOAD_FREQ).
    if u64::from(ticks) % crate::loadavg::SAMPLE_INTERVAL_TICKS == 0
        && crate::smp::current_cpu_index() == 0
    {
        crate::loadavg::sample();
    }

    // IRQ storm detection: check once per second on BSP.
    if u64::from(ticks) % 100 == 0 && crate::smp::current_cpu_index() == 0 {
        crate::irq_storm::periodic_check();
    }
}

/// Scheduler softirq handler — proactive push-based load balancing.
///
/// Runs with interrupts enabled (unlike the timer ISR's reactive
/// pull-based work stealing).  Checks if this CPU has significantly
/// more tasks than the lightest CPU and migrates excess tasks to
/// equalize load.
///
/// Raised every [`BALANCE_INTERVAL`](crate::sched) ticks (100 ms)
/// when the local CPU has real work.  Idle CPUs use the reactive
/// pull path in [`timer_tick`](crate::sched::timer_tick) instead.
fn handle_sched() {
    crate::sched::push_balance();
}

/// IRQ poll softirq handler.
///
/// Retries deferred IRQ wakes for userspace driver tasks.  If a
/// device ISR's `try_wake` failed (scheduler lock held by the
/// interrupted code), this handler retries the wake with interrupts
/// enabled (reducing lock contention vs. the previous approach of
/// retrying in timer ISR context).
fn handle_irq_poll() {
    crate::ioapic::process_deferred_wakes();
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run softirq self-test.
///
/// Tests:
/// 1. `raise` sets the pending bits on the current CPU.
/// 2. `process_pending` clears pending bits and runs handlers.
/// 3. Re-entry prevention works.
/// 4. Fast path (no pending) is a no-op.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("[softirq] Running self-test...");

    let cpu = crate::smp::current_cpu_index();

    // Test 1: raise sets pending bits.
    // Disable interrupts so a timer ISR can't process the bits between
    // raise() and the check (the timer ISR calls process_pending which
    // would clear the bits, causing a false failure).
    unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)); }
    raise(TIMER_SOFTIRQ | IRQ_POLL_SOFTIRQ);
    let bits = PENDING.get(cpu).map_or(0, |p| p.load(Ordering::Acquire));
    unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)); }
    if bits & TIMER_SOFTIRQ == 0 || bits & IRQ_POLL_SOFTIRQ == 0 {
        serial_println!("[softirq]   FAIL: raise did not set expected bits (got {:#x})", bits);
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[softirq]   raise() sets pending bits: OK");

    // Clear for clean state before next test.
    if let Some(p) = PENDING.get(cpu) {
        p.store(0, Ordering::Release);
    }

    // Test 2: process_pending with no bits is a no-op.
    let runs_before = TOTAL_RUNS.load(Ordering::Acquire);
    // SAFETY: test context, interrupts should be enabled.
    // process_pending does STI→handlers→CLI internally, so we
    // re-enable interrupts after the call (we're in boot context,
    // not an ISR that would do IRETQ to restore RFLAGS).
    unsafe {
        process_pending();
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
    let runs_after = TOTAL_RUNS.load(Ordering::Acquire);
    if runs_after != runs_before {
        serial_println!("[softirq]   FAIL: process_pending ran with no bits pending");
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[softirq]   No-op fast path: OK");

    // Test 3: process_pending processes raised bits.
    raise(TIMER_SOFTIRQ);
    let handlers_before = TOTAL_HANDLERS.load(Ordering::Acquire);
    // SAFETY: See test 2 safety comment.
    unsafe {
        process_pending();
        // process_pending() does STI→handlers→CLI internally.
        // In boot context (no IRETQ), we must re-enable interrupts
        // so the rest of the boot doesn't run with IF=0.
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
    let handlers_after = TOTAL_HANDLERS.load(Ordering::Acquire);
    if handlers_after <= handlers_before {
        serial_println!("[softirq]   FAIL: process_pending did not run handler");
        return Err(crate::error::KernelError::InternalError);
    }
    // Verify bits were cleared.
    let remaining = PENDING.get(cpu).map_or(0, |p| p.load(Ordering::Acquire));
    if remaining != 0 {
        serial_println!("[softirq]   FAIL: bits not cleared after processing (remaining {:#x})", remaining);
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[softirq]   process_pending dispatches and clears: OK");

    // Test 4: re-entry prevention.
    //
    // Manually set IN_SOFTIRQ to simulate re-entry, then verify
    // process_pending returns immediately.
    raise(TIMER_SOFTIRQ);
    if let Some(f) = IN_SOFTIRQ.get(cpu) {
        f.store(true, Ordering::Release);
    }
    let reentry_before = REENTRY_PREVENTED.load(Ordering::Acquire);
    // SAFETY: See test 2.
    unsafe {
        process_pending();
    }
    let reentry_after = REENTRY_PREVENTED.load(Ordering::Acquire);
    // Clean up the flag.
    if let Some(f) = IN_SOFTIRQ.get(cpu) {
        f.store(false, Ordering::Release);
    }
    if reentry_after <= reentry_before {
        serial_println!("[softirq]   FAIL: re-entry was not prevented");
        return Err(crate::error::KernelError::InternalError);
    }
    // The bits should still be pending (handler was NOT called).
    let still_pending = PENDING.get(cpu).map_or(0, |p| p.load(Ordering::Acquire));
    if still_pending & TIMER_SOFTIRQ == 0 {
        serial_println!("[softirq]   FAIL: bits were consumed despite re-entry guard");
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[softirq]   Re-entry prevention: OK");

    // Clean up remaining bits from re-entry test.
    if let Some(p) = PENDING.get(cpu) {
        p.store(0, Ordering::Release);
    }

    serial_println!("[softirq] Self-test PASSED");
    Ok(())
}
