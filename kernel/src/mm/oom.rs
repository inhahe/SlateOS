//! Out-of-memory (OOM) handler.
//!
//! When the frame allocator cannot satisfy an allocation even after
//! swap reclamation and kswapd wakeup, the OOM handler is invoked as
//! a last resort.  Its job is to free memory by killing a process or
//! returning an error to the allocator.
//!
//! ## OOM Policies
//!
//! The `mm.oom_policy` sysctl parameter controls what happens:
//!
//! | Policy | Action                      | Use case               |
//! |--------|-----------------------------|------------------------|
//! | 0      | Kill the largest process    | Desktop (default)      |
//! | 1      | Kill the most recent process| Protects long-running  |
//! | 2      | Return error to allocator   | Server (graceful fail) |
//!
//! ## Callback Architecture
//!
//! The actual process killing is not implemented here — it lives in the
//! `proc` module (kernel-process zone).  This module provides a callback
//! registration mechanism:
//!
//! 1. During boot, `proc` registers a callback via [`register_kill_callback`].
//! 2. When OOM occurs, [`handle_oom`] reads the policy, invokes the
//!    callback (if policy 0 or 1), and returns the result.
//! 3. If no callback is registered (early boot, or policy 2), the
//!    allocator gets `OutOfMemory` and the caller deals with it.
//!
//! This keeps the OOM policy logic in kernel-core (mm/) while the
//! process-killing implementation stays in kernel-process (proc/).
//!
//! ## Design Rationale
//!
//! Unlike Linux's OOM killer, which is deeply integrated into the memory
//! reclaim path and has complex heuristics (badness score, oom_adj,
//! memory cgroups), ours is intentionally simple:
//!
//! - We use **committed allocation** by default, so OOM only happens
//!   when physical memory + swap is genuinely exhausted.  No overcommit
//!   surprises.
//! - The OOM handler is a last resort, not a load-bearing mechanism.
//!   kswapd + swap should handle normal memory pressure.
//!
//! ## References
//!
//! - Linux `mm/oom_kill.c` — oom_badness(), select_bad_process()
//! - Our design spec: "don't allow swapping to tie up the system"

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Number of OOM events since boot.
static OOM_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Number of processes killed by OOM since boot.
static OOM_KILLS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Kill callback
// ---------------------------------------------------------------------------

/// Callback signature for process killing.
///
/// The OOM handler calls this when policy 0 or 1 is active.
///
/// Arguments:
/// - `policy`: the OOM policy (0 = kill largest, 1 = kill newest).
/// - `needed_pages`: how many pages the allocator needs.
///
/// Returns the number of pages freed by killing the process.
/// Returns 0 if no suitable victim was found.
///
/// The callback must not allocate memory (it's called from the
/// allocation failure path!).  It should:
/// 1. Iterate the process table.
/// 2. Select a victim based on the policy.
/// 3. Kill the victim's threads.
/// 4. Free the victim's address space.
/// 5. Return the approximate number of frames freed.
type KillCallback = fn(policy: u8, needed_pages: usize) -> usize;

/// Registered kill callback.  `None` during early boot before the
/// proc module registers its handler.
///
/// We use a simple `Option<fn(...)>` stored behind a spinlock
/// because:
/// - It's set once during boot, then read on every OOM event.
/// - OOM events are rare (the whole system is designed to avoid them).
/// - No need for atomic function pointer tricks.
static KILL_CALLBACK: spin::Mutex<Option<KillCallback>> = spin::Mutex::new(None);

/// Register the OOM kill callback.
///
/// Called once during boot by the process management module after it
/// has initialized the process table.  The callback remains registered
/// for the lifetime of the kernel.
///
/// If a callback is already registered, it is replaced (the new one
/// wins).
pub fn register_kill_callback(cb: KillCallback) {
    *KILL_CALLBACK.lock() = Some(cb);
    serial_println!("[oom] Kill callback registered");
}

// ---------------------------------------------------------------------------
// OOM handler
// ---------------------------------------------------------------------------

