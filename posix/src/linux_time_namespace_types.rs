//! `<linux/time_namespace.h>` — Time namespace constants.
//!
//! Time namespaces (Linux 5.6+) allow processes to observe different
//! values for CLOCK_MONOTONIC and CLOCK_BOOTTIME. Each time namespace
//! has offsets that are added to the real clock values. This enables
//! containers to have their own view of system uptime, which is useful
//! for checkpoint/restore (CRIU): a restored container sees continuous
//! uptime rather than a jump. CLOCK_REALTIME is not affected (it
//! must remain consistent for distributed systems).

// ---------------------------------------------------------------------------
// Time namespace clone flag
// ---------------------------------------------------------------------------

/// Clone flag for creating a new time namespace.
pub const CLONE_NEWTIME: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Time namespace offset clocks (which clocks can be offset)
// ---------------------------------------------------------------------------

/// CLOCK_MONOTONIC can be offset in time namespace.
pub const TIMENS_OFFSET_MONOTONIC: u32 = 0;
/// CLOCK_BOOTTIME can be offset in time namespace.
pub const TIMENS_OFFSET_BOOTTIME: u32 = 1;

// ---------------------------------------------------------------------------
// /proc/<pid>/timens_offsets format identifiers
// ---------------------------------------------------------------------------

/// Monotonic offset identifier in timens_offsets file.
pub const TIMENS_MONOTONIC_ID: u32 = 1;
/// Boottime offset identifier in timens_offsets file.
pub const TIMENS_BOOTTIME_ID: u32 = 7;

// ---------------------------------------------------------------------------
// Time namespace states
// ---------------------------------------------------------------------------

/// Namespace is active (offsets applied).
pub const TIMENS_STATE_ACTIVE: u32 = 0;
/// Namespace is being set up (offsets not yet frozen).
pub const TIMENS_STATE_SETUP: u32 = 1;
/// Namespace offsets are frozen (first process entered).
pub const TIMENS_STATE_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_flag() {
        assert!(CLONE_NEWTIME.is_power_of_two());
    }

    #[test]
    fn test_offset_types_distinct() {
        assert_ne!(TIMENS_OFFSET_MONOTONIC, TIMENS_OFFSET_BOOTTIME);
    }

    #[test]
    fn test_id_values_distinct() {
        assert_ne!(TIMENS_MONOTONIC_ID, TIMENS_BOOTTIME_ID);
    }

    #[test]
    fn test_states_distinct() {
        let states = [TIMENS_STATE_ACTIVE, TIMENS_STATE_SETUP, TIMENS_STATE_FROZEN];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
