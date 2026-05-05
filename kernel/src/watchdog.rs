//! Soft lockup detector (watchdog).
//!
//! Detects CPUs that stop servicing timer interrupts — typically caused
//! by an infinite loop with interrupts disabled, or a deadlock in an
//! interrupt handler.
//!
//! ## How it works
//!
//! Each CPU maintains a per-CPU heartbeat counter that is incremented on
//! every timer tick (100 Hz APIC timer).  The BSP periodically checks all
//! CPUs' heartbeats.  If any CPU's heartbeat hasn't advanced for longer
//! than [`LOCKUP_THRESHOLD_TICKS`], a soft lockup warning is logged with
//! the CPU index and last known tick.
//!
//! ## Limitations
//!
//! - This is a *soft* lockup detector: it only detects lockups on CPUs
//!   other than the one running the check.  If the BSP itself locks up,
//!   no warning is generated (would require NMI-based hard lockup detection
//!   via PMC or HPET comparator, which we don't have on QEMU).
//! - The check runs from the BSP's timer softirq context.  If softirq
//!   processing is delayed, the detection window is extended.
//!
//! ## Integration
//!
//! Called from the timer softirq handler (softirq.rs) on CPU 0 only,
//! every [`CHECK_INTERVAL_TICKS`] ticks.

use crate::serial_println;
use crate::smp;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How many ticks (at 100 Hz) without a heartbeat before we declare a
/// soft lockup.  1000 ticks = 10 seconds.
const LOCKUP_THRESHOLD_TICKS: u64 = 1000;

/// How often (in global ticks) the BSP checks all CPUs.
/// 100 ticks = 1 second.
pub const CHECK_INTERVAL_TICKS: u64 = 100;

// ---------------------------------------------------------------------------
// Per-CPU heartbeat
// ---------------------------------------------------------------------------

/// Per-CPU heartbeat counters.  Index = CPU logical index (0 = BSP).
/// Each CPU increments its counter on every timer tick.
static HEARTBEATS: [AtomicU64; smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; smp::MAX_CPUS]
};

/// Last observed heartbeat for each CPU (used by the BSP checker).
/// If `HEARTBEATS[cpu]` hasn't changed since `LAST_SEEN[cpu]`, the
/// CPU is potentially stuck.
static LAST_SEEN: [AtomicU64; smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; smp::MAX_CPUS]
};

/// How many consecutive checks each CPU has been stale.
/// Only triggers a warning when this reaches the threshold.
static STALE_COUNT: [AtomicU64; smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; smp::MAX_CPUS]
};

/// Whether we already printed a warning for this CPU in the current
/// lockup episode (avoids flooding serial output).
static WARNED: [AtomicU64; smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; smp::MAX_CPUS]
};

