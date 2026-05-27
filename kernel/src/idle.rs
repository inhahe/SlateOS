//! CPU idle state management with MWAIT/C-state support.
//!
//! When a CPU has no runnable tasks, it enters an idle state.  This module
//! provides a smarter idle implementation than a bare `HLT` instruction:
//!
//! - **MWAIT-based idle** (if supported): Uses `MONITOR`/`MWAIT` to enter
//!   an optimal C-state while waiting for a memory write to a monitored
//!   address.  This allows the CPU to wake on cache-line invalidation
//!   without a full interrupt, and enables deeper power states.
//!
//! - **HLT fallback**: If MWAIT is not supported (rare on modern x86_64),
//!   falls back to `HLT` which wakes only on interrupt.
//!
//! ## C-States (CPU Power States)
//!
//! | State | Description | Wake latency | Power |
//! |-------|-------------|-------------|-------|
//! | C0    | Active (executing) | 0 | Full |
//! | C1    | HLT / MWAIT C1 | ~1-10 cycles | Low |
//! | C1E   | Enhanced halt | ~10-100 cycles | Lower |
//! | C2    | Stop clock (MWAIT) | ~100-1000 cycles | Very low |
//! | C3+   | Deep sleep (cache flush) | ~1000+ cycles | Minimal |
//!
//! We use **C1** by default (fast wake, minimal latency) and deeper
//! states only when the CPU has been idle for extended periods.
//!
//! ## MONITOR/MWAIT Protocol
//!
//! 1. `MONITOR(addr, 0, 0)` — arm the address monitoring hardware for
//!    the cache line containing `addr`.
//! 2. `MWAIT(hints, extensions)` — enter the specified C-state until:
//!    - A store to the monitored cache line occurs, OR
//!    - An interrupt is delivered, OR
//!    - Various implementation-specific events.
//!
//! The key advantage over HLT: another CPU writing to a shared variable
//! (e.g., a run-queue flag) wakes the idle CPU without sending an IPI.
//! This reduces inter-processor interrupt overhead for scheduler wakeups.
//!
//! ## Integration
//!
//! The idle loop in `main.rs` calls [`idle_once`] instead of `cpu::hlt()`.
//! Each CPU has a per-CPU "need_resched" flag that schedulers write to
//! when enqueuing work.  The MONITOR watches this flag.
//!
//! ## References
//!
//! - Intel SDM Vol. 2B, MONITOR/MWAIT instructions
//! - Intel SDM Vol. 3A §8.10 "MONITOR/MWAIT address range"
//! - Linux `arch/x86/kernel/process.c` — `mwait_idle()`, `select_idle_routine()`

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::cpu;
use crate::smp;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Per-CPU "need reschedule" flags.
///
/// The scheduler sets this when it enqueues a task for a specific CPU.
/// The MWAIT MONITOR watches the cache line containing this flag.
/// When set, the CPU wakes from idle and picks up the new task.
static NEED_RESCHED: [AtomicBool; smp::MAX_CPUS] = {
    const FALSE: AtomicBool = AtomicBool::new(false);
    [FALSE; smp::MAX_CPUS]
};

/// Whether MWAIT-based idle is active (set during init).
static MWAIT_ENABLED: AtomicBool = AtomicBool::new(false);

/// Which C-state hint to pass to MWAIT.
///
/// Format: bits [3:0] = sub C-state, bits [7:4] = target C-state.
/// C1 = 0x00, C1E = 0x01, C2 = 0x10, C3 = 0x20.
/// Default: C1 (0x00) — fast wake, minimal latency.
static CSTATE_HINT: AtomicU64 = AtomicU64::new(0x00);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total number of idle cycles entered across all CPUs.
static IDLE_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// Number of idle cycles entered via MWAIT (vs HLT fallback).
static MWAIT_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// Number of idle cycles entered via HLT.
static HLT_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// Number of times a CPU woke from idle due to need_resched (memory write).
static RESCHED_WAKES: AtomicU64 = AtomicU64::new(0);

/// Idle state statistics.
#[derive(Debug, Clone, Copy)]
pub struct IdleStats {
    /// Total idle entries across all CPUs.
    pub total_entries: u64,
    /// Entries via MWAIT instruction.
    pub mwait_entries: u64,
    /// Entries via HLT instruction (fallback).
    pub hlt_entries: u64,
    /// Wakes triggered by need_resched flag.
    pub resched_wakes: u64,
    /// Whether MWAIT is enabled.
    pub mwait_enabled: bool,
    /// Current C-state hint.
    pub cstate_hint: u8,
}

