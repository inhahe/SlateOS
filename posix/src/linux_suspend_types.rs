//! `<linux/suspend.h>` — System suspend/hibernate constants.
//!
//! System suspend saves state and powers down most hardware to save
//! energy. Linux supports multiple sleep states: suspend-to-idle
//! (freeze, lowest latency), suspend-to-RAM (S3, hardware powered
//! off except RAM), and hibernate (S4, state saved to disk, full
//! power off). The PM core coordinates freezing userspace, suspending
//! devices, saving state, and eventually entering the platform sleep.

// ---------------------------------------------------------------------------
// System sleep states (from /sys/power/state)
// ---------------------------------------------------------------------------

/// Suspend-to-idle (s2idle, freeze): CPU halted, devices at low power.
pub const PM_SUSPEND_TO_IDLE: u32 = 1;
/// Standby (S1): CPU powered off, devices at low power.
pub const PM_SUSPEND_STANDBY: u32 = 2;
/// Suspend-to-RAM (S3): most hardware off, RAM refreshed.
pub const PM_SUSPEND_MEM: u32 = 3;
/// Hibernate (S4): state saved to swap, full power off.
pub const PM_SUSPEND_DISK: u32 = 4;
/// System is on (not suspended).
pub const PM_SUSPEND_ON: u32 = 0;
/// Maximum suspend state value.
pub const PM_SUSPEND_MAX: u32 = 5;

// ---------------------------------------------------------------------------
// Suspend phases (device PM callbacks)
// ---------------------------------------------------------------------------

/// Prepare phase (before freezing tasks).
pub const PM_PHASE_PREPARE: u32 = 0;
/// Suspend phase (devices saving state).
pub const PM_PHASE_SUSPEND: u32 = 1;
/// Suspend-late phase (after interrupts disabled).
pub const PM_PHASE_SUSPEND_LATE: u32 = 2;
/// Suspend-noirq phase (no IRQ handlers running).
pub const PM_PHASE_SUSPEND_NOIRQ: u32 = 3;
/// Resume-noirq phase (IRQs still disabled).
pub const PM_PHASE_RESUME_NOIRQ: u32 = 4;
/// Resume-early phase (before full IRQ restore).
pub const PM_PHASE_RESUME_EARLY: u32 = 5;
/// Resume phase (devices restoring state).
pub const PM_PHASE_RESUME: u32 = 6;
/// Complete phase (after thawing tasks).
pub const PM_PHASE_COMPLETE: u32 = 7;

// ---------------------------------------------------------------------------
// Hibernate flags
// ---------------------------------------------------------------------------

/// Use platform (ACPI) mechanisms for hibernate.
pub const HIBERNATE_PLATFORM: u32 = 0x01;
/// Compress hibernate image.
pub const HIBERNATE_COMPRESS: u32 = 0x02;
/// Encrypt hibernate image.
pub const HIBERNATE_ENCRYPT: u32 = 0x04;
/// Test mode (suspend/resume without actual platform sleep).
pub const HIBERNATE_TEST: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_states_distinct() {
        let states = [
            PM_SUSPEND_ON,
            PM_SUSPEND_TO_IDLE,
            PM_SUSPEND_STANDBY,
            PM_SUSPEND_MEM,
            PM_SUSPEND_DISK,
        ];
        for i in 0..states.len() {
            assert!(states[i] < PM_SUSPEND_MAX);
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_phases_distinct() {
        let phases = [
            PM_PHASE_PREPARE,
            PM_PHASE_SUSPEND,
            PM_PHASE_SUSPEND_LATE,
            PM_PHASE_SUSPEND_NOIRQ,
            PM_PHASE_RESUME_NOIRQ,
            PM_PHASE_RESUME_EARLY,
            PM_PHASE_RESUME,
            PM_PHASE_COMPLETE,
        ];
        for i in 0..phases.len() {
            for j in (i + 1)..phases.len() {
                assert_ne!(phases[i], phases[j]);
            }
        }
    }

    #[test]
    fn test_hibernate_flags_no_overlap() {
        let flags = [
            HIBERNATE_PLATFORM,
            HIBERNATE_COMPRESS,
            HIBERNATE_ENCRYPT,
            HIBERNATE_TEST,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
