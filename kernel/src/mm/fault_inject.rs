//! Memory fault injection — controlled failure simulation for testing.
//!
//! Provides a mechanism to inject artificial failures into the memory
//! subsystem for testing error recovery paths.  Without fault injection,
//! many error paths (OOM handling, allocation retry, graceful degradation)
//! are never exercised because they only trigger under extreme conditions
//! that are hard to reproduce.
//!
//! ## Supported Injections
//!
//! - **Allocation failure**: make the next N alloc_frame() calls fail
//!   with OutOfMemory, even when memory is available.
//! - **Delayed failure**: fail after N successful allocations (tests
//!   cleanup paths mid-operation).
//! - **Probabilistic failure**: fail with probability 1/N (stress testing).
//!
//! ## Design
//!
//! Fault injection state is global (not per-CPU) for simplicity.
//! The frame allocator checks `should_fail_alloc()` before proceeding.
//! Injection is always disabled in production builds — it's gated behind
//! explicit calls from self-tests or kshell commands.
//!
//! ## Safety
//!
//! Fault injection can cause kernel panics if essential allocations fail
//! without proper error handling.  Only use it in controlled test contexts
//! where you expect failures and have verified the recovery paths.
//!
//! ## References
//!
//! - Linux `lib/fault-inject.c` — kernel fault injection framework
//! - Linux `mm/failslab.c` — slab allocation failure injection
//! - Linux `Documentation/fault-injection/` — usage guide

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Injection modes
// ---------------------------------------------------------------------------

/// Injection mode enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMode {
    /// No injection — normal operation.
    None,
    /// Fail the next N allocations unconditionally.
    FailNext,
    /// Fail after N successful allocations (then fail once).
    FailAfter,
    /// Fail with probability 1/N (approximate, using a counter).
    Probabilistic,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether any injection is active.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Current injection mode (stored as u8 for atomic access).
/// 0=None, 1=FailNext, 2=FailAfter, 3=Probabilistic
static MODE: AtomicU32 = AtomicU32::new(0);

/// Counter for the injection (mode-dependent meaning):
/// - FailNext: number of failures remaining
/// - FailAfter: number of successful allocs before failure
/// - Probabilistic: denominator (fail every Nth attempt)
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Call counter (incremented on each should_fail_alloc check).
static CALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total injected failures since boot.
static TOTAL_INJECTED: AtomicU64 = AtomicU64::new(0);

