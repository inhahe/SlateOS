//! `<linux/io.h>` — I/O polling and scheduling constants.
//!
//! The Linux block layer supports different I/O polling modes for
//! low-latency storage (NVMe, etc.). I/O priority classes control
//! how the I/O scheduler services requests from different processes.
//! These constants are used with ioprio_set/ioprio_get syscalls.

// ---------------------------------------------------------------------------
// I/O priority classes
// ---------------------------------------------------------------------------

/// No I/O priority set (inherit from CPU scheduler).
pub const IOPRIO_CLASS_NONE: u32 = 0;
/// Real-time I/O class (highest priority, 8 levels).
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort I/O class (normal, 8 levels).
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle I/O class (lowest priority, only when disk is idle).
pub const IOPRIO_CLASS_IDLE: u32 = 3;

// ---------------------------------------------------------------------------
// I/O priority encoding
// ---------------------------------------------------------------------------

/// Shift for class field in ioprio value.
pub const IOPRIO_CLASS_SHIFT: u32 = 13;
/// Mask for priority level within class (0-7).
pub const IOPRIO_PRIO_MASK: u32 = (1 << IOPRIO_CLASS_SHIFT) - 1;
/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;
/// Best (highest) priority level within a class.
pub const IOPRIO_BEST_PRIO: u32 = 0;
/// Worst (lowest) priority level within a class.
pub const IOPRIO_WORST_PRIO: u32 = 7;

// ---------------------------------------------------------------------------
// ioprio_set/get "who" argument
// ---------------------------------------------------------------------------

/// Set/get priority for a process.
pub const IOPRIO_WHO_PROCESS: u32 = 1;
/// Set/get priority for a process group.
pub const IOPRIO_WHO_PGRP: u32 = 2;
/// Set/get priority for a user.
pub const IOPRIO_WHO_USER: u32 = 3;

// ---------------------------------------------------------------------------
// I/O poll modes (block layer)
// ---------------------------------------------------------------------------

/// Classic IRQ-based completion (no polling).
pub const BLK_POLL_DISABLE: u32 = 0;
/// Busy-poll for completions (spin, lowest latency).
pub const BLK_POLL_CLASSIC: u32 = 1;
/// Hybrid poll (sleep briefly then poll).
pub const BLK_POLL_HYBRID: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_classes_distinct() {
        let classes = [
            IOPRIO_CLASS_NONE, IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_priority_encoding() {
        // Compose: class=BE, level=4 → (2 << 13) | 4
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 4;
        assert_eq!(prio >> IOPRIO_CLASS_SHIFT, IOPRIO_CLASS_BE);
        assert_eq!(prio & IOPRIO_PRIO_MASK, 4);
    }

    #[test]
    fn test_priority_levels() {
        assert_eq!(IOPRIO_BEST_PRIO, 0);
        assert_eq!(IOPRIO_WORST_PRIO, 7);
        assert_eq!(IOPRIO_NR_LEVELS, 8);
    }

    #[test]
    fn test_who_values_distinct() {
        let whos = [IOPRIO_WHO_PROCESS, IOPRIO_WHO_PGRP, IOPRIO_WHO_USER];
        for i in 0..whos.len() {
            for j in (i + 1)..whos.len() {
                assert_ne!(whos[i], whos[j]);
            }
        }
    }

    #[test]
    fn test_poll_modes_distinct() {
        let modes = [BLK_POLL_DISABLE, BLK_POLL_CLASSIC, BLK_POLL_HYBRID];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_prio_mask_width() {
        // Mask should cover bits 0..(IOPRIO_CLASS_SHIFT-1)
        assert_eq!(IOPRIO_PRIO_MASK, 0x1FFF);
        assert_eq!(IOPRIO_PRIO_MASK.count_ones(), IOPRIO_CLASS_SHIFT);
    }
}