/// Whether the watchdog has been initialized.
static ENABLED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the watchdog.  Call after SMP bootstrap so cpu_count() is
/// accurate.
pub fn init() {
    // Seed LAST_SEEN with current heartbeat values to avoid false
    // positives on first check.
    let cpus = smp::cpu_count();
    for i in 0..cpus {
        if let Some(h) = HEARTBEATS.get(i) {
            if let Some(ls) = LAST_SEEN.get(i) {
                ls.store(h.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        }
    }
    ENABLED.store(1, Ordering::Release);
    serial_println!(
        "[watchdog] Soft lockup detector enabled ({} CPUs, threshold={}s)",
        cpus,
        LOCKUP_THRESHOLD_TICKS / 100,
    );
}

/// Increment the heartbeat for the current CPU.
///
/// Called from the APIC timer ISR on every CPU (not just BSP).
/// Must be extremely fast — single atomic increment.
#[inline]
pub fn heartbeat() {
    let cpu = smp::current_cpu_index();
    if let Some(h) = HEARTBEATS.get(cpu) {
        h.fetch_add(1, Ordering::Relaxed);
    }
}

/// Check all CPUs for soft lockup.
///
/// Called from the BSP's timer softirq handler every
/// [`CHECK_INTERVAL_TICKS`] ticks.  Compares each CPU's current
/// heartbeat against its last observed value.
///
/// A CPU is declared locked up if its heartbeat hasn't advanced for
/// [`LOCKUP_THRESHOLD_TICKS`] consecutive ticks (checked every
/// CHECK_INTERVAL_TICKS, so the number of stale checks before trigger =
/// LOCKUP_THRESHOLD_TICKS / CHECK_INTERVAL_TICKS).
pub fn check() {
    if ENABLED.load(Ordering::Acquire) == 0 {
        return;
    }

    let cpus = smp::cpu_count();
    #[allow(clippy::arithmetic_side_effects)]
    let stale_threshold = LOCKUP_THRESHOLD_TICKS / CHECK_INTERVAL_TICKS;

    for cpu in 0..cpus {
        let current = HEARTBEATS.get(cpu)
            .map_or(0, |h| h.load(Ordering::Relaxed));
        let last = LAST_SEEN.get(cpu)
            .map_or(0, |ls| ls.load(Ordering::Relaxed));

        if current == last {
            // Heartbeat hasn't advanced since last check.
            if let Some(sc) = STALE_COUNT.get(cpu) {
                let count = sc.fetch_add(1, Ordering::Relaxed);
                #[allow(clippy::arithmetic_side_effects)]
                if count + 1 >= stale_threshold {
                    // Soft lockup detected!
                    let warned = WARNED.get(cpu)
                        .map_or(0, |w| w.load(Ordering::Relaxed));
                    if warned == 0 {
                        serial_println!(
                            "[watchdog] SOFT LOCKUP on CPU {} (heartbeat stuck at {}, \
                             stale for {} checks ≈ {}s)",
                            cpu,
                            current,
                            count + 1,
                            ((count + 1) * CHECK_INTERVAL_TICKS) / 100,
                        );
                        if let Some(w) = WARNED.get(cpu) {
                            w.store(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        } else {
            // Heartbeat advanced — CPU is alive.  Update last_seen.
            if let Some(ls) = LAST_SEEN.get(cpu) {
                ls.store(current, Ordering::Relaxed);
            }
            // Clear stale count and warning flag.
            if let Some(sc) = STALE_COUNT.get(cpu) {
                sc.store(0, Ordering::Relaxed);
            }
            if let Some(w) = WARNED.get(cpu) {
                if w.load(Ordering::Relaxed) != 0 {
                    serial_println!(
                        "[watchdog] CPU {} recovered (heartbeat advancing again)",
                        cpu
                    );
                    w.store(0, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Return whether the watchdog is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire) != 0
}

/// Disable the watchdog (e.g., before entering a known long operation
/// that disables interrupts, like firmware calls).
pub fn disable() {
    ENABLED.store(0, Ordering::Release);
}

/// Re-enable the watchdog after a known long operation.
pub fn enable() {
    // Reset all state to avoid false positive from the disabled period.
    let cpus = smp::cpu_count();
    for i in 0..cpus {
        if let Some(h) = HEARTBEATS.get(i) {
            if let Some(ls) = LAST_SEEN.get(i) {
                ls.store(h.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        }
        if let Some(sc) = STALE_COUNT.get(i) {
            sc.store(0, Ordering::Relaxed);
        }
        if let Some(w) = WARNED.get(i) {
            w.store(0, Ordering::Relaxed);
        }
    }
    ENABLED.store(1, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the watchdog subsystem.
///
/// Tests:
/// 1. Heartbeat increments correctly.
/// 2. Check doesn't false-positive when heartbeats are advancing.
/// 3. Check detects a simulated stale heartbeat.
pub fn self_test() {
    serial_println!("[watchdog] Running self-test...");

    // Test 1: Heartbeat increments.
    let cpu = smp::current_cpu_index();
    let before = HEARTBEATS.get(cpu)
        .map_or(0, |h| h.load(Ordering::Relaxed));
    heartbeat();
    let after = HEARTBEATS.get(cpu)
        .map_or(0, |h| h.load(Ordering::Relaxed));
    assert_eq!(after, before + 1, "heartbeat should increment");
    serial_println!("[watchdog]   Heartbeat increment: OK");

    // Test 2: Check doesn't false-positive.
    // Make sure last_seen is current.
    if let Some(ls) = LAST_SEEN.get(cpu) {
        ls.store(after, Ordering::Relaxed);
    }
    // Advance heartbeat once more.
    heartbeat();
    // Run check — should see advancement, no warning.
    check();
    let stale = STALE_COUNT.get(cpu)
        .map_or(u64::MAX, |sc| sc.load(Ordering::Relaxed));
    assert_eq!(stale, 0, "no false positive when heartbeat advances");
    serial_println!("[watchdog]   No false positive: OK");

    // Test 3: Simulated stale detection.
    // Freeze the heartbeat (don't call heartbeat()) and run multiple checks.
    let frozen = HEARTBEATS.get(cpu)
        .map_or(0, |h| h.load(Ordering::Relaxed));
    if let Some(ls) = LAST_SEEN.get(cpu) {
        ls.store(frozen, Ordering::Relaxed);
    }
    if let Some(sc) = STALE_COUNT.get(cpu) {
        sc.store(0, Ordering::Relaxed);
    }
    // Run check — heartbeat unchanged, stale count should increment.
    check();
    let stale_after = STALE_COUNT.get(cpu)
        .map_or(0, |sc| sc.load(Ordering::Relaxed));
    assert_eq!(stale_after, 1, "stale count should be 1 after one stale check");
    serial_println!("[watchdog]   Stale detection: OK");

    // Clean up: advance heartbeat so we don't trigger a real warning.
    heartbeat();
    if let Some(ls) = LAST_SEEN.get(cpu) {
        ls.store(
            HEARTBEATS.get(cpu).map_or(0, |h| h.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }
    if let Some(sc) = STALE_COUNT.get(cpu) {
        sc.store(0, Ordering::Relaxed);
    }
    if let Some(w) = WARNED.get(cpu) {
        w.store(0, Ordering::Relaxed);
    }

    serial_println!("[watchdog] Self-test PASSED");
}