/// Total injection sessions started.
static SESSIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Arm the injection: fail the next `count` allocations.
///
/// After `count` failures, injection automatically disarms.
pub fn arm_fail_next(count: u32) {
    MODE.store(1, Ordering::Release);
    COUNTER.store(count, Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
    SESSIONS.fetch_add(1, Ordering::Relaxed);
}

/// Arm the injection: allow `count` successful allocations, then fail once.
///
/// Useful for testing cleanup paths mid-operation (e.g., fail the 3rd
/// alloc in a sequence of 5).
pub fn arm_fail_after(count: u32) {
    MODE.store(2, Ordering::Release);
    COUNTER.store(count, Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
    SESSIONS.fetch_add(1, Ordering::Relaxed);
}

/// Arm probabilistic injection: fail approximately 1 in `denominator` attempts.
///
/// Runs indefinitely until `disarm()` is called.
pub fn arm_probabilistic(denominator: u32) {
    if denominator == 0 {
        return;
    }
    MODE.store(3, Ordering::Release);
    COUNTER.store(denominator, Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
    SESSIONS.fetch_add(1, Ordering::Relaxed);
}

/// Disarm all injection — return to normal operation.
pub fn disarm() {
    ACTIVE.store(false, Ordering::Release);
    MODE.store(0, Ordering::Release);
    COUNTER.store(0, Ordering::Release);
}

/// Check whether the current allocation should fail.
///
/// Called by the frame allocator on every allocation attempt.
/// Returns `true` if this allocation should be failed (injected OOM).
///
/// This function is designed to be extremely cheap when no injection is
/// active (single atomic load + branch on the fast path).
#[inline]
pub fn should_fail_alloc() -> bool {
    // Fast path: no injection active (~1 cycle: load + branch).
    if !ACTIVE.load(Ordering::Acquire) {
        return false;
    }

    // Slow path: injection is active, evaluate the mode.
    should_fail_alloc_slow()
}

/// Slow path for should_fail_alloc (called only when injection is active).
#[cold]
fn should_fail_alloc_slow() -> bool {
    CALL_COUNT.fetch_add(1, Ordering::Relaxed);

    let mode = MODE.load(Ordering::Acquire);
    match mode {
        1 => {
            // FailNext: fail and decrement counter.
            let remaining = COUNTER.fetch_sub(1, Ordering::AcqRel);
            if remaining == 0 || remaining > 0x8000_0000 {
                // Counter underflow — disarm.
                disarm();
                return false;
            }
            if remaining == 1 {
                // Last failure — disarm after this.
                disarm();
            }
            TOTAL_INJECTED.fetch_add(1, Ordering::Relaxed);
            true
        }
        2 => {
            // FailAfter: count down successes, then fail once.
            let remaining = COUNTER.fetch_sub(1, Ordering::AcqRel);
            if remaining <= 1 {
                // Time to fail.
                TOTAL_INJECTED.fetch_add(1, Ordering::Relaxed);
                disarm();
                return true;
            }
            false // Not yet.
        }
        3 => {
            // Probabilistic: fail every Nth call.
            let denom = COUNTER.load(Ordering::Relaxed);
            if denom == 0 {
                disarm();
                return false;
            }
            let calls = CALL_COUNT.load(Ordering::Relaxed);
            if calls.is_multiple_of(u64::from(denom)) {
                TOTAL_INJECTED.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            false
        }
        _ => {
            // Unknown mode — disarm and don't fail.
            disarm();
            false
        }
    }
}

/// Whether injection is currently active.
#[must_use]
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct InjectStats {
    /// Whether injection is currently active.
    pub active: bool,
    /// Current mode.
    pub mode: InjectionMode,
    /// Remaining counter value.
    pub counter: u32,
    /// Total calls to should_fail_alloc.
    pub total_calls: u64,
    /// Total injected failures.
    pub total_injected: u64,
    /// Number of injection sessions started.
    pub sessions: u64,
}

/// Get injection statistics.
#[must_use]
pub fn stats() -> InjectStats {
    let mode_raw = MODE.load(Ordering::Relaxed);
    let mode = match mode_raw {
        1 => InjectionMode::FailNext,
        2 => InjectionMode::FailAfter,
        3 => InjectionMode::Probabilistic,
        _ => InjectionMode::None,
    };

    InjectStats {
        active: ACTIVE.load(Ordering::Relaxed),
        mode,
        counter: COUNTER.load(Ordering::Relaxed),
        total_calls: CALL_COUNT.load(Ordering::Relaxed),
        total_injected: TOTAL_INJECTED.load(Ordering::Relaxed),
        sessions: SESSIONS.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the fault injection system.
pub fn self_test() {
    serial_println!("[fault_inject] Running self-test...");

    // Test 1: Default state is inactive.
    assert!(!is_active());
    assert!(!should_fail_alloc());
    serial_println!("[fault_inject]   Default inactive: OK");

    // Test 2: arm_fail_next(3) fails exactly 3 times.
    arm_fail_next(3);
    assert!(is_active());
    assert!(should_fail_alloc());  // Fail 1.
    assert!(should_fail_alloc());  // Fail 2.
    assert!(should_fail_alloc());  // Fail 3 (last, disarms).
    assert!(!is_active());         // Now disarmed.
    assert!(!should_fail_alloc()); // No more failures.
    serial_println!("[fault_inject]   arm_fail_next(3): OK");

    // Test 3: arm_fail_after(3) succeeds 2 times, fails on 3rd.
    arm_fail_after(3);
    assert!(!should_fail_alloc()); // Success 1 (counter: 3→2).
    assert!(!should_fail_alloc()); // Success 2 (counter: 2→1).
    assert!(should_fail_alloc());  // Fail (counter reached 1, disarm).
    assert!(!is_active());
    serial_println!("[fault_inject]   arm_fail_after(3): OK");

    // Test 4: Disarm works mid-injection.
    arm_fail_next(100);
    assert!(is_active());
    disarm();
    assert!(!is_active());
    assert!(!should_fail_alloc());
    serial_println!("[fault_inject]   Manual disarm: OK");

    // Test 5: Statistics tracking.
    let s = stats();
    assert!(s.total_injected >= 4, "should have 4+ injections");
    assert!(s.sessions >= 3, "should have 3+ sessions");
    serial_println!("[fault_inject]   Stats: injected={}, sessions={}, calls={}",
        s.total_injected, s.sessions, s.total_calls);

    serial_println!("[fault_inject] Self-test PASSED");
}
