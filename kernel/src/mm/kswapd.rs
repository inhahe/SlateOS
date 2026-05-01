//! Background page reclaimer — kswapd equivalent.
//!
//! Linux has `kswapd`, a per-NUMA-node kernel thread that proactively
//! reclaims pages when free memory drops below a watermark.  We implement
//! the same concept as a single kernel task (no NUMA awareness needed
//! yet — our systems are single-socket desktop machines).
//!
//! ## Why This Exists
//!
//! Without a background reclaimer, page reclamation only happens
//! synchronously when `alloc_frame()` / `alloc_order()` fails.  This
//! means:
//!
//! 1. The first allocation failure pays the full cost of reclamation.
//! 2. Under sustained memory pressure, every allocation blocks.
//! 3. The system becomes unresponsive during swap storms.
//!
//! The design spec says: "If possible, don't allow swapping to tie up
//! the system."  This module is the primary mechanism for that.
//!
//! ## Watermark Design
//!
//! Two watermarks control reclaimer behavior:
//!
//! - **Low watermark** (`watermark_low`): when free frames drop below
//!   this, kswapd wakes up and starts reclaiming.  Read from the
//!   `mm.min_free_pages` sysctl parameter (default 32 = 512 KiB).
//!
//! - **High watermark** (`watermark_high`): kswapd keeps reclaiming
//!   until free frames rise above this.  Set to `low × 2` to provide
//!   headroom and avoid thrashing (constantly waking and sleeping).
//!
//! ## Wake Mechanism
//!
//! kswapd uses a hybrid sleep approach:
//!
//! 1. **Periodic check**: sleeps for ~1 second via `sleep_until_tick`,
//!    then checks watermarks.  Catches gradual memory exhaustion from
//!    many small allocations hitting per-CPU caches (which bypass the
//!    global allocator and thus can't trigger an explicit wake).
//!
//! 2. **Explicit wake**: `wake_kswapd()` sets an atomic flag and wakes
//!    the task via `try_wake`.  Called from `alloc_order()`'s inline
//!    reclamation path (the slow path only — the per-CPU cache hot path
//!    is untouched).
//!
//! ## References
//!
//! - Linux `mm/vmscan.c` — kswapd and direct reclaim
//! - FreeBSD `vm/vm_pageout.c` — page daemon

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether kswapd has been spawned.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// kswapd task ID (set once at spawn, then immutable).
static TASK_ID: AtomicU64 = AtomicU64::new(0);

/// Wake flag — set to `true` to signal kswapd to check watermarks.
/// kswapd clears this after waking.  Multiple concurrent wakes are
/// coalesced (the flag is a single bit, not a counter).
static WAKE_FLAG: AtomicBool = AtomicBool::new(false);

/// Number of reclaim cycles completed since boot (diagnostic counter).
static RECLAIM_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Total pages reclaimed by kswapd since boot (diagnostic counter).
static TOTAL_RECLAIMED: AtomicU64 = AtomicU64::new(0);

/// kswapd periodic check interval in ticks.
/// At 100 Hz, 100 ticks = 1 second.
const CHECK_INTERVAL_TICKS: u64 = 100;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Signal the background reclaimer to check watermarks and reclaim if needed.
///
/// This is a lightweight, non-blocking operation: sets an atomic flag and
/// wakes the kswapd task via `try_wake`.  Safe to call from any context
/// that can take the scheduler lock (not from ISR — use the deferred-wake
/// softirq path if needed from interrupt context).
///
/// If kswapd is already running or not yet spawned, this is a no-op.
pub fn wake_kswapd() {
    if !SPAWNED.load(Ordering::Relaxed) {
        return;
    }
    WAKE_FLAG.store(true, Ordering::Release);
    let tid = TASK_ID.load(Ordering::Relaxed);
    if tid != 0 {
        crate::sched::try_wake(tid);
    }
}

