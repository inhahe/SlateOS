//! `<linux/mutex.h>` — Kernel mutex constants.
//!
//! Kernel mutexes are sleeping locks that provide mutual exclusion
//! in process context. Unlike spinlocks (which busy-wait), mutexes
//! put the waiting task to sleep, making them suitable for protecting
//! operations that may take a long time. Mutexes support features
//! like trylock, interruptible waiting, and lockdep integration.
//! They cannot be used in interrupt context (use spinlocks there).

// ---------------------------------------------------------------------------
// Mutex states (internal count encoding)
// ---------------------------------------------------------------------------

/// Mutex is unlocked (available).
pub const MUTEX_UNLOCKED: u32 = 1;
/// Mutex is locked (held by a task).
pub const MUTEX_LOCKED: u32 = 0;
/// Mutex is locked with waiters (contended).
pub const MUTEX_LOCKED_WAITERS: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Mutex flags
// ---------------------------------------------------------------------------

/// Mutex supports priority inheritance (rt_mutex underneath).
pub const MUTEX_FLAG_PI: u32 = 0x01;
/// Mutex uses wait-die deadlock avoidance (ww_mutex).
pub const MUTEX_FLAG_WW: u32 = 0x02;
/// Mutex uses optimistic spinning.
pub const MUTEX_FLAG_SPIN: u32 = 0x04;
/// Mutex allows handoff (pass directly to first waiter).
pub const MUTEX_FLAG_HANDOFF: u32 = 0x08;

// ---------------------------------------------------------------------------
// Wait types (how to wait when mutex is contended)
// ---------------------------------------------------------------------------

/// Uninterruptible wait (cannot be killed by signal).
pub const MUTEX_WAIT_UNINTERRUPTIBLE: u32 = 0;
/// Interruptible wait (can be woken by signal, returns -EINTR).
pub const MUTEX_WAIT_INTERRUPTIBLE: u32 = 1;
/// Killable wait (can be woken by fatal signal only).
pub const MUTEX_WAIT_KILLABLE: u32 = 2;

// ---------------------------------------------------------------------------
// ww_mutex (wound-wait) constants
// ---------------------------------------------------------------------------

/// Wound-wait: this task should back off (wound).
pub const WW_WOUND: u32 = 0;
/// Wound-wait: this task should wait (die).
pub const WW_WAIT: u32 = 1;
/// Maximum ww_mutex acquire context nesting depth.
pub const WW_MAX_NESTING: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        // LOCKED and LOCKED_WAITERS might be same on some encodings,
        // but UNLOCKED must differ from both
        assert_ne!(MUTEX_UNLOCKED, MUTEX_LOCKED);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [MUTEX_FLAG_PI, MUTEX_FLAG_WW, MUTEX_FLAG_SPIN, MUTEX_FLAG_HANDOFF];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_wait_types_distinct() {
        let types = [
            MUTEX_WAIT_UNINTERRUPTIBLE,
            MUTEX_WAIT_INTERRUPTIBLE,
            MUTEX_WAIT_KILLABLE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ww_values_distinct() {
        assert_ne!(WW_WOUND, WW_WAIT);
        assert!(WW_MAX_NESTING > 0);
    }
}