/// Invoke the OOM handler.
///
/// Called by the frame allocator when:
/// 1. The allocation failed.
/// 2. Swap reclamation freed 0 pages.
/// 3. kswapd has been woken (but can't help in time).
///
/// Based on the `mm.oom_policy` sysctl:
/// - Policy 0: call the kill callback to kill the largest process.
/// - Policy 1: call the kill callback to kill the newest process.
/// - Policy 2: return 0 (let the allocator propagate `OutOfMemory`).
///
/// Returns the number of pages freed.  If > 0, the allocator should
/// retry the allocation.
#[allow(clippy::arithmetic_side_effects)]
pub fn handle_oom(needed_pages: usize) -> usize {
    OOM_EVENTS.fetch_add(1, Ordering::Relaxed);

    // Last-resort pressure notification: give all caches one final
    // chance to free memory before we resort to killing processes.
    super::pressure::notify(super::pressure::PressureLevel::Critical);

    let policy = crate::sysctl::get(crate::sysctl::PARAM_MM_OOM_POLICY)
        .unwrap_or(0) as u8;

    serial_println!(
        "[oom] OOM event #{}: need {} pages, policy={}",
        OOM_EVENTS.load(Ordering::Relaxed),
        needed_pages,
        policy,
    );

    match policy {
        // Policy 2: return error to caller.
        2 => {
            serial_println!("[oom] Policy 2: returning OutOfMemory to allocator");
            0
        }

        // Policy 0 or 1: try to kill a process.
        _ => {
            let callback = *KILL_CALLBACK.lock();
            match callback {
                Some(cb) => {
                    let freed = cb(policy, needed_pages);
                    if freed > 0 {
                        OOM_KILLS.fetch_add(1, Ordering::Relaxed);
                        serial_println!(
                            "[oom] Killed a process, freed {} pages (total kills: {})",
                            freed,
                            OOM_KILLS.load(Ordering::Relaxed),
                        );
                    } else {
                        serial_println!(
                            "[oom] Kill callback returned 0 — no suitable victim found"
                        );
                    }
                    freed
                }
                None => {
                    // No kill callback registered (early boot or proc not
                    // initialized).  Can't kill anything — treat as policy 2.
                    serial_println!(
                        "[oom] No kill callback registered — returning OutOfMemory"
                    );
                    0
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics API
// ---------------------------------------------------------------------------

/// Number of OOM events since boot.
#[must_use]
#[allow(dead_code)] // Public API for diagnostics.
pub fn oom_event_count() -> u64 {
    OOM_EVENTS.load(Ordering::Relaxed)
}

/// Number of processes killed by OOM since boot.
#[must_use]
#[allow(dead_code)] // Public API for diagnostics.
pub fn oom_kill_count() -> u64 {
    OOM_KILLS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the OOM handler module.
///
/// Verifies:
/// 1. Policy 2 returns 0 (no kill attempt).
/// 2. No callback → returns 0 gracefully.
/// 3. Statistics counters increment.
/// 4. Callback registration works.
pub fn self_test() {
    serial_println!("[oom] Running self-test...");

    // Save original policy and event count.
    let original_policy = crate::sysctl::get(crate::sysctl::PARAM_MM_OOM_POLICY)
        .unwrap_or(0);
    let events_before = oom_event_count();

    // -- Test 1: Policy 2 returns 0 --
    let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_OOM_POLICY, 2);
    let freed = handle_oom(10);
    assert_eq!(freed, 0, "policy 2 should return 0");
    assert_eq!(
        oom_event_count(),
        events_before + 1,
        "event counter should increment"
    );
    serial_println!("[oom]   Policy 2 (return error): OK");

    // -- Test 2: Policy 0 with no callback returns 0 --
    // (The callback may already be registered in production, so
    // we test this only if no callback is set yet.)
    let has_callback = KILL_CALLBACK.lock().is_some();
    if !has_callback {
        let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_OOM_POLICY, 0);
        let freed = handle_oom(10);
        assert_eq!(freed, 0, "no callback should return 0");
        serial_println!("[oom]   No callback fallback: OK");
    } else {
        serial_println!("[oom]   Callback already registered (skip no-callback test)");
    }

    // -- Test 3: Register a test callback --
    fn test_callback(_policy: u8, _needed: usize) -> usize {
        42 // Pretend we freed 42 pages.
    }
    register_kill_callback(test_callback);
    let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_OOM_POLICY, 0);
    let freed = handle_oom(10);
    assert_eq!(freed, 42, "test callback should return 42");
    assert!(
        oom_kill_count() > 0,
        "kill counter should increment"
    );
    serial_println!("[oom]   Callback registration and invocation: OK");

    // Restore original state.
    let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_OOM_POLICY, original_policy);

    // Clear the test callback — replace with None by registering a
    // no-op that returns 0 (until the real proc callback is registered).
    fn noop_callback(_policy: u8, _needed: usize) -> usize { 0 }
    register_kill_callback(noop_callback);

    serial_println!("[oom] Self-test PASSED");
}