/// Read the low watermark from sysctl (`mm.min_free_pages`).
///
/// Returns the number of frames below which kswapd should wake.
#[must_use]
fn watermark_low() -> usize {
    crate::sysctl::get(crate::sysctl::PARAM_MM_MIN_FREE_PAGES)
        .unwrap_or(32) as usize
}

/// High watermark = 2× low watermark.
///
/// kswapd reclaims until free frames exceed this target, providing
/// a buffer so allocations don't immediately re-trigger reclamation.
#[must_use]
fn watermark_high() -> usize {
    watermark_low().saturating_mul(2)
}

/// Current free frame count (acquires allocator lock briefly).
#[must_use]
fn free_frames() -> usize {
    super::frame::stats().map_or(0, |s| s.free_frames)
}

/// Whether the kswapd task has been spawned.
#[must_use]
#[allow(dead_code)] // Public API for diagnostics and MemoryInfo.
pub fn is_running() -> bool {
    SPAWNED.load(Ordering::Relaxed)
}

/// Diagnostic: number of reclaim cycles completed.
#[must_use]
#[allow(dead_code)] // Public diagnostic API.
pub fn reclaim_cycles() -> u64 {
    RECLAIM_CYCLES.load(Ordering::Relaxed)
}

/// Diagnostic: total pages reclaimed by kswapd.
#[must_use]
#[allow(dead_code)] // Public diagnostic API.
pub fn total_reclaimed() -> u64 {
    TOTAL_RECLAIMED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// kswapd task
// ---------------------------------------------------------------------------

/// kswapd entry point — runs as a kernel background task.
///
/// The task loops indefinitely:
/// 1. Sleep for ~1 second or until explicitly woken.
/// 2. Check if free frames < low watermark.
/// 3. If so, reclaim in batches until free frames > high watermark
///    or nothing is left to reclaim.
/// 4. Repeat.
#[allow(clippy::arithmetic_side_effects)]
extern "C" fn kswapd_entry(_arg: u64) {
    serial_println!("[kswapd] Background page reclaimer started");
    serial_println!(
        "[kswapd]   low_wm={} frames ({} KiB), high_wm={} frames ({} KiB)",
        watermark_low(),
        watermark_low().saturating_mul(super::frame::FRAME_SIZE) / 1024,
        watermark_high(),
        watermark_high().saturating_mul(super::frame::FRAME_SIZE) / 1024,
    );

    loop {
        // ---- Sleep phase ----
        // Sleep for ~1 second, then check watermarks.  The sleep is
        // interruptible via wake_kswapd() → try_wake(), which pulls
        // us out of the sleep queue early.
        if !WAKE_FLAG.load(Ordering::Acquire) {
            let now = crate::apic::tick_count();
            crate::sched::sleep_until_tick(now.saturating_add(CHECK_INTERVAL_TICKS));

            // After waking (either from timeout or explicit wake),
            // check if there's actually work to do.
            if !WAKE_FLAG.load(Ordering::Acquire) {
                let free = free_frames();
                let low = watermark_low();
                if free >= low {
                    continue; // Memory is fine — go back to sleep.
                }
                // Memory below low watermark — fall through to reclaim.
            }
        }

        // Clear the wake flag (we're handling it now).
        WAKE_FLAG.store(false, Ordering::Release);

        // ---- Reclaim phase ----
        let high = watermark_high();
        let batch_size = crate::sysctl::get(crate::sysctl::PARAM_MM_SWAP_BATCH_SIZE)
            .unwrap_or(4) as usize;
        let batch_size = if batch_size == 0 { 4 } else { batch_size };

        let mut cycle_reclaimed = 0usize;

        loop {
            let free = free_frames();
            if free >= high {
                break; // Enough free memory now.
            }

            // Request a batch of pages to reclaim.  We ask for the
            // full deficit but try_reclaim does its own batching with
            // yields, so this won't monopolize the CPU.
            let deficit = high.saturating_sub(free);
            let target = deficit.min(batch_size);

            let reclaimed = super::swap::try_reclaim(target);
            if reclaimed == 0 {
                // Nothing left to reclaim — either no reclaimable pages
                // registered, or all pages recently accessed (second
                // chance).  Stop this cycle.
                break;
            }
            cycle_reclaimed = cycle_reclaimed.saturating_add(reclaimed);

            // Yield between batches to let other tasks run.  kswapd
            // runs at below-normal priority, so yield_now() lets any
            // higher-priority task preempt us.
            crate::sched::yield_now();
        }

        if cycle_reclaimed > 0 {
            RECLAIM_CYCLES.fetch_add(1, Ordering::Relaxed);
            TOTAL_RECLAIMED.fetch_add(cycle_reclaimed as u64, Ordering::Relaxed);
            serial_println!(
                "[kswapd] Reclaimed {} pages (free now: {}, high_wm: {})",
                cycle_reclaimed,
                free_frames(),
                high,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the kswapd background task.
///
/// Called once during boot, after the scheduler and swap subsystem are
/// initialized.
///
/// kswapd runs at `DEFAULT_PRIORITY + 4` (slightly below normal tasks)
/// to avoid interfering with interactive work while still being responsive
/// enough to reclaim pages before allocations start failing.  Under
/// memory pressure, kswapd's priority is effectively boosted by the
/// fact that most other tasks are blocked waiting for memory.
///
/// # Errors
///
/// Returns an error if the scheduler fails to create the task (e.g.,
/// out of memory for the task stack).
pub fn spawn() -> KernelResult<()> {
    if SPAWNED.load(Ordering::Relaxed) {
        return Ok(()); // Already spawned — idempotent.
    }

    let pml4 = super::page_table::active_pml4_phys();
    let priority = crate::sched::task::DEFAULT_PRIORITY.saturating_add(4);

    let tid = crate::sched::spawn(
        b"kswapd",
        priority,
        kswapd_entry,
        0,
        pml4,
    )?;

    TASK_ID.store(tid, Ordering::Release);
    SPAWNED.store(true, Ordering::Release);

    serial_println!(
        "[kswapd] Spawned as task {} (priority {})",
        tid, priority,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kswapd module.
///
/// Verifies:
/// 1. Watermark computation is sane.
/// 2. Wake flag set/clear works.
/// 3. Diagnostic counters are accessible.
/// 4. kswapd is running (if spawned).
pub fn self_test() {
    serial_println!("[kswapd] Running self-test...");

    // -- Watermark sanity --
    let low = watermark_low();
    let high = watermark_high();
    assert!(low > 0, "low watermark must be > 0");
    assert!(high >= low, "high watermark must be >= low watermark");
    assert_eq!(high, low.saturating_mul(2), "high = 2× low");
    serial_println!(
        "[kswapd]   Watermarks: low={}, high={} (OK)",
        low, high,
    );

    // -- Wake flag --
    WAKE_FLAG.store(false, Ordering::Release);
    assert!(!WAKE_FLAG.load(Ordering::Acquire));
    WAKE_FLAG.store(true, Ordering::Release);
    assert!(WAKE_FLAG.load(Ordering::Acquire));
    WAKE_FLAG.store(false, Ordering::Release);
    serial_println!("[kswapd]   Wake flag: OK");

    // -- Diagnostic counters --
    let _cycles = reclaim_cycles();
    let _total = total_reclaimed();
    serial_println!(
        "[kswapd]   Counters: cycles={}, total_reclaimed={} (OK)",
        _cycles, _total,
    );

    // -- Running check --
    if is_running() {
        let tid = TASK_ID.load(Ordering::Relaxed);
        serial_println!("[kswapd]   Running as task {} (OK)", tid);
    } else {
        serial_println!("[kswapd]   Not yet spawned (expected during early boot)");
    }

    serial_println!("[kswapd] Self-test PASSED");
}