/// Get idle state statistics.
#[must_use]
pub fn stats() -> IdleStats {
    IdleStats {
        total_entries: IDLE_ENTRIES.load(Ordering::Relaxed),
        mwait_entries: MWAIT_ENTRIES.load(Ordering::Relaxed),
        hlt_entries: HLT_ENTRIES.load(Ordering::Relaxed),
        resched_wakes: RESCHED_WAKES.load(Ordering::Relaxed),
        mwait_enabled: MWAIT_ENABLED.load(Ordering::Relaxed),
        cstate_hint: CSTATE_HINT.load(Ordering::Relaxed) as u8,
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the idle subsystem.
///
/// Checks CPU support for MONITOR/MWAIT and enables it if available.
/// Must be called after `cpu::detect_features()`.
pub fn init() {
    let has_mwait = cpu::features()
        .is_some_and(|f| f.mwait);

    if has_mwait {
        MWAIT_ENABLED.store(true, Ordering::Release);
        crate::serial_println!(
            "[idle] MWAIT idle enabled (C-state hint: {:#04x})",
            CSTATE_HINT.load(Ordering::Relaxed),
        );
    } else {
        crate::serial_println!("[idle] MWAIT not supported, using HLT idle");
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Signal that a CPU needs to be rescheduled.
///
/// Called by the scheduler when it enqueues a task for a specific CPU.
/// If that CPU is in MWAIT, this write to the monitored cache line
/// will wake it without requiring an IPI.
///
/// Note: An IPI may still be needed for wakeup reliability (MWAIT can
/// have implementation quirks).  This flag provides a low-latency
/// "hint" path.
#[inline]
pub fn signal_resched(cpu_idx: usize) {
    if let Some(flag) = NEED_RESCHED.get(cpu_idx) {
        flag.store(true, Ordering::Release);
    }
}

/// Clear the need-resched flag for the current CPU.
///
/// Called at the start of the scheduler's `pick_next_task` to acknowledge
/// the wakeup signal.
#[inline]
pub fn clear_resched() {
    let cpu = smp::current_cpu_index();
    if let Some(flag) = NEED_RESCHED.get(cpu) {
        flag.store(false, Ordering::Release);
    }
}

/// Check if the current CPU has a pending reschedule signal.
#[inline]
#[must_use]
pub fn resched_pending() -> bool {
    let cpu = smp::current_cpu_index();
    NEED_RESCHED.get(cpu)
        .is_some_and(|f| f.load(Ordering::Acquire))
}

/// Enter the idle state once.
///
/// Uses MWAIT if available, HLT otherwise.  Returns when the CPU is
/// woken (by interrupt or monitored memory write).
///
/// This should be called in a loop — the caller should check for
/// runnable work after each return.
#[inline]
pub fn idle_once() {
    IDLE_ENTRIES.fetch_add(1, Ordering::Relaxed);

    // CPU time accounting: mark entry into idle state.
    crate::cputime::enter_idle();

    if MWAIT_ENABLED.load(Ordering::Relaxed) {
        idle_mwait();
    } else {
        idle_hlt();
    }

    // CPU time accounting: mark exit from idle state.
    crate::cputime::exit_idle();
}

/// Set the target C-state for MWAIT idle.
///
/// Common hints:
/// - `0x00` = C1 (lightest, ~1us wake latency)
/// - `0x01` = C1E (slightly deeper, ~10us)
/// - `0x10` = C2 (stop clock, ~100us wake)
/// - `0x20` = C3 (deep sleep, ~1ms wake — flushes caches!)
///
/// For desktop/interactive use, C1 is preferred (fast wake).
/// For servers under low load, C2 saves more power.
#[allow(dead_code)]
pub fn set_cstate_hint(hint: u8) {
    CSTATE_HINT.store(u64::from(hint), Ordering::Relaxed);
    crate::serial_println!("[idle] C-state hint set to {:#04x}", hint);
}

// ---------------------------------------------------------------------------
// Idle implementations
// ---------------------------------------------------------------------------

/// MWAIT-based idle.
///
/// MONITORs the current CPU's `NEED_RESCHED` flag, then enters MWAIT.
/// The CPU will wake when:
/// - Another CPU writes to the NEED_RESCHED flag (cache line invalidation)
/// - An interrupt is delivered (timer tick, IPI, device IRQ)
#[inline]
fn idle_mwait() {
    MWAIT_ENTRIES.fetch_add(1, Ordering::Relaxed);

    let cpu = smp::current_cpu_index();
    let monitor_addr = NEED_RESCHED.get(cpu)
        .map_or(core::ptr::null(), |f| f as *const AtomicBool as *const u8);

    if monitor_addr.is_null() {
        // Fallback if CPU index is out of range.
        idle_hlt();
        return;
    }

    // Check if we already have a pending resched before entering idle.
    if NEED_RESCHED.get(cpu).is_some_and(|f| f.load(Ordering::Acquire)) {
        RESCHED_WAKES.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let hint = CSTATE_HINT.load(Ordering::Relaxed) as u32;

    // SAFETY: MONITOR/MWAIT are safe in ring 0 when CPUID indicates
    // support.  The monitored address is a valid kernel static.
    // We check the flag after MONITOR to avoid the race where
    // signal_resched fires between our check and MWAIT entry.
    unsafe {
        // MONITOR: arm address monitoring for the cache line.
        // ECX=0 (no extensions), EDX=0 (no hints).
        core::arch::asm!(
            "monitor",
            in("rax") monitor_addr,
            in("ecx") 0u32,
            in("edx") 0u32,
            options(nomem, nostack, preserves_flags),
        );

        // Re-check the flag after MONITOR but before MWAIT.
        // This closes the race window: if signal_resched wrote between
        // our earlier check and the MONITOR setup, MWAIT would wait
        // forever (the write already happened, won't write again).
        if NEED_RESCHED.get(cpu).is_some_and(|f| f.load(Ordering::Acquire)) {
            RESCHED_WAKES.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // MWAIT: enter the specified C-state.
        // EAX = hints (C-state).
        // ECX = 0 (no extensions — bit 0 "interrupts as break event"
        //         requires CPUID.05H.ECX[1]=1 which not all CPUs/emulators
        //         support; without it, MWAIT still breaks on unmasked
        //         interrupts which is fine since we idle with IF=1).
        core::arch::asm!(
            "mwait",
            in("eax") hint,
            in("ecx") 0u32,
            options(nomem, nostack, preserves_flags),
        );
    }

    // Check if we woke due to resched signal.
    if NEED_RESCHED.get(cpu).is_some_and(|f| f.load(Ordering::Acquire)) {
        RESCHED_WAKES.fetch_add(1, Ordering::Relaxed);
    }
}

/// HLT-based idle (fallback when MWAIT is not available).
#[inline]
fn idle_hlt() {
    HLT_ENTRIES.fetch_add(1, Ordering::Relaxed);
    cpu::hlt();
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run idle subsystem self-test.
pub fn self_test() {
    crate::serial_println!("[idle] Running self-test...");

    let has_mwait = cpu::features().is_some_and(|f| f.mwait);
    crate::serial_println!("[idle]   MWAIT support: {}", has_mwait);

    // Test 1: signal/clear/pending cycle.
    let cpu = smp::current_cpu_index();
    clear_resched();
    assert!(!resched_pending(), "should not be pending after clear");
    signal_resched(cpu);
    assert!(resched_pending(), "should be pending after signal");
    clear_resched();
    assert!(!resched_pending(), "should not be pending after second clear");
    crate::serial_println!("[idle]   Signal/clear/pending: OK");

    // Test 2: Stats are incrementing.
    let before = stats().total_entries;
    idle_once();
    let after = stats().total_entries;
    assert!(after > before, "idle_once should increment entry counter");
    crate::serial_println!("[idle]   idle_once increments stats: OK");

    // Test 3: If MWAIT available, verify MWAIT entries counter.
    if has_mwait {
        let mw_before = stats().mwait_entries;
        // Signal resched so MWAIT returns immediately.
        signal_resched(cpu);
        idle_once();
        clear_resched();
        let mw_after = stats().mwait_entries;
        assert!(mw_after > mw_before, "MWAIT entry should be counted");
        crate::serial_println!("[idle]   MWAIT idle path: OK");
    }

    crate::serial_println!("[idle] Self-test PASSED");
}
